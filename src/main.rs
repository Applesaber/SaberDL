use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use saber_dl::{auth, build_downloader, config, qrlogin, url_expander};
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "saber-dl", version, about = "SaberDL 下载器")]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// 下载 URL (可多个) / 歌单 / 合集
    Get {
        /// 要下载的 URL (可重复多个)
        urls: Vec<String>,
        /// 从文件读 URL 列表 (每行一个,# 开头是注释)
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,
        /// 输出路径 (单 URL 时是文件名,多 URL 时忽略)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// 下载并发数 (不传则用 config.toml 里的 default_jobs)
        #[arg(short = 'j', long)]
        jobs: Option<usize>,
    },
    /// 通过二维码登录 (bilibili 或 netease,默认 bilibili)
    Login {
        #[arg(default_value = "bilibili")]
        site: String,
        /// 跳过二维码,直接用浏览器复制的 cookie 字符串
        /// (网易云 8821 风控时使用,只支持 netease)
        #[arg(long)]
        cookie: Option<String>,
    },
    /// 删除本地 cookies (bilibili 或 netease,默认 bilibili)
    Logout {
        #[arg(default_value = "bilibili")]
        site: String,
    },
    /// 查看当前登录账号 (B 站 + 网易云)
    Whoami,
    /// 生成 shell 补全脚本 (bash/zsh/fish/powershell/elvish)
    Completion {
        shell: Shell,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match args.cmd {
        Cmd::Get {
            urls,
            file,
            output,
            jobs,
        } => run_get(urls, file, output, jobs).await,
        Cmd::Login { site, cookie } => run_login(&site, cookie).await,
        Cmd::Logout { site } => run_logout(&site).await,
        Cmd::Whoami => run_whoami().await,
        Cmd::Completion { shell } => {
            run_completion(shell);
            Ok(())
        }
    }
}

async fn run_get(
    mut urls: Vec<String>,
    file: Option<PathBuf>,
    output: Option<PathBuf>,
    jobs: Option<usize>,
) -> Result<()> {
    let cfg = config::load().await;
    let jobs = jobs.unwrap_or(cfg.download.default_jobs);

    if let Some(p) = file {
        let text = tokio::fs::read_to_string(&p)
            .await
            .with_context(|| format!("读取 {} 失败", p.display()))?;
        for line in text.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') {
                urls.push(line.to_string());
            }
        }
    }

    if urls.is_empty() {
        return Err(anyhow::anyhow!(
            "没有 URL — 用 `saber-dl get URL` 或 `saber-dl get -f urls.txt`"
        ));
    }

    let mut expanded = Vec::new();
    for u in urls {
        let parts = url_expander::expand(&u)
            .await
            .with_context(|| format!("URL 展开失败: {}", u))?;
        expanded.extend(parts);
    }

    let n = expanded.len();
    if n > 1 && output.is_some() {
        eprintln!("[WARN] 多 URL 时忽略 -o,使用 downloader 自动命名");
    }
    let single_output = if n == 1 { output } else { None };

    if n > 1 {
        println!("[批量] 共 {} 个 URL", n);
    }

    let mut failed = Vec::new();
    for (i, u) in expanded.iter().enumerate() {
        if n > 1 {
            println!("\n──[ {}/{} ] {}", i + 1, n, u);
        }
        if let Err(e) = download_one(u, single_output.as_deref(), jobs).await {
            eprintln!("[FAIL] {}: {:#}", u, e);
            failed.push(u.clone());
        }
    }

    if !failed.is_empty() {
        eprintln!("\n[失败汇总] {} / {} 个 URL:", failed.len(), n);
        for u in &failed {
            eprintln!("  ✗ {}", u);
        }
        return Err(anyhow::anyhow!("{} 个 URL 下载失败", failed.len()));
    }

    if n > 1 {
        println!("\n[全部完成] {} 个 URL", n);
    }
    Ok(())
}

async fn download_one(url: &str, output: Option<&Path>, jobs: usize) -> Result<()> {
    let downloader = build_downloader(url)
        .await
        .with_context(|| "构建 downloader 失败".to_string())?;
    eprintln!("[模式] {}", downloader.name());
    let outcome = downloader
        .fetch(url, output, jobs)
        .await
        .with_context(|| format!("下载失败: {}", url))?;
    println!(
        "[OK] 已保存到 {} ({} 字节)",
        outcome.path.display(),
        outcome.bytes
    );
    Ok(())
}

async fn run_login(site: &str, cookie: Option<String>) -> Result<()> {
    match site {
        "bilibili" | "bili" | "b" => {
            if cookie.is_some() {
                return Err(anyhow::anyhow!(
                    "B 站暂不支持 --cookie 模式,用 `saber-dl login` 扫码"
                ));
            }
            qrlogin::bilibili::login_with_qrcode()
                .await
                .with_context(|| "B 站扫码登录失败".to_string())?;
        }
        "netease" | "music" | "n" => match cookie {
            Some(c) => {
                qrlogin::netease::login_with_cookie(&c)
                    .await
                    .with_context(|| "网易云 cookie 登录失败".to_string())?;
            }
            None => {
                qrlogin::netease::login_with_qrcode()
                    .await
                    .with_context(|| "网易云扫码登录失败".to_string())?;
            }
        },
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

fn run_completion(shell: Shell) {
    let mut cmd = Args::command();
    clap_complete::generate(shell, &mut cmd, "saber-dl", &mut std::io::stdout());
}
