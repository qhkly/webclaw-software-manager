//! 容器内 root 软件的唯一出口：`/usr/local/bin/webclaw-app-admin`（broker v2）。
//!
//! Software Manager 只给 broker「高层动作 + app_id」，从不拼 shell、不传路径/包名。
//! 安装方式、包名、下载地址全部由镜像里 root 所有的 /opt/on-demand-apps/<id>.json
//! 与 runtime-catalog 决定。
//!
//! 调用走 sudoers 里唯一放行的受控入口 `sudo -n -- /usr/local/bin/webclaw-app-admin`
//! （与 webclaw-docker 的 webclaw-app-launcher 相同），不依赖任何宽泛的免密 sudo。
//! 若 Docker 侧改成 broker 自行提权，只需改 [`BROKER_PREFIX`]。

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::OnceCell;

pub const BROKER_PATH: &str = "/usr/local/bin/webclaw-app-admin";
/// 固定前缀：非交互 sudo，`--` 之后只有固定路径的 broker。
const BROKER_PREFIX: [&str; 4] = ["sudo", "-n", "--", BROKER_PATH];
pub const BROKER_API_VERSION: u32 = 2;
pub const RUNTIME_OUTDATED_MSG: &str = "WebClaw 镜像运行时过旧，需要一次性升级镜像";

/// 与 broker 自己的校验一致：小写字母数字开头，只含 [a-z0-9._-]，不含 ..，长度 ≤ 64。
static APP_ID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z0-9][a-z0-9._-]{0,63}$").unwrap());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerOp {
    ApiVersion,
    CatalogInfo,
    Status,
    Install,
    Upgrade,
    Uninstall,
}

impl BrokerOp {
    fn as_str(self) -> &'static str {
        match self {
            BrokerOp::ApiVersion => "api-version",
            BrokerOp::CatalogInfo => "catalog-info",
            BrokerOp::Status => "status",
            BrokerOp::Install => "install",
            BrokerOp::Upgrade => "upgrade",
            BrokerOp::Uninstall => "uninstall",
        }
    }

    fn takes_app_id(self) -> bool {
        !matches!(self, BrokerOp::ApiVersion | BrokerOp::CatalogInfo)
    }
}

pub fn validate_app_id(app_id: &str) -> Result<(), String> {
    if APP_ID_RE.is_match(app_id) && !app_id.contains("..") {
        Ok(())
    } else {
        Err(format!("非法 broker app_id：{:?}", app_id))
    }
}

