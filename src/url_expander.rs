// URL 展开器 — 把一个「集合 URL」展开成 N 个「单项 URL」
//   网易云歌单/收藏夹  → N 个 song URL  (POST /weapi/v6/playlist/detail)
//   B 站收藏夹 (ml ID) → N 个 BV URL    (GET /x/v3/fav/resource/list,分页)
//   其他              → passthrough (自己 1 个)

use crate::error::DownloadError;

pub async fn expand(url: &str) -> Result<Vec<String>, DownloadError> {
    if let Some(id) = netease::match_playlist(url) {
        return netease::expand_playlist(id).await;
    }
    if let Some(id) = bilibili::match_favlist(url) {
        return bilibili::expand_favlist(id).await;
    }
    Ok(vec![url.to_string()])
}

mod netease {
    use reqwest::Client;
    use serde::Deserialize;

    use crate::downloader::netease::weapi;
    use crate::error::DownloadError;

    const PLAYLIST_DETAIL: &str = "https://music.163.com/weapi/v6/playlist/detail";

    #[derive(Debug, Deserialize)]
    struct PlaylistResp {
        code: i64,
        #[serde(default)]
        playlist: Option<Playlist>,
    }

    #[derive(Debug, Deserialize)]
    struct Playlist {
        #[serde(default)]
        name: String,
        #[serde(default, rename = "trackIds")]
        track_ids: Vec<TrackId>,
    }

    #[derive(Debug, Deserialize)]
    struct TrackId {
        id: i64,
    }

    pub fn match_playlist(url: &str) -> Option<i64> {
        if !url.contains("music.163.com") {
            return None;
        }
        let normalized = url.replace("/#/", "/");
        let parsed = url::Url::parse(&normalized).ok()?;
        let has_playlist = parsed.path_segments()?.any(|s| s == "playlist");
        if !has_playlist {
            return None;
        }
        for (k, v) in parsed.query_pairs() {
            if k == "id" {
                return v.parse::<i64>().ok();
            }
        }
        None
    }

    pub async fn expand_playlist(playlist_id: i64) -> Result<Vec<String>, DownloadError> {
        let client = Client::builder()
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .build()
            .map_err(DownloadError::Http)?;

        let payload_json = format!(r#"{{"id":{},"n":100000,"s":0}}"#, playlist_id);
        let payload = weapi::encrypt(&payload_json);

        let resp: PlaylistResp = client
            .post(PLAYLIST_DETAIL)
            .header("Referer", "https://music.163.com/")
            .form(&[
                ("params", payload.params.as_str()),
                ("encSecKey", payload.enc_sec_key.as_str()),
            ])
            .send()
            .await
            .map_err(DownloadError::Http)?
            .json()
            .await
            .map_err(DownloadError::Http)?;

        if resp.code != 200 {
            return Err(DownloadError::Other(format!(
                "网易云 playlist/detail 返回 code={}",
                resp.code
            )));
        }

        let pl = resp
            .playlist
            .ok_or_else(|| DownloadError::Other("playlist 字段缺失".into()))?;

        eprintln!("[歌单] 「{}」共 {} 首歌", pl.name, pl.track_ids.len());

        Ok(pl
            .track_ids
            .into_iter()
            .map(|t| format!("https://music.163.com/song?id={}", t.id))
            .collect())
    }
}

mod bilibili {
    use reqwest::Client;
    use serde::Deserialize;

    use crate::auth::bilibili as auth_bili;
    use crate::error::DownloadError;

    const FAV_RESOURCE_LIST: &str = "https://api.bilibili.com/x/v3/fav/resource/list";
    const PAGE_SIZE: u32 = 20;

    #[derive(Debug, Deserialize)]
    struct FavResp {
        code: i64,
        #[serde(default)]
        data: Option<FavData>,
    }

    #[derive(Debug, Deserialize)]
    struct FavData {
        #[serde(default)]
        info: Option<FavInfo>,
        #[serde(default)]
        medias: Vec<Media>,
        #[serde(default)]
        has_more: bool,
    }

    #[derive(Debug, Deserialize)]
    struct FavInfo {
        #[serde(default)]
        title: String,
    }

    #[derive(Debug, Deserialize)]
    struct Media {
        #[serde(default)]
        bvid: String,
    }

