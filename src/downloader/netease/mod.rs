// 网易云音乐下载器
//   完整流程: 解析 URL → song_detail 拿元信息 → song_url_v1 拿直链 →
//             复用 HttpDownloader 拉文件 → 下封面 → lofty 写标签
//   音质选择: 默认 exhigh (320K),无权限自动降级到 standard (128K)

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::auth::netease::NeteaseCookies;
use crate::downloader::{Downloader, FetchOutcome, download_with_client};
use crate::error::DownloadError;

pub mod api;
pub mod meta;
pub mod weapi;

use api::{
    SongDetailItem, SongUrlItem, level_label, parse_song_id_from_url, song_detail, song_url_v1,
};
use meta::{ext_for_type, write_tags};

const NETEASE_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

const LEVEL_FALLBACKS: &[&str] = &["exhigh", "higher", "standard"];

pub struct NeteaseDownloader {
    cookies: Option<NeteaseCookies>,
    api_client: reqwest::Client,
    cdn_client: reqwest::Client,
}

impl NeteaseDownloader {
    pub fn new(cookies: Option<NeteaseCookies>) -> Self {
        let api_client = Self::build_api_client(cookies.as_ref());
        let cdn_client = Self::build_cdn_client();
        Self {
            cookies,
            api_client,
            cdn_client,
        }
    }

    pub fn matches(url: &str) -> bool {
        url.contains("music.163.com")
    }

    fn build_api_client(cookies: Option<&NeteaseCookies>) -> reqwest::Client {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::REFERER,
            "https://music.163.com/".parse().unwrap(),
        );
        headers.insert(
            reqwest::header::ORIGIN,
            "https://music.163.com".parse().unwrap(),
        );

        let mut builder = reqwest::Client::builder()
            .user_agent(NETEASE_UA)
            .default_headers(headers);

        if let Some(c) = cookies {
            let jar = std::sync::Arc::new(reqwest::cookie::Jar::default());
            let url: reqwest::Url = "https://music.163.com".parse().unwrap();
            jar.add_cookie_str(
                &format!("MUSIC_U={}; Domain=.music.163.com", c.music_u),
                &url,
            );
            if !c.csrf.is_empty() {
                jar.add_cookie_str(
                    &format!("__csrf={}; Domain=.music.163.com", c.csrf),
                    &url,
                );
            }
            builder = builder.cookie_provider(jar);
        }

        builder
            .build()
            .expect("netease api_client build should not fail")
    }

    fn build_cdn_client() -> reqwest::Client {
        reqwest::Client::builder()
            .user_agent(NETEASE_UA)
            .build()
            .expect("netease cdn_client build should not fail")
    }

    async fn fetch_with_fallback(&self, song_id: i64) -> Result<SongUrlItem, DownloadError> {
        let mut last_err = None;
        for level in LEVEL_FALLBACKS {
            match song_url_v1(&self.api_client, self.cookies.as_ref(), song_id, level).await {
                Ok(item) => return Ok(item),
                Err(e) => {
                    eprintln!("[WARN] 音质 {} 不可用: {}", level, e);
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| DownloadError::Other("song_url_v1 全部音质都失败".into())))
    }

    async fn download_cover(&self, pic_url: &str) -> Option<Vec<u8>> {
        if pic_url.is_empty() {
            return None;
        }
        match self.cdn_client.get(pic_url).send().await {
            Ok(r) => r.bytes().await.ok().map(|b| b.to_vec()),
            Err(_) => None,
        }
    }

    fn print_song_info(&self, detail: &SongDetailItem, song: &SongUrlItem) {
        println!("════ 网易云歌曲信息 ════");
        println!("  标题: {}", detail.name);
        println!(
            "  歌手: {}",
            detail
                .ar
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(" / ")
        );
        if !detail.al.name.is_empty() {
            println!("  专辑: {}", detail.al.name);
        }
        let level = song.level.as_deref().unwrap_or("?");
        println!(
            "  音质: {} ({} kbps,{:.2} MB)",
            level_label(level),
            song.br / 1000,
            song.size as f64 / 1024.0 / 1024.0
        );
    }
}

#[async_trait]
impl Downloader for NeteaseDownloader {
    fn name(&self) -> &'static str {
        "网易云解析"
    }

    async fn fetch(
        &self,
        url: &str,
        output: Option<&Path>,
        jobs: usize,
    ) -> Result<FetchOutcome, DownloadError> {
        let song_id = parse_song_id_from_url(url)?;

        let detail = song_detail(&self.api_client, self.cookies.as_ref(), song_id).await?;
        let song = self.fetch_with_fallback(song_id).await?;
        self.print_song_info(&detail, &song);

        let direct_url = song
            .url
            .as_deref()
            .ok_or_else(|| DownloadError::Other("song url 缺失".into()))?;

        let ext = ext_for_type(song.r#type.as_deref());

        let output_path = match output {
            Some(p) => p.to_path_buf(),
            None => {
                let artists = detail
                    .ar
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                let title = sanitize(&detail.name);
                let artist_label = sanitize(if artists.is_empty() {
                    "未知歌手"
                } else {
                    &artists
                });
                PathBuf::from(format!("[{} - {}].{}", artist_label, title, ext))
            }
        };

        println!("--- 下载音频流 ---");
        download_with_client(self.cdn_client.clone(), direct_url, &output_path, jobs).await?;

        let cover = self.download_cover(&detail.al.pic_url).await;
        if let Err(e) = write_tags(&output_path, &detail, cover.as_deref()) {
            eprintln!("[WARN] 写元数据失败: {}", e);
        } else {
            println!("--- 元数据 + 封面已写入 ---");
        }

        let bytes = tokio::fs::metadata(&output_path).await?.len();
        Ok(FetchOutcome {
            bytes,
            path: output_path,
        })
    }
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c == ' ');
    let limited: String = trimmed.chars().take(120).collect();
    if limited.is_empty() {
        "untitled".to_string()
    } else {
        limited
    }
}
