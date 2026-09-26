use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::broker::{self, BrokerOp};
use super::manifest::{load_effective_manifest, platform_entries};

static HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent("webclaw-software-manager/0.1")
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build reqwest client")
});

const USER_NODE_RUNNER: &str = "/usr/local/bin/webclaw-user-node-run";

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
    #[serde(default)]
    pub store_slug: Option<String>,
    #[serde(default)]
    pub official_url: Option<String>,
    pub platforms: HashMap<String, PlatformSoftwareSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(try_from = "RawPlatformSpec")]
pub struct PlatformSoftwareSpec {
    pub detect: ActionSpec,
    pub latest: ActionSpec,
    pub install: ActionSpec,
    #[serde(default)]
    pub upgrade: Option<ActionSpec>,
}

/// broker 条目只需要写 `install`：检测、最新版本、升级都由同一个 broker app 负责。
#[derive(Deserialize)]
struct RawPlatformSpec {
    #[serde(default)]
    detect: Option<ActionSpec>,
    #[serde(default)]
    latest: Option<ActionSpec>,
    install: ActionSpec,
    #[serde(default)]
    upgrade: Option<ActionSpec>,
}

impl TryFrom<RawPlatformSpec> for PlatformSoftwareSpec {
    type Error = String;

    fn try_from(raw: RawPlatformSpec) -> Result<Self, Self::Error> {
        if let ActionSpec::Broker { app_id, .. } = &raw.install {
            broker::validate_app_id(app_id)?;
            return Ok(PlatformSoftwareSpec {
                detect: raw.install.clone(),
                latest: raw.install.clone(),
                upgrade: Some(raw.install.clone()),
                install: raw.install,
            });
        }
        Ok(PlatformSoftwareSpec {
            detect: raw.detect.ok_or("missing field `detect`")?,
            latest: raw.latest.ok_or("missing field `latest`")?,
            install: raw.install,
            upgrade: raw.upgrade,
        })
    }
}

impl PlatformSoftwareSpec {
    pub fn broker(app_id: &str, min_api_version: u32) -> Self {
        let action = ActionSpec::Broker {
            app_id: app_id.into(),
            min_api_version,
        };
        PlatformSoftwareSpec {
            detect: action.clone(),
            latest: action.clone(),
            install: action.clone(),
            upgrade: Some(action),
        }
    }

    /// (app_id, min_api_version)，仅 broker 条目
    pub fn broker_app(&self) -> Option<(&str, u32)> {
        match &self.install {
            ActionSpec::Broker {
                app_id,
                min_api_version,
            } => Some((app_id.as_str(), *min_api_version)),
            _ => None,
        }
    }

    /// 给界面看的后端类型
    fn backend(&self, platform: &str) -> &'static str {
        match (&self.install, platform) {
            (ActionSpec::Broker { .. }, _) => "broker",
            (ActionSpec::NpmGlobal { .. }, "container") => "user-node",
            (_, "container") => "legacy",
            _ => "native",
        }
    }
}

