use std::fs::File;
use std::io;
use clap::Parser;

use anyhow::{Context, Result};

#[derive(Parser)]
struct Args {
    url: String,
    #[arg(short, long)]
    output: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let output_path = args.output.unwrap_or_else(|| {
        args.url
            .rsplit('/')
            .next()
            .unwrap_or("downloaded_file")
            .to_string()
    });

    println!("[GET] {url}", url=args.url);

    // 传递 &args.url，不移动所有权；闭包中直接引用即可（format! 只借用）
    let mut response = reqwest::blocking::get(&args.url)
        .with_context(|| format!("发起 HTTP 请求失败: {0}", args.url))?
        .error_for_status()
        .context("服务器返回了错误状态码")?;

    // 传递 &output_path，不移动所有权
    let mut file = File::create(&output_path)
        .with_context(|| format!("无法创建文件: {output_path}"))?;

    // 执行下载，获取字节数
    let bytes_copied = io::copy(&mut response, &mut file)?;

    println!("[OK] 已保存到 {output_path}（{bytes_copied} 字节）");

    Ok(())
}
