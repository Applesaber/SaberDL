use std::io;
use clap::Parser;
use thiserror::Error;

use anyhow::{Context, Result};

#[derive(Parser, Debug)]
struct Args {
    url: String,
    #[arg(short, long)]
    output: Option<String>,
}

#[derive(Debug, Error)]  // 添加 Debug 并使用 thiserror::Error
pub enum DownloadError {
    #[error("HTTP error: {0}")]
    Http(reqwest::Error),
    #[error("IO error: {0}")]
    Io(io::Error),
    #[error("Bad HTTP status: {0}")]
    BadStatus(u16),
}

// 实现 From trait 以便自动转换
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

fn download(url: &str, output_path: &str) -> Result<u64, DownloadError> {
    let mut response = reqwest::blocking::get(url)?;

    if !response.status().is_success() {
        return Err(DownloadError::BadStatus(response.status().as_u16()));
    }

    let mut file = std::fs::File::create(output_path)?;
    let bytes_copied = std::io::copy(&mut response, &mut file)?;

    Ok(bytes_copied)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let output_path = args.output.unwrap_or_else(|| {
        args.url
            .rsplit('/')
            .next()
            .unwrap_or("downloaded_file")
            .to_string()
    });

    println!("[GET] {url}", url=args.url);
    
    let bytes_copied = download(&args.url, &output_path)
        .with_context(|| format!("下载失败：{}", args.url))?;

    println!("[OK] 已保存到 {output_path}（{bytes_copied} 字节）");

    Ok(())
}
