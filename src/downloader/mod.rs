use std::path::Path;

use async_trait::async_trait;

use crate::auth::Cookies;
use crate::error::DownloadError;

pub mod bilibili;
mod http;

pub use bilibili::BilibiliDownloader;
pub use http::HttpDownloader;
pub(crate) use http::download_with_client;

#[async_trait]
pub trait Downloader: Send + Sync {
    fn can_handle(&self, url: &str) -> bool;
    fn name(&self) -> &'static str;

    async fn fetch(&self, url: &str, output: &Path, jobs: usize) -> Result<u64, DownloadError>;
}

pub fn build_downloader(url: &str, cookies: Option<Cookies>) -> Box<dyn Downloader> {
    let bili = BilibiliDownloader::new(cookies);
    if bili.can_handle(url) {
        return Box::new(bili);
    }
    Box::new(HttpDownloader::new())
}
