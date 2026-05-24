use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::Deserialize;
use tokio::process::Command;

use crate::auth::Cookies;
use crate::downloader::{Downloader, FetchOutcome, download_with_client};
use crate::error::DownloadError;
use crate::wbi::WbiSigner;

const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

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

// playurl API + DASH 结构
#[derive(Debug, Deserialize)]
pub struct PlayUrlData {
    pub accept_quality: Vec<i32>,
    pub accept_description: Vec<String>,
    pub dash: DashRoot,
}

#[derive(Debug, Deserialize)]
pub struct DashRoot {
    pub duration: u32,
    pub video: Vec<DashStream>,
    pub audio: Vec<DashStream>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DashStream {
    pub id: i32,
    pub codecs: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    pub bandwidth: u64,
    #[serde(rename = "baseUrl")]
    pub base_url: String,
}

// ========== 文件名工具 ==========

// Windows + Unix 兼容的文件名净化:替换 / \ : * ? " < > | 及控制字符为 _,trim 末尾点/空格,限长 150 字符
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c == ' ');
    let limited: String = trimmed.chars().take(150).collect();
    if limited.is_empty() {
        "untitled".to_string()
    } else {
        limited
    }
}

fn quality_label(height: u32) -> &'static str {
    match height {
        h if h >= 4320 => "8K",
        h if h >= 2160 => "4K",
        h if h >= 1440 => "2K",
        h if h >= 1080 => "1080P",
        h if h >= 720 => "720P",
        h if h >= 480 => "480P",
        h if h >= 360 => "360P",
        _ => "240P",
    }
}

// ========== ffmpeg ==========
async fn merge_with_ffmpeg(video: &Path, audio: &Path, output: &Path) -> Result<(), DownloadError> {
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "warning",
            "-i",
            video.to_str().unwrap(),
            "-i",
            audio.to_str().unwrap(),
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            output.to_str().unwrap(),
        ])
        .status()
        .await
        .map_err(|e| DownloadError::Ffmpeg(format!("spawn 失败: {e}")))?;

    if !status.success() {
        return Err(DownloadError::Ffmpeg(format!(
            "合并失败,退出码 {:?}",
            status.code()
        )));
    }
    Ok(())
}

async fn check_ffmpeg() -> Result<(), DownloadError> {
    let r = Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    match r {
        Ok(s) if s.success() => Ok(()),
        _ => Err(DownloadError::Ffmpeg(
            "未检测到 ffmpeg。请安装:\n  \
             Arch:    sudo pacman -S ffmpeg\n  \
             Debian:  sudo apt install ffmpeg\n  \
             macOS:   brew install ffmpeg\n  \
             Windows: 下载 https://www.gyan.dev/ffmpeg/builds/ 加到 PATH"
                .to_string(),
        )),
    }
}

// ========== BilibiliDownloader ==========
pub struct BilibiliDownloader {
    api_client: reqwest::Client,
    cdn_client: reqwest::Client,
    wbi: WbiSigner,
}

impl Default for BilibiliDownloader {
    fn default() -> Self {
        Self::new(None)
    }
}

impl BilibiliDownloader {
    pub fn new(cookies: Option<Cookies>) -> Self {
        let api_client = Self::build_api_client(cookies);
        let cdn_client = Self::build_cdn_client();
        let wbi = WbiSigner::new(api_client.clone());
        Self {
            api_client,
            cdn_client,
            wbi
        }
    }

