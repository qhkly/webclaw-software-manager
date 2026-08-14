use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

const ALLOWED_EXTERNAL_HOSTS: [&str; 3] = [
    "webclaw.qhkly.com",
    "ai-studio.qhkly.com",
    "store.qhkly.com",
];

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

fn validate_external_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "无效的外部链接".to_string())?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || !parsed
            .host_str()
            .map(|host| ALLOWED_EXTERNAL_HOSTS.contains(&host))
            .unwrap_or(false)
    {
        return Err("不允许打开此外部链接".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    validate_external_url(&url)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::validate_external_url;

    #[test]
    fn external_url_allowlist_accepts_product_sites() {
        assert!(
            validate_external_url("https://store.qhkly.com/products/webcode-ai-studio").is_ok()
        );
        assert!(validate_external_url("https://webclaw.qhkly.com").is_ok());
        assert!(validate_external_url("https://ai-studio.qhkly.com/pricing").is_ok());
    }

    #[test]
    fn external_url_allowlist_rejects_lookalikes_and_unsafe_schemes() {
        assert!(validate_external_url("http://store.qhkly.com/products/test").is_err());
        assert!(
            validate_external_url("https://store.qhkly.com.evil.example/products/test").is_err()
        );
        assert!(
            validate_external_url("https://store.qhkly.com@evil.example/products/test").is_err()
        );
        assert!(validate_external_url("file:///tmp/test").is_err());
    }
}
