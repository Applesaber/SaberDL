use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{StreamExt, TryStreamExt};
use indicatif::{MultiProgress, ProgressBar};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::StreamReader;

use crate::downloader::{Downloader, FetchOutcome};
use crate::error::DownloadError;
use crate::progress::{total_style, worker_style};
use crate::state::{
    Chunk, ChunkStatus, DownloadMeta, DownloadState, load_or_create_state, save_state_atomic,
    state_path,
};

const MIN_PARALLEL_SIZE: u64 = 1024 * 1024;

pub struct HttpDownloader {
    client: reqwest::Client,
}

impl HttpDownloader {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("SaberDL/", env!("CARGO_PKG_VERSION")))
            .pool_max_idle_per_host(32)
            .build()
            .expect("reqwest client build (basic config) should not fail");
        Self { client }
    }

    // url crate 解析 → 取最后非空 path 段(自动过滤 query/fragment)
    // 失败时 fallback "downloaded_file"
    fn default_output_name(url: &str) -> PathBuf {
        url::Url::parse(url)
            .ok()
            .as_ref()
            .and_then(|u| u.path_segments())
            .and_then(|s| s.filter(|p| !p.is_empty()).last())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("downloaded_file"))
    }
}

impl Default for HttpDownloader {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Downloader for HttpDownloader {
    fn can_handle(&self, _url: &str) -> bool {
        true
    }
    fn name(&self) -> &'static str {
        "通用下载"
    }

    async fn fetch(
        &self,
        url: &str,
        output: Option<&Path>,
        jobs: usize,
    ) -> Result<FetchOutcome, DownloadError> {
        let output_path = output
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| Self::default_output_name(url));
        let bytes =
            download_with_client(self.client.clone(), url, &output_path, jobs).await?;
        Ok(FetchOutcome { bytes, path: output_path })
    }
}
pub(crate) async fn download_with_client(
    client: reqwest::Client,
    url: &str,
    output: &Path,
    jobs: usize,
) -> Result<u64, DownloadError> {
    let (meta, supports_range) = probe_with_range(&client, url).await?;
    let multi = MultiProgress::new();

    let parallel = supports_range && meta.total >= MIN_PARALLEL_SIZE && jobs > 1;
    if parallel {
        println!("[GET] {} ({} bytes, {} jobs)", meta.url, meta.total, jobs);
        let sp = state_path(output);
        let state = load_or_create_state(&sp, &meta, jobs).await?;
        download_parallel(client, output, state, &multi).await
    } else {
        let reason = if !supports_range {
            "no Range"
        } else if meta.total < MIN_PARALLEL_SIZE {
            "size<1MB"
        } else {
            "jobs=1"
        };
        println!(
            "[GET] {} ({} bytes, single - {})",
            meta.url, meta.total, reason
        );
        download_single(client, &meta.url, output, meta.total, &multi).await
    }
}

/// 用 `Range: bytes=0-0` GET 探测,比 HEAD 更可靠(CDN/重定向场景下 HEAD 经常返回错误的 Content-Length)
///
/// 返回 `(DownloadMeta, supports_range)`,Meta 里的 url 已是重定向后的最终 URL
async fn probe_with_range(
    client: &reqwest::Client,
    url: &str,
) -> Result<(DownloadMeta, bool), DownloadError> {
    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await?;

    let status = resp.status();
    let final_url = resp.url().to_string();

    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let last_modified = resp
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    match status.as_u16() {
        206 => {
            // Content-Range: "bytes 0-0/302078113" -> 取斜杠后的总大小
            let cr = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .ok_or(DownloadError::NoContentLength)?;
            let total: u64 = cr
                .rsplit('/')
                .next()
                .and_then(|s| s.parse().ok())
                .filter(|n: &u64| *n > 0)
                .ok_or(DownloadError::NoContentLength)?;
            Ok((
                DownloadMeta {
                    url: final_url,
                    total,
                    etag,
                    last_modified,
                },
                true,
            ))
        }
        200 => {
            // 服务器忽略 Range,只能走单连接;此处仍记录大小供 fallback 使用
            let total = resp
                .content_length()
                .filter(|n| *n > 0)
                .ok_or(DownloadError::NoContentLength)?;
            Ok((
                DownloadMeta {
                    url: final_url,
                    total,
                    etag,
                    last_modified,
                },
                false,
            ))
        }
        _ => Err(DownloadError::BadStatus(status.as_u16())),
    }
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
    let resp = client
        .get(&url)
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
    output_path: &Path,
    state: DownloadState,
    multi: &MultiProgress,
) -> Result<u64, DownloadError> {
    let sp = state_path(output_path);
    let url = state.url.clone();

    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(output_path)
        .await?;
    file.set_len(state.total).await?;
    drop(file);

    let total = state.total;
    let state = Arc::new(tokio::sync::Mutex::new(state));

    let total_pb = multi.add(ProgressBar::new(total));
    total_pb.set_style(total_style());
    total_pb.set_prefix("TOTAL");

    let (done_bytes, chunks_to_do) = {
        let s = state.lock().await;
        let done: u64 = s
            .chunks
            .iter()
            .filter(|c| c.status == ChunkStatus::Completed)
            .map(|c| c.end - c.start + 1)
            .sum();
        let todo: Vec<_> = s
            .chunks
            .iter()
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
        let url = url.clone();
        let output_path = output_path.to_path_buf();
        let total_pb_w = total_pb.clone();
        let state = Arc::clone(&state);
        let sp = sp.clone();

        set.spawn(async move {
            let chunk = Chunk {
                index: ck.index,
                start: ck.start,
                end: ck.end,
            };
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
            let chunk_bytes =
                result.map_err(|e| DownloadError::Io(io::Error::other(e.to_string())))??;
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
