use serde::{Deserialize, Serialize};
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
        Self {
            sessdata,
            bili_jct: String::new(),
            dedeuserid: String::new(),
            refresh_token: None,
        }
    }
}

// 加密信封:实际密文 + 算法元信息,future-proof
// version bump → 旧文件自动失败,提示用户重新登录
#[derive(Debug, Serialize, Deserialize)]
struct EncryptedCookies {
    version: u32,
    algo: String,
    data: String,
}

const CURRENT_VERSION: u32 = 1;
const CURRENT_ALGO: &str = "aes-256-gcm/blake3-machine-id";

fn config_path() -> Result<PathBuf, AuthError> {
    let dir = dirs::config_dir()
        .ok_or(AuthError::NoConfigDir)?
        .join("saber-dl");
    Ok(dir.join("cookies.toml"))
}

pub async fn save(c: &Cookies) -> Result<(), AuthError> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // 流程:Cookies → JSON 字符串 → AES-GCM 加密 → base64 → TOML envelope
    let plaintext_json = serde_json::to_string(c)?;
    let encrypted = crate::crypto::encrypt(&plaintext_json)?;
    let envelope = EncryptedCookies {
        version: CURRENT_VERSION,
        algo: CURRENT_ALGO.to_string(),
        data: encrypted,
    };
    let toml_text = toml::to_string_pretty(&envelope)?;
    tokio::fs::write(&path, toml_text).await?;

    // 双重防线: 0600 让同机器其他用户读不到 + AES 让能读到的人解不开
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&path).await?.permissions();
        perms.set_mode(0o600);
        tokio::fs::set_permissions(&path, perms).await?;
    }
    Ok(())
}

pub async fn load() -> Result<Option<Cookies>, AuthError> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let toml_text = tokio::fs::read_to_string(&path).await?;
    let envelope: EncryptedCookies = toml::from_str(&toml_text)?;

    if envelope.version != CURRENT_VERSION {
        return Err(AuthError::Crypto(format!(
            "cookies.toml 版本不匹配(文件 v{},程序 v{}),请重新 saber-dl login",
            envelope.version, CURRENT_VERSION
        )));
    }

    let plaintext_json = crate::crypto::decrypt(&envelope.data)?;
    let cookies: Cookies = serde_json::from_str(&plaintext_json)?;
    Ok(Some(cookies))
}

pub async fn delete() -> Result<bool, AuthError> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(false);
    }
    tokio::fs::remove_file(&path).await?;
    Ok(true)
}

