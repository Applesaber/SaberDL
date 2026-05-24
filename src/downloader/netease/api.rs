// 网易云业务 API 封装
//   song_url_v1: 拿 mp3/flac 直链 + 音质 + 文件大小
//   song_detail: 拿标题/歌手/专辑/封面 URL
//   parse_song_id_from_url: 从 URL 提取歌曲 id (支持 ?id= / #/song?id= 多种形式)

use reqwest::Client;
use serde::Deserialize;

use crate::auth::netease::NeteaseCookies;
use crate::downloader::netease::weapi;
use crate::error::DownloadError;

const SONG_URL_V1: &str = "https://music.163.com/weapi/song/enhance/player/url/v1";
const SONG_DETAIL: &str = "https://music.163.com/weapi/v3/song/detail";

#[derive(Debug, Deserialize)]
struct SongUrlResp {
    code: i64,
    #[serde(default)]
    data: Vec<SongUrlItem>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SongUrlItem {
    pub id: i64,
    #[serde(default)]
    pub url: Option<String>,
    pub br: i64,
    pub size: i64,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub level: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SongDetailResp {
    code: i64,
    #[serde(default)]
    songs: Vec<SongDetailItem>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SongDetailItem {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub ar: Vec<Artist>,
    #[serde(default)]
    pub al: Album,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Artist {
    pub name: String,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct Album {
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "picUrl")]
    pub pic_url: String,
}

// 音质等级 (网易云 song_url_v1 接受的 level 参数)
//   standard:  128 kbps mp3      (免费)
//   higher:    192 kbps mp3      (免费)
//   exhigh:    320 kbps mp3      (黑胶 VIP)
//   lossless:  FLAC               (黑胶 VIP)
//   hires:     Hi-Res FLAC        (黑胶 SVIP)
pub fn level_label(level: &str) -> &'static str {
    match level {
        "standard" => "128K",
        "higher" => "192K",
        "exhigh" => "320K",
        "lossless" => "FLAC",
        "hires" => "Hi-Res",
        _ => "未知",
    }
}

// 解析歌曲 ID
//   支持以下 URL 形式:
//     https://music.163.com/song?id=12345
//     https://music.163.com/#/song?id=12345     (fragment hash)
//     https://music.163.com/m/song?id=12345     (移动版)
//     https://music.163.com/song/12345          (RESTful 路径,少见)
pub fn parse_song_id_from_url(url: &str) -> Result<i64, DownloadError> {
    let normalized = url.replace("/#/", "/");

    let parsed =
        url::Url::parse(&normalized).map_err(|e| DownloadError::UrlParse(e.to_string()))?;

    for (k, v) in parsed.query_pairs() {
        if k == "id" {
            return v
                .parse::<i64>()
                .map_err(|e| DownloadError::UrlParse(format!("invalid song id: {e}")));
        }
    }

    for seg in parsed.path_segments().into_iter().flatten() {
        if let Ok(id) = seg.parse::<i64>() {
            return Ok(id);
        }
    }

    Err(DownloadError::UrlParse(format!("找不到歌曲 ID: {}", url)))
}

async fn weapi_post(
    client: &Client,
    url: &str,
    payload_json: &str,
) -> Result<reqwest::Response, DownloadError> {
    let payload = weapi::encrypt(payload_json);
    client
        .post(url)
        .form(&[
            ("params", payload.params.as_str()),
            ("encSecKey", payload.enc_sec_key.as_str()),
        ])
        .send()
        .await
        .map_err(DownloadError::Http)
}

pub async fn song_url_v1(
    client: &Client,
    cookies: Option<&NeteaseCookies>,
    song_id: i64,
    level: &str,
) -> Result<SongUrlItem, DownloadError> {
    let csrf = cookies.map(|c| c.csrf.as_str()).unwrap_or("");
    let payload_json = format!(
        r#"{{"ids":"[{}]","level":"{}","encodeType":"flac","csrf_token":"{}"}}"#,
        song_id, level, csrf
    );

    let resp: SongUrlResp = weapi_post(client, SONG_URL_V1, &payload_json)
        .await?
        .json()
        .await?;

    if resp.code != 200 {
        return Err(DownloadError::Other(format!(
            "网易云 song_url_v1 返回 code={}",
            resp.code
        )));
    }

    let item = resp
        .data
        .into_iter()
        .next()
        .ok_or_else(|| DownloadError::Other("song_url_v1 返回空 data,歌曲可能已下架".into()))?;

    if item.url.is_none() || item.url.as_deref() == Some("") {
        return Err(DownloadError::Other(format!(
            "歌曲无版权 / VIP 限制 (id={})",
            item.id
        )));
    }

    Ok(item)
}

pub async fn song_detail(
    client: &Client,
    cookies: Option<&NeteaseCookies>,
    song_id: i64,
) -> Result<SongDetailItem, DownloadError> {
    let csrf = cookies.map(|c| c.csrf.as_str()).unwrap_or("");
    let payload_json = format!(
        r#"{{"c":"[{{\"id\":{}}}]","csrf_token":"{}"}}"#,
        song_id, csrf
    );

    let resp: SongDetailResp = weapi_post(client, SONG_DETAIL, &payload_json)
        .await?
        .json()
        .await?;

    if resp.code != 200 {
        return Err(DownloadError::Other(format!(
            "网易云 song_detail 返回 code={}",
            resp.code
        )));
    }

    resp.songs
        .into_iter()
        .next()
        .ok_or_else(|| DownloadError::Other("song_detail 返回空 songs".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_id_standard_url() {
        assert_eq!(
            parse_song_id_from_url("https://music.163.com/song?id=12345").unwrap(),
            12345
        );
    }

    #[test]
    fn parse_id_fragment_url() {
        assert_eq!(
            parse_song_id_from_url("https://music.163.com/#/song?id=67890").unwrap(),
            67890
        );
    }

    #[test]
    fn parse_id_mobile_url() {
        assert_eq!(
            parse_song_id_from_url("https://music.163.com/m/song?id=999").unwrap(),
            999
        );
    }

    #[test]
    fn parse_id_path_form() {
        assert_eq!(
            parse_song_id_from_url("https://music.163.com/song/12345").unwrap(),
            12345
        );
    }

    #[test]
    fn parse_id_missing() {
        assert!(parse_song_id_from_url("https://music.163.com/").is_err());
    }

    #[test]
    fn parse_id_invalid_number() {
        assert!(parse_song_id_from_url("https://music.163.com/song?id=notanumber").is_err());
    }
}
