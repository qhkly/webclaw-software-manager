use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

use super::broker::BROKER_API_VERSION;
use super::software::{ActionSpec, PlatformSoftwareSpec, SoftwareEntry};

static HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent("webclaw-software-manager/0.1")
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build reqwest client")
});

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestFile {
    pub version: String,
    #[serde(default)]
    pub _remote_url: Option<String>,
    #[serde(deserialize_with = "deserialize_entries")]
    pub software: Vec<SoftwareEntry>,
}

/// 单个条目解析失败（例如将来新增的 action 类型）只跳过该条目，不让整份清单失效。
fn deserialize_entries<'de, D>(deserializer: D) -> std::result::Result<Vec<SoftwareEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Vec<serde_json::Value> = Deserialize::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|value| {
            let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            serde_json::from_value::<SoftwareEntry>(value)
                .map_err(|e| eprintln!("[manifest] 跳过无法解析的条目 {}: {}", id, e))
                .ok()
        })
        .collect())
}

#[derive(Debug, Clone, Serialize)]
pub struct ManifestSource {
    pub source: String,
    pub version: String,
}

fn manifest_candidate_paths(app: &AppHandle) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        paths.push(resource_dir.join("_up_/software-manifest.json"));
        paths.push(resource_dir.join("software-manifest.json"));
    }
    paths.push(PathBuf::from("../software-manifest.json"));
    paths.push(PathBuf::from("software-manifest.json"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            paths.push(parent.join("../../software-manifest.json"));
            paths.push(parent.join("../../../software-manifest.json"));
        }
    }
    paths
}

fn cache_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("webclaw-software-manager")
        .join("manifest-cache.json")
}

async fn parse_manifest(content: &str) -> Result<ManifestFile> {
    let manifest: ManifestFile =
        serde_json::from_str(content).context("parse software-manifest.json")?;
    if manifest.software.is_empty() {
        return Err(anyhow!("software-manifest.json 没有可用条目"));
    }
    Ok(manifest)
}

