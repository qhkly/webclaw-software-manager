//! 权益：读 store 的 /api/licenses，知道"这个用户买了什么"。
//!
//! 客户端只跟 store 一个域打交道；store 内部会去 platform 合并后台补发的权益。
//! 这里不做任何价格/套餐的展示，也不拦安装——徽章是提示，不是授权校验。真正的
//! 校验由各软件自己启动时做。

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use super::auth::{clear_auth_data, read_auth_data, store_base_url};

static HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent("webclaw-software-manager/0.1")
        .timeout(Duration::from_secs(20))
        .build()
        .expect("build reqwest client")
});

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Entitlement {
    pub product_slug: String,
    #[serde(default)]
    pub plan_slug: Option<String>,
    #[serde(default)]
    pub license_key: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    pub status: String,
}

/// store 的响应是 camelCase，本地缓存和给前端的都用同一套字段名。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LicensesResponse {
    entitlements: Vec<RemoteEntitlement>,
    #[serde(default)]
    degraded: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteEntitlement {
    product_slug: String,
    #[serde(default)]
    plan_slug: Option<String>,
    #[serde(default)]
    license_key: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
    status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EntitlementSnapshot {
    pub entitlements: Vec<Entitlement>,
    /// 数据来自 "remote" 还是 "cache"，前端据此决定要不要标注"离线数据"
    pub source: String,
    /// unix 秒。缓存命中时是当初抓取的时间，不是现在。
    pub fetched_at: u64,
    /// store 侧没能合并到 platform 权益，结果可能不全
    pub degraded: bool,
}

fn cache_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("webclaw-software-manager")
        .join("entitlements-cache.json")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

fn read_cache() -> Option<EntitlementSnapshot> {
    let content = std::fs::read_to_string(cache_path()).ok()?;
    let mut snapshot: EntitlementSnapshot = serde_json::from_str(&content).ok()?;
    snapshot.source = "cache".to_string();
    Some(snapshot)
}

fn write_cache(snapshot: &EntitlementSnapshot) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(content) = serde_json::to_string_pretty(snapshot) {
        let _ = std::fs::write(&path, content);
    }
}

pub fn clear_entitlements_cache() {
    let _ = std::fs::remove_file(cache_path());
}

/// 拉取权益。
///
/// 和清单那边"缓存无条件优先"的策略不同：这里永远先打网络，只有失败才回退缓存，
/// 并且如实告诉前端数据是旧的。权益是会过期的东西，拿陈旧缓存当真会让刚续费的
/// 用户看到"未购买"。
#[tauri::command]
pub async fn refresh_entitlements() -> Result<Option<EntitlementSnapshot>, String> {
    let Some(auth) = read_auth_data() else {
        // 未登录不是错误，前端据此不显示任何权益徽章
        clear_entitlements_cache();
        return Ok(None);
    };

    let response = HTTP
        .get(format!("{}/api/licenses", store_base_url()))
        .bearer_auth(&auth.token)
        .send()
        .await;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return match read_cache() {
                Some(cached) => Ok(Some(cached)),
                None => Err(format!("获取权益失败：{}", error)),
            }
        }
    };

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        // token 失效了，别留着让后续每次请求都吃 401
        clear_auth_data()?;
        clear_entitlements_cache();
        return Ok(None);
    }
    if !response.status().is_success() {
        return match read_cache() {
            Some(cached) => Ok(Some(cached)),
            None => Err(format!("获取权益失败：HTTP {}", response.status().as_u16())),
        };
    }

    let body: LicensesResponse = response
        .json()
        .await
        .map_err(|e| format!("解析权益数据失败：{}", e))?;

    let snapshot = EntitlementSnapshot {
        entitlements: body
            .entitlements
            .into_iter()
            .map(|item| Entitlement {
                product_slug: item.product_slug,
                plan_slug: item.plan_slug,
                license_key: item.license_key,
                expires_at: item.expires_at,
                status: item.status,
            })
            .collect(),
        source: "remote".to_string(),
        fetched_at: now_secs(),
        degraded: body.degraded,
    };
    write_cache(&snapshot);
    Ok(Some(snapshot))
}

/// 不打网络，只看缓存。启动时先用它把界面点亮，再让 refresh_entitlements 覆盖。
#[tauri::command]
pub async fn get_cached_entitlements() -> Result<Option<EntitlementSnapshot>, String> {
    if read_auth_data().is_none() {
        return Ok(None);
    }
    Ok(read_cache())
}
