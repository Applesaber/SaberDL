use qrcode::{QrCode, render::unicode};
use reqwest::Client;
use reqwest::cookie::{CookieStore, Jar};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, sleep};

use crate::auth::AuthError;
use crate::auth::bilibili::{BilibiliCookies, save as save_cookies};

const GENERATE_URL: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/generate";
const POLL_URL: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/poll";
const TIMEOUT: Duration = Duration::from_secs(180);
const INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Deserialize)]
struct BiliResp<T> {
    #[allow(dead_code)]
    code: i64,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct GenerateData {
    url: String,
    qrcode_key: String,
}

#[derive(Debug, Deserialize)]
struct PollData {
    code: i64,
}

pub async fn login_with_qrcode() -> Result<BilibiliCookies, AuthError> {
    let jar = Arc::new(Jar::default());
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
        .cookie_provider(Arc::clone(&jar))
        .build()?;

    let resp_gen: BiliResp<GenerateData> = client.get(GENERATE_URL).send().await?.json().await?;
    let g = resp_gen.data.ok_or(AuthError::EmptyData)?;

    println!("\n{}", render_qrcode(&g.url)?);
    println!("请用 B 站手机 APP 扫码登录(超时 180 秒)\n");

    poll_until_login(&client, &g.qrcode_key, &jar).await
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
    key: &str,
    jar: &Arc<Jar>,
) -> Result<BilibiliCookies, AuthError> {
    let start = Instant::now();
    let mut prompted = false;

    loop {
        if start.elapsed() > TIMEOUT {
            return Err(AuthError::Timeout);
        }

        let p: BiliResp<PollData> = client
            .get(POLL_URL)
            .query(&[("qrcode_key", key)])
            .send()
            .await?
            .json()
            .await?;
        let d = p.data.ok_or(AuthError::EmptyData)?;

        match d.code {
            0 => {
                let cookies = extract_cookies_from_jar(jar)?;
                save_cookies(&cookies).await?;
                println!("登录成功,Cookie 已保存");
                return Ok(cookies);
            }
            86038 => return Err(AuthError::QrExpired),
            86090 if !prompted => {
                eprintln!("已扫码,请在 APP 内确认...");
                prompted = true;
            }
            86090 | 86101 => {}
            other => eprintln!("[WARN] 未知状态 code={other}"),
        }
        sleep(INTERVAL).await;
    }
}

fn extract_cookies_from_jar(jar: &Jar) -> Result<BilibiliCookies, AuthError> {
    let url: reqwest::Url = "https://www.bilibili.com".parse().unwrap();
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

    Ok(BilibiliCookies {
        sessdata: pick(&all, "SESSDATA").ok_or(AuthError::MissingCookie("SESSDATA"))?,
        bili_jct: pick(&all, "bili_jct").ok_or(AuthError::MissingCookie("bili_jct"))?,
        dedeuserid: pick(&all, "DedeUserID").ok_or(AuthError::MissingCookie("DedeUserID"))?,
        refresh_token: None,
    })
}
