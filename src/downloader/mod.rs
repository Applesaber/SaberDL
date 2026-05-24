use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::error::DownloadError;

mod http;
pub mod bilibili;
pub mod netease;

pub use bilibili::BilibiliDownloader;
pub use http::HttpDownloader;
pub use netease::NeteaseDownloader;
pub(crate) use http::download_with_client;

pub struct FetchOutcome {
    pub bytes: u64,
    pub path: PathBuf,
}

#[async_trait]
pub trait Downloader: Send + Sync {
    fn name(&self) -> &'static str;

    async fn fetch(
        &self,
        url: &str,
        output: Option<&Path>,
        jobs: usize,
    ) -> Result<FetchOutcome, DownloadError>;
}

// 路由 + 自动 load 对应站点 cookies
//   1. URL 匹配 B 站 → load BilibiliCookies → BilibiliDownloader
//   2. URL 匹配 网易云 → load NeteaseCookies → NeteaseDownloader
//   3. 其他 → HttpDownloader (fallback)
//
// cookies load 失败 (文件损坏/版本不匹配) 不阻塞下载,仅匿名访问
pub async fn build_downloader(url: &str) -> Result<Box<dyn Downloader>, DownloadError> {
    if BilibiliDownloader::matches(url) {
        let cookies = crate::auth::bilibili::load().await.ok().flatten();
        return Ok(Box::new(BilibiliDownloader::new(cookies)));
    }
    if NeteaseDownloader::matches(url) {
        let cookies = crate::auth::netease::load().await.ok().flatten();
        return Ok(Box::new(NeteaseDownloader::new(cookies)));
    }
    Ok(Box::new(HttpDownloader::new()))
}
