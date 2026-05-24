use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::error::DownloadError;

#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadState {
    pub version: u32,
    pub url: String,
    pub total: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub chunks: Vec<ChunkState>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChunkState {
    pub index: usize,
    pub start: u64,
    pub end: u64,
    pub status: ChunkStatus,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub enum ChunkStatus {
    NotStarted,
    InProgress,
    Completed,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub index: usize,
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone)]
pub struct DownloadMeta {
    pub url: String,
    pub total: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

pub fn plan_chunks(total: u64, jobs: usize) -> Vec<Chunk> {
    if total == 0 || jobs == 0 {
        return Vec::new();
    }
    let chunk_size = total.div_ceil(jobs as u64);
    (0..jobs)
        .map(|i| {
            let start = i as u64 * chunk_size;
            let end = ((i as u64 + 1) * chunk_size - 1).min(total - 1);
            Chunk {
                index: i,
                start,
                end,
            }
        })
        .filter(|c| c.start <= c.end)
        .collect()
}

pub fn state_path(output_path: &Path) -> PathBuf {
    let mut p = output_path.to_path_buf();
    let fname = format!(
        ".{}.saber-state",
        output_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("download")
    );
    p.set_file_name(fname);
    p
}

pub async fn load_or_create_state(
    sp: &Path,
    meta: &DownloadMeta,
    jobs: usize,
) -> Result<DownloadState, DownloadError> {
    if sp.exists() {
        let bytes = tokio::fs::read(sp).await?;
        let existing: DownloadState = serde_json::from_slice(&bytes).map_err(io::Error::other)?;

        let same =
            existing.url == meta.url && existing.total == meta.total && existing.etag == meta.etag;

        if same {
            let done = existing
                .chunks
                .iter()
                .filter(|c| c.status == ChunkStatus::Completed)
                .count();
            eprintln!(
                "[RESUME] state 一致,续传 ({}/{} chunks done)",
                done,
                existing.chunks.len()
            );
            return Ok(existing);
        } else {
            eprintln!("[WARN] state 不一致(URL/total/etag 变化),重置");
            tokio::fs::remove_file(sp).await.ok();
        }
    }

    let chunks_plan = plan_chunks(meta.total, jobs);
    let state = DownloadState {
        version: 1,
        url: meta.url.clone(),
        total: meta.total,
        etag: meta.etag.clone(),
        last_modified: meta.last_modified.clone(),
        chunks: chunks_plan
            .into_iter()
            .map(|c| ChunkState {
                index: c.index,
                start: c.start,
                end: c.end,
                status: ChunkStatus::NotStarted,
            })
            .collect(),
    };
    // 立即落盘:即便没 chunk 完成就被中断,重启也能识别已规划的分块
    save_state_atomic(&state, sp).await?;
    Ok(state)
}

/// 原子写:write tmp → sync_all → rename,保证 state 永远不会半截
pub async fn save_state_atomic(state: &DownloadState, path: &Path) -> Result<(), DownloadError> {
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;

    let mut f = tokio::fs::File::create(&tmp).await?;
    f.write_all(&json).await?;
    f.sync_all().await?;
    drop(f);

    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}
