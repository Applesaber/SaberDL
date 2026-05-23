use std::path::Path;

use async_trait::async_trait;

use crate::error::DownloadError;

mod http;

pub use http::HttpDownloader;

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

pub fn build_downloader(_url: &str) -> Box<dyn Downloader> {
    Box::new(HttpDownloader::new())
}
