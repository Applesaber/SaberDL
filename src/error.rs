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
}

impl From<reqwest::Error> for DownloadError {
    fn from(err: reqwest::Error) -> Self { DownloadError::Http(err) }
}

impl From<io::Error> for DownloadError {
    fn from(err: io::Error) -> Self { DownloadError::Io(err) }
}
