use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use thiserror::Error;
use futures_util::{StreamExt, TryStreamExt};
use tokio_util::io::StreamReader;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::{Serialize, Deserialize};

const MIN_PARALLEL_SIZE: u64 = 1024 * 1024;

#[derive(Parser, Debug)]
struct Args {
    url: String,
    #[arg(short, long)]
    output: Option<PathBuf>,
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
    #[error("download interrupted by user")]
    Interrupted,
}

impl From<reqwest::Error> for DownloadError {
    fn from(err: reqwest::Error) -> Self { DownloadError::Http(err) }
}

impl From<io::Error> for DownloadError {
    fn from(err: io::Error) -> Self { DownloadError::Io(err) }
}

#[derive(Debug, Serialize, Deserialize)]
struct DownloadState {
    version: u32,
    url: String,
    total: u64,
    etag: Option<String>,
    last_modified: Option<String>,
    chunks: Vec<ChunkState>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChunkState {
    index: usize,
    start: u64,
    end: u64,
    status: ChunkStatus,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
enum ChunkStatus {
    NotStarted,
    InProgress,
    Completed,
}

struct ProbeResult {
    total: u64,
    supports_range: bool,
    final_url: String,
    etag: Option<String>,
    last_modified: Option<String>,
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
        .user_agent(concat!("SaberDL/", env!("CARGO_PKG_VERSION")))
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

fn state_path(output_path: &Path) -> PathBuf {
    let mut p = output_path.to_path_buf();
    let fname = format!(".{}.saber-state",
        output_path.file_name().and_then(|s| s.to_str()).unwrap_or("download"));
    p.set_file_name(fname);
    p
}

/// 用 `Range: bytes=0-0` GET 探测,比 HEAD 更可靠(CDN/重定向场景下 HEAD 经常返回错误的 Content-Length)
async fn probe_with_range(
    client: &reqwest::Client,
    url: &str,
) -> Result<ProbeResult, DownloadError> {
    let resp = client.get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await?;

    let status = resp.status();
    let final_url = resp.url().to_string();

    let etag = resp.headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let last_modified = resp.headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

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
            Ok(ProbeResult { total, supports_range: true, final_url, etag, last_modified })
        }
        200 => {
            // 服务器忽略 Range,只能走单连接;此处仍记录大小供 fallback 使用
            let total = resp.content_length()
                .filter(|n| *n > 0)
                .ok_or(DownloadError::NoContentLength)?;
            Ok(ProbeResult { total, supports_range: false, final_url, etag, last_modified })
        }
        _ => Err(DownloadError::BadStatus(status.as_u16())),
    }
}

async fn load_or_create_state(
    state_path: &Path,
    probe: &ProbeResult,
    jobs: usize,
) -> Result<DownloadState, DownloadError> {
    if state_path.exists() {
        let bytes = tokio::fs::read(state_path).await?;
        let existing: DownloadState = serde_json::from_slice(&bytes)
            .map_err(io::Error::other)?;

        // 一致性检查:URL/total/etag 任一变化都视为不同文件
        let same = existing.url == probe.final_url
            && existing.total == probe.total
            && existing.etag == probe.etag;

        if same {
            let done = existing.chunks.iter()
                .filter(|c| c.status == ChunkStatus::Completed).count();
            eprintln!("[RESUME] state 一致,续传 ({}/{} chunks done)",
                done, existing.chunks.len());
            return Ok(existing);
        } else {
            eprintln!("[WARN] state 不一致(URL/total/etag 变化),重置");
            tokio::fs::remove_file(state_path).await.ok();
        }
    }

    let chunks_plan = plan_chunks(probe.total, jobs);
    let state = DownloadState {
        version: 1,
        url: probe.final_url.clone(),
        total: probe.total,
        etag: probe.etag.clone(),
        last_modified: probe.last_modified.clone(),
        chunks: chunks_plan.into_iter().map(|c| ChunkState {
            index: c.index, start: c.start, end: c.end,
            status: ChunkStatus::NotStarted,
        }).collect(),
    };
    // 立即落盘:即便没 chunk 完成就被中断,重启也能识别已规划的分块
    save_state_atomic(&state, state_path).await?;
    Ok(state)
}

/// 原子写:write tmp → sync_all → rename,保证 state 永远不会半截
async fn save_state_atomic(state: &DownloadState, path: &Path) -> Result<(), DownloadError> {
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_vec_pretty(state)
        .map_err(io::Error::other)?;

    let mut f = tokio::fs::File::create(&tmp).await?;
    f.write_all(&json).await?;
    f.sync_all().await?;
    drop(f);

    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

async fn download_chunk(
    client: reqwest::Client,
    url: String,
    output_path: PathBuf,
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
    output_path: &Path,
    state: DownloadState,
    multi: &MultiProgress,
) -> Result<u64, DownloadError> {
    let sp = state_path(output_path);

    // create + write 打开足以做预分配;set_len 是幂等的(已有 state 时文件大小已对)
    let file = tokio::fs::OpenOptions::new()
        .create(true).write(true).truncate(false).open(output_path).await?;
    file.set_len(state.total).await?;
    drop(file);

    let total = state.total;
    let state = Arc::new(tokio::sync::Mutex::new(state));

    let total_pb = multi.add(ProgressBar::new(total));
    total_pb.set_style(total_style());
    total_pb.set_prefix("TOTAL");

    // 已完成部分先 inc 到 total_pb(续传时进度条不从 0 起跳)
    let (done_bytes, chunks_to_do): (u64, Vec<ChunkState>) = {
        let s = state.lock().await;
        let done: u64 = s.chunks.iter()
            .filter(|c| c.status == ChunkStatus::Completed)
            .map(|c| c.end - c.start + 1)
            .sum();
        let todo = s.chunks.iter()
            .filter(|c| c.status != ChunkStatus::Completed)
            .cloned()
            .collect();
        (done, todo)
    };
    total_pb.inc(done_bytes);

    let mut set = tokio::task::JoinSet::new();
    for ck in chunks_to_do {
        let pb = multi.add(ProgressBar::new(ck.end - ck.start + 1));
        pb.set_style(worker_style());
        pb.set_prefix(format!("W{}", ck.index));

        let client = client.clone();
        let url = url.to_string();
        let output_path = output_path.to_path_buf();
        let total_pb_w = total_pb.clone();
        let state = Arc::clone(&state);
        let sp = sp.clone();

        set.spawn(async move {
            let chunk = Chunk { index: ck.index, start: ck.start, end: ck.end };
            let bytes = download_chunk(client, url, output_path, chunk, pb, total_pb_w).await?;

            let mut s = state.lock().await;
            if let Some(c) = s.chunks.iter_mut().find(|c| c.index == ck.index) {
                c.status = ChunkStatus::Completed;
            }
            save_state_atomic(&s, &sp).await?;
            Ok::<u64, DownloadError>(bytes)
        });
    }

    let download_task = async {
        let mut total_bytes = done_bytes;
        while let Some(result) = set.join_next().await {
            // 外层 ? : tokio JoinError(任务 panic 等);内层 ? : DownloadError
            let chunk_bytes = result
                .map_err(|e| DownloadError::Io(io::Error::other(e.to_string())))??;
            total_bytes += chunk_bytes;
        }
        Ok::<u64, DownloadError>(total_bytes)
    };

    let result = tokio::select! {
        r = download_task => r,
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\n[INTERRUPTED] state 已保存,可重启续传");
            Err(DownloadError::Interrupted)
        }
    };

    total_pb.finish_and_clear();

    // 成功 → 删 state(下次是全新下载)
    if result.is_ok() {
        tokio::fs::remove_file(&sp).await.ok();
    }

    result
}

async fn download_single(
    client: reqwest::Client,
    url: &str,
    output_path: &Path,
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

    let output_path: PathBuf = args.output.unwrap_or_else(|| {
        PathBuf::from(
            args.url.rsplit('/').next().unwrap_or("downloaded_file")
        )
    });

    let client = build_client()?;
    let probe = probe_with_range(&client, &args.url)
        .await
        .with_context(|| format!("探测失败:{}", args.url))?;

    let multi = MultiProgress::new();

    let parallel = probe.supports_range
        && probe.total >= MIN_PARALLEL_SIZE
        && args.jobs > 1;

    let bytes = if parallel {
        println!("[GET] {} ({} bytes, {} jobs)", probe.final_url, probe.total, args.jobs);
        let sp = state_path(&output_path);
        let state = load_or_create_state(&sp, &probe, args.jobs)
            .await
            .with_context(|| format!("加载 state 失败:{}", sp.display()))?;
        download_parallel(client, &probe.final_url, &output_path, state, &multi)
            .await
            .with_context(|| format!("下载失败:{}", probe.final_url))?
    } else {
        let reason = if !probe.supports_range { "no Range" }
                     else if probe.total < MIN_PARALLEL_SIZE { "size<1MB" }
                     else { "jobs=1" };
        println!("[GET] {} ({} bytes, single - {})", probe.final_url, probe.total, reason);
        download_single(client, &probe.final_url, &output_path, probe.total, &multi)
            .await
            .with_context(|| format!("下载失败:{}", probe.final_url))?
    };

    println!("[OK] 已保存到 {}({} 字节)", output_path.display(), bytes);
    Ok(())
}