/// 构造 broker argv。直接交给 execve，不经过 shell。
pub fn broker_argv(op: BrokerOp, app_id: Option<&str>) -> Result<Vec<String>, String> {
    let mut argv: Vec<String> = BROKER_PREFIX.iter().map(|s| (*s).to_string()).collect();
    argv.push(op.as_str().into());
    match (op.takes_app_id(), app_id) {
        (true, Some(id)) => {
            validate_app_id(id)?;
            argv.push(id.into());
        }
        (true, None) => return Err(format!("broker {} 需要 app_id", op.as_str())),
        (false, Some(_)) => return Err(format!("broker {} 不接受 app_id", op.as_str())),
        (false, None) => {}
    }
    Ok(argv)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Ok,
    /// 镜像里没有 broker，或 broker 是不认识 api-version 的 v1
    Outdated,
    /// broker 存在但 sudoers 没放行（镜像配置损坏或过旧）
    Unauthorized,
    /// 调用失败但原因不明，不要把它当成「过旧」
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrokerRuntime {
    pub state: RuntimeState,
    pub api_version: Option<u32>,
    pub catalog_schema: Option<u32>,
    pub message: Option<String>,
}

impl BrokerRuntime {
    fn failed(state: RuntimeState, message: String) -> Self {
        BrokerRuntime { state, api_version: None, catalog_schema: None, message: Some(message) }
    }

    /// 该 app 要求的 API 版本是否满足；不满足时返回给用户看的错误。
    pub fn require(&self, min_api_version: u32) -> Result<(), String> {
        match (&self.state, self.api_version) {
            (RuntimeState::Ok, Some(v)) if v >= min_api_version => Ok(()),
            (RuntimeState::Ok, Some(v)) => Err(format!(
                "{}（broker API v{}，此软件需要 v{}）",
                RUNTIME_OUTDATED_MSG, v, min_api_version
            )),
            _ => Err(self.message.clone().unwrap_or_else(|| RUNTIME_OUTDATED_MSG.into())),
        }
    }
}

fn last_line(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take(200)
        .collect()
}

/// 把 `api-version` 的执行结果归类。纯函数，便于测试。
pub fn classify_api_probe(
    broker_exists: bool,
    exit_ok: bool,
    stdout: &[u8],
    stderr: &[u8],
) -> BrokerRuntime {
    if !broker_exists {
        return BrokerRuntime::failed(
            RuntimeState::Outdated,
            format!("{}（缺少 {}）", RUNTIME_OUTDATED_MSG, BROKER_PATH),
        );
    }
    let err_line = last_line(stderr);
    if !exit_ok {
        if err_line.starts_with("sudo:") {
            return BrokerRuntime::failed(
                RuntimeState::Unauthorized,
                format!("{}（受控入口未授权：{}）", RUNTIME_OUTDATED_MSG, err_line),
            );
        }
        // v1 broker 不认识 api-version：「未知动作」或把空 app_id 判为非法，均退出码 2。
        return BrokerRuntime::failed(
            RuntimeState::Outdated,
            format!("{}（broker 不支持 api-version：{}）", RUNTIME_OUTDATED_MSG, err_line),
        );
    }
    let json: serde_json::Value = match serde_json::from_slice(stdout) {
        Ok(v) => v,
        Err(e) => {
            return BrokerRuntime::failed(
                RuntimeState::Error,
                format!("broker api-version 输出不是 JSON：{}", e),
            )
        }
    };
    let api_version = json.get("api_version").and_then(|v| v.as_u64()).map(|v| v as u32);
    let catalog_schema = json.get("catalog_schema").and_then(|v| v.as_u64()).map(|v| v as u32);
    match api_version {
        Some(v) if v >= BROKER_API_VERSION => BrokerRuntime {
            state: RuntimeState::Ok,
            api_version: Some(v),
            catalog_schema,
            message: None,
        },
        Some(v) => BrokerRuntime {
            state: RuntimeState::Outdated,
            api_version: Some(v),
            catalog_schema,
            message: Some(format!(
                "{}（broker API v{}，需要 v{}）",
                RUNTIME_OUTDATED_MSG, v, BROKER_API_VERSION
            )),
        },
        None => BrokerRuntime::failed(RuntimeState::Error, "broker api-version 缺少 api_version".into()),
    }
}

async fn run_capture(op: BrokerOp, app_id: Option<&str>) -> Result<std::process::Output, String> {
    let argv = broker_argv(op, app_id)?;
    Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("无法启动 broker：{}", e))
}

static RUNTIME: OnceCell<BrokerRuntime> = OnceCell::const_new();

/// 探测一次并缓存（镜像运行时在进程生命周期内不会变）。
pub async fn runtime() -> BrokerRuntime {
    RUNTIME
        .get_or_init(|| async {
            if !Path::new(BROKER_PATH).exists() {
                return classify_api_probe(false, false, b"", b"");
            }
            match run_capture(BrokerOp::ApiVersion, None).await {
                Ok(out) => classify_api_probe(true, out.status.success(), &out.stdout, &out.stderr),
                Err(e) => BrokerRuntime::failed(RuntimeState::Error, e),
            }
        })
        .await
        .clone()
}

/// `status <app_id>` 的解析结果。字段都可缺省，broker 给多少用多少。
///
/// Docker broker v2 的形状：
/// `{"installed":true,"installed_version":"1.2","catalog":{"version":"1.3","released_at":..,
///   "artifact_for_arch":true}|null,"update_available":true|false|null,"upgrade_via":"broker"}`
/// 也兼容扁平的 `latest_version` / `catalog_state`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BrokerStatus {
    pub installed: bool,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: Option<bool>,
    /// 该 app 是否出现在 broker 信任的 runtime-catalog 里
    pub in_catalog: bool,
    /// broker 直接给出的 catalog 状态（若有），否则由 catalog-info 推导
    pub catalog_state: Option<String>,
    /// broker / launcher / unsupported
    pub upgrade_via: Option<String>,
    pub supported: bool,
    pub message: Option<String>,
}

