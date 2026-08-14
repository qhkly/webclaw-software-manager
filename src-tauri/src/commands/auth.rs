//! 登录：走 platform 的签名票据回调，和 webclaw-launcher-tauri、webcode-ai-studio
//! 是同一套流程（见 webclaw-platform/lib/launcher-callback.ts）。
//!
//! 流程：
//!   1. 本机随机端口起一个一次性 HTTP 监听
//!   2. 浏览器打开 platform 的 /login?callback=http://127.0.0.1:<port>/auth-callback&state=<csrf>
//!   3. platform 登录完成后带 auth_payload / auth_sig 跳回来
//!   4. 验签（RSA-SHA256，公钥内置）+ 校验 iss/aud/state/exp
//!   5. 拿 jti 去换正式 token，落到本地 auth.json
//!
//! 和 launcher 的差别只有两处：launcher 复用它常驻的 18791 本地 API 收回调，这里用
//! 一次性监听、收完就关；launcher 用 openssl 验签，这里用纯 Rust 的 rsa crate——
//! 本项目 CI 要出 5 种平台产物，openssl 会给交叉编译添麻烦。

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use once_cell::sync::Lazy;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Pkcs1v15Sign, RsaPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt;

const AUTH_CALLBACK_ISSUER: &str = "webclaw-platform";
/// 和 launcher 共用同一个 audience。platform 的 signLauncherCallbackPayload 里是写死的，
/// 不是每个客户端一个值，所以这里不能改成 webclaw-software-manager。
const AUTH_CALLBACK_AUDIENCE: &str = "webclaw-launcher";
const AUTH_CALLBACK_PUBLIC_KEY_PEM: &str = include_str!("../../auth-callback-public.pem");
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

pub fn platform_base_url() -> String {
    std::env::var("WEBCLAW_PLATFORM_URL")
        .unwrap_or_else(|_| "https://webclaw.qhkly.com".to_string())
}

pub fn store_base_url() -> String {
    std::env::var("WEBCLAW_STORE_URL").unwrap_or_else(|_| "https://store.qhkly.com".to_string())
}

static HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent("webclaw-software-manager/0.1")
        // 比 manifest 那个 5s 宽松：这条路径上有登录换票和权益查询，5s 太紧
        .timeout(Duration::from_secs(20))
        .build()
        .expect("build reqwest client")
});

/// 同一时间只允许一个登录流程，否则多个监听端口和 state 会互相干扰。
///
/// 用原子标志而不是 Mutex：MutexGuard 不是 Send，跨不过 tauri 命令里的 await。
static LOGIN_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// 出了作用域就把标志放掉，中途 `?` 提前返回也不会把登录卡死。
struct LoginGuard;

impl LoginGuard {
    fn acquire() -> Option<Self> {
        LOGIN_IN_PROGRESS
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| LoginGuard)
    }
}