    fn build_api_client(cookies: Option<Cookies>) -> reqwest::Client {
        let mut builder = reqwest::Client::builder().user_agent(BROWSER_UA);

        if let Some(c) = cookies {
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

        builder.build().expect("api_client build should not fail")
    }

    fn build_cdn_client() -> reqwest::Client {
        let mut headers = reqwest::header::HeaderMap::new();
        // CDN 防盗链:必须带 Referer,且不能带 SESSDATA Cookie
        headers.insert(
            reqwest::header::REFERER,
            "https://www.bilibili.com/".parse().unwrap(),
        );
        reqwest::Client::builder()
            .user_agent(BROWSER_UA)
            .default_headers(headers)
            .build()
            .expect("cdn_client build should not fail")
    }

    // 内部辅助方法
    async fn fetch_video_info(&self, bvid: &str) -> Result<VideoInfo, DownloadError> {
        let resp = self
            .api_client
            .get("https://api.bilibili.com/x/web-interface/view")
            .query(&[("bvid", bvid)])
            .send()
            .await?
            .error_for_status()?;

        let parsed: BiliResponse<VideoInfo> = resp.json().await?;

        if parsed.code != 0 {
            return Err(DownloadError::BiliApi(parsed.message, parsed.code));
        }
        parsed
            .data
            .ok_or(DownloadError::BiliApi("no data".into(), 0))
    }

    // 辅助函数：解析 BV 号
    fn parse_bvid_from_url(&self, url: &str) -> Result<String, DownloadError> {
        let parsed = url::Url::parse(url).map_err(|e| DownloadError::UrlParse(e.to_string()))?;
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
        println!(
            "  统计: {} 播放 · {} 弹幕 · {} 点赞",
            info.stat.view, info.stat.danmaku, info.stat.like
        );
        println!("  分 P: {} 个", info.pages.len());
        for p in &info.pages {
            println!(
                "    P{}: {} (cid={}, {} 秒)",
                p.page, p.part, p.cid, p.duration
            );
        }
    }

    async fn fetch_playurl(&self, bvid: &str, cid: u64)
        -> Result<PlayUrlData, DownloadError>
    {
        let params: Vec<(String, String)> = vec![
            ("bvid".into(), bvid.to_string()),
            ("cid".into(), cid.to_string()),
            ("qn".into(), "80".to_string()),
            ("fnval".into(), "4048".to_string()),
            ("fnver".into(), "0".to_string()),
            ("fourk".into(), "1".to_string()),
        ];
        let signed_query = self.wbi.sign(params).await?;

        let url = format!(
            "https://api.bilibili.com/x/player/wbi/playurl?{}",
            signed_query
        );

        let resp = self.api_client.get(&url)
            .send().await?
            .error_for_status()?;

        let parsed: BiliResponse<PlayUrlData> = resp.json().await?;
        if parsed.code != 0 {
            return Err(DownloadError::BiliApi(parsed.message, parsed.code));
        }
        parsed.data.ok_or(DownloadError::BiliApi("no playurl data".into(), 0))
    }
}

// 实现 Downloader trait
#[async_trait]
impl Downloader for BilibiliDownloader {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("bilibili.com/video/") || url.contains("b23.tv/")
    }

    fn name(&self) -> &'static str {
        "B站解析"
    }

    async fn fetch(
        &self,
        url: &str,
        output: Option<&Path>,
        jobs: usize,
    ) -> Result<FetchOutcome, DownloadError> {
        check_ffmpeg().await?;

        let bvid = self.parse_bvid_from_url(url)?;
        let info = self.fetch_video_info(&bvid).await?;
        self.print_video_info(&info);
        let cid = info.pages[0].cid;

        let play = self.fetch_playurl(&bvid, cid).await?;
        let video = play
            .dash
            .video
            .into_iter()
            .max_by_key(|v| v.bandwidth)
            .ok_or(DownloadError::NoStream("video"))?;
        let audio = play
            .dash
            .audio
            .into_iter()
            .max_by_key(|a| a.bandwidth)
            .ok_or(DownloadError::NoStream("audio"))?;
        println!(
            "选中: {}x{} {}kbps + {}kbps",
            video.width,
            video.height,
            video.bandwidth / 1000,
            audio.bandwidth / 1000
        );

        // 输出路径优先级: 用户 -o > 自动 [标题 + (BV号)][清晰度].mp4
        let output_path = match output {
            Some(p) => p.to_path_buf(),
            None => {
                let title = sanitize_filename(&info.title);
                let quality = quality_label(video.height);
                PathBuf::from(format!("[{} + ({})][{}].mp4", title, info.bvid, quality))
            }
        };

        let tmpdir = tempfile::tempdir().map_err(DownloadError::Io)?;
        let v_path = tmpdir.path().join("video.m4s");
        let a_path = tmpdir.path().join("audio.m4s");

        // 复用 HttpDownloader 的多线程能力
        println!("--- 下载视频流 ---");
        download_with_client(self.cdn_client.clone(), &video.base_url, &v_path, jobs).await?;
        println!("--- 下载音频流 ---");
        download_with_client(self.cdn_client.clone(), &audio.base_url, &a_path, jobs).await?;

        println!("--- ffmpeg 合并 ---");
        merge_with_ffmpeg(&v_path, &a_path, &output_path).await?;

        let bytes = tokio::fs::metadata(&output_path).await?.len();
        Ok(FetchOutcome { bytes, path: output_path })
    }
}