fn str_field(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(|x| x.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn parse_status(stdout: &[u8]) -> Result<BrokerStatus, String> {
    let v: serde_json::Value =
        serde_json::from_slice(stdout).map_err(|e| format!("broker status 输出不是 JSON：{}", e))?;
    if !v.is_object() {
        return Err("broker status 输出不是 JSON 对象".into());
    }
    let installed_version = str_field(&v, &["installed_version", "version"]);
    let installed = v
        .get("installed")
        .and_then(|x| x.as_bool())
        .unwrap_or(installed_version.is_some());
    let catalog = v.get("catalog").filter(|c| c.is_object());
    let latest_version = catalog
        .and_then(|c| str_field(c, &["version", "latest_version"]))
        .or_else(|| str_field(&v, &["latest_version", "latest"]));
    let catalog_state = str_field(&v, &["catalog_state"])
        .or_else(|| catalog.and_then(|c| str_field(c, &["state"])))
        .or_else(|| v.get("catalog").and_then(|c| c.as_str()).map(String::from));
    Ok(BrokerStatus {
        installed,
        installed_version: if installed { installed_version } else { None },
        // 只看 catalog 对象：扁平的 latest_version（例如 apt 候选版本）不代表收录于 runtime-catalog。
        in_catalog: catalog.is_some(),
        latest_version,
        update_available: v.get("update_available").and_then(|x| x.as_bool()),
        catalog_state,
        upgrade_via: str_field(&v, &["upgrade_via"]),
        supported: v.get("supported").and_then(|x| x.as_bool()).unwrap_or(true),
        message: str_field(&v, &["message", "error"]),
    })
}

/// 超过这个时间没有成功刷新 catalog 就算陈旧；与 Docker broker 的 catalog_state（24 小时）一致。
const CATALOG_STALE_HOURS: i64 = 24;

/// 由 `catalog-info` 推导整体 catalog 状态：
/// missing（镜像里还没有 cache）/ untrusted（cache 不可信被忽略）/
/// offline（最近一次拉取失败，正在用旧 cache）/ stale（太久没成功刷新）/ fresh。
pub fn catalog_state_from_info(info: &serde_json::Value, now: chrono::DateTime<chrono::Utc>) -> String {
    let flag = |k: &str| info.get(k).and_then(|x| x.as_bool());
    let time = |k: &str| {
        info.get(k)
            .and_then(|x| x.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&chrono::Utc))
    };
    if flag("cache_present") == Some(false) {
        return "missing".into();
    }
    if flag("cache_trusted") == Some(false) {
        return "untrusted".into();
    }
    let success = time("last_success_at");
    let error = time("last_error_at");
    if let Some(err) = error {
        if success.map_or(true, |ok| err > ok) {
            return "offline".into();
        }
    }
    match success.or_else(|| time("generated_at")) {
        Some(ok) if now - ok > chrono::Duration::hours(CATALOG_STALE_HOURS) => "stale".into(),
        Some(_) => "fresh".into(),
        None => "unknown".into(),
    }
}

/// 单个 app 的 catalog 状态：app 不在 catalog 里时，整体正常则报 not_in_catalog。
pub fn item_catalog_state(status: &BrokerStatus, global: Option<&str>) -> Option<String> {
    if let Some(state) = &status.catalog_state {
        return Some(state.clone());
    }
    match (status.in_catalog, global) {
        (true, g) => g.map(String::from),
        (false, Some("fresh" | "stale" | "offline")) => Some("not_in_catalog".into()),
        (false, g) => g.map(String::from),
    }
}

pub async fn status(app_id: &str) -> Result<BrokerStatus, String> {
    let out = run_capture(BrokerOp::Status, Some(app_id)).await?;
    // 即使退出码非 0，broker 也可能在 stdout 给出结构化状态（例如 unsupported）。
    match parse_status(&out.stdout) {
        Ok(s) => Ok(s),
        Err(e) if out.status.success() => Err(e),
        Err(_) => Err(format!(
            "broker status 失败（退出码 {}）：{}",
            out.status.code().unwrap_or(-1),
            last_line(&out.stderr)
        )),
    }
}

pub async fn catalog_info() -> Result<serde_json::Value, String> {
    let out = run_capture(BrokerOp::CatalogInfo, None).await?;
    if !out.status.success() {
        return Err(format!("broker catalog-info 失败：{}", last_line(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("broker catalog-info 输出不是 JSON：{}", e))
}

/// 由 broker 状态得出界面状态。
pub fn state_from_status(s: &BrokerStatus) -> String {
    if !s.supported {
        return "unsupported".into();
    }
    if !s.installed {
        return "not_installed".into();
    }
    let state = match s.update_available {
        Some(true) => "upgradable".to_string(),
        Some(false) if s.latest_version.is_some() => "up_to_date".into(),
        _ => super::software::compute_state(
            Some(s.installed_version.as_deref().unwrap_or("installed")),
            s.latest_version.as_deref(),
        ),
    };
    // 有新版本但 broker 不负责升级（例如要走 launcher），不能给出一个点了必然失败的「更新」按钮。
    if state == "upgradable" && s.upgrade_via.as_deref().map_or(false, |via| via != "broker") {
        return "unsupported".into();
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_is_fixed_prefix_plus_op_and_validated_id() {
        assert_eq!(
            broker_argv(BrokerOp::Install, Some("vscode")).unwrap(),
            vec!["sudo", "-n", "--", BROKER_PATH, "install", "vscode"]
        );
        assert_eq!(
            broker_argv(BrokerOp::Upgrade, Some("webclaw-launcher")).unwrap(),
            vec!["sudo", "-n", "--", BROKER_PATH, "upgrade", "webclaw-launcher"]
        );
        assert_eq!(
            broker_argv(BrokerOp::Status, Some("cc-switch")).unwrap(),
            vec!["sudo", "-n", "--", BROKER_PATH, "status", "cc-switch"]
        );
        assert_eq!(
            broker_argv(BrokerOp::ApiVersion, None).unwrap(),
            vec!["sudo", "-n", "--", BROKER_PATH, "api-version"]
        );
    }

    #[test]
    fn argv_rejects_injection_and_option_smuggling() {
        for bad in [
            "",
            "vscode; rm -rf /",
            "$(id)",
            "`id`",
            "vs code",
            "-u",
            "--help",
            "../etc",
            "a..b",
            "VSCode",
            "vscode\n",
            "x/../../bin/sh",
            &"a".repeat(65),
        ] {
            for op in [BrokerOp::Install, BrokerOp::Upgrade, BrokerOp::Status, BrokerOp::Uninstall] {
                assert!(broker_argv(op, Some(bad)).is_err(), "accepted {:?}", bad);
            }
        }
        assert!(broker_argv(BrokerOp::Install, None).is_err());
        assert!(broker_argv(BrokerOp::ApiVersion, Some("vscode")).is_err());
        // sudo 只出现在固定前缀里，argv 永远不含 shell。
        let argv = broker_argv(BrokerOp::Install, Some("vscode")).unwrap();
        assert_eq!(argv.iter().filter(|a| *a == "sudo").count(), 1);
        assert!(!argv.iter().any(|a| a == "bash" || a == "sh" || a == "-c"));
    }

    #[test]
    fn api_probe_classification() {
        let ok = classify_api_probe(true, true, br#"{"api_version":2,"catalog_schema":1}"#, b"");
        assert_eq!(ok.state, RuntimeState::Ok);
        assert_eq!(ok.api_version, Some(2));
        assert_eq!(ok.catalog_schema, Some(1));
        assert!(ok.require(2).is_ok());
        assert!(ok.require(3).unwrap_err().contains(RUNTIME_OUTDATED_MSG));

        let newer = classify_api_probe(true, true, br#"{"api_version":3,"catalog_schema":1}"#, b"");
        assert_eq!(newer.state, RuntimeState::Ok);

        let missing = classify_api_probe(false, false, b"", b"");
        assert_eq!(missing.state, RuntimeState::Outdated);
        assert!(missing.require(2).unwrap_err().contains(RUNTIME_OUTDATED_MSG));

        let v1 = classify_api_probe(true, false, b"", "[webclaw-app-admin] 拒绝：非法 app_id：\n".as_bytes());
        assert_eq!(v1.state, RuntimeState::Outdated);
        assert!(v1.message.unwrap().contains(RUNTIME_OUTDATED_MSG));

        let old_api = classify_api_probe(true, true, br#"{"api_version":1}"#, b"");
        assert_eq!(old_api.state, RuntimeState::Outdated);
        assert!(old_api.require(2).is_err());

        let no_sudo = classify_api_probe(true, false, b"", b"sudo: a password is required\n");
        assert_eq!(no_sudo.state, RuntimeState::Unauthorized);
        assert!(no_sudo.require(2).is_err());

        let garbage = classify_api_probe(true, true, b"hello", b"");
        assert_eq!(garbage.state, RuntimeState::Error);
    }

    #[test]
    fn catalog_info_states() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-25T00:00:00Z").unwrap().with_timezone(&chrono::Utc);
        let st = |j: &str| catalog_state_from_info(&serde_json::from_str(j).unwrap(), now);
        assert_eq!(st(r#"{"cache_present":false,"cache_trusted":false}"#), "missing");
        assert_eq!(st(r#"{"cache_present":true,"cache_trusted":false}"#), "untrusted");
        assert_eq!(
            st(r#"{"cache_present":true,"cache_trusted":true,"last_success_at":"2026-09-24T00:00:00Z","last_error_at":"2026-09-24T12:00:00Z"}"#),
            "offline"
        );
        assert_eq!(
            st(r#"{"cache_present":true,"cache_trusted":true,"last_success_at":"2026-09-24T12:00:00Z","last_error_at":"2026-09-24T00:00:00Z"}"#),
            "fresh"
        );
        assert_eq!(
            st(r#"{"cache_present":true,"cache_trusted":true,"last_success_at":"2026-09-01T00:00:00Z","last_error_at":null}"#),
            "stale"
        );
        // 与 Docker 一致的 24 小时阈值
        assert_eq!(st(r#"{"cache_present":true,"cache_trusted":true,"last_success_at":"2026-09-24T01:00:00Z"}"#), "fresh");
        assert_eq!(st(r#"{"cache_present":true,"cache_trusted":true,"last_success_at":"2026-09-23T23:00:00Z"}"#), "stale");
    }

    #[test]
    fn status_parsing_and_state() {
        let s = parse_status(
            br#"{"app_id":"vscode","installed":true,"installed_version":"1.90.0","latest_version":"1.91.0","update_available":true,"catalog_state":"fresh"}"#,
        )
        .unwrap();
        assert_eq!(state_from_status(&s), "upgradable");
        assert_eq!(s.catalog_state.as_deref(), Some("fresh"));

        let s = parse_status(br#"{"installed":true,"version":"1.91.0","latest":"1.91.0","update_available":false}"#).unwrap();
        assert_eq!(state_from_status(&s), "up_to_date");

        // 目录离线：已安装但没有 latest，不能显示成「未安装」。
        let s = parse_status(br#"{"installed":true,"installed_version":"2.0","catalog":{"state":"offline"}}"#).unwrap();
        assert_eq!(state_from_status(&s), "unknown");
        assert_eq!(s.catalog_state.as_deref(), Some("offline"));

        let s = parse_status(br#"{"installed":false,"latest_version":"3.0"}"#).unwrap();
        assert_eq!(state_from_status(&s), "not_installed");

        let s = parse_status(r#"{"installed":false,"supported":false,"message":"仅支持 amd64"}"#.as_bytes()).unwrap();
        assert_eq!(state_from_status(&s), "unsupported");
        assert_eq!(s.message.as_deref(), Some("仅支持 amd64"));

        // 没有版本号的已安装软件 + 有 latest：按 installed 处理为可升级
        let s = parse_status(br#"{"installed":true,"latest_version":"3.0"}"#).unwrap();
        assert_eq!(state_from_status(&s), "upgradable");

        // Docker v2 的嵌套形状：latest 来自 catalog.version
        let s = parse_status(
            br#"{"app_id":"cc-switch","install_method":"github_release","arch":"amd64","installed":true,
                "installed_version":"3.20.3","installed_version_source":"dpkg",
                "catalog":{"version":"3.20.4","released_at":null,"artifact_for_arch":true},
                "update_available":true,"upgrade_via":"broker"}"#,
        )
        .unwrap();
        assert_eq!(s.latest_version.as_deref(), Some("3.20.4"));
        assert!(s.in_catalog);
        assert_eq!(state_from_status(&s), "upgradable");
        assert_eq!(item_catalog_state(&s, Some("stale")).as_deref(), Some("stale"));

        // 扁平 latest_version（例如 apt 候选版本）不算收录于 catalog
        let s = parse_status(br#"{"installed":true,"installed_version":"1.0","latest_version":"1.1","catalog":null}"#).unwrap();
        assert!(!s.in_catalog);
        assert_eq!(s.latest_version.as_deref(), Some("1.1"));
        assert_eq!(state_from_status(&s), "upgradable");
        assert_eq!(item_catalog_state(&s, Some("fresh")).as_deref(), Some("not_in_catalog"));

        // 不在 catalog：update_available null，已安装 → unknown，并标 not_in_catalog / missing
        let s = parse_status(br#"{"installed":true,"installed_version":"1.0","catalog":null,"update_available":null,"upgrade_via":"broker"}"#).unwrap();
        assert_eq!(s.latest_version, None);
        assert_eq!(state_from_status(&s), "unknown");
        assert_eq!(item_catalog_state(&s, Some("fresh")).as_deref(), Some("not_in_catalog"));
        assert_eq!(item_catalog_state(&s, Some("missing")).as_deref(), Some("missing"));

        // 可升级但要走 launcher：不给必然失败的更新按钮
        let s = parse_status(br#"{"installed":true,"installed_version":"1.0","catalog":{"version":"2.0"},"update_available":true,"upgrade_via":"launcher"}"#).unwrap();
        assert_eq!(state_from_status(&s), "unsupported");

        assert!(parse_status(b"not json").is_err());
        assert!(parse_status(b"[]").is_err());
    }
}
