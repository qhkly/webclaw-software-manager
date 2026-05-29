use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::manifest::{load_effective_manifest, platform_entries};

static HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent("webclaw-software-manager/0.1")
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build reqwest client")
});

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SoftwareEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub name_zh: Option<String>,
    pub category: String,
    pub group: String,
    pub risk: String,
    pub desc: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub platforms: HashMap<String, PlatformSoftwareSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlatformSoftwareSpec {
    pub detect: ActionSpec,
    pub latest: ActionSpec,
    pub install: ActionSpec,
    #[serde(default)]
    pub upgrade: Option<ActionSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ActionSpec {
    Dpkg {
        pkg: String,
    },
    NpmGlobal {
        pkg: String,
    },
    NpmRegistry {
        pkg: String,
    },
    AptPolicy {
        pkg: String,
    },
    Apt {
        pkg: String,
    },
    CustomScript {
        script: String,
    },
    Shell {
        cmd: String,
        #[serde(default)]
        version_regex: Option<String>,
    },
    Static {
        version: String,
    },
    /// detect 用：检查文件是否存在，存在则视为已安装
    Binary {
        path: String,
    },
    /// latest 用：从 GitHub API 查询最新 release tag
    GithubReleaseLatest {
        repo: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogItem {
    pub id: String,
    pub name: String,
    pub name_zh: Option<String>,
    pub category: String,
    pub group: String,
    pub risk: String,
    pub desc: String,
    pub icon: Option<String>,
    pub platform: String,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub state: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SoftwareProgress {
    pub id: String,
    pub stage: String,
    pub percent: Option<f32>,
    pub line: Option<String>,
}

fn item_from_entry(entry: SoftwareEntry, platform: String) -> CatalogItem {
    CatalogItem {
        id: entry.id,
        name: entry.name,
        name_zh: entry.name_zh,
        category: entry.category,
        group: entry.group,
        risk: entry.risk,
        desc: entry.desc,
        icon: entry.icon,
        platform,
        installed_version: None,
        latest_version: None,
        state: "not_installed".into(),
        error: None,
    }
}

#[tauri::command]
pub async fn get_platform_catalog(
    app: AppHandle,
    platform: String,
) -> Result<Vec<CatalogItem>, String> {
    let (manifest, _) = load_effective_manifest(&app)
        .await
        .map_err(|e| e.to_string())?;
    let mut items: Vec<_> = platform_entries(manifest, &platform)
        .into_iter()
        .map(|(entry, _)| item_from_entry(entry, platform.clone()))
        .collect();
    items.sort_by(|a, b| a.group.cmp(&b.group).then(a.name.cmp(&b.name)));
    Ok(items)
}

#[tauri::command]
pub async fn detect_installed(
    app: AppHandle,
    platform: String,
) -> Result<Vec<CatalogItem>, String> {
    let (manifest, _) = load_effective_manifest(&app)
        .await
        .map_err(|e| e.to_string())?;
    let mut handles = Vec::new();
    for (entry, spec) in platform_entries(manifest, &platform) {
        let platform_key = platform.clone();
        handles.push(tokio::spawn(async move {
            let mut item = item_from_entry(entry, platform_key);
            match detect_one(&spec.detect).await {
                Ok(Some(version)) => {
                    item.installed_version = Some(version);
                    item.state = "unknown".into();
                }
                Ok(None) => {
                    item.state = "not_installed".into();
                }
                Err(e) => {
                    item.state = "unknown".into();
                    item.error = Some(e.to_string());
                }
            }
            item
        }));
    }
    collect_items(handles).await
}

#[tauri::command]
pub async fn check_latest(app: AppHandle, platform: String) -> Result<Vec<CatalogItem>, String> {
    let (manifest, _) = load_effective_manifest(&app)
        .await
        .map_err(|e| e.to_string())?;
    let mut handles = Vec::new();
    for (entry, spec) in platform_entries(manifest, &platform) {
        let platform_key = platform.clone();
        handles.push(tokio::spawn(async move {
            let mut item = item_from_entry(entry, platform_key);
            let (installed, latest) = tokio::join!(detect_one(&spec.detect), latest_one(&spec.latest));
            match installed {
                Ok(version) => item.installed_version = version,
                Err(e) => item.error = Some(e.to_string()),
            }
            match latest {
                Ok(version) => item.latest_version = version,
                Err(e) => item.error = Some(e.to_string()),
            }
            item.state = compute_state(
                item.installed_version.as_deref(),
                item.latest_version.as_deref(),
            );
            item
        }));
    }
    collect_items(handles).await
}

async fn collect_items(
    handles: Vec<tokio::task::JoinHandle<CatalogItem>>,
) -> Result<Vec<CatalogItem>, String> {
    let mut items = Vec::new();
    for handle in handles {
        if let Ok(item) = handle.await {
            items.push(item);
        }
    }
    items.sort_by(|a, b| a.group.cmp(&b.group).then(a.name.cmp(&b.name)));
    Ok(items)
}

async fn detect_one(spec: &ActionSpec) -> Result<Option<String>> {
    match spec {
        ActionSpec::Dpkg { pkg } => detect_dpkg(pkg).await,
        ActionSpec::NpmGlobal { pkg } => detect_npm_global(pkg).await,
        ActionSpec::Shell { cmd, version_regex } => detect_shell(cmd, version_regex.as_deref()).await,
        ActionSpec::Static { version } => Ok(Some(version.clone())),
        ActionSpec::Binary { path } => {
            if Path::new(path).exists() {
                Ok(Some("installed".into()))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

async fn latest_one(spec: &ActionSpec) -> Result<Option<String>> {
    match spec {
        ActionSpec::NpmRegistry { pkg } => fetch_npm(pkg).await,
        ActionSpec::AptPolicy { pkg } => fetch_apt_policy(pkg).await,
        ActionSpec::Static { version } => Ok(Some(version.clone())),
        ActionSpec::Shell { cmd, version_regex } => detect_shell(cmd, version_regex.as_deref()).await,
        ActionSpec::GithubReleaseLatest { repo } => fetch_github_latest(repo).await,
        _ => Ok(None),
    }
}

async fn detect_dpkg(pkg: &str) -> Result<Option<String>> {
    let out = Command::new("dpkg-query")
        .args(["-W", "-f=${Version}", pkg])
        .output()
        .await
        .context("dpkg-query spawn failed")?;
    if !out.status.success() {
        return Ok(None);
    }
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((!version.is_empty()).then(|| strip_apt_version(&version)))
}

async fn detect_npm_global(pkg: &str) -> Result<Option<String>> {
    let out = Command::new("npm")
        .args(["ls", "-g", pkg, "--depth=0", "--json"])
        .output()
        .await
        .context("npm ls spawn failed")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    let pointer = format!("/dependencies/{}/version", pkg.replace('/', "~1"));
    Ok(json
        .pointer(&pointer)
        .and_then(|x| x.as_str())
        .map(String::from))
}

async fn detect_shell(cmd: &str, version_regex: Option<&str>) -> Result<Option<String>> {
    let out = shell_output(cmd).await?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() && text.trim().is_empty() {
        return Ok(None);
    }
    if let Some(pattern) = version_regex {
        let re = Regex::new(pattern).context("bad version_regex in manifest")?;
        return Ok(re
            .captures(&text)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string()));
    }
    let trimmed = text.trim();
    Ok((!trimmed.is_empty()).then(|| trimmed.lines().next().unwrap_or(trimmed).to_string()))
}

async fn fetch_npm(pkg: &str) -> Result<Option<String>> {
    let encoded = pkg.replace('/', "%2F");
    let url = format!("https://registry.npmjs.org/{}/latest", encoded);
    let resp = HTTP.get(url).send().await.context("npm request")?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let json: serde_json::Value = resp.json().await.context("npm json")?;
    Ok(json
        .get("version")
        .and_then(|v| v.as_str())
        .map(String::from))
}

async fn fetch_apt_policy(pkg: &str) -> Result<Option<String>> {
    let out = Command::new("apt-cache")
        .args(["policy", pkg])
        .output()
        .await
        .context("apt-cache policy spawn failed")?;
    if !out.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let re = Regex::new(r"Candidate:\s*(\S+)").unwrap();
    Ok(re
        .captures(&stdout)
        .and_then(|c| c.get(1))
        .map(|m| strip_apt_version(m.as_str())))
}

async fn fetch_github_latest(repo: &str) -> Result<Option<String>> {
    // 优先通过 redirect 获取最新 tag（不消耗 API 配额，无 rate limit）
    let redirect_url = format!("https://github.com/{}/releases/latest", repo);
    let resp = HTTP
        .get(&redirect_url)
        .send()
        .await
        .context("github redirect request")?;
    if let Some(location) = resp.headers().get("location") {
        let loc = location.to_str().unwrap_or("");
        if let Some(tag) = loc.split("/tag/").nth(1) {
            return Ok(Some(tag.trim_start_matches('v').to_string()));
        }
    }
    // 降级：用 GitHub API（可能受 rate limit 影响）
    let api_url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    let resp = HTTP
        .get(&api_url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("github api request")?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let json: serde_json::Value = resp.json().await.context("github api json")?;
    Ok(json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches('v').to_string()))
}

#[tauri::command]
pub async fn install_software(
    app: AppHandle,
    id: String,
    platform: String,
) -> Result<(), String> {
    execute_software_action(app, id, platform, false).await
}

#[tauri::command]
pub async fn upgrade_software(
    app: AppHandle,
    id: String,
    platform: String,
) -> Result<(), String> {
    execute_software_action(app, id, platform, true).await
}

async fn execute_software_action(
    app: AppHandle,
    id: String,
    platform: String,
    upgrade: bool,
) -> Result<(), String> {
    let (manifest, _) = load_effective_manifest(&app)
        .await
        .map_err(|e| e.to_string())?;
    let (entry, platform_spec) = platform_entries(manifest, &platform)
        .into_iter()
        .find(|(entry, _)| entry.id == id)
        .ok_or_else(|| format!("未知或不支持当前平台的软件: {}", id))?;

    let action = if upgrade {
        platform_spec.upgrade.as_ref().unwrap_or(&platform_spec.install)
    } else {
        &platform_spec.install
    };
    let command = build_action_command(action)?;
    let stage = if upgrade { "upgrading" } else { "installing" };

    let _ = app.emit(
        "software-progress",
        SoftwareProgress {
            id: id.clone(),
            stage: "starting".into(),
            percent: Some(5.0),
            line: Some(format!("准备{} {}", if upgrade { "升级" } else { "安装" }, entry.name)),
        },
    );
    run_action_command(&app, &id, stage, &command).await?;
    let _ = app.emit(
        "software-progress",
        SoftwareProgress {
            id,
            stage: "done".into(),
            percent: Some(100.0),
            line: Some("完成".into()),
        },
    );
    Ok(())
}

fn build_action_command(spec: &ActionSpec) -> Result<Vec<String>, String> {
    match spec {
        ActionSpec::NpmGlobal { pkg } => Ok(vec![
            "npm".into(),
            "install".into(),
            "-g".into(),
            format!("{}@latest", pkg),
        ]),
        ActionSpec::Apt { pkg } => Ok(vec![
            "sudo".into(),
            "apt-get".into(),
            "install".into(),
            "-y".into(),
            pkg.clone(),
        ]),
        ActionSpec::CustomScript { script } => Ok(vec!["bash".into(), script.clone()]),
        ActionSpec::Shell { cmd, .. } => Ok(vec!["bash".into(), "-c".into(), cmd.clone()]),
        _ => Err("该 action 类型不能用于安装/升级".into()),
    }
}

async fn run_action_command(
    app: &AppHandle,
    id: &str,
    stage: &str,
    cmd: &[String],
) -> Result<(), String> {
    let display = cmd.join(" ");
    let _ = app.emit(
        "software-progress",
        SoftwareProgress {
            id: id.into(),
            stage: stage.into(),
            percent: Some(15.0),
            line: Some(format!("$ {}", display)),
        },
    );

    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("spawn failed: {}", e))?;

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let stdout_app = app.clone();
    let stdout_id = id.to_string();
    let stdout_stage = stage.to_string();
    let stdout_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = stdout_app.emit(
                "software-progress",
                SoftwareProgress {
                    id: stdout_id.clone(),
                    stage: stdout_stage.clone(),
                    percent: Some(50.0),
                    line: Some(line),
                },
            );
        }
    });

    let stderr_app = app.clone();
    let stderr_id = id.to_string();
    let stderr_stage = stage.to_string();
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = stderr_app.emit(
                "software-progress",
                SoftwareProgress {
                    id: stderr_id.clone(),
                    stage: stderr_stage.clone(),
                    percent: Some(50.0),
                    line: Some(format!("[stderr] {}", line)),
                },
            );
        }
    });

    let status = child
        .wait()
        .await
        .map_err(|e| format!("wait failed: {}", e))?;
    let _ = stdout_task.await;
    let _ = stderr_task.await;
    if status.success() {
        Ok(())
    } else {
        let msg = format!("命令退出码 {}", status.code().unwrap_or(-1));
        let _ = app.emit(
            "software-progress",
            SoftwareProgress {
                id: id.into(),
                stage: "error".into(),
                percent: None,
                line: Some(msg.clone()),
            },
        );
        Err(msg)
    }
}

