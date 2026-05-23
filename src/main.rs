use std::path::PathBuf;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use saber_dl::{auth, auth::Cookies, build_downloader, qrlogin};
#[derive(Parser, Debug)]
#[command(name = "saber-dl", version, about = "SaberDL 下载器")]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}
#[derive(Subcommand, Debug)]
enum Cmd {
    /// 下载 URL
    Get {
        url: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(short = 'j', long, default_value_t = 8)]
        jobs: usize,
    },
    /// 通过二维码登录 B 站
    Login,
    /// 删除本地 cookies
    Logout,
    /// 查看当前登录账号(简化版,仅检查 cookies 是否存在)
    Whoami,
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match args.cmd {
        Cmd::Get { url, output, jobs } => run_get(url, output, jobs).await,
        Cmd::Login => {
            qrlogin::login_with_qrcode().await
                .with_context(|| "扫码登录失败".to_string())?;
            Ok(())
        }
        Cmd::Logout => {
            if auth::delete().await? {
                println!("已删除本地 cookies");
            } else {
                println!("本地没有 cookies");
            }
            Ok(())
        }
        Cmd::Whoami => {
            match auth::load().await? {
                Some(c) => println!("已登录 (DedeUserID={})", c.dedeuserid),
                None => println!("尚未登录(用 saber-dl login)"),
            }
            Ok(())
        }
    }
}
async fn run_get(url: String, output: Option<PathBuf>, jobs: usize) -> Result<()> {
    let output: PathBuf = output.unwrap_or_else(|| {
        PathBuf::from(url.rsplit('/').next().unwrap_or("downloaded_file"))
    });
    let cookies = std::env::var("BILIBILI_SESSDATA").ok()
        .map(Cookies::sessdata_only)
        .or(auth::load().await.ok().flatten());
    let downloader = build_downloader(&url, cookies);
    let bytes = downloader.fetch(&url, &output, jobs)
        .await
        .with_context(|| format!("下载失败: {}", url))?;
    println!("[OK] 已保存到 {}({} 字节)", output.display(), bytes);
    Ok(())
}