fn default_min_api_version() -> u32 {
    broker::BROKER_API_VERSION
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
    AiStudioInstalled,
    AiStudioLatest,
    /// 容器内 root 软件：交给 webclaw-app-admin 的高层 API（status/install/upgrade）
    Broker {
        app_id: String,
        #[serde(default = "default_min_api_version")]
        min_api_version: u32,
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
    pub store_slug: Option<String>,
    pub official_url: Option<String>,
    pub platform: String,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub state: String,
    pub error: Option<String>,
    /// broker / user-node / legacy / native
    pub backend: String,
    /// broker 报告的 runtime-catalog 状态（fresh/stale/offline/...）
    pub catalog_state: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SoftwareProgress {
    pub id: String,
    pub stage: String,
    pub percent: Option<f32>,
    pub line: Option<String>,
}

fn item_from_entry(entry: SoftwareEntry, spec: &PlatformSoftwareSpec, platform: String) -> CatalogItem {
    let backend = spec.backend(&platform).into();
    CatalogItem {
        id: entry.id,
        name: entry.name,
        name_zh: entry.name_zh,
        category: entry.category,
        group: entry.group,
        risk: entry.risk,
        desc: entry.desc,
        icon: entry.icon,
        store_slug: entry.store_slug,
        official_url: entry.official_url,
        platform,
        installed_version: None,
        latest_version: None,
        state: "not_installed".into(),
        error: None,
        backend,
        catalog_state: None,
    }
}

fn resolve_icon(icon: &str, app: &AppHandle) -> String {
    // If the file already exists at the manifest path (e.g. /opt/ on Linux), use as-is
    if std::path::Path::new(icon).exists() {
        let path = std::path::Path::new(icon);
        return path
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| icon.to_string());
    }
    let filename = std::path::Path::new(icon)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if filename.is_empty() {
        return icon.to_string();
    }
    // Candidate search paths — try each, return the first file path Tauri can serve.
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    // 1. resource_dir / on-demand-icons / filename (production bundle)
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("on-demand-icons").join(filename));
        candidates.push(resource_dir.join(filename));
    }
    // 2. CWD / on-demand-icons / filename (dev: CWD = webclaw-software-manager/)
    candidates.push(std::path::PathBuf::from("on-demand-icons").join(filename));
    // 3. Exe-relative (dev binary at target/debug/, go up to project root)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for up in &[
                "../../../../on-demand-icons",
                "../../../on-demand-icons",
                "../../on-demand-icons",
            ] {
                candidates.push(parent.join(up).join(filename));
            }
        }
    }
    for candidate in &candidates {
        if candidate.exists() {
            return candidate
                .canonicalize()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| candidate.to_string_lossy().to_string());
        }
    }
    icon.to_string()
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
        .map(|(entry, spec)| item_from_entry(entry, &spec, platform.clone()))
        .collect();
    for item in &mut items {
        if let Some(ref icon) = item.icon.clone() {
            item.icon = Some(resolve_icon(icon, &app));
        }
    }
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
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new((cores * 2).clamp(4, 16)));
    let mut handles = Vec::new();
    let catalog_state = scan_catalog_state(&platform).await;
    for (entry, spec) in platform_entries(manifest, &platform) {
        let platform_key = platform.clone();
        let catalog_state = catalog_state.clone();
        let permit = std::sync::Arc::clone(&sem).acquire_owned().await.unwrap();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            let mut item = item_from_entry(entry, &spec, platform_key);
            if spec.broker_app().is_some() {
                apply_broker_status(&mut item, &spec, catalog_state.as_deref()).await;
                return item;
            }
            match detect_one(&spec.detect, &item.platform).await {
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
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new((cores * 2).clamp(4, 16)));
    let mut tasks = tokio::task::JoinSet::new();
    let catalog_state = scan_catalog_state(&platform).await;
    for (entry, spec) in platform_entries(manifest, &platform) {
        let platform_key = platform.clone();
        let catalog_state = catalog_state.clone();
        let sem = std::sync::Arc::clone(&sem);
        tasks.spawn(async move {
            let _permit = sem.acquire_owned().await.unwrap();
            let is_ai_studio = entry.id == "webcode-ai-studio";
            let mut item = item_from_entry(entry, &spec, platform_key);
            if spec.broker_app().is_some() {
                apply_broker_status(&mut item, &spec, catalog_state.as_deref()).await;
                return item;
            }
            let (installed, latest) = if is_ai_studio {
                tokio::join!(detect_ai_studio(), fetch_ai_studio_latest())
            } else {
                tokio::join!(detect_one(&spec.detect, &item.platform), latest_one(&spec.latest))
            };
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
        });
    }
    let mut items = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        if let Ok(item) = joined {
            app.emit("catalog-item-update", &item).ok();
            items.push(item);
        }
    }
    items.sort_by(|a, b| a.group.cmp(&b.group).then(a.name.cmp(&b.name)));
    Ok(items)
}

#[tauri::command]
pub async fn check_software(
    app: AppHandle,
    platform: String,
    id: String,
) -> Result<CatalogItem, String> {
    let (manifest, _) = load_effective_manifest(&app)
        .await
        .map_err(|e| e.to_string())?;
    let (entry, spec) = platform_entries(manifest, &platform)
        .into_iter()
        .find(|(entry, _)| entry.id == id)
        .ok_or_else(|| format!("未知或不支持当前平台的软件: {}", id))?;
    let is_ai_studio = entry.id == "webcode-ai-studio";
    let mut item = item_from_entry(entry, &spec, platform);
    if spec.broker_app().is_some() {
        let catalog_state = scan_catalog_state(&item.platform).await;
        apply_broker_status(&mut item, &spec, catalog_state.as_deref()).await;
        return match (&item.state[..], item.error.clone()) {
            ("runtime_outdated", Some(e)) => Err(e),
            ("unknown", Some(e)) if item.installed_version.is_none() => Err(e),
            _ => Ok(item),
        };
    }
    let (installed, latest) = if is_ai_studio {
        tokio::join!(detect_ai_studio(), fetch_ai_studio_latest())
    } else {
        tokio::join!(detect_one(&spec.detect, &item.platform), latest_one(&spec.latest))
    };
    item.installed_version = installed.map_err(|e| e.to_string())?;
    item.latest_version = latest.map_err(|e| e.to_string())?;
    item.state = compute_state(
        item.installed_version.as_deref(),
        item.latest_version.as_deref(),
    );
    Ok(item)
}

