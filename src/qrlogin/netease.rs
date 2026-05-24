// 网易云音乐二维码登录
//   1. POST /weapi/login/qrcode/unikey       (weapi 加密 {type:1}) → unikey
//   2. 终端打印二维码 (URL = music.163.com/login?codekey={unikey})
//   3. 轮询 POST /weapi/login/qrcode/client/login (weapi 加密 {key:unikey,type:1})
//      状态码: 800=过期 | 801=等待扫码 | 802=已扫码等确认 | 803=授权成功
//   4. 803 时从 cookie jar 提取 MUSIC_U + __csrf,加密保存

use qrcode::{QrCode, render::unicode};
use reqwest::Client;
use reqwest::cookie::{CookieStore, Jar};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, sleep};

use crate::auth::AuthError;
use crate::auth::netease::{NeteaseCookies, save as save_cookies};
use crate::downloader::netease::weapi;

const UNIKEY_URL: &str = "https://music.163.com/weapi/login/qrcode/unikey";
const POLL_URL: &str = "https://music.163.com/weapi/login/qrcode/client/login";
const QR_URL_PREFIX: &str = "https://music.163.com/login?codekey=";
const TIMEOUT: Duration = Duration::from_secs(180);
const INTERVAL: Duration = Duration::from_secs(2);

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
    let jar = Arc::new(Jar::default());
    let client = build_client(Arc::clone(&jar))?;

    let unikey = fetch_unikey(&client).await?;

    let login_url = format!("{}{}", QR_URL_PREFIX, unikey);
    println!("\n{}", render_qrcode(&login_url)?);
    println!("请用网易云音乐手机 APP 扫码登录(超时 180 秒)\n");

    poll_until_login(&client, &unikey, &jar).await
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
                     原因: 非浏览器 TLS 指纹被识别。建议改用 cookie 登录:\n  \
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
    println!("网易云 Cookie 已保存 ({} 字段)", if cookies.csrf.is_empty() { "MUSIC_U" } else { "MUSIC_U + __csrf" });
    Ok(cookies)
}