async fn shell_output(cmd: &str) -> Result<std::process::Output> {
    if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", cmd])
            .output()
            .await
            .context("cmd spawn failed")
    } else {
        Command::new("bash")
            .args(["-c", cmd])
            .output()
            .await
            .context("shell spawn failed")
    }
}

fn compute_state(installed: Option<&str>, latest: Option<&str>) -> String {
    match (installed, latest) {
        (None, _) => "not_installed".into(),
        (Some(_), Some("latest")) => "up_to_date".into(),
        (Some(cur), Some(latest)) if cur == latest => "up_to_date".into(),
        (Some(cur), Some(latest)) if version_lt(cur, latest) => "upgradable".into(),
        (Some(_), Some(_)) => "up_to_date".into(),
        (Some(_), None) => "unknown".into(),
    }
}

fn strip_apt_version(v: &str) -> String {
    v.split(|c: char| c == '-' || c == '+' || c == '~')
        .next()
        .unwrap_or(v)
        .to_string()
}

fn version_lt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
            .map(|s| s.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let va = parse(a);
    let vb = parse(b);
    let n = va.len().max(vb.len());
    for i in 0..n {
        let x = *va.get(i).unwrap_or(&0);
        let y = *vb.get(i).unwrap_or(&0);
        if x < y {
            return true;
        }
        if x > y {
            return false;
        }
    }
    false
}