/// Docker broker（api v2）`install`/`upgrade <id>` 都能端到端完成的 app：
///   - apt、.deb（github_release / direct_download）；
///   - AppImage / zip / tar / cursor_api / direct_download 归档类（broker 内部 fetch + 校验 + install-tree）；
///   - qq / telegram / discord：custom_script，Docker manifest 声明了 `upgrade_by_reinstall=true`。
///
/// 刻意不在表中（继续 legacy/special）：
///   - webcode-ai-studio、webclaw-software-manager、hermes、webcode-git-manager：broker 不支持可靠升级
///     或客户端已有专用升级逻辑；
///   - claude-code / codex / opencode：用户 NVM CLI，永远不走 root broker
///     （claude-code 经固定脚本 install-claude-code.sh 安装，脚本内再交给 user-node runner）；
///   - 没有 Docker policy 的 manager-only 工具。
///
/// 这是硬编码白名单：容器平台上的 broker 条目必须 `app_id == entry.id` 且在此表中，
/// 远程清单不能把任意商品条目指向任意 root app。新增 broker app 需要发客户端版本。
///
/// 第二列是 bundled 旧清单里的历史内置写法（每个 id 精确到包名 / 脚本路径），只有
/// install（以及存在时的 upgrade）与之**完全一致**时才映射到 broker；
/// 任意别的 custom-script / apt / shell 都原样保留，绝不因此获得 broker 权限。
enum LegacyInstall {
    Apt(&'static str),
    Script(&'static str),
}

const BROKER_APPS: &[(&str, LegacyInstall)] = &[
    // apt
    ("audacity", LegacyInstall::Apt("audacity")),
    ("flameshot", LegacyInstall::Apt("flameshot")),
    ("gimp", LegacyInstall::Apt("gimp")),
    ("wireshark", LegacyInstall::Apt("wireshark")),
    ("blender", LegacyInstall::Apt("blender")),
    ("vscode", LegacyInstall::Apt("code")),
    ("antigravity", LegacyInstall::Apt("antigravity")),
    // .deb
    ("cc-switch", LegacyInstall::Script("/opt/install-scripts/install-cc-switch.sh")),
    ("dbeaver", LegacyInstall::Script("/opt/install-scripts/install-dbeaver.sh")),
    ("dockyard", LegacyInstall::Script("/opt/install-scripts/install-dockyard.sh")),
    ("opentypeless", LegacyInstall::Script("/opt/install-scripts/install-opentypeless.sh")),
    ("trae", LegacyInstall::Script("/opt/install-scripts/install-trae.sh")),
    ("wechat", LegacyInstall::Script("/opt/install-scripts/install-wechat.sh")),
    // AppImage / 归档 / cursor_api
    ("obsidian", LegacyInstall::Script("/opt/install-scripts/install-obsidian.sh")),
    ("ghostty", LegacyInstall::Script("/opt/install-scripts/install-ghostty.sh")),
    ("cursor", LegacyInstall::Script("/opt/install-scripts/install-cursor.sh")),
    ("webclaw-launcher", LegacyInstall::Script("/opt/install-scripts/install-webclaw-launcher.sh")),
    ("intellij", LegacyInstall::Script("/opt/install-scripts/install-intellij.sh")),
    ("pycharm", LegacyInstall::Script("/opt/install-scripts/install-pycharm.sh")),
    ("eclipse", LegacyInstall::Script("/opt/install-scripts/install-eclipse.sh")),
    ("android-studio", LegacyInstall::Script("/opt/install-scripts/install-android-studio.sh")),
    // custom_script + upgrade_by_reinstall（旧清单没有 upgrade 字段）
    ("qq", LegacyInstall::Script("/opt/install-scripts/install-qq.sh")),
    ("telegram", LegacyInstall::Script("/opt/install-scripts/install-telegram.sh")),
    ("discord", LegacyInstall::Script("/opt/install-scripts/install-discord.sh")),
];

fn is_broker_app(id: &str) -> bool {
    BROKER_APPS.iter().any(|(known, _)| *known == id)
}

fn is_known_legacy_action(kind: &LegacyInstall, action: &ActionSpec) -> bool {
    match (kind, action) {
        (LegacyInstall::Apt(expected), ActionSpec::Apt { pkg }) => pkg == expected,
        (LegacyInstall::Script(expected), ActionSpec::CustomScript { script }) => script == expected,
        _ => false,
    }
}

fn legacy_broker_spec(id: &str, spec: &PlatformSoftwareSpec) -> Option<PlatformSoftwareSpec> {
    let (_, kind) = BROKER_APPS.iter().find(|(known, _)| *known == id)?;
    let install_ok = is_known_legacy_action(kind, &spec.install);
    let upgrade_ok = spec
        .upgrade
        .as_ref()
        .map_or(true, |action| is_known_legacy_action(kind, action));
    (install_ok && upgrade_ok).then(|| {
        let mut broker = PlatformSoftwareSpec::broker(id, BROKER_API_VERSION);
        // Docker broker 的 status 只报告 runtime-catalog 的版本，apt 不进 catalog。
        // apt 的最新版本由客户端只读查询 `apt-cache policy <pkg>`（argv，无 root），
        // 包名来自上面的硬编码白名单，不来自远程清单。
        if let LegacyInstall::Apt(pkg) = kind {
            broker.latest = ActionSpec::AptPolicy { pkg: (*pkg).into() };
        }
        broker
    })
}

/// 容器平台的安装策略只信任随客户端发布的 bundled 清单。
///
/// 远程/缓存清单可以更新名称、描述、分组、商品链接、排序等非执行元数据，
/// 但 `platforms.container`（其中的 shell / custom-script / apt 等可执行 action）
/// 一律替换为 bundled 里同 id 的版本；bundled 里没有的 app 在容器平台上不提供任何后端。
/// bundled 读不到时 fail closed：所有容器条目都被移除。macOS / Windows 暂保持原样。
pub fn apply_container_policy(mut manifest: ManifestFile, bundled: Option<&ManifestFile>) -> ManifestFile {
    for entry in &mut manifest.software {
        let trusted = bundled
            .and_then(|b| b.software.iter().find(|e| e.id == entry.id))
            .and_then(|e| e.platforms.get("container"))
            .cloned();
        match trusted {
            Some(spec) => {
                entry.platforms.insert("container".into(), spec);
            }
            None => {
                if entry.platforms.remove("container").is_some() {
                    eprintln!("[manifest] {} 不在内置清单中，容器平台不提供安装后端", entry.id);
                }
            }
        }
    }
    manifest
}

pub async fn read_bundled_manifest(app: &AppHandle) -> Result<ManifestFile> {
    for path in manifest_candidate_paths(app) {
        if path.exists() {
            let content = tokio::fs::read_to_string(&path)
                .await
                .with_context(|| format!("read manifest {}", path.display()))?;
            return parse_manifest(&content).await;
        }
    }
    Err(anyhow!("software-manifest.json not found"))
}

async fn read_cached_manifest() -> Result<ManifestFile> {
    let content = tokio::fs::read_to_string(cache_path()).await?;
    parse_manifest(&content).await
}

async fn write_cached_manifest(content: &str) -> Result<()> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, content).await?;
    Ok(())
}

