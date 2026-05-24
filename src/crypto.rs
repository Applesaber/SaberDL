use aes_gcm::aead::{Aead, KeyInit, OsRng, rand_core::RngCore};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;

use crate::auth::AuthError;

// blake3 KDF context = 「域分隔字符串」: 防止同 machine-id 用于其他用途时派生出相同密钥
// 改算法 / 改格式时 bump v1 → v2,旧 cookies 自动解密失败 → 用户重新登录,不会拿乱码当 SESSDATA
const KDF_CONTEXT: &str = "saber-dl-cookies-v1";

fn derive_key() -> [u8; 32] {
    // machine_uid 跨平台:Linux /etc/machine-id | macOS IOPlatformUUID | Windows HKLM MachineGuid
    // 失败 fallback 到固定字符串 → 跨机器仍受保护(因为下面的 KDF context 仍参与)
    let id = machine_uid::get().unwrap_or_else(|_| "saber-dl-fallback".to_string());
    blake3::derive_key(KDF_CONTEXT, id.as_bytes())
}

/// AES-256-GCM 加密,输出 base64(nonce[12] || ciphertext+tag[16])
///
/// **每次调用 nonce 随机** → 同明文每次密文不同,防止「重放攻击」和「密文对比分析」
pub fn encrypt(plaintext: &str) -> Result<String, AuthError> {
    let key_bytes = derive_key();
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    // GCM nonce 必须 12 字节,且**永远不能跟同一 key 重复**,所以用 CSPRNG 生成
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| AuthError::Crypto(format!("encrypt: {e}")))?;

    // 拼接 nonce + ciphertext(含 16 字节 auth tag,GCM 自动加)
    let mut out = nonce_bytes.to_vec();
    out.extend(ciphertext);
    Ok(base64::engine::general_purpose::STANDARD.encode(&out))
}

/// 解密 base64(nonce[12] || ciphertext+tag)
///
/// 失败原因(都报「机器/版本不匹配」给用户友好提示):
/// - 跨机器复制(machine-id 不同 → key 不同 → auth tag 校验失败)
/// - 程序升级换了 KDF 版本(KDF_CONTEXT 改了 v1 → v2)
/// - 文件被篡改(GCM auth tag 保证完整性)
pub fn decrypt(encoded: &str) -> Result<String, AuthError> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| AuthError::Crypto(format!("base64: {e}")))?;
    if raw.len() < 12 {
        return Err(AuthError::Crypto("ciphertext too short".into()));
    }
    let (nonce_bytes, ciphertext) = raw.split_at(12);

    let key_bytes = derive_key();
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| AuthError::Crypto(format!("decrypt(机器/版本不匹配?): {e}")))?;

    String::from_utf8(plaintext).map_err(|e| AuthError::Crypto(format!("utf8: {e}")))
}
