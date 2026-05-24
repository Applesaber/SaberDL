use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::auth::Cookies;
use crate::error::DownloadError;

mod http;
pub mod bilibili;

pub use bilibili::BilibiliDownloader;
pub use http::HttpDownloader;
pub(crate) use http::download_with_client;

pub struct FetchOutcome {
    pub bytes: u64,
    pub path: PathBuf,
}

#[async_trait]
pub trait Downloader: Send + Sync {
    fn can_handle(&self, url: &str) -> bool;

    fn name(&self) -> &'static str;

    async fn fetch(
        &self,
        url: &str,
        output: Option<&Path>,
        jobs: usize,
    ) -> Result<FetchOutcome, DownloadError>;
}

pub fn build_downloader(url: &str, cookies: Option<Cookies>) -> Box<dyn Downloader> {
    let bili = BilibiliDownloader::new(cookies);
    if bili.can_handle(url) {
        return Box::new(bili);
    }
    Box::new(HttpDownloader::new())
}
