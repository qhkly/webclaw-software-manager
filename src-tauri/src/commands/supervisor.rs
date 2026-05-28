// webclaw-upgrader supervisor 状态板
// 通过 sudo supervisorctl 与容器内的 supervisord 通信

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub name: String,
    pub state: String,
    pub pid: Option<u32>,
    pub uptime_secs: Option<u64>,
    pub description: String,
}

#[tauri::command]
pub async fn supervisor_status() -> Result<Vec<ProcessInfo>, String> {
    let out = Command::new("sudo")
        .args(["supervisorctl", "status"])
        .output()
        .await
        .map_err(|e| format!("supervisorctl spawn 失败: {}", e))?;
    // supervisorctl status 即使有进程 STOPPED 也是 exit 0；exit 非 0 通常是 supervisord 没起
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() && combined.trim().is_empty() {
        return Err("supervisorctl 调用失败：supervisord 可能未运行".into());
    }
    Ok(parse_supervisor_status(&combined))
}

/// 解析 supervisorctl status 输出，例如：
///   code-server     RUNNING   pid 1234, uptime 0:02:13
///   openclaw        STOPPED   Not started
///   dashboard       FATAL     Exited too quickly (process log may have details)
fn parse_supervisor_status(text: &str) -> Vec<ProcessInfo> {
    let mut out = Vec::new();
    let pid_uptime_re = Regex::new(r"pid\s+(\d+),\s+uptime\s+(\d+):(\d+):(\d+)").unwrap();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, char::is_whitespace);
        let name = match parts.next() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };
        // 跳过空白
        let rest: String = line[name.len()..].trim_start().to_string();
        let mut rest_parts = rest.splitn(2, char::is_whitespace);
        let state = rest_parts.next().unwrap_or("").to_string();
        let description = rest_parts.next().unwrap_or("").trim().to_string();

        let (pid, uptime_secs) = if let Some(caps) = pid_uptime_re.captures(&description) {
            let pid = caps.get(1).and_then(|m| m.as_str().parse::<u32>().ok());
            let h: u64 = caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let mn: u64 = caps.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let s: u64 = caps.get(4).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            (pid, Some(h * 3600 + mn * 60 + s))
        } else {
            (None, None)
        };

        // 过滤掉非进程行（比如 "Server requires authentication" 之类）
        let known_states = ["RUNNING", "STOPPED", "STARTING", "BACKOFF", "STOPPING", "EXITED", "FATAL", "UNKNOWN"];
        if !known_states.contains(&state.as_str()) {
            continue;
        }

        out.push(ProcessInfo {
            name,
            state,
            pid,
            uptime_secs,
            description,
        });
    }
    out
}

#[tauri::command]
pub async fn supervisor_restart(name: String) -> Result<(), String> {
    // 仅允许字母数字和 - _ 的进程名，杜绝 shell 注入（sudo 白名单虽然限制了命令，参数仍需校验）
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("非法进程名: {}", name));
    }
    let status = Command::new("sudo")
        .args(["supervisorctl", "restart", &name])
        .status()
        .await
        .map_err(|e| format!("supervisorctl restart spawn 失败: {}", e))?;
    if !status.success() {
        return Err(format!(
            "supervisorctl restart {} 退出码 {}",
            name,
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn supervisor_tail_log(name: String, lines: u32) -> Result<String, String> {
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("非法进程名: {}", name));
    }
    let lines = lines.clamp(10, 2000);
    // 优先用 supervisorctl tail（不需要 sudo 即可读取）；不行再从 /var/log/supervisor 直接读
    if let Ok(text) = tail_via_supervisorctl(&name, lines).await {
        return Ok(text);
    }
    tail_via_logfile(&name, lines).await.map_err(|e| e.to_string())
}

async fn tail_via_supervisorctl(name: &str, lines: u32) -> Result<String> {
    let out = Command::new("sudo")
        .args(["supervisorctl", "tail", "-", name, "stdout"])
        .output()
        .await
        .context("supervisorctl tail spawn")?;
    if !out.status.success() {
        return Err(anyhow::anyhow!("supervisorctl tail failed"));
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let total: Vec<&str> = text.lines().collect();
    let n = (lines as usize).min(total.len());
    Ok(total[total.len() - n..].join("\n"))
}

async fn tail_via_logfile(name: &str, lines: u32) -> Result<String> {
    // /var/log/supervisor/<name>-stdout---supervisor-XXXX.log 命名规则不固定
    // 用 ls + tail
    let script = format!(
        "ls -1t /var/log/supervisor/{name}-stdout*.log 2>/dev/null | head -1 | xargs -r sudo tail -n {lines}",
        name = shell_escape(name),
        lines = lines
    );
    let out = Command::new("bash")
        .args(["-c", &script])
        .output()
        .await
        .context("tail logfile spawn")?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn shell_escape(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_running_line() {
        let text = "code-server                      RUNNING   pid 1234, uptime 0:02:13";
        let parsed = parse_supervisor_status(text);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "code-server");
        assert_eq!(parsed[0].state, "RUNNING");
        assert_eq!(parsed[0].pid, Some(1234));
        assert_eq!(parsed[0].uptime_secs, Some(133));
    }

    #[test]
    fn parse_mixed_states() {
        let text = "\
code-server                      RUNNING   pid 1234, uptime 0:02:13
openclaw                         STOPPED   Not started
dashboard                        FATAL     Exited too quickly
some-noise                       irrelevant text here
";
        let parsed = parse_supervisor_status(text);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[1].state, "STOPPED");
        assert_eq!(parsed[2].state, "FATAL");
    }
}
