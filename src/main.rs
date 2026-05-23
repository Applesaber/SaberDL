use std::io;

use anyhow::{Context, Result};
use clap::Parser;
use thiserror::Error;
use futures_util::{StreamExt, TryStreamExt};
use tokio_util::io::StreamReader;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

const MIN_PARALLEL_SIZE: u64 = 1024 * 1024;

#[derive(Parser, Debug)]
struct Args {
    url: String,
    #[arg(short, long)]
    output: Option<String>,
    #[arg(short = 'j', long, default_value_t = 8)]
    jobs: usize,
}

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
}

impl From<reqwest::Error> for DownloadError {
    fn from(err: reqwest::Error) -> Self { DownloadError::Http(err) }
}

impl From<io::Error> for DownloadError {
    fn from(err: io::Error) -> Self { DownloadError::Io(err) }
}

struct Chunk { index: usize, start: u64, end: u64 }

fn plan_chunks(total: u64, jobs: usize) -> Vec<Chunk> {
    if total == 0 || jobs == 0 {
        return Vec::new();
    }
    let chunk_size = total.div_ceil(jobs as u64);
    (0..jobs)
        .map(|i| {
            let start = i as u64 * chunk_size;
            let end = ((i as u64 + 1) * chunk_size - 1).min(total - 1);
            Chunk { index: i, start, end }
        })
        .filter(|c| c.start <= c.end)
        .collect()
}

fn build_client() -> Result<reqwest::Client, DownloadError> {
    reqwest::Client::builder()
        .pool_max_idle_per_host(32)
        .build()
        .map_err(DownloadError::Http)
}

fn worker_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{prefix:>6.cyan} {wide_bar:.cyan/blue} {percent:>3}% {bytes:>10}/{total_bytes:>10}"
    ).unwrap().progress_chars("#>-")
}

fn total_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{prefix:>6.green.bold} {wide_bar:.green} {percent:>3}% {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})"
    ).unwrap().progress_chars("#>-")
}

/// 用 `Range: bytes=0-0` GET 探测,比 HEAD 更可靠(CDN/重定向场景下 HEAD 经常返回错误的 Content-Length)
///
/// 返回 `(total_size, supports_range, final_url_after_redirects)`
async fn probe_with_range(
    client: &reqwest::Client,
    url: &str,
) -> Result<(u64, bool, String), DownloadError> {
    let resp = client.get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await?;

    let status = resp.status();
    let final_url = resp.url().to_string();

    match status.as_u16() {
        206 => {
            // Content-Range: "bytes 0-0/302078113" -> 取斜杠后的总大小
            let cr = resp.headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .ok_or(DownloadError::NoContentLength)?;
            let total: u64 = cr.rsplit('/')
                .next()
                .and_then(|s| s.parse().ok())
                .filter(|n: &u64| *n > 0)
                .ok_or(DownloadError::NoContentLength)?;
            Ok((total, true, final_url))
        }
        200 => {
            // 服务器忽略 Range,只能走单连接;此处仍记录大小供 fallback 使用
            let total = resp.content_length()
                .filter(|n| *n > 0)
                .ok_or(DownloadError::NoContentLength)?;
            Ok((total, false, final_url))
        }
        _ => Err(DownloadError::BadStatus(status.as_u16())),
    }
}

async fn download_chunk(
    client: reqwest::Client,
    url: String,
    output_path: String,
    chunk: Chunk,
    pb: ProgressBar,
    total_pb: ProgressBar,
) -> Result<u64, DownloadError> {
    let range = format!("bytes={}-{}", chunk.start, chunk.end);
    let resp = client.get(&url)
        .header(reqwest::header::RANGE, range)
        .send()
        .await?;

    // 严守 206 校验:200 = 服务器吐了整个文件,继续 seek+write 会损坏目标文件
    if resp.status().as_u16() != 206 {
        return Err(DownloadError::BadStatus(resp.status().as_u16()));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&output_path)
        .await?;
    file.seek(io::SeekFrom::Start(chunk.start)).await?;

    // 手动 chunk loop:每读一段同时推 worker pb 和共享 total_pb
    let mut stream = resp.bytes_stream();
    let mut bytes_written = 0u64;
    while let Some(item) = stream.next().await {
        let buf = item?;
        file.write_all(&buf).await?;
        let n = buf.len() as u64;
        bytes_written += n;
        pb.inc(n);
        total_pb.inc(n);
    }

    pb.finish_and_clear();
    Ok(bytes_written)
}

