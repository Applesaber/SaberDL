// 网易云音乐二维码登录 (Web 模式 - 模拟浏览器)
//   2025 起 NetEase 对 weapi 加了 8821 风控 (需要行为验证码)
//   关键绕过 (调研自 chaunsin/netease-cloud-music 2026 实现):
//     ① 持久化 deviceId (32 字符 hex) 写入 cookie jar
//     ② 首页预热 GET / 让服务端下发 _ntes_nuid / NMTID 等匿名 cookies
//     ③ 二维码 URL 拼 chainId = v1_{deviceId}_web_login_{timestamp_ms}
//     ④ 完整浏览器 headers: sec-ch-ua / sec-fetch-* / Accept-Language
//   通过这 4 个改动让请求"看起来像 web 浏览器",降低 8821 触发率

use aes_gcm::aead::{OsRng, rand_core::RngCore};
use qrcode::{QrCode, render::unicode};
use reqwest::Client;
use reqwest::cookie::{CookieStore, Jar};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, sleep};

use crate::auth::AuthError;
use crate::auth::netease::{NeteaseCookies, save as save_cookies};
use crate::downloader::netease::weapi;

const HOME_URL: &str = "https://music.163.com/";
const UNIKEY_URL: &str = "https://music.163.com/weapi/login/qrcode/unikey";
const POLL_URL: &str = "https://music.163.com/weapi/login/qrcode/client/login";
const QR_URL_PREFIX: &str = "https://music.163.com/login?codekey=";
const TIMEOUT: Duration = Duration::from_secs(180);
const INTERVAL: Duration = Duration::from_secs(2);
const DEVICE_ID_FILE: &str = "netease_device_id.txt";

const NETEASE_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Deserialize)]
struct UnikeyResp {
    code: i64,
    #[serde(default)]
    unikey: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PollResp {
    code: i64,
}

pub async fn login_with_qrcode() -> Result<NeteaseCookies, AuthError> {
    let device_id = load_or_create_device_id().await;

    let jar = Arc::new(Jar::default());
    let home: reqwest::Url = HOME_URL.parse().unwrap();
    jar.add_cookie_str(
        &format!("deviceId={}; Domain=.music.163.com; Path=/", device_id),
        &home,
    );
    jar.add_cookie_str(
        &format!("os=pc; Domain=.music.163.com; Path=/"),
        &home,
    );
    jar.add_cookie_str(
        &format!("appver=2.10.18; Domain=.music.163.com; Path=/"),
        &home,
    );

    let client = build_client(Arc::clone(&jar))?;

    // 首页预热: 让服务端下发 _ntes_nuid / NMTID 等匿名 cookies
    let _ = client.get(HOME_URL).send().await;

    let unikey = fetch_unikey(&client).await?;

    let chain_id = generate_chain_id(&device_id);
    let login_url = format!("{}{}&chainId={}", QR_URL_PREFIX, unikey, chain_id);

    println!("\n{}", render_qrcode(&login_url)?);
    println!("请用网易云音乐手机 APP 扫码登录(超时 180 秒)\n");

    poll_until_login(&client, &unikey, &jar).await
}

// 生成 32 字符 hex deviceId,持久化到 ~/.config/saber-dl/netease_device_id.txt
// 同一台机器每次扫码用同一个 deviceId,模拟"同一台浏览器"
async fn load_or_create_device_id() -> String {
    let dir = match crate::auth::config_dir() {
        Ok(d) => d,
        Err(_) => return generate_device_id(),
    };
    let path: PathBuf = dir.join(DEVICE_ID_FILE);
    if let Ok(text) = tokio::fs::read_to_string(&path).await {
        let trimmed = text.trim();
        if trimmed.len() == 32 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return trimmed.to_string();
        }
    }
    let id = generate_device_id();
    let _ = tokio::fs::create_dir_all(&dir).await;
    let _ = tokio::fs::write(&path, &id).await;
    id
}

