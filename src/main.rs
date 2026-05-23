use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use saber_dl::build_downloader;

#[derive(Parser, Debug)]
struct Args {
    url: String,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(short = 'j', long, default_value_t = 8)]
    jobs: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let output: PathBuf = args.output.unwrap_or_else(|| {
        PathBuf::from(
            args.url.rsplit('/').next().unwrap_or("downloaded_file")
        )
    });

    let downloader = build_downloader(&args.url);
    let bytes = downloader.fetch(&args.url, &output, args.jobs)
        .await
        .with_context(|| format!("下载失败: {}", args.url))?;

    println!("[OK] 已保存到 {}({} 字节)", output.display(), bytes);
    Ok(())
}
