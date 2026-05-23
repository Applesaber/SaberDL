use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("TOML deserialize: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("QR code build: {0}")]
    Qr(String),
    #[error("could not determine config directory")]
    NoConfigDir,
    #[error("missing cookie in server response: {0}")]
    MissingCookie(&'static str),
    #[error("Bilibili API error: code={0}")]
    Api(i64),
    #[error("二维码已过期")]
    QrExpired,
    #[error("登录超时")]
    Timeout,
    #[error("response data missing")]
    EmptyData,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Cookies {
    pub sessdata: String,
    pub bili_jct: String,
    pub dedeuserid: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

impl Cookies {
    pub fn sessdata_only(sessdata: String) -> Self {
        Self { sessdata, bili_jct: String::new(), dedeuserid: String::new(), refresh_token: None }
    }
}

fn config_path() -> Result<PathBuf, AuthError> {
    let dir = dirs::config_dir().ok_or(AuthError::NoConfigDir)?.join("saber-dl");
    Ok(dir.join("cookies.toml"))
}

pub async fn save(c: &Cookies) -> Result<(), AuthError> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let s = toml::to_string_pretty(c)?;
    tokio::fs::write(&path, s).await?;
    Ok(())
}

pub async fn load() -> Result<Option<Cookies>, AuthError> {
    let path = config_path()?;
    if !path.exists() { return Ok(None); }
    let s = tokio::fs::read_to_string(&path).await?;
    Ok(Some(toml::from_str::<Cookies>(&s)?))
}

pub async fn delete() -> Result<bool, AuthError> {
    let path = config_path()?;
    if !path.exists() { return Ok(false); }
    tokio::fs::remove_file(&path).await?;
    Ok(true)
}