impl Drop for LoginGuard {
    fn drop(&mut self) {
        LOGIN_IN_PROGRESS.store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthData {
    pub token: String,
    pub email: String,
    #[serde(default)]
    pub saved_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthStatus {
    pub logged_in: bool,
    pub email: Option<String>,
    /// token 的 exp（秒）。前端用来提示"登录即将过期"。
    pub expires_at: Option<u64>,
}

impl AuthStatus {
    fn logged_out() -> Self {
        Self {
            logged_in: false,
            email: None,
            expires_at: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct SignedAuthPayload {
    iss: String,
    aud: String,
    jti: String,
    state: String,
    iat: u64,
    exp: u64,
}

#[derive(Debug, Deserialize)]
struct JwtHeader {
    alg: String,
}

#[derive(Debug, Deserialize)]
struct TokenClaims {
    email: String,
    exp: u64,
}

#[derive(Debug, Deserialize)]
struct ExchangeTicketResponse {
    token: String,
    user: ExchangeTicketUser,
}

#[derive(Debug, Deserialize)]
struct ExchangeTicketUser {
    email: String,
}

// ---------------------------------------------------------------- 本地存储

pub fn auth_file_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("webclaw-software-manager")
        .join("auth.json")
}

pub fn clear_auth_data() -> Result<(), String> {
    let path = auth_file_path();
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("清除登录信息失败：{}", e))?;
    }
    Ok(())
}

fn save_auth_data(auth: &AuthData) -> Result<(), String> {
    let path = auth_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败：{}", e))?;
    }
    let content =
        serde_json::to_string_pretty(auth).map_err(|e| format!("序列化登录信息失败：{}", e))?;
    std::fs::write(&path, content).map_err(|e| format!("写入登录信息失败：{}", e))?;
    // token 是明文的，别让同机其他用户读到
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// 读取本地 token。签名无效或已过期的一律清掉并当作未登录——留着只会让后续请求
/// 拿 401，不如在这里就归零。
pub fn read_auth_data() -> Option<AuthData> {
    let content = std::fs::read_to_string(auth_file_path()).ok()?;
    let auth: AuthData = serde_json::from_str(&content).ok()?;
    match verify_token(&auth.token) {
        Ok(claims) if claims.exp > now_secs() => Some(auth),
        _ => {
            let _ = clear_auth_data();
            None
        }
    }
}

// ---------------------------------------------------------------- 验签

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

/// RSA-SHA256 验签，公钥是内置的那份。
fn verify_signature(message: &[u8], signature_b64: &str) -> Result<(), String> {
    let public_key = RsaPublicKey::from_public_key_pem(AUTH_CALLBACK_PUBLIC_KEY_PEM)
        .map_err(|e| format!("加载验签公钥失败：{}", e))?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature_b64.as_bytes())
        .map_err(|e| format!("解析签名失败：{}", e))?;
    let hashed = Sha256::digest(message);
    public_key
        .verify(Pkcs1v15Sign::new::<Sha256>(), &hashed, &signature)
        .map_err(|_| "签名校验失败".to_string())
}

/// 校验 platform 回调带回来的票据。
///
/// 五项都要过：签名、issuer、audience、state（防 CSRF）、时间窗。少任何一项，
/// 别人构造一个回调 URL 就能把任意账号塞进本地。
fn verify_signed_callback(
    payload_b64: &str,
    signature_b64: &str,
    expected_state: &str,
) -> Result<SignedAuthPayload, String> {
    verify_signature(payload_b64.as_bytes(), signature_b64)?;

    let payload_json = URL_SAFE_NO_PAD
        .decode(payload_b64.as_bytes())
        .map_err(|e| format!("解析登录票据失败：{}", e))?;
    let payload: SignedAuthPayload = serde_json::from_slice(&payload_json)
        .map_err(|e| format!("读取登录票据失败：{}", e))?;

    if payload.iss != AUTH_CALLBACK_ISSUER {
        return Err("登录票据签发方不符".into());
    }
    if payload.aud != AUTH_CALLBACK_AUDIENCE {
        return Err("登录票据受众不符".into());
    }
    if payload.jti.trim().is_empty() {
        return Err("登录票据缺少 jti".into());
    }
    if payload.state != expected_state {
        return Err("登录票据 state 校验失败".into());
    }
    let now = now_secs();
    // iat 放 60s 余量，容忍两端时钟小幅偏差
    if payload.exp <= now || payload.iat > now.saturating_add(60) {
        return Err("登录票据已过期或时间异常".into());
    }
    Ok(payload)
}

/// 校验换回来的 JWT。platform 签 JWT 用的是同一对密钥（见 webclaw-platform/lib/jwt.ts
/// 里 JWT_PRIVATE_KEY 回退到 AUTH_CALLBACK_SIGNING_PRIVATE_KEY），所以能用同一份公钥验。
fn verify_token(token: &str) -> Result<TokenClaims, String> {
    let segments: Vec<&str> = token.split('.').collect();
    if segments.len() != 3 {
        return Err("token 格式不正确".into());
    }
    let header: JwtHeader = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(segments[0].as_bytes())
            .map_err(|e| format!("解析 token header 失败：{}", e))?,
    )
    .map_err(|e| format!("读取 token header 失败：{}", e))?;
    if header.alg != "RS256" {
        return Err("不支持的 token 签名算法".into());
    }

    verify_signature(
        format!("{}.{}", segments[0], segments[1]).as_bytes(),
        segments[2],
    )?;

    serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(segments[1].as_bytes())
            .map_err(|e| format!("解析 token 载荷失败：{}", e))?,
    )
    .map_err(|e| format!("读取 token 载荷失败：{}", e))
}