pub async fn load_effective_manifest(app: &AppHandle) -> Result<(ManifestFile, String)> {
    let bundled = read_bundled_manifest(app).await;
    if let Ok(cached) = read_cached_manifest().await {
        // 缓存里是远程内容：每次使用时都收敛容器策略，而不只是在 refresh 时。
        return Ok((apply_container_policy(cached, bundled.as_ref().ok()), "cache".into()));
    }
    Ok((bundled?, "bundled".into()))
}

#[tauri::command]
pub async fn refresh_manifest(app: AppHandle) -> Result<String, String> {
    let bundled = read_bundled_manifest(&app).await.map_err(|e| e.to_string())?;
    if let Some(url) = bundled._remote_url.clone() {
        let remote = HTTP
            .get(url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string());
        if let Ok(resp) = remote {
            let text = resp.text().await.map_err(|e| e.to_string())?;
            parse_manifest(&text).await.map_err(|e| e.to_string())?;
            let _ = write_cached_manifest(&text).await;
            return Ok("remote".into());
        }
    }
    if read_cached_manifest().await.is_ok() {
        Ok("cache".into())
    } else {
        Ok("bundled".into())
    }
}

#[tauri::command]
pub async fn get_manifest_source(app: AppHandle) -> Result<ManifestSource, String> {
    let (manifest, source) = load_effective_manifest(&app)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ManifestSource {
        source,
        version: manifest.version,
    })
}

/// 只有旧镜像（没有 v2 broker）才允许从远程刷新 root 安装脚本。
fn legacy_scripts_refresh_allowed(runtime: &super::broker::BrokerRuntime) -> bool {
    runtime.require(BROKER_API_VERSION).is_err()
}

#[tauri::command]
pub async fn refresh_scripts() -> Result<String, String> {
    if !std::path::Path::new("/.dockerenv").exists() {
        return Ok("skipped".into());
    }
    // broker v2 镜像里，安装策略与 legacy 脚本都以镜像内的受信版本为准：
    // 不再从远程 main 热更新 /opt/install-scripts（那些脚本随后会以 root 执行）。
    // 只有没有 v2 broker 的旧镜像，为兼容老架构才继续走 webclaw-scripts-updater。
    if !legacy_scripts_refresh_allowed(&super::broker::runtime().await) {
        return Ok("skipped:broker-v2".into());
    }

    let output = tokio::process::Command::new("sudo")
        .arg("/usr/local/bin/webclaw-scripts-updater")
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        let _ = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(concat!(
                r#"curl -fsSL --max-time 30 "#,
                r#""https://raw.githubusercontent.com/qhkly/webclaw-software-manager/main/preinstall-full-apps.json" "#,
                r#"-o /opt/preinstall-full-apps.json 2>/dev/null"#
            ))
            .output()
            .await;
        Ok("updated".into())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(format!("warn:{}", stderr.lines().last().unwrap_or("network error")))
    }
}

