# SaberDL

> 用 Rust 编写的多站点命令行下载器,支持 B 站视频、网易云音乐和通用 HTTP 下载,内置扫码登录、批量任务、断点续传与加密凭据保存。

[![CI](https://github.com/Applesaber/SaberDL/actions/workflows/ci.yml/badge.svg)](https://github.com/Applesaber/SaberDL/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-2024_edition-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)

---

## Why SaberDL

与现有的单站点下载器(如 BBDown、yutto、NeteaseCloudMusicApi)相比,SaberDL 尝试在一个 CLI 中统一处理多种下载场景:

- 一个命令覆盖通用 HTTP、B 站视频、网易云音乐
- 同一套批量、配置、登录体验
- Rust 实现,静态二进制部署,无 runtime 依赖
- 本地凭据使用 AES-256-GCM 加密保存

## Features

### 平台支持

- **B 站**: 视频下载 (DASH + ffmpeg)、分 P、b23 短链解析、封面、弹幕、收藏夹批量、wbi 签名、扫码登录
- **网易云音乐**: 单曲、歌单批量、ID3 / FLAC 元数据写入、封面嵌入、320K / Hi-Res、扫码登录、cookie 登录
- **通用 HTTP**: 多线程分块、断点续传、进度显示

### 通用能力

- URL 自动识别站点并路由
- 多 URL 串行批量下载
- 从文件读取 URL 列表 (`--file urls.txt`)
- Shell 自动补全(bash / zsh / fish / powershell / elvish)
- 全局配置文件,首次启动自动生成

## Quick Start

### Requirements

- Rust 1.75+ (`edition = "2024"`)
- `ffmpeg`(仅在下载 B 站视频并合并音视频时需要)
- 支持平台: Linux / macOS / Windows

### Build from source

```bash
git clone https://github.com/Applesaber/SaberDL.git
cd SaberDL
cargo build --release
```

构建产物:

- Linux / macOS: `target/release/SaberDL`
- Windows: `target/release/SaberDL.exe`

## Usage

### 子命令一览

| 命令 | 用途 |
| --- | --- |
| `get <URL...>` | 下载一个或多个 URL |
| `login [site]` | 二维码或 cookie 登录(默认 B 站) |
| `logout [site]` | 删除本地凭据(默认 B 站) |
| `whoami` | 查看当前登录状态 |
| `completion <shell>` | 生成 shell 补全脚本 |

### 登录

```bash
saber-dl login                                    # B 站扫码登录
saber-dl login netease                            # 网易云扫码登录
saber-dl login netease --cookie 'MUSIC_U=xxx'     # 网易云 cookie 登录
```

凭据加密保存到 `~/.config/saber-dl/cookies.{bilibili,netease}.toml`。

### 下载

```bash
# URL 自动识别站点
saber-dl get 'https://www.bilibili.com/video/BV1xxx'
saber-dl get 'https://www.bilibili.com/video/BV1xxx/?p=3'           # 指定分 P
saber-dl get 'https://b23.tv/xxxxxx'                                # B 站短链
saber-dl get 'https://music.163.com/song?id=12345'                  # 网易云单曲
saber-dl get 'https://music.163.com/playlist?id=xxx'                # 歌单批量
saber-dl get 'https://www.bilibili.com/medialist/detail/mlxxx'      # B 站收藏夹
saber-dl get 'https://example.com/big.zip' -j 16                    # 通用 HTTP
```

### 批量

```bash
# 多 URL 串行
saber-dl get URL1 URL2 URL3

# 从文件读
saber-dl get -f urls.txt
```

`urls.txt` 格式(每行一个 URL,`#` 开头为注释,空行跳过):

```text
# 我的下载清单
https://music.163.com/song?id=12345
https://music.163.com/song?id=67890

https://www.bilibili.com/video/BV1xxx
```

### 状态管理

```bash
saber-dl whoami                # 显示 B 站 + 网易云登录状态
saber-dl logout                # 删除 B 站凭据
saber-dl logout netease        # 删除网易云凭据
```

### Shell 补全

```bash
# bash
saber-dl completion bash > ~/.local/share/bash-completion/completions/saber-dl

# zsh
saber-dl completion zsh > "${fpath[1]}/_saber-dl"

# fish
saber-dl completion fish > ~/.config/fish/completions/saber-dl.fish
```

## Configuration

配置文件位于 `~/.config/saber-dl/config.toml`,首次启动时自动生成。

| 配置项 | 默认值 | 说明 |
| --- | --- | --- |
| `download.default_jobs` | `8` | 默认下载并发数(`-j N` 会覆盖) |
| `netease.default_level` | `"exhigh"` | 网易云默认音质 |

网易云音质等级: `standard` (128K) / `higher` (192K) / `exhigh` (320K) / `lossless` (FLAC) / `hires` (Hi-Res)

## Security

- Cookies 使用 **AES-256-GCM + blake3(machine-id)** 加密保存,可降低被直接复制到其他机器后复用的风险
- 凭据分文件保存: `cookies.bilibili.toml` / `cookies.netease.toml`
- Unix 系统下文件权限为 `0600`
- 不要将 `~/.config/saber-dl/` 下的任何文件提交到代码仓库

## Roadmap

- [ ] TUI 多任务进度面板 (`ratatui`)
- [ ] 跨平台预编译二进制 release
- [ ] 网易云解灰能力(从其他平台兜底下架/无版权曲目)
- [ ] 更多站点支持
- [ ] 跨平台音频转码

## Documentation

完整的渐进式学习教程见 [`lesson.md`](./lesson.md):从「同步阻塞下载 MVP」到「网易云 weapi 加密 + 8821 风控适配」,共 16 节课、约 9800 行,记录了项目从零搭建的每一步技术决策与踩坑过程。

## Acknowledgments

本项目在功能与协议实现上参考了以下开源项目:

- [BBDown](https://github.com/nilaoda/BBDown) — B 站下载能力参考
- [yutto](https://github.com/yutto-dev/yutto) — B 站 CLI 体验参考
- [NeteaseCloudMusicApi](https://github.com/Binaryify/NeteaseCloudMusicApi) — 网易云 weapi 加密协议参考
- [chaunsin/netease-cloud-music](https://github.com/chaunsin/netease-cloud-music) — 网易云风控适配参考

## Contributing

欢迎提交 Issue 与 Pull Request。

简要约定:

- 提交前请运行 `cargo check`、`cargo test --lib` 与 `bash check.sh`
- Commit message 推荐 [Conventional Commits](https://www.conventionalcommits.org/) 风格
- 涉及外部 API 协议变化时,请在 PR 描述中附上抓包或对照实现的链接

## License

[MIT](./LICENSE) © Applesaber