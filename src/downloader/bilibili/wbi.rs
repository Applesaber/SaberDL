use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::error::DownloadError;

const NAV_URL: &str = "https://api.bilibili.com/x/web-interface/nav";

const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35,
    27, 43, 5, 49, 33, 9, 42, 19, 29, 28, 14, 39, 12, 38, 41, 13,
    37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4,
    22, 25, 54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

#[derive(Debug, Deserialize)]
struct NavData {
    wbi_img: WbiImg,
}

#[derive(Debug, Deserialize)]
struct WbiImg {
    img_url: String,
    sub_url: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]   // code 字段保留作为 API 文档
struct NavResp {
    code: i64,
    data: Option<NavData>,
}

pub struct WbiSigner {
    client: Client,
    cache: Arc<RwLock<Option<(String, Instant)>>>,
}

impl WbiSigner {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    /// 拿 mixin_key,缓存 23h
    pub async fn mixin_key(&self) -> Result<String, DownloadError> {
        if let Some((k, t)) = self.cache.read().await.clone() {
            if t.elapsed() < Duration::from_secs(23 * 3600) {
                return Ok(k);
            }
        }
        let mut w = self.cache.write().await;
        if let Some((k, t)) = w.clone() {
            if t.elapsed() < Duration::from_secs(23 * 3600) {
                return Ok(k);
            }
        }
        let new_key = self.fetch_mixin_key().await?;
        *w = Some((new_key.clone(), Instant::now()));
        Ok(new_key)
    }

    async fn fetch_mixin_key(&self) -> Result<String, DownloadError> {
        let resp: NavResp = self.client.get(NAV_URL).send().await?.json().await?;
        // nav 即使 code=-101(未登录)也会返回 wbi_img,所以不检查 code
        let data = resp.data.ok_or(DownloadError::BiliApi("no nav data".into(), 0))?;
        let img_key = strip_ext(&data.wbi_img.img_url);
        let sub_key = strip_ext(&data.wbi_img.sub_url);
        let raw = format!("{}{}", img_key, sub_key);
        Ok(MIXIN_KEY_ENC_TAB
            .iter()
            .take(32)
            .map(|&i| raw.as_bytes()[i] as char)
            .collect())
    }

    /// 给一组 query 参数加 wts + w_rid,返回完整 query string
    pub async fn sign(&self, mut params: Vec<(String, String)>)
                      -> Result<String, DownloadError>
    {
        let mixin = self.mixin_key().await?;
        let wts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        params.push(("wts".into(), wts.to_string()));
        params.sort_by(|a, b| a.0.cmp(&b.0));

        let query: String = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(&filter_chars(v))))
            .collect::<Vec<_>>()
            .join("&");

        let w_rid = format!("{:x}", md5::compute(format!("{}{}", query, mixin)));
        Ok(format!("{}&w_rid={}", query, w_rid))
    }
}

fn strip_ext(url: &str) -> &str {
    url.rsplit('/').next()
        .and_then(|f| f.split('.').next())
        .unwrap_or("")
}

fn filter_chars(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '!' | '\'' | '(' | ')' | '*'))
        .collect()
}