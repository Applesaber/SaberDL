# SaberDL 下载器

Rust 写的多站点命令行下载器,跟 BBDown / yutto / NeteaseCloudMusicApi 三合一对齐。

## 功能

- **通用 HTTP**: 多线程分块 / 断点续传 / 进度条
- **哔哩哔哩**: 视频 (DASH + ffmpeg) / 多 P / b23 短链 / 封面 / 弹幕 / 收藏夹批量 / wbi 签名 / 扫码登录
- **网易云音乐**: 单曲 / 歌单批量 / ID3 + FLAC 元数据 / 封面嵌入 / 320K + Hi-Res / 扫码 + cookie 双登录
- **统一**: 自动按 URL 路由,多 URL 串行批量,`--file urls.txt`,5 种 shell tab 补全

## 安装

```bash
git clone https://github.com/Applesaber/SaberDL.git && cd SaberDL
cargo build --release
# 产物: target/release/SaberDL
```

依赖: Rust 1.75+ (edition 2024),`ffmpeg` (B 站视频合并需要)。

## 用法

```bash
# 登录 (一次配,终生用,cookies 加密保存)
saber-dl login                                    # B 站扫码
saber-dl login netease                            # 网易云扫码 (含 8821 风控绕过)
saber-dl login netease --cookie 'MUSIC_U=xxx'     # 网易云 cookie 备用方案

# 下载 (URL 自动识别站点)
saber-dl get 'https://www.bilibili.com/video/BV1xxx'
saber-dl get 'https://www.bilibili.com/video/BV1xxx/?p=3'           # 指定分 P
saber-dl get 'https://b23.tv/xxxxxx'                                # B 站短链
saber-dl get 'https://music.163.com/song?id=12345'                  # 网易云单曲
saber-dl get 'https://music.163.com/playlist?id=xxx'                # 歌单一键批量
saber-dl get 'https://www.bilibili.com/medialist/detail/mlxxx'      # B 站收藏夹批量
saber-dl get 'https://example.com/big.zip' -j 16                    # 通用 HTTP

# 批量
saber-dl get URL1 URL2 URL3
saber-dl get -f urls.txt                          # # 开头为注释,空行跳过

# 状态 + 注销
saber-dl whoami
saber-dl logout / logout netease

# Shell 补全 (bash/zsh/fish/powershell/elvish)
saber-dl completion bash > ~/.local/share/bash-completion/completions/saber-dl
```

## 配置

`~/.config/saber-dl/config.toml` 首次启动自动生成:

```toml
[download]
default_jobs = 8                  # 默认下载并发数 (CLI -j 覆盖)

[netease]
default_level = "exhigh"          # 默认音质
```

## 安全

- Cookies 使用 **AES-256-GCM + blake3(machine-id)** 加密保存,跨机器复制无效
- 单独文件: `cookies.bilibili.toml` / `cookies.netease.toml`
- Unix 文件权限 `0600`

## 开发笔记

完整渐进式学习教程见 `lesson.md`(~10000 行,Lesson 1 → Lesson 16,从「同步阻塞 MVP」到「网易云 weapi 加密 + 8821 风控绕过」)。