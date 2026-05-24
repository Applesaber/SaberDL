// auth 模块: 站点 cookies 持久化(AES-256-GCM 加密 + machine-id KDF)
//   bilibili: SESSDATA / bili_jct / DedeUserID
//   netease : MUSIC_U  / __csrf

pub mod bilibili;
pub mod netease;

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("TOML serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("TOML deserialize: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("QR code build: {0}")]
    Qr(String),
    #[error("crypto: {0}")]
    Crypto(String),
    #[error("could not determine config directory")]
    NoConfigDir,
    #[error("missing cookie in server response: {0}")]
    MissingCookie(&'static str),
    #[error("API error: code={0}")]
    Api(i64),
    #[error("二维码已过期")]
    QrExpired,
    #[error("登录超时")]
    Timeout,
    #[error("response data missing")]
    EmptyData,
}

// 公共: 配置目录路径(~/.config/saber-dl/)
pub(crate) fn config_dir() -> Result<PathBuf, AuthError> {
    Ok(dirs::config_dir()
        .ok_or(AuthError::NoConfigDir)?
        .join("saber-dl"))
}