/// 用 broker `status` 填充条目。运行时过旧、目录离线、broker 出错分别落到不同状态，
/// 不会一律显示成「未安装」。`catalog_state` 是这次扫描的整体 catalog 状态。
async fn apply_broker_status(
    item: &mut CatalogItem,
    spec: &PlatformSoftwareSpec,
    catalog_state: Option<&str>,
) {
    let Some((app_id, min_api_version)) = spec.broker_app() else {
        return;
    };
    let runtime = broker::runtime().await;
    if let Err(message) = runtime.require(min_api_version) {
        item.state = "runtime_outdated".into();
        item.error = Some(message);
        return;
    }
    match broker::status(app_id).await {
        Ok(mut status) => {
            // apt 类不进 runtime-catalog：用只读的 apt-cache policy 补最新版本，更新仍走 broker。
            if status.latest_version.is_none() {
                if let ActionSpec::AptPolicy { pkg } = &spec.latest {
                    match fetch_apt_policy(pkg).await {
                        Ok(latest) => status.latest_version = latest,
                        Err(e) => item.error = Some(e.to_string()),
                    }
                }
            }
            item.state = broker::state_from_status(&status);
            item.installed_version = status.installed_version.clone();
            item.latest_version = status.latest_version.clone();
            item.catalog_state = broker::item_catalog_state(&status, catalog_state);
            if status.message.is_some() {
                item.error = status.message.clone();
            }
            if item.state == "unsupported" && item.error.is_none() {
                item.error = Some(match status.upgrade_via.as_deref() {
                    Some(via) if via != "broker" => format!("当前镜像不支持在软件管理器中升级此软件（{}）", via),
                    _ => "当前镜像/架构不支持此软件".into(),
                });
            }
        }
        Err(e) => {
            item.state = "unknown".into();
            item.error = Some(e);
        }
    }
}

/// 本次扫描的整体 catalog 状态；broker 不可用或 catalog-info 失败时为 None。
async fn scan_catalog_state(platform: &str) -> Option<String> {
    if platform != "container" || broker::runtime().await.state != broker::RuntimeState::Ok {
        return None;
    }
    broker::catalog_info()
        .await
        .ok()
        .map(|info| broker::catalog_state_from_info(&info, chrono::Utc::now()))
}

#[derive(Debug, Clone, Serialize)]
pub struct BrokerRuntimeReport {
    pub runtime: broker::BrokerRuntime,
    pub catalog: Option<serde_json::Value>,
    /// fresh / stale / offline / missing / untrusted / unknown
    pub catalog_state: Option<String>,
    pub catalog_error: Option<String>,
}