// ---------------------------------------------------------------- 回调监听

fn decode_url_component(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                output.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(&input[i + 1..i + 3], 16) {
                    Ok(value) => {
                        output.push(value);
                        i += 3;
                    }
                    Err(_) => {
                        output.push(b'%');
                        i += 1;
                    }
                }
            }
            byte => {
                output.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn encode_url_component(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(byte as char)
            }
            _ => output.push_str(&format!("%{:02X}", byte)),
        }
    }
    output
}

fn parse_query_string(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let (key, value) = segment.split_once('=').unwrap_or((segment, ""));
            (decode_url_component(key), decode_url_component(value))
        })
        .collect()
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        body.as_bytes().len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn result_page(title: &str, detail: &str) -> String {
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{title}</title>\
         <div style=\"font:16px/1.6 -apple-system,system-ui,sans-serif;padding:48px;text-align:center\">\
         <h1 style=\"font-size:20px\">{title}</h1><p style=\"color:#666\">{detail}</p></div>"
    )
}

/// 读请求行，取出路径和查询串。只需要第一行，剩下的头直接丢掉。
fn read_request_target(stream: &TcpStream) -> Option<String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    line.split_whitespace().nth(1).map(|s| s.to_string())
}

/// 等浏览器把票据送回来。
///
/// 浏览器可能顺带请求 /favicon.ico 之类，所以拿不到 auth_payload 的请求一律回 404
/// 继续等，不能当成失败。
fn wait_for_callback(
    listener: TcpListener,
    expected_state: &str,
) -> Result<SignedAuthPayload, String> {
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("配置回调监听失败：{}", e))?;
    let deadline = Instant::now() + LOGIN_TIMEOUT;

    while Instant::now() < deadline {
        let (mut stream, _) = match listener.accept() {
            Ok(pair) => pair,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) => return Err(format!("接收登录回调失败：{}", e)),
        };

        let target = match read_request_target(&stream) {
            Some(target) => target,
            None => {
                respond(&mut stream, "400 Bad Request", "");
                continue;
            }
        };

        let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
        let params = parse_query_string(query);
        let (Some(payload), Some(signature)) =
            (params.get("auth_payload"), params.get("auth_sig"))
        else {
            respond(&mut stream, "404 Not Found", "");
            continue;
        };

        return match verify_signed_callback(payload, signature, expected_state) {
            Ok(verified) => {
                respond(
                    &mut stream,
                    "200 OK",
                    &result_page("登录成功", "可以关闭此页面，回到软件管理器。"),
                );
                Ok(verified)
            }
            Err(error) => {
                respond(
                    &mut stream,
                    "400 Bad Request",
                    &result_page("登录失败", &error),
                );
                Err(error)
            }
        };
    }
    Err("登录超时：5 分钟内没有完成浏览器登录".into())
}

fn random_state() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ---------------------------------------------------------------- 命令

#[tauri::command]
pub async fn auth_status() -> Result<AuthStatus, String> {
    Ok(match read_auth_data() {
        Some(auth) => AuthStatus {
            logged_in: true,
            expires_at: verify_token(&auth.token).ok().map(|claims| claims.exp),
            email: Some(auth.email),
        },
        None => AuthStatus::logged_out(),
    })
}

#[tauri::command]
pub async fn auth_logout() -> Result<AuthStatus, String> {
    clear_auth_data()?;
    Ok(AuthStatus::logged_out())
}

