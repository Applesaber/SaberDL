use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use saber_dl::{auth, build_downloader, qrlogin};
use std::path::PathBuf;

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
    /// 通过二维码登录 (bilibili 或 netease,默认 bilibili)
    Login {
        #[arg(default_value = "bilibili")]
        site: String,
    },
    /// 删除本地 cookies (bilibili 或 netease,默认 bilibili)
    Logout {
        #[arg(default_value = "bilibili")]
        site: String,
    },
    /// 查看当前登录账号 (B 站 + 网易云)
    Whoami,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match args.cmd {
        Cmd::Get { url, output, jobs } => run_get(url, output, jobs).await,
        Cmd::Login { site } => run_login(&site).await,
        Cmd::Logout { site } => run_logout(&site).await,
        Cmd::Whoami => run_whoami().await,
    }
}

async fn run_get(url: String, output: Option<PathBuf>, jobs: usize) -> Result<()> {
    let downloader = build_downloader(&url)
        .await
        .with_context(|| "构建 downloader 失败".to_string())?;
    eprintln!("[模式] {}", downloader.name());

    let outcome = downloader
        .fetch(&url, output.as_deref(), jobs)
        .await
        .with_context(|| format!("下载失败: {}", url))?;

    println!("[OK] 已保存到 {}({} 字节)", outcome.path.display(), outcome.bytes);
    Ok(())
}

async fn run_login(site: &str) -> Result<()> {
    match site {
        "bilibili" | "bili" | "b" => {
            qrlogin::bilibili::login_with_qrcode()
                .await
                .with_context(|| "B 站扫码登录失败".to_string())?;
        }
        "netease" | "music" | "n" => {
            return Err(anyhow::anyhow!("网易云二维码登录即将在 Lesson 14c 实现"));
        }
        other => {
            return Err(anyhow::anyhow!(
                "未知 site: {} (支持: bilibili / netease)",
                other
            ));
        }
    }
    Ok(())
}

async fn run_logout(site: &str) -> Result<()> {
    let (label, deleted) = match site {
        "bilibili" | "bili" | "b" => ("B 站", auth::bilibili::delete().await?),
        "netease" | "music" | "n" => ("网易云", auth::netease::delete().await?),
        other => {
            return Err(anyhow::anyhow!(
                "未知 site: {} (支持: bilibili / netease)",
                other
            ));
        }
    };
    if deleted {
        println!("已删除本地 {} cookies", label);
    } else {
        println!("本地没有 {} cookies", label);
    }
    Ok(())
}

async fn run_whoami() -> Result<()> {
    match auth::bilibili::load().await.ok().flatten() {
        Some(c) => println!("B 站  : 已登录 (DedeUserID={})", c.dedeuserid),
        None => println!("B 站  : 尚未登录 (用 `saber-dl login`)"),
    }
    match auth::netease::load().await.ok().flatten() {
        Some(_) => println!("网易云: 已登录"),
        None => println!("网易云: 尚未登录 (用 `saber-dl login netease`)"),
    }
    Ok(())
}