pub fn platform_entries(
    manifest: ManifestFile,
    platform: &str,
) -> Vec<(SoftwareEntry, super::software::PlatformSoftwareSpec)> {
    manifest
        .software
        .into_iter()
        .filter_map(|entry| {
            let spec = entry.platforms.get(platform).cloned();
            spec.and_then(|mut platform_spec| {
                if platform == "container" {
                    if let Some((app_id, _)) = platform_spec.broker_app() {
                        if app_id != entry.id || !is_broker_app(app_id) {
                            eprintln!(
                                "[manifest] 拒绝 broker 条目 {}：app_id {} 不在容器 broker 白名单或与条目 id 不一致",
                                entry.id, app_id
                            );
                            return None;
                        }
                    }
                    if let Some(broker_spec) = legacy_broker_spec(&entry.id, &platform_spec) {
                        platform_spec = broker_spec;
                    }
                    // OpenCode 是用户 NVM 里的 CLI，旧清单的 root .deb 脚本改走 user-node。
                    if entry.id == "opencode"
                        && matches!(&platform_spec.install, ActionSpec::CustomScript { script } if script == "/opt/install-scripts/install-opencode.sh")
                    {
                        platform_spec = PlatformSoftwareSpec {
                            detect: ActionSpec::NpmGlobal { pkg: "opencode-ai".into() },
                            latest: ActionSpec::NpmRegistry { pkg: "opencode-ai".into() },
                            install: ActionSpec::NpmGlobal { pkg: "opencode-ai".into() },
                            upgrade: None,
                        };
                    }
                    // Older manifests detect Codex through the ambient PATH, which
                    // may contain the system Node installation instead of ubuntu NVM.
                    if entry.id == "codex"
                        && matches!(&platform_spec.install, ActionSpec::NpmGlobal { .. })
                        && matches!(&platform_spec.detect, ActionSpec::Shell { .. })
                    {
                        platform_spec.detect = ActionSpec::NpmGlobal {
                            pkg: "@openai/codex".into(),
                        };
                    }
                }
                Some((entry, platform_spec))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 仓库发布的清单保持老客户端可读的旧格式；新架构完全由客户端 normalize 得到。
    const PUBLISHED: &str = include_str!("../../../software-manifest.json");
    const BROKER_IDS: [&str; 24] = [
        "audacity", "flameshot", "gimp", "wireshark", "blender", "vscode", "antigravity",
        "cc-switch", "dbeaver", "dockyard", "opentypeless", "trae", "wechat",
        "obsidian", "ghostty", "cursor", "webclaw-launcher", "intellij", "pycharm", "eclipse",
        "android-studio", "qq", "telegram", "discord",
    ];
    /// Docker 有 policy，但 broker 不支持可靠升级 / 客户端有专用升级逻辑：继续 legacy/special
    const LEGACY_IDS: [&str; 4] = [
        "webcode-ai-studio", "webclaw-software-manager", "hermes", "webcode-git-manager",
    ];
    const USER_NODE: [(&str, &str); 2] = [
        ("codex", "@openai/codex"),
        ("opencode", "opencode-ai"),
    ];

    fn container_specs(json: &str) -> HashMap<String, PlatformSoftwareSpec> {
        let manifest: ManifestFile = serde_json::from_str(json).unwrap();
        platform_entries(manifest, "container")
            .into_iter()
            .map(|(entry, spec)| (entry.id, spec))
            .collect()
    }

    fn assert_broker(spec: &PlatformSoftwareSpec, id: &str) {
        assert_eq!(spec.broker_app(), Some((id, 2)), "{} not routed to broker", id);
        for action in [&spec.detect, &spec.install, spec.upgrade.as_ref().unwrap()] {
            assert!(matches!(action, ActionSpec::Broker { app_id, .. } if app_id == id));
        }
        // latest：apt 类是只读的 apt-cache policy（包名来自硬编码白名单），其它由 broker/catalog 提供
        match &spec.latest {
            ActionSpec::Broker { app_id, .. } => assert_eq!(app_id, id),
            ActionSpec::AptPolicy { pkg } => {
                let expected = if id == "vscode" { "code" } else { id };
                assert_eq!(pkg, expected);
            }
            other => panic!("{} latest must be broker or apt-policy, got {:?}", id, other),
        }
    }

    fn spec_has_executable(spec: &PlatformSoftwareSpec, needle: &str) -> bool {
        let dump = format!("{:?}", spec);
        dump.contains(needle)
    }

    fn bundled() -> ManifestFile {
        serde_json::from_str(PUBLISHED).unwrap()
    }

    #[test]
    fn malicious_remote_manifest_cannot_change_container_policy() {
        let mut remote: ManifestFile = serde_json::from_str(PUBLISHED).unwrap();
        let evil_shell = ActionSpec::Shell { cmd: "touch /tmp/pwn".into(), version_regex: None };
        let evil_script = ActionSpec::CustomScript { script: "/tmp/evil.sh".into() };
        for entry in remote.software.iter_mut() {
            // 非执行元数据允许远程更新
            entry.desc = format!("remote desc {}", entry.id);
            if let Some(spec) = entry.platforms.get_mut("container") {
                spec.detect = evil_shell.clone();
                spec.latest = evil_shell.clone();
                spec.install = evil_script.clone();
                spec.upgrade = Some(evil_script.clone());
            }
        }
        // 远程新增的 app：只有远程版本
        let mut remote_only = remote.software[0].clone();
        remote_only.id = "remote-only-app".into();
        remote_only.platforms.clear();
        remote_only.platforms.insert(
            "container".into(),
            PlatformSoftwareSpec {
                detect: evil_shell.clone(),
                latest: evil_shell.clone(),
                install: evil_script.clone(),
                upgrade: None,
            },
        );
        remote.software.push(remote_only);

        let safe = apply_container_policy(remote, Some(&bundled()));
        assert!(safe.software.iter().any(|e| e.id == "codex" && e.desc == "remote desc codex"));
        let entries = platform_entries(safe.clone(), "container");
        let expected: HashMap<String, PlatformSoftwareSpec> = container_specs(PUBLISHED);
        assert_eq!(entries.len(), expected.len());
        for (entry, spec) in &entries {
            assert!(!spec_has_executable(spec, "/tmp/pwn"), "{} kept remote shell", entry.id);
            assert!(!spec_has_executable(spec, "/tmp/evil.sh"), "{} kept remote script", entry.id);
            assert_eq!(format!("{:?}", spec), format!("{:?}", expected[&entry.id]), "{}", entry.id);
        }
        // remote-only app：容器平台没有任何可执行后端，也不会出现在容器列表里
        let remote_only = safe.software.iter().find(|e| e.id == "remote-only-app").unwrap();
        assert!(!remote_only.platforms.contains_key("container"));
        assert!(!entries.iter().any(|(e, _)| e.id == "remote-only-app"));
        assert_broker(&expected["vscode"], "vscode");

        // macOS / Windows 暂保持远程原样
        let mut remote: ManifestFile = serde_json::from_str(PUBLISHED).unwrap();
        let claude = remote.software.iter_mut().find(|e| e.id == "claude-code").unwrap();
        claude.platforms.get_mut("macos").unwrap().detect = evil_shell.clone();
        let safe = apply_container_policy(remote, Some(&bundled()));
        let claude = safe.software.iter().find(|e| e.id == "claude-code").unwrap();
        assert!(spec_has_executable(&claude.platforms["macos"], "/tmp/pwn"));
    }

    #[test]
    fn container_policy_fails_closed_without_bundled_manifest() {
        let remote: ManifestFile = serde_json::from_str(PUBLISHED).unwrap();
        let safe = apply_container_policy(remote, None);
        assert!(platform_entries(safe.clone(), "container").is_empty());
        assert!(!platform_entries(safe, "macos").is_empty());
    }

    #[test]
    fn legacy_scripts_refresh_only_without_v2_broker() {
        let rt = |state, api| super::super::broker::BrokerRuntime {
            state,
            api_version: api,
            catalog_schema: None,
            message: None,
        };
        use super::super::broker::RuntimeState;
        assert!(!legacy_scripts_refresh_allowed(&rt(RuntimeState::Ok, Some(2))));
        assert!(!legacy_scripts_refresh_allowed(&rt(RuntimeState::Ok, Some(3))));
        assert!(legacy_scripts_refresh_allowed(&rt(RuntimeState::Outdated, None)));
        assert!(legacy_scripts_refresh_allowed(&rt(RuntimeState::Outdated, Some(1))));
        assert!(legacy_scripts_refresh_allowed(&rt(RuntimeState::Unauthorized, None)));
    }

    fn assert_user_node(spec: &PlatformSoftwareSpec, pkg: &str) {
        assert!(matches!(&spec.install, ActionSpec::NpmGlobal { pkg: p } if p == pkg));
        assert!(matches!(&spec.detect, ActionSpec::NpmGlobal { pkg: p } if p == pkg));
        assert!(spec.broker_app().is_none());
    }

    #[test]
    fn published_manifest_stays_old_client_compatible() {
        // 老客户端不认识 broker：远端老地址上绝不能出现它。
        let raw: serde_json::Value = serde_json::from_str(PUBLISHED).unwrap();
        fn no_broker(v: &serde_json::Value) {
            match v {
                serde_json::Value::Object(map) => {
                    assert_ne!(map.get("type").and_then(|t| t.as_str()), Some("broker"), "{}", v);
                    map.values().for_each(no_broker);
                }
                serde_json::Value::Array(items) => items.iter().for_each(no_broker),
                _ => {}
            }
        }
        no_broker(&raw);
        assert!(!PUBLISHED.contains("\"broker\""));
        // 每个条目的每个平台都有完整的 detect/latest/install（旧客户端 PlatformSoftwareSpec 的必填字段）
        for entry in raw["software"].as_array().unwrap() {
            for (platform, spec) in entry["platforms"].as_object().unwrap() {
                for key in ["detect", "latest", "install"] {
                    assert!(spec.get(key).is_some(), "{} {} missing {}", entry["id"], platform, key);
                }
            }
        }
        // 新客户端的严格解析不会丢任何条目
        let manifest: ManifestFile = serde_json::from_str(PUBLISHED).unwrap();
        assert_eq!(manifest.software.len(), raw["software"].as_array().unwrap().len());
    }

    #[test]
    fn published_manifest_normalizes_to_broker_and_user_node() {
        let specs = container_specs(PUBLISHED);
        for id in BROKER_IDS {
            assert_broker(&specs[id], id);
        }
        for (id, pkg) in USER_NODE {
            assert_user_node(&specs[id], pkg);
        }
        // claude-code：检测走用户 NVM npm，安装/升级走固定 sudo 脚本（兼容无 runner 的旧容器）
        let claude = &specs["claude-code"];
        assert!(claude.broker_app().is_none());
        assert!(matches!(&claude.detect, ActionSpec::NpmGlobal { pkg } if pkg == "@anthropic-ai/claude-code"));
        for action in [&claude.install, claude.upgrade.as_ref().unwrap()] {
            assert!(matches!(action, ActionSpec::CustomScript { script } if script == "/opt/install-scripts/install-claude-code.sh"));
        }
        for id in LEGACY_IDS {
            assert!(specs[id].broker_app().is_none(), "{} must stay legacy", id);
        }
        // 不再有任何 apt；剩下的 custom-script 只能是固定目录下的同名脚本。
        for (id, spec) in &specs {
            for action in [&spec.install].into_iter().chain(spec.upgrade.as_ref()) {
                assert!(!matches!(action, ActionSpec::Apt { .. }), "{}", id);
                if let ActionSpec::CustomScript { script } = action {
                    assert_eq!(*script, format!("/opt/install-scripts/install-{}.sh", id));
                }
            }
        }
    }

    #[test]
    fn legacy_manifest_keeps_non_broker_apps_on_legacy_scripts() {
        let specs = container_specs(PUBLISHED);
        // 非端到端 broker app 与 manager-only 工具：保持原来的受限脚本，不被提升。
        for &id in LEGACY_IDS.iter().chain(&["webstorm", "ipshield", "sshield", "webcode-i18n-manager"]) {
            assert!(specs[id].broker_app().is_none(), "{}", id);
            assert!(matches!(&specs[id].install, ActionSpec::CustomScript { .. }), "{}", id);
        }
    }

    #[test]
    fn unknown_remote_scripts_are_never_upgraded_to_broker() {
        let mut manifest: ManifestFile = serde_json::from_str(PUBLISHED).unwrap();
        for entry in manifest.software.iter_mut() {
            let spec = entry.platforms.get_mut("container").unwrap();
            match entry.id.as_str() {
                // 白名单 id，但脚本路径不是历史内置那一个
                "vscode" => spec.install = ActionSpec::CustomScript { script: "/tmp/evil.sh".into() },
                "wechat" => spec.install = ActionSpec::Apt { pkg: "wechat".into() },
                "gimp" => spec.install = ActionSpec::Apt { pkg: "gimp-evil".into() },
                // 白名单 id，install 正确但 upgrade 被换掉
                "dbeaver" => {
                    spec.upgrade = Some(ActionSpec::Shell { cmd: "curl x | sh".into(), version_regex: None })
                }
                // 新增 broker id：别的 app 的合法脚本路径 / Docker 侧路径 / 被换掉的 upgrade 都不行
                "qq" => {
                    spec.install = ActionSpec::CustomScript { script: "/opt/install-scripts/install-telegram.sh".into() }
                }
                "telegram" => spec.install = ActionSpec::CustomScript { script: "/opt/install-telegram.sh".into() },
                "discord" => {
                    spec.upgrade = Some(ActionSpec::CustomScript { script: "/opt/install-scripts/install-discord-v2.sh".into() })
                }
                "obsidian" => spec.upgrade = Some(ActionSpec::CustomScript { script: "/tmp/evil.sh".into() }),
                "cursor" => spec.install = ActionSpec::Shell { cmd: "bash /opt/install-scripts/install-cursor.sh".into(), version_regex: None },
                "intellij" => spec.install = ActionSpec::CustomScript { script: "/opt/install-scripts/../../tmp/evil.sh".into() },
                // 非白名单 id，即使符合命名规则
                "webstorm" => {}
                _ => {}
            }
        }
        let specs: HashMap<_, _> = platform_entries(manifest, "container")
            .into_iter()
            .map(|(entry, spec)| (entry.id, spec))
            .collect();
        for id in [
            "vscode", "wechat", "gimp", "dbeaver", "webstorm", "qq", "telegram", "discord", "obsidian",
            "cursor", "intellij",
        ] {
            assert!(specs[id].broker_app().is_none(), "{} escalated to broker", id);
        }
        // 未被篡改的照常 broker 化
        for id in ["audacity", "ghostty", "pycharm", "eclipse", "android-studio", "webclaw-launcher"] {
            assert_broker(&specs[id], id);
        }
    }

    fn entry(id: &str, container: &str) -> String {
        format!(
            r#"{{"id":"{}","name":"X","category":"c","group":"g","risk":"low","desc":"","platforms":{{"container":{}}}}}"#,
            id, container
        )
    }

    #[test]
    fn broker_spec_rejects_bad_app_id_and_unknown_actions_skip_entry() {
        let json = format!(
            r#"{{"version":"1","software":[{},{},{}]}}"#,
            entry("a", r#"{"install":{"type":"broker","app_id":"a;rm -rf /"}}"#),
            entry("b", r#"{"install":{"type":"future-thing"}}"#),
            entry("vscode", r#"{"install":{"type":"broker","app_id":"vscode"}}"#),
        );
        let manifest: ManifestFile = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest.software.len(), 1);
        let specs = container_specs(&json);
        assert_broker(&specs["vscode"], "vscode");
    }

    #[test]
    fn remote_broker_entry_cannot_point_at_another_root_app() {
        let json = format!(
            r#"{{"version":"1","software":[{},{},{},{}]}}"#,
            // 商品条目指向别的已知 root app
            entry("webcode-ai-studio", r#"{"install":{"type":"broker","app_id":"vscode"}}"#),
            entry("fake-product", r#"{"install":{"type":"broker","app_id":"wireshark"}}"#),
            // id 一致但不在 broker 白名单（没有 Docker broker policy / 不走统一内核）
            entry("webstorm", r#"{"install":{"type":"broker","app_id":"webstorm"}}"#),
            entry("gimp", r#"{"install":{"type":"broker","app_id":"gimp"}}"#),
        );
        let specs = container_specs(&json);
        assert_eq!(specs.len(), 1);
        assert_broker(&specs["gimp"], "gimp");
    }

    #[test]
    fn cached_codex_shell_detect_uses_nvm_npm() {
        let mut manifest: ManifestFile =
            serde_json::from_str(include_str!("../../../software-manifest.json")).unwrap();
        let codex = manifest
            .software
            .iter_mut()
            .find(|entry| entry.id == "codex")
            .unwrap();
        codex.platforms.get_mut("container").unwrap().detect = ActionSpec::Shell {
            cmd: "codex --version".into(),
            version_regex: None,
        };
        let entries = platform_entries(manifest, "container");
        let (_, spec) = entries
            .iter()
            .find(|(entry, _)| entry.id == "codex")
            .unwrap();
        assert!(matches!(&spec.detect, ActionSpec::NpmGlobal { pkg } if pkg == "@openai/codex"));
    }
}
