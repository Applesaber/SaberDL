use std::io;
use anyhow::{Context, Result};
use clap::Parser;
use thiserror::Error;
use indicatif::{ProgressBar, ProgressStyle};
use futures_util::TryStreamExt;
use tokio_util::io::StreamReader;

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

async fn download(url: &str, output_path: &str) -> Result<u64, DownloadError> {
    let response = reqwest::get(url).await?;

    if !response.status().is_success() {
        return Err(DownloadError::BadStatus(response.status().as_u16()));
    }

    let pb = match response.content_length() {
        Some(total) => {
            let pb = ProgressBar::new(total);
            let style = ProgressStyle::with_template(
                "{wide_bar:.cyan/blue} {percent:>3}% [{elapsed_precise}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})"
            ).unwrap();
            pb.set_style(style);
            pb
        }
        None => {
            let pb = ProgressBar::new_spinner();
            let style = ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] {bytes} ({bytes_per_sec})"
            ).unwrap();
            pb.set_style(style);
            pb
        }
    };

    let file = tokio::fs::File::create(output_path).await?;

    let stream = response
        .bytes_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
    let mut reader = StreamReader::new(stream);
    let mut writer = pb.wrap_async_write(file);
    let bytes_copied = tokio::io::copy(&mut reader, &mut writer).await?;

    pb.finish_and_clear();
    Ok(bytes_copied)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let output_path = args.output.unwrap_or_else(|| {
        args.url
            .rsplit('/')
            .next()
            .unwrap_or("downloaded_file")
            .to_string()
    });

    println!("[GET] {}", args.url);

    let bytes_copied = download(&args.url, &output_path)
        .await
        .with_context(|| format!("下载失败：{}", args.url))?;

    println!("[OK] 已保存到 {output_path}（{bytes_copied} 字节）");

    Ok(())
}
