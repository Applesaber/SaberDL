use std::path::Path;
use async_trait::async_trait;
use serde::Deserialize;
use crate::auth::Cookies;
use crate::error::DownloadError;
use crate::downloader::Downloader;  // 使用 mod.rs 中定义的 trait

// ========== B站 API 响应结构 ==========
#[derive(Debug, Deserialize)]
pub struct BiliResponse<T> {
    pub code: i64,
    pub message: String,
    pub data: Option<T>,
}

#[derive(Debug, Deserialize)]
pub struct VideoInfo {
    pub title: String,
    pub bvid: String,
    pub aid: u64,
    pub owner: Owner,
    pub duration: i32,
    pub stat: Stat,
    pub pages: Vec<Page>,
}

#[derive(Debug, Deserialize)]
pub struct Owner {
    pub name: String,
    pub mid: u64,
}

#[derive(Debug, Deserialize)]
pub struct Stat {
    pub view: u64,
    pub danmaku: u64,
    #[serde(default)]
    pub like: u64,
}

#[derive(Debug, Deserialize)]
pub struct Page {
    pub page: i32,
    pub part: String,
    pub cid: u64,
    pub duration: i32,
}

// ========== BilibiliDownloader ==========
pub struct BilibiliDownloader {
    client: reqwest::Client,
}

impl Default for BilibiliDownloader {
    fn default() -> Self { Self::new(None) }
}

impl BilibiliDownloader {
    pub fn new(cookies: Option<Cookies>) -> Self {
        let mut builder = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
                         (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");

        if let Some(c) = cookies {
            let jar = std::sync::Arc::new(reqwest::cookie::Jar::default());
            let url: reqwest::Url = "https://www.bilibili.com".parse().unwrap();
            for (k, v) in [("SESSDATA", c.sessdata.as_str()),
                ("bili_jct", c.bili_jct.as_str()),
                ("DedeUserID", c.dedeuserid.as_str())]
            {
                jar.add_cookie_str(
                    &format!("{}={}; Domain=.bilibili.com", k, v),
                    &url,
                );
            }
            builder = builder.cookie_provider(jar);
        }

        Self { client: builder.build().expect("BilibiliDownloader: client build should not fail") }
    }

    // 内部辅助方法
    async fn fetch_video_info(&self, bvid: &str)
                              -> Result<VideoInfo, DownloadError>
    {
        let resp = self.client.get("https://api.bilibili.com/x/web-interface/view")
            .query(&[("bvid", bvid)])
            .send()
            .await?
            .error_for_status()?;

        let parsed: BiliResponse<VideoInfo> = resp.json().await?;

        if parsed.code != 0 {
            return Err(DownloadError::BiliApi(parsed.message, parsed.code));
        }
        parsed.data.ok_or(DownloadError::BiliApi("no data".into(), 0))
    }

    // 辅助函数：解析 BV 号
    fn parse_bvid_from_url(&self, url: &str) -> Result<String, DownloadError> {
        let parsed = url::Url::parse(url)
            .map_err(|e| DownloadError::UrlParse(e.to_string()))?;
        for seg in parsed.path_segments().into_iter().flatten() {
            if seg.starts_with("BV") && seg.len() == 12 {
                return Ok(seg.to_string());
            }
        }
        Err(DownloadError::UrlParse(format!("找不到 BV 号: {}", url)))
    }

    // 辅助函数：打印视频信息
    fn print_video_info(&self, info: &VideoInfo) {
        println!("════ B 站视频元信息 ════");
        println!("  标题: {}", info.title);
        println!("  BV:   {}", info.bvid);
        println!("  AV:   av{}", info.aid);
        println!("  UP:   {} (mid={})", info.owner.name, info.owner.mid);
        println!("  时长: {} 秒", info.duration);
        println!("  统计: {} 播放 · {} 弹幕 · {} 点赞",
                 info.stat.view, info.stat.danmaku, info.stat.like);
        println!("  分 P: {} 个", info.pages.len());
        for p in &info.pages {
            println!("    P{}: {} (cid={}, {} 秒)",
                     p.page, p.part, p.cid, p.duration);
        }
    }
}

// 实现 Downloader trait
#[async_trait]
impl Downloader for BilibiliDownloader {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("bilibili.com/video/") || url.contains("b23.tv/")
    }

    async fn fetch(
        &self,
        url: &str,
        _output: &Path,
        _jobs: usize,
    ) -> Result<u64, DownloadError> {
        let bvid = self.parse_bvid_from_url(url)?;
        let info = self.fetch_video_info(&bvid).await?;
        self.print_video_info(&info);
        // TODO: 实现实际下载逻辑
        Err(DownloadError::Other("Lesson 10 实装下载".into()))
    }
}