fn generate_device_id() -> String {
    let mut buf = [0u8; 16];
    OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

// chainId 格式: v1_{deviceId}_web_login_{毫秒时间戳}
// 来自 chaunsin/netease-cloud-music 2026 commit f811a4b 实现
fn generate_chain_id(device_id: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("v1_{}_web_login_{}", device_id, ts)
}

fn build_client(jar: Arc<Jar>) -> Result<Client, AuthError> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::REFERER,
        "https://music.163.com/".parse().unwrap(),
    );
    headers.insert(
        reqwest::header::ORIGIN,
        "https://music.163.com".parse().unwrap(),
    );
    headers.insert(
        reqwest::header::ACCEPT_LANGUAGE,
        "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap(),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        "application/json, text/plain, */*".parse().unwrap(),
    );
    // 浏览器 client hints + fetch metadata (Chrome 120 真实值)
    headers.insert(
        "sec-ch-ua",
        "\"Not_A Brand\";v=\"8\", \"Chromium\";v=\"120\", \"Google Chrome\";v=\"120\""
            .parse()
            .unwrap(),
    );
    headers.insert("sec-ch-ua-mobile", "?0".parse().unwrap());
    headers.insert("sec-ch-ua-platform", "\"Windows\"".parse().unwrap());
    headers.insert("sec-fetch-dest", "empty".parse().unwrap());
    headers.insert("sec-fetch-mode", "cors".parse().unwrap());
    headers.insert("sec-fetch-site", "same-origin".parse().unwrap());

    Ok(Client::builder()
        .user_agent(NETEASE_UA)
        .default_headers(headers)
        .cookie_provider(jar)
        .build()?)
}

async fn weapi_post(
    client: &Client,
    url: &str,
    payload_json: &str,
) -> Result<reqwest::Response, AuthError> {
    let payload = weapi::encrypt(payload_json);
    Ok(client
        .post(url)
        .form(&[
            ("params", payload.params.as_str()),
            ("encSecKey", payload.enc_sec_key.as_str()),
        ])
        .send()
        .await?)
}

async fn fetch_unikey(client: &Client) -> Result<String, AuthError> {
    let resp: UnikeyResp = weapi_post(client, UNIKEY_URL, r#"{"type":1}"#)
        .await?
        .json()
        .await?;
    if resp.code != 200 {
        return Err(AuthError::Api(resp.code));
    }
    resp.unikey.ok_or(AuthError::EmptyData)
}

fn render_qrcode(url: &str) -> Result<String, AuthError> {
    let code = QrCode::new(url.as_bytes()).map_err(|e| AuthError::Qr(e.to_string()))?;
    Ok(code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build())
}

async fn poll_until_login(
    client: &Client,
    unikey: &str,
    jar: &Arc<Jar>,
) -> Result<NeteaseCookies, AuthError> {
    let start = Instant::now();
    let mut prompted = false;
    let payload_json = format!(r#"{{"key":"{}","type":1}}"#, unikey);

    loop {
        if start.elapsed() > TIMEOUT {
            return Err(AuthError::Timeout);
        }

        let resp: PollResp = weapi_post(client, POLL_URL, &payload_json)
            .await?
            .json()
            .await?;

        match resp.code {
            803 => {
                let cookies = extract_cookies_from_jar(jar)?;
                save_cookies(&cookies).await?;
                println!("登录成功,网易云 Cookie 已保存");
                return Ok(cookies);
            }
            800 => return Err(AuthError::QrExpired),
            8821 => {
                return Err(AuthError::Crypto(
                    "网易云风控触发 (code=8821 需要行为验证码)\n  \
                     即使补了 deviceId/chainId/浏览器 headers,仍可能命中风控。\n  \
                     最稳方案: 改用 cookie 登录\n  \
                       1) 浏览器登录 music.163.com\n  \
                       2) F12 → Application → Cookies → 复制 MUSIC_U 值\n  \
                       3) saber-dl login netease --cookie \"MUSIC_U=xxx\""
                        .into(),
                ));
            }
            802 if !prompted => {
                eprintln!("已扫码,请在 APP 内确认...");
                prompted = true;
            }
            801 | 802 => {}
            other => eprintln!("[WARN] 网易云未知状态 code={other}"),
        }
        sleep(INTERVAL).await;
    }
}

fn extract_cookies_from_jar(jar: &Jar) -> Result<NeteaseCookies, AuthError> {
    let url: reqwest::Url = "https://music.163.com".parse().unwrap();
    let all = jar
        .cookies(&url)
        .map(|h| h.to_str().unwrap_or("").to_string())
        .unwrap_or_default();

    fn pick(all: &str, name: &str) -> Option<String> {
        all.split(';').map(|s| s.trim()).find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == name).then(|| v.to_string())
        })
    }

    Ok(NeteaseCookies {
        music_u: pick(&all, "MUSIC_U").ok_or(AuthError::MissingCookie("MUSIC_U"))?,
        csrf: pick(&all, "__csrf").unwrap_or_default(),
    })
}

// 从浏览器复制的 cookie 字符串直接登录(绕过 8821 风控)
//   接受格式:
//     "MUSIC_U=xxx"
//     "MUSIC_U=xxx; __csrf=yyy"
//     "随便其他=foo; MUSIC_U=xxx; __csrf=yyy; bar=baz"  (混杂也能解)
pub async fn login_with_cookie(cookie_str: &str) -> Result<NeteaseCookies, AuthError> {
    let mut music_u = None;
    let mut csrf = None;
    for kv in cookie_str.split(';') {
        if let Some((k, v)) = kv.trim().split_once('=') {
            match k.trim() {
                "MUSIC_U" => music_u = Some(v.trim().to_string()),
                "__csrf" => csrf = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    let music_u = music_u.ok_or(AuthError::MissingCookie("MUSIC_U"))?;

    let cookies = NeteaseCookies {
        music_u,
        csrf: csrf.unwrap_or_default(),
    };
    save_cookies(&cookies).await?;
    println!(
        "网易云 Cookie 已保存 ({} 字段)",
        if cookies.csrf.is_empty() {
            "MUSIC_U"
        } else {
            "MUSIC_U + __csrf"
        }
    );
    Ok(cookies)
}