/// 容器里 broker 运行时与 runtime-catalog 的状态；非容器平台返回 None。
#[tauri::command]
pub async fn broker_runtime_status(platform: String) -> Result<Option<BrokerRuntimeReport>, String> {
    if platform != "container" {
        return Ok(None);
    }
    let runtime = broker::runtime().await;
    let (catalog, catalog_state, catalog_error) = if runtime.state == broker::RuntimeState::Ok {
        match broker::catalog_info().await {
            Ok(v) => {
                let state = broker::catalog_state_from_info(&v, chrono::Utc::now());
                (Some(v), Some(state), None)
            }
            Err(e) => (None, None, Some(e)),
        }
    } else {
        (None, None, None)
    };
    Ok(Some(BrokerRuntimeReport {
        runtime,
        catalog,
        catalog_state,
        catalog_error,
    }))
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

async fn detect_one(spec: &ActionSpec, platform: &str) -> Result<Option<String>> {
    match spec {
        ActionSpec::Dpkg { pkg } => detect_dpkg(pkg).await,
        ActionSpec::NpmGlobal { pkg } => detect_npm_global(pkg, platform).await,
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

async fn detect_ai_studio() -> Result<Option<String>> {
    if let Some(version) = detect_dpkg("ai-cli-studio").await? {
        return Ok(Some(version));
    }
    if !Path::new("/usr/bin/webcode-ai-studio").exists() {
        return Ok(None);
    }
    let marker = Path::new("/opt/ai-cli-studio/.webclaw-version");
    match tokio::fs::read_to_string(marker).await {
        Ok(version) if !version.trim().is_empty() => Ok(Some(version.trim().to_string())),
        _ => Ok(Some("installed".into())),
    }
}

async fn fetch_ai_studio_latest() -> Result<Option<String>> {
    let json: serde_json::Value = HTTP
        .get("https://launcher.qhkly.com/launcher/webcode-ai-studio/latest.json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(json
        .get("version")
        .or_else(|| json.get("latest"))
        .and_then(|value| value.as_str())
        .filter(|version| !version.is_empty())
        .map(String::from))
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

fn npm_command(platform: &str, args: &[&str]) -> Vec<String> {
    let mut command = if platform == "container" {
        vec![USER_NODE_RUNNER.into(), "npm".into()]
    } else {
        vec!["npm".into()]
    };
    command.extend(args.iter().map(|arg| (*arg).into()));
    command
}

async fn detect_npm_global(pkg: &str, platform: &str) -> Result<Option<String>> {
    let command = npm_command(platform, &["ls", "-g", pkg, "--depth=0", "--json"]);
    let out = Command::new(&command[0])
        .args(&command[1..])
        .output()
        .await
        .context("npm ls spawn failed")?;
    parse_npm_global_result(
        pkg,
        &out.stdout,
        &out.stderr,
        out.status.code().unwrap_or(-1),
    )
}

fn parse_npm_global_result(
    pkg: &str,
    stdout: &[u8],
    stderr: &[u8],
    exit_code: i32,
) -> Result<Option<String>> {
    let json: serde_json::Value = serde_json::from_slice(stdout).map_err(|e| {
        let stderr = String::from_utf8_lossy(stderr);
        let detail: String = stderr.lines().next().unwrap_or("无 stderr").chars().take(200).collect();
        anyhow::anyhow!("npm ls 输出不是有效 JSON（退出码 {}）：{}；{}", exit_code, detail, e)
    })?;
    if !json.is_object() {
        return Err(anyhow::anyhow!("npm ls 输出不是 JSON 对象（退出码 {}）", exit_code));
    }
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

/// 卸载只开放给 broker 条目；broker 对部分 app 会返回 unsupported。
#[tauri::command]
pub async fn uninstall_software(app: AppHandle, id: String, platform: String) -> Result<(), String> {
    let (manifest, _) = load_effective_manifest(&app)
        .await
        .map_err(|e| e.to_string())?;
    let (_, spec) = platform_entries(manifest, &platform)
        .into_iter()
        .find(|(entry, _)| entry.id == id)
        .ok_or_else(|| format!("未知或不支持当前平台的软件: {}", id))?;
    let (app_id, min_api) = spec
        .broker_app()
        .ok_or("该软件不支持在软件管理器中卸载")?;
    broker::runtime().await.require(min_api)?;
    let command = broker::broker_argv(BrokerOp::Uninstall, Some(app_id))?;
    run_action_command(&app, &id, "uninstalling", &command).await?;
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

    // Install is a manager-level idempotent operation. The shared root script
    // always ensures latest because AI Studio's own updater calls it directly.
    if !upgrade && id == "webcode-ai-studio" && platform_spec.broker_app().is_none() {
        if detect_ai_studio().await.map_err(|e| e.to_string())?.is_some() {
            let _ = app.emit(
                "software-progress",
                SoftwareProgress {
                    id,
                    stage: "done".into(),
                    percent: Some(100.0),
                    line: Some("已安装，跳过安装".into()),
                },
            );
            return Ok(());
        }
    }

    let action = if upgrade {
        platform_spec.upgrade.as_ref().unwrap_or(&platform_spec.install)
    } else {
        &platform_spec.install
    };
    if let Some((_, min_api)) = platform_spec.broker_app() {
        broker::runtime().await.require(min_api)?;
    }
    let command = build_action_command(action, &platform, upgrade)?;
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

fn build_action_command(spec: &ActionSpec, platform: &str, upgrade: bool) -> Result<Vec<String>, String> {
    match spec {
        ActionSpec::Broker { app_id, .. } => {
            if platform != "container" {
                return Err("broker 后端只用于容器平台".into());
            }
            broker::broker_argv(
                if upgrade { BrokerOp::Upgrade } else { BrokerOp::Install },
                Some(app_id),
            )
        }
        ActionSpec::NpmGlobal { pkg } => Ok(npm_command(
            platform,
            &["install", "-g", &format!("{}@latest", pkg)],
        )),
        ActionSpec::Apt { pkg } => Ok(vec![
            "sudo".into(),
            "apt-get".into(),
            "install".into(),
            "-y".into(),
            pkg.clone(),
        ]),
        ActionSpec::CustomScript { script } => Ok(vec!["sudo".into(), script.clone()]),
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
        .stdin(Stdio::null())
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

pub(crate) fn compute_state(installed: Option<&str>, latest: Option<&str>) -> String {
    match (installed, latest) {
        (None, _) => "not_installed".into(),
        (Some(_), Some("latest")) => "up_to_date".into(),
        (Some(cur), Some(latest)) if cur == latest => "up_to_date".into(),
        (Some("installed"), Some(_)) => "upgradable".into(),
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::manifest::ManifestFile;

    fn bundled_manifest() -> ManifestFile {
        serde_json::from_str(include_str!("../../../software-manifest.json"))
            .expect("software-manifest.json should parse")
    }

    fn claude_spec(platform: &str) -> PlatformSoftwareSpec {
        let manifest = bundled_manifest();
        let entry = manifest
            .software
            .into_iter()
            .find(|entry| entry.id == "claude-code")
            .expect("claude-code entry should exist");
        entry
            .platforms
            .get(platform)
            .cloned()
            .unwrap_or_else(|| panic!("claude-code should support {platform}"))
    }

    #[test]
    fn container_claude_install_and_upgrade_use_fixed_sudo_script() {
        let (_, spec) = crate::commands::manifest::platform_entries(bundled_manifest(), "container")
            .into_iter()
            .find(|(entry, _)| entry.id == "claude-code")
            .expect("claude-code should support container");
        let expected = vec!["sudo", "/opt/install-scripts/install-claude-code.sh"];

        // manifest 写什么就执行什么：platform_entries 不改写 Claude 的 action
        let upgrade = spec.upgrade.as_ref().expect("container claude-code declares upgrade");
        for (action, is_upgrade) in [(&spec.install, false), (upgrade, true)] {
            assert!(matches!(
                action,
                ActionSpec::CustomScript { script } if script == "/opt/install-scripts/install-claude-code.sh"
            ));
            assert_eq!(
                build_action_command(action, "container", is_upgrade).expect("build container claude command"),
                expected
            );
        }
    }

    #[test]
    fn claude_install_script_prefers_user_node_runner_with_legacy_fallback() {
        let script = include_str!("../../../scripts/install-claude-code.sh");
        assert!(script.contains("USER_NODE_RUN=/usr/local/bin/webclaw-user-node-run\n"));
        // 新容器：runner 可执行时所有 node 命令都经它进入 ubuntu NVM 环境
        assert!(script.contains("if [ -x \"$USER_NODE_RUN\" ]; then\n    # 新容器：装进 ubuntu 的 NVM 用户环境\n    run_node() { \"$USER_NODE_RUN\" \"$@\"; }\n"));
        // 旧容器：runner 缺失时回退到 system npm/claude
        assert!(script.contains("else\n    # 旧容器（无 runner）：回退到 system Node\n    run_node() { \"$@\"; }\nfi\n"));
        assert!(script.contains(
            "run_node npm install -g --fetch-retries=5 --fetch-retry-mintimeout=20000 --fetch-retry-maxtimeout=120000 --fetch-timeout=300000 @anthropic-ai/claude-code@latest\n"
        ));
        // npm/claude 只能经 run_node 调用
        for line in script.lines().map(str::trim_start) {
            assert!(!line.starts_with("npm ") && !line.starts_with("claude "), "bare call: {line}");
        }
        // 固定白名单入口：不读取调用方参数，"$@" 只出现在两个 run_node 定义里
        assert_eq!(script.matches("\"$@\"").count(), 2);
        for forbidden in ["$1", "$*", "${@", "${1", "eval "] {
            assert!(!script.contains(forbidden), "script must not use {forbidden}");
        }
    }

    #[test]
    fn desktop_claude_install_keeps_unprivileged_npm_global_behavior() {
        for platform in ["macos", "windows"] {
            let spec = claude_spec(platform);
            assert!(matches!(
                spec.install,
                ActionSpec::NpmGlobal { ref pkg } if pkg == "@anthropic-ai/claude-code"
            ));

            let command =
                build_action_command(&spec.install, platform, false).expect("build npm-global command");
            assert_eq!(command.first().map(String::as_str), Some("npm"));
            assert!(!command.iter().any(|arg| arg == "sudo"));
        }
    }

    #[test]
    fn npm_global_uses_user_node_only_in_container() {
        for pkg in ["@anthropic-ai/claude-code", "@openai/codex", "@google/gemini-cli"] {
            let action = ActionSpec::NpmGlobal { pkg: pkg.into() };
            let container = build_action_command(&action, "container", false).unwrap();
            assert_eq!(
                container,
                vec![USER_NODE_RUNNER, "npm", "install", "-g", &format!("{}@latest", pkg)]
            );
            assert!(!container.iter().any(|part| part == "sudo"));
            assert_eq!(
                npm_command("container", &["ls", "-g", pkg, "--depth=0", "--json"]),
                vec![USER_NODE_RUNNER, "npm", "ls", "-g", pkg, "--depth=0", "--json"]
            );
            for platform in ["macos", "windows"] {
                assert_eq!(
                    build_action_command(&action, platform, false).unwrap(),
                    vec!["npm", "install", "-g", &format!("{}@latest", pkg)]
                );
            }
        }
    }

    #[test]
    fn broker_action_uses_fixed_broker_argv_only_in_container() {
        let action = ActionSpec::Broker { app_id: "vscode".into(), min_api_version: 2 };
        let prefix = ["sudo", "-n", "--", broker::BROKER_PATH];
        let install = build_action_command(&action, "container", false).unwrap();
        assert_eq!(install[..4], prefix);
        assert_eq!(install[4..], ["install", "vscode"]);
        let upgrade = build_action_command(&action, "container", true).unwrap();
        assert_eq!(upgrade[4..], ["upgrade", "vscode"]);
        assert!(build_action_command(&action, "macos", false).is_err());
        let evil = ActionSpec::Broker { app_id: "vscode --x".into(), min_api_version: 2 };
        assert!(build_action_command(&evil, "container", false).is_err());
        // user-npm 不受 broker 影响
        let npm = ActionSpec::NpmGlobal { pkg: "@openai/codex".into() };
        assert_eq!(build_action_command(&npm, "container", true).unwrap()[0], USER_NODE_RUNNER);
    }

    #[test]
    fn broker_spec_only_needs_install() {
        let spec: PlatformSoftwareSpec =
            serde_json::from_str(r#"{"install":{"type":"broker","app_id":"gimp"}}"#).unwrap();
        assert_eq!(spec.broker_app(), Some(("gimp", broker::BROKER_API_VERSION)));
        assert_eq!(spec.backend("container"), "broker");
        assert!(serde_json::from_str::<PlatformSoftwareSpec>(r#"{"install":{"type":"apt","pkg":"gimp"}}"#).is_err());
    }

    #[test]
    fn npm_ls_valid_json_reports_installed_version() {
        let stdout = br#"{"dependencies":{"@openai/codex":{"version":"1.2.3"}}}"#;
        assert_eq!(
            parse_npm_global_result("@openai/codex", stdout, b"", 0).unwrap(),
            Some("1.2.3".into())
        );
    }

    #[test]
    fn npm_ls_valid_json_with_nonzero_exit_can_mean_not_installed() {
        assert_eq!(
            parse_npm_global_result("@openai/codex", br#"{"dependencies":{}}"#, b"npm ERR! missing", 1).unwrap(),
            None
        );
    }

    #[test]
    fn npm_ls_runner_failure_is_error() {
        let error = parse_npm_global_result("@openai/codex", b"", b"NVM unavailable\nmore detail", 127)
            .unwrap_err()
            .to_string();
        assert!(error.contains("127"));
        assert!(error.contains("NVM unavailable"));
    }

    #[test]
    fn npm_ls_bad_json_with_success_is_error() {
        assert!(parse_npm_global_result("@openai/codex", b"not json", b"", 0).is_err());
    }
}