#[tauri::command]
pub async fn auth_login(app: AppHandle) -> Result<AuthStatus, String> {
    let _guard = LoginGuard::acquire().ok_or("已有一个登录流程在进行中")?;

    // 绑 0 端口让系统分配，拿到实际端口再拼 callback
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("无法启动本地回调监听：{}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("读取回调端口失败：{}", e))?
        .port();

    let state = random_state();
    let callback = format!("http://127.0.0.1:{}/auth-callback", port);
    let login_url = format!(
        "{}/login?callback={}&state={}",
        platform_base_url(),
        encode_url_component(&callback),
        encode_url_component(&state),
    );

    // 这个 URL 是我们自己拼的，不经 validate_external_url——那个白名单是给
    // 清单里的外部链接用的，而且它禁止带端口，登录回调地址本身也过不了。
    app.opener()
        .open_url(login_url, None::<&str>)
        .map_err(|e| format!("打开浏览器失败：{}。若当前环境没有浏览器，请在别处登录。", e))?;
    let _ = app.emit("auth-progress", "waiting_browser");

    let expected_state = state.clone();
    let payload =
        tauri::async_runtime::spawn_blocking(move || wait_for_callback(listener, &expected_state))
            .await
            .map_err(|e| format!("等待登录回调失败：{}", e))??;

    let _ = app.emit("auth-progress", "exchanging");

    let response = HTTP
        .post(format!(
            "{}/api/auth/exchange-launcher-ticket",
            platform_base_url()
        ))
        .json(&serde_json::json!({ "jti": payload.jti }))
        .send()
        .await
        .map_err(|e| format!("换取登录凭证失败：{}", e))?;
    if !response.status().is_success() {
        return Err(format!(
            "换取登录凭证失败：HTTP {}",
            response.status().as_u16()
        ));
    }
    let exchanged: ExchangeTicketResponse = response
        .json()
        .await
        .map_err(|e| format!("解析登录凭证失败：{}", e))?;

    let claims = verify_token(&exchanged.token)?;
    // token 里的 email 和响应体里的对不上，说明这不是同一个账号的凭证，不能存
    if claims.email != exchanged.user.email {
        return Err("登录凭证与账号信息不一致".into());
    }
    let auth = AuthData {
        token: exchanged.token,
        email: exchanged.user.email,
        saved_at: Some(now_secs()),
    };
    save_auth_data(&auth)?;

    Ok(AuthStatus {
        logged_in: true,
        email: Some(auth.email),
        expires_at: Some(claims.exp),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个签名结构正确但公钥对不上的票据：所有字段校验都该在验签这一步之前就失败。
    fn payload_b64(iss: &str, aud: &str, state: &str, exp: u64, iat: u64) -> String {
        let json = serde_json::json!({
            "iss": iss, "aud": aud, "jti": "test-jti", "state": state,
            "iat": iat, "exp": exp,
        });
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json).unwrap())
    }

    #[test]
    fn rejects_tampered_signature() {
        let now = now_secs();
        let payload = payload_b64(
            AUTH_CALLBACK_ISSUER,
            AUTH_CALLBACK_AUDIENCE,
            "s",
            now + 60,
            now,
        );
        // 长度对得上但内容是伪造的签名
        let fake_sig = URL_SAFE_NO_PAD.encode([0u8; 256]);
        assert!(verify_signed_callback(&payload, &fake_sig, "s").is_err());
    }

    #[test]
    fn rejects_malformed_signature() {
        let now = now_secs();
        let payload = payload_b64(
            AUTH_CALLBACK_ISSUER,
            AUTH_CALLBACK_AUDIENCE,
            "s",
            now + 60,
            now,
        );
        assert!(verify_signed_callback(&payload, "!!!not-base64!!!", "s").is_err());
    }

    #[test]
    fn token_rejects_wrong_algorithm() {
        // alg=none 是 JWT 的经典绕过，必须挡住
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let body = URL_SAFE_NO_PAD.encode(br#"{"email":"a@b.c","exp":9999999999}"#);
        assert!(verify_token(&format!("{}.{}.", header, body)).is_err());
    }

    #[test]
    fn token_rejects_malformed_input() {
        assert!(verify_token("").is_err());
        assert!(verify_token("a.b").is_err());
    }

    #[test]
    fn url_component_roundtrip() {
        let callback = "http://127.0.0.1:54321/auth-callback";
        assert_eq!(decode_url_component(&encode_url_component(callback)), callback);
    }

    #[test]
    fn query_string_parses_callback_params() {
        let params = parse_query_string("auth_payload=abc&auth_sig=d%2Bf&other=1");
        assert_eq!(params.get("auth_payload").map(String::as_str), Some("abc"));
        assert_eq!(params.get("auth_sig").map(String::as_str), Some("d+f"));
    }

    #[test]
    fn request_target_query_is_extracted() {
        let target = "/auth-callback?auth_payload=x&auth_sig=y";
        let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
        assert_eq!(query, "auth_payload=x&auth_sig=y");
    }

    #[test]
    fn state_is_unique_and_long_enough() {
        let a = random_state();
        assert_eq!(a.len(), 32);
        assert_ne!(a, random_state());
    }
}