    // 匹配 /medialist/play/ml{id}/... 或 /medialist/detail/ml{id} 或 ?fid={id}
    pub fn match_favlist(url: &str) -> Option<i64> {
        if !url.contains("bilibili.com") {
            return None;
        }
        if let Some(pos) = url.find("/ml") {
            let after = &url[pos + 3..];
            let end = after
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after.len());
            if let Ok(id) = after[..end].parse::<i64>()
                && id > 0
            {
                return Some(id);
            }
        }
        let parsed = url::Url::parse(url).ok()?;
        for (k, v) in parsed.query_pairs() {
            if matches!(k.as_ref(), "fid" | "media_id")
                && let Ok(id) = v.parse::<i64>()
            {
                return Some(id);
            }
        }
        None
    }

    pub async fn expand_favlist(media_id: i64) -> Result<Vec<String>, DownloadError> {
        let cookies = auth_bili::load().await.ok().flatten();
        let mut builder = Client::builder().user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        );

        if let Some(c) = &cookies {
            let jar = std::sync::Arc::new(reqwest::cookie::Jar::default());
            let url: reqwest::Url = "https://www.bilibili.com".parse().unwrap();
            for (k, v) in [
                ("SESSDATA", c.sessdata.as_str()),
                ("bili_jct", c.bili_jct.as_str()),
                ("DedeUserID", c.dedeuserid.as_str()),
            ] {
                jar.add_cookie_str(&format!("{}={}; Domain=.bilibili.com", k, v), &url);
            }
            builder = builder.cookie_provider(jar);
        }

        let client = builder.build().map_err(DownloadError::Http)?;

        let mut all_bvids = Vec::new();
        let mut title = String::new();
        let mut pn = 1u32;
        loop {
            let resp: FavResp = client
                .get(FAV_RESOURCE_LIST)
                .query(&[
                    ("media_id", media_id.to_string()),
                    ("ps", PAGE_SIZE.to_string()),
                    ("pn", pn.to_string()),
                ])
                .send()
                .await
                .map_err(DownloadError::Http)?
                .json()
                .await
                .map_err(DownloadError::Http)?;

            if resp.code != 0 {
                return Err(DownloadError::Other(format!(
                    "B 站 fav/resource/list 返回 code={} (私密收藏夹需登录)",
                    resp.code
                )));
            }

            let data = resp
                .data
                .ok_or_else(|| DownloadError::Other("fav data 缺失".into()))?;

            if pn == 1 {
                if let Some(info) = data.info {
                    title = info.title;
                }
            }

            let has_more = data.has_more;
            let new_count = data.medias.len();
            for m in data.medias {
                if !m.bvid.is_empty() {
                    all_bvids.push(m.bvid);
                }
            }

            if !has_more || new_count == 0 {
                break;
            }
            pn += 1;
            if pn > 100 {
                eprintln!("[WARN] 收藏夹超过 2000 项,只取前 2000");
                break;
            }
        }

        eprintln!("[收藏夹] 「{}」共 {} 个视频", title, all_bvids.len());

        Ok(all_bvids
            .into_iter()
            .map(|b| format!("https://www.bilibili.com/video/{}", b))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn passthrough_unknown_url() {
        let result = expand("https://example.com/foo").await.unwrap();
        assert_eq!(result, vec!["https://example.com/foo".to_string()]);
    }

    #[test]
    fn match_netease_playlist_standard() {
        assert_eq!(
            netease::match_playlist("https://music.163.com/playlist?id=12345"),
            Some(12345)
        );
    }

    #[test]
    fn match_netease_playlist_fragment() {
        assert_eq!(
            netease::match_playlist("https://music.163.com/#/playlist?id=67890"),
            Some(67890)
        );
    }

    #[test]
    fn match_netease_playlist_not_a_playlist() {
        assert_eq!(
            netease::match_playlist("https://music.163.com/song?id=12345"),
            None
        );
    }

    #[test]
    fn match_bilibili_favlist_medialist_play() {
        assert_eq!(
            bilibili::match_favlist("https://www.bilibili.com/medialist/play/ml12345/BV1xx"),
            Some(12345)
        );
    }

    #[test]
    fn match_bilibili_favlist_medialist_detail() {
        assert_eq!(
            bilibili::match_favlist("https://www.bilibili.com/medialist/detail/ml67890"),
            Some(67890)
        );
    }

    #[test]
    fn match_bilibili_favlist_not_a_favlist() {
        assert_eq!(
            bilibili::match_favlist("https://www.bilibili.com/video/BV1xx411c7mu"),
            None
        );
    }
}
