use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Platform {
    Container,
    Macos,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlatformInfo {
    pub platform: Platform,
    pub key: String,
    pub label: String,
    pub in_container: bool,
    pub os: String,
    pub arch: String,
}

pub fn detect_platform_sync() -> PlatformInfo {
    let in_container = std::path::Path::new("/.dockerenv").exists()
        || std::env::var("WEBCLAW_PLATFORM")
            .map(|v| v.eq_ignore_ascii_case("container"))
            .unwrap_or(false);

    let platform = if in_container {
        Platform::Container
    } else if cfg!(target_os = "macos") {
        Platform::Macos
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    };

    let (key, label) = match platform {
        Platform::Container => ("container", "容器内软件商店"),
        Platform::Macos => ("macos", "macOS 软件商店"),
        Platform::Windows => ("windows", "Windows 软件商店"),
        Platform::Linux => ("linux", "Linux 软件商店"),
    };

    PlatformInfo {
        platform,
        key: key.into(),
        label: label.into(),
        in_container,
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
    }
}

#[tauri::command]
pub async fn detect_platform() -> Result<PlatformInfo, String> {
    Ok(detect_platform_sync())
}
