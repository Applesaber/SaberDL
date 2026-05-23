use std::path::Path;

use async_trait::async_trait;

use crate::error::DownloadError;

mod http;
pub mod bilibili;

pub use http::HttpDownloader;
pub use bilibili::BilibiliDownloader;

#[async_trait]
pub trait Downloader: Send + Sync {
    fn can_handle(&self, url: &str) -> bool;

    async fn fetch(
        &self,
        url: &str,
        output: &Path,
        jobs: usize,
    ) -> Result<u64, DownloadError>;
}

pub fn build_downloader(url: &str) -> Box<dyn Downloader> {
    let bili = BilibiliDownloader::new();
    if bili.can_handle(url) {
        return Box::new(bili);
    }
    Box::new(HttpDownloader::new())
}
