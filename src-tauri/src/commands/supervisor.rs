// webclaw-upgrader supervisor 状态板
// 通过 ubuntu 可访问的 supervisor socket 与容器内的 supervisord 通信

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tokio::process::Command;

fn supervisorctl(args: &[&str]) -> Command {
    let mut command = Command::new("supervisorctl");
    command.args(args);
    command
}

fn supervisor_tail_command(name: &str) -> Command {
    supervisorctl(&["tail", name, "stdout"])
}

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
    let out = supervisorctl(&["status"])
        .output()
        .await
        .map_err(|e| format!("supervisorctl spawn 失败: {}", e))?;
    // supervisorctl status 即使有进程 STOPPED 也是 exit 0；exit 非 0 通常是 supervisord 没起
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        let detail = combined.trim();
        return Err(format!(
            "supervisorctl status 失败：{}",
            if detail.is_empty() { "supervisord 可能未运行" } else { detail }
        ));
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
    // 只接受 supervisor 进程名允许的字符。
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("非法进程名: {}", name));
    }
    let status = supervisorctl(&["restart", &name])
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
    // 优先用 supervisorctl tail；失败时读取容器配置的 /tmp 日志或旧版日志目录。
    match tail_via_supervisorctl(&name, lines).await {
        Ok(text) => Ok(text),
        Err(supervisor_error) => tail_via_logfile(&name, lines).await.map_err(|file_error| {
            format!(
                "无法读取 {} 日志：supervisorctl: {}; 日志文件: {}",
                name, supervisor_error, file_error
            )
        }),
    }
}

async fn tail_via_supervisorctl(name: &str, lines: u32) -> Result<String> {
    let out = supervisor_tail_command(name)
        .output()
        .await
        .context("supervisorctl tail spawn")?;
    if !out.status.success() {
        return Err(anyhow::anyhow!("{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let total: Vec<&str> = text.lines().collect();
    let n = (lines as usize).min(total.len());
    Ok(total[total.len() - n..].join("\n"))
}

async fn tail_via_logfile(name: &str, lines: u32) -> Result<String> {
    let path = find_logfile(name, Path::new("/tmp"), Path::new("/var/log/supervisor")).await?;
    let out = Command::new("tail")
        .arg("-n")
        .arg(lines.to_string())
        .arg(&path)
        .output()
        .await
        .with_context(|| format!("无法读取 {}", path.display()))?;
    if !out.status.success() {
        return Err(anyhow::anyhow!("{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

async fn find_logfile(name: &str, tmp_dir: &Path, legacy_dir: &Path) -> Result<PathBuf> {
    for filename in [
        format!("{}_stdout.log", name),
        format!("{}_stdout.log", name.replace('-', "_")),
    ] {
        let path = tmp_dir.join(filename);
        if matches!(tokio::fs::symlink_metadata(&path).await, Ok(metadata) if metadata.file_type().is_file()) {
            return Ok(path);
        }
    }

    let mut dir = match tokio::fs::read_dir(legacy_dir).await {
        Ok(dir) => dir,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(anyhow::anyhow!("没有找到 {} 的 stdout 日志", name));
        }
        Err(e) => return Err(e).with_context(|| format!("无法访问 {}", legacy_dir.display())),
    };
    let prefix = format!("{}-stdout", name);
    let mut newest = None;
    while let Some(entry) = dir.next_entry().await? {
        let filename = entry.file_name();
        let filename = filename.to_string_lossy();
        if !filename.starts_with(&prefix) || !filename.ends_with(".log") {
            continue;
        }
        if !entry.file_type().await?.is_file() {
            continue;
        }
        let metadata = entry.metadata().await?;
        let modified = metadata.modified()?;
        if newest.as_ref().map_or(true, |(time, _)| modified > *time) {
            newest = Some((modified, entry.path()));
        }
    }
    newest
        .map(|(_, path)| path)
        .ok_or_else(|| anyhow::anyhow!("没有找到 {} 的 stdout 日志", name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn logfile_fallback_finds_tmp_names_and_reports_missing() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("webclaw-supervisor-test-{}-{}", std::process::id(), unique));
        let tmp_dir = root.join("tmp");
        let legacy_dir = root.join("supervisor");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&tmp_dir).unwrap();
        std::fs::create_dir(&legacy_dir).unwrap();

        let code_server = tmp_dir.join("code-server_stdout.log");
        let deepseek = tmp_dir.join("deepseek_harness_stdout.log");
        std::fs::write(&code_server, "code-server log").unwrap();
        std::fs::write(&deepseek, "deepseek log").unwrap();
        assert_eq!(find_logfile("code-server", &tmp_dir, &legacy_dir).await.unwrap(), code_server);
        assert_eq!(find_logfile("deepseek-harness", &tmp_dir, &legacy_dir).await.unwrap(), deepseek);

        std::fs::create_dir(tmp_dir.join("missing_stdout.log")).unwrap();
        let error = find_logfile("missing", &tmp_dir, &legacy_dir).await.unwrap_err().to_string();
        assert!(error.contains("没有找到 missing 的 stdout 日志"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn supervisor_commands_do_not_use_sudo() {
        for args in [
            vec!["status"],
            vec!["restart", "code-server"],
        ] {
            let command = supervisorctl(&args);
            assert_eq!(command.as_std().get_program(), "supervisorctl");
            assert_eq!(command.as_std().get_args().collect::<Vec<_>>(), args);
        }
        let tail = supervisor_tail_command("code-server");
        assert_eq!(tail.as_std().get_program(), "supervisorctl");
        assert_eq!(
            tail.as_std().get_args().collect::<Vec<_>>(),
            ["tail", "code-server", "stdout"]
        );
    }

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
