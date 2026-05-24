// 网易云音乐下载器 (L14a 占位,L14b/c/L15 填充)
//   weapi.rs: AES-CBC ×2 + RSA-1024 加密(L14b)
//   api.rs  : song/url/v1, song/detail 等接口(L15)
//   meta.rs : lofty 写 ID3/FLAC 标签 + 封面嵌入(L15)

use std::path::Path;

use async_trait::async_trait;

use crate::auth::netease::NeteaseCookies;
use crate::downloader::{Downloader, FetchOutcome};
use crate::error::DownloadError;

#[allow(dead_code)]   // L14b 起开始用
pub mod weapi;
#[allow(dead_code)]
pub mod api;
#[allow(dead_code)]
pub mod meta;

pub struct NeteaseDownloader {
    #[allow(dead_code)]   // L14b 起开始用
    cookies: Option<NeteaseCookies>,
}

impl NeteaseDownloader {
    pub fn new(cookies: Option<NeteaseCookies>) -> Self {
        Self { cookies }
    }

    // 静态 URL 匹配(给 build_downloader 路由用,避免无谓构建)
    pub fn matches(url: &str) -> bool {
        url.contains("music.163.com")
    }
}

#[async_trait]
impl Downloader for NeteaseDownloader {
    fn name(&self) -> &'static str {
        "网易云解析"
    }

    async fn fetch(
        &self,
        _url: &str,
        _output: Option<&Path>,
        _jobs: usize,
    ) -> Result<FetchOutcome, DownloadError> {
        Err(DownloadError::Other(
            "网易云下载功能即将在 Lesson 14b/L15 实现".into(),
        ))
    }
}