async fn download_parallel(
    client: reqwest::Client,
    url: &str,
    output_path: &str,
    total: u64,
    jobs: usize,
    multi: &MultiProgress,
) -> Result<u64, DownloadError> {
    let chunks = plan_chunks(total, jobs);

    let file = tokio::fs::File::create(output_path).await?;
    file.set_len(total).await?;
    drop(file);

    let total_pb = multi.add(ProgressBar::new(total));
    total_pb.set_style(total_style());
    total_pb.set_prefix("TOTAL");

    let mut set = tokio::task::JoinSet::new();
    for chunk in chunks {
        let chunk_size = chunk.end - chunk.start + 1;
        let pb = multi.add(ProgressBar::new(chunk_size));
        pb.set_style(worker_style());
        pb.set_prefix(format!("W{}", chunk.index));

        let client = client.clone();
        let url = url.to_string();
        let output_path = output_path.to_string();
        let total_pb_w = total_pb.clone();
        set.spawn(async move {
            download_chunk(client, url, output_path, chunk, pb, total_pb_w).await
        });
    }

    let mut total_bytes = 0u64;
    while let Some(result) = set.join_next().await {
        // 外层 ? : tokio JoinError(任务 panic 等);内层 ? : DownloadError
        let chunk_bytes = result
            .map_err(|e| DownloadError::Io(io::Error::other(e.to_string())))??;
        total_bytes += chunk_bytes;
    }

    total_pb.finish_with_message("done");
    Ok(total_bytes)
}

async fn download_single(
    client: reqwest::Client,
    url: &str,
    output_path: &str,
    total: u64,
    multi: &MultiProgress,
) -> Result<u64, DownloadError> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(DownloadError::BadStatus(resp.status().as_u16()));
    }

    let pb = multi.add(ProgressBar::new(total));
    pb.set_style(total_style());
    pb.set_prefix("SINGLE");

    let file = tokio::fs::File::create(output_path).await?;
    let stream = resp.bytes_stream().map_err(io::Error::other);
    let mut reader = StreamReader::new(stream);
    let mut writer = pb.wrap_async_write(file);
    let bytes = tokio::io::copy(&mut reader, &mut writer).await?;
    pb.finish_with_message("done");
    Ok(bytes)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let output_path = args.output.unwrap_or_else(|| {
        args.url
            .rsplit('/')
            .next()
            .unwrap_or("downloaded_file")
            .to_string()
    });

    let client = build_client()?;
    let (total, supports_range, final_url) = probe_with_range(&client, &args.url)
        .await
        .with_context(|| format!("探测失败:{}", args.url))?;

    let multi = MultiProgress::new();

    let parallel = supports_range && total >= MIN_PARALLEL_SIZE && args.jobs > 1;
    let bytes = if parallel {
        println!("[GET] {} ({} bytes, {} jobs)", final_url, total, args.jobs);
        download_parallel(client, &final_url, &output_path, total, args.jobs, &multi)
            .await
            .with_context(|| format!("下载失败:{}", final_url))?
    } else {
        let reason = if !supports_range { "no Range" }
                     else if total < MIN_PARALLEL_SIZE { "size<1MB" }
                     else { "jobs=1" };
        println!("[GET] {} ({} bytes, single - {})", final_url, total, reason);
        download_single(client, &final_url, &output_path, total, &multi)
            .await
            .with_context(|| format!("下载失败:{}", final_url))?
    };

    println!("[OK] 已保存到 {output_path}({bytes} 字节)");
    Ok(())
}
