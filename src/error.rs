use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("HTTP error: {0}")]
    Http(reqwest::Error),
    #[error("IO error: {0}")]
    Io(io::Error),
    #[error("Bad HTTP status: {0}")]
    BadStatus(u16),
    #[error("server did not report a valid total size")]
    NoContentLength,
    #[error("download interrupted by user")]
    Interrupted,

    #[error("ffmpeg not found or failed: {0}")]
    Ffmpeg(String),
    #[error("DASH stream selection failed: {0}")]
    NoStream(&'static str),

    #[error("Bilibili API error: {0} (code={1})")]
    BiliApi(String, i64),
    #[error("URL parse failed: {0}")]
    UrlParse(String),
    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for DownloadError {
    fn from(err: reqwest::Error) -> Self {
        DownloadError::Http(err)
    }
}

impl From<io::Error> for DownloadError {
    fn from(err: io::Error) -> Self {
        DownloadError::Io(err)
    }
}
