use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

use super::software::SoftwareEntry;

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
    pub software: Vec<SoftwareEntry>,
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
    serde_json::from_str(content).context("parse software-manifest.json")
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
    if let Ok(cached) = read_cached_manifest().await {
        return Ok((cached, "cache".into()));
    }
    Ok((read_bundled_manifest(app).await?, "bundled".into()))
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

#[tauri::command]
pub async fn refresh_scripts() -> Result<String, String> {
    if !std::path::Path::new("/.dockerenv").exists() {
        return Ok("skipped".into());
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
            spec.map(|platform_spec| (entry, platform_spec))
        })
        .collect()
}

