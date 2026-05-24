// 网易云 cookies: MUSIC_U(登录态) + __csrf(防 CSRF)
// 加密保存到 ~/.config/saber-dl/cookies.netease.toml

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::auth::{AuthError, config_dir};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NeteaseCookies {
    pub music_u: String,
    #[serde(default)]
    pub csrf: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedCookies {
    version: u32,
    algo: String,
    data: String,
}

const CURRENT_VERSION: u32 = 1;
const CURRENT_ALGO: &str = "aes-256-gcm/blake3-machine-id";

fn config_path() -> Result<PathBuf, AuthError> {
    Ok(config_dir()?.join("cookies.netease.toml"))
}

pub async fn save(c: &NeteaseCookies) -> Result<(), AuthError> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let plaintext_json = serde_json::to_string(c)?;
    let encrypted = crate::crypto::encrypt(&plaintext_json)?;
    let envelope = EncryptedCookies {
        version: CURRENT_VERSION,
        algo: CURRENT_ALGO.to_string(),
        data: encrypted,
    };
    let toml_text = toml::to_string_pretty(&envelope)?;
    tokio::fs::write(&path, toml_text).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&path).await?.permissions();
        perms.set_mode(0o600);
        tokio::fs::set_permissions(&path, perms).await?;
    }
    Ok(())
}

pub async fn load() -> Result<Option<NeteaseCookies>, AuthError> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let toml_text = tokio::fs::read_to_string(&path).await?;
    let envelope: EncryptedCookies = toml::from_str(&toml_text)?;

    if envelope.version != CURRENT_VERSION {
        return Err(AuthError::Crypto(format!(
            "cookies.netease.toml 版本不匹配(文件 v{},程序 v{}),请重新 saber-dl login netease",
            envelope.version, CURRENT_VERSION
        )));
    }

    let plaintext_json = crate::crypto::decrypt(&envelope.data)?;
    let cookies: NeteaseCookies = serde_json::from_str(&plaintext_json)?;
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
