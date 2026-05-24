// 网易云 weapi 加密
//   POST /weapi/* 接口的请求体加密算法
//   完整算法链:
//     Step 1: secKey = 16 字节随机 ASCII
//     Step 2: enc1   = AES-128-CBC-PKCS7(plaintext, PRESET_KEY, IV) → base64
//     Step 3: params = AES-128-CBC-PKCS7(enc1,      secKey,    IV) → base64
//     Step 4: encSecKey = RSA-1024 raw(reverse(secKey), modulus, exponent) → hex
//   POST body = { params, encSecKey }

use aes::Aes128;
use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use aes_gcm::aead::{OsRng, rand_core::RngCore};
use base64::Engine;
use cbc::Encryptor;
use num_bigint_dig::BigUint;
use num_traits::Num;

type Aes128CbcEnc = Encryptor<Aes128>;

// 网易云硬编码常量 (2009 ~ 至今,十几年没换过)
const PRESET_KEY: &[u8; 16] = b"0CoJUm6Qyw8W8jud";
const IV: &[u8; 16] = b"0102030405060708";
const RSA_MODULUS_HEX: &str = concat!(
    "00e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725",
    "152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312",
    "ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424",
    "d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7",
);
const RSA_EXP_HEX: &str = "010001";
const RSA_OUT_LEN: usize = 128; // 1024 bit / 8 = 128 bytes = 256 hex chars

#[derive(Debug, Clone)]
pub struct WeapiPayload {
    pub params: String,
    pub enc_sec_key: String,
}

// 加密任意 JSON 字符串为 (params, encSecKey) 对
//
// `secKey` 每次调用都随机生成 → 同样的 plaintext 每次密文都不同
pub fn encrypt(plaintext: &str) -> WeapiPayload {
    let sec_key = random_sec_key();

    let enc1 = aes_cbc_pkcs7(plaintext.as_bytes(), PRESET_KEY, IV);
    let enc1_b64 = base64::engine::general_purpose::STANDARD.encode(&enc1);

    let params_bytes = aes_cbc_pkcs7(enc1_b64.as_bytes(), &sec_key, IV);
    let params = base64::engine::general_purpose::STANDARD.encode(&params_bytes);

    // 网易云协议怪癖: secKey 字节反转后才喂给 RSA
    let sec_key_reversed: Vec<u8> = sec_key.iter().rev().copied().collect();
    let enc_sec_key = rsa_no_pad(&sec_key_reversed, RSA_MODULUS_HEX, RSA_EXP_HEX, RSA_OUT_LEN);

    WeapiPayload {
        params,
        enc_sec_key,
    }
}

fn aes_cbc_pkcs7(plaintext: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    Aes128CbcEnc::new(key.into(), iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plaintext)
}

// Raw RSA (无 padding): c = m^e mod n
//
// `out_len` 是固定输出字节数 (128 = 1024 bit RSA)
// 必须补前导零,否则服务端按固定长度解析会失败
fn rsa_no_pad(message: &[u8], pub_key_hex: &str, exp_hex: &str, out_len: usize) -> String {
    let m = BigUint::from_bytes_be(message);
    let n = BigUint::from_str_radix(pub_key_hex, 16).expect("invalid modulus hex");
    let e = BigUint::from_str_radix(exp_hex, 16).expect("invalid exponent hex");

    let c = m.modpow(&e, &n);
    let mut bytes = c.to_bytes_be();

    if bytes.len() < out_len {
        let mut padded = vec![0u8; out_len - bytes.len()];
        padded.extend_from_slice(&bytes);
        bytes = padded;
    } else if bytes.len() > out_len {
        // c < n,所以理论上 bytes.len() <= out_len,真出现说明常量错了
        panic!("RSA output longer than {} bytes", out_len);
    }

    hex::encode(bytes)
}

// 16 字节随机 ASCII secKey
//   字符集跟网易云 JS 客户端一致: [0-9 a-z A-Z]
//   用 OsRng 而非 thread_rng 避免引入 rand 依赖 (aes-gcm 已经间接拉了 OsRng)
fn random_sec_key() -> [u8; 16] {
    const CHARSET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut raw = [0u8; 16];
    OsRng.fill_bytes(&mut raw);
    let mut out = [0u8; 16];
    for (i, &b) in raw.iter().enumerate() {
        out[i] = CHARSET[(b as usize) % CHARSET.len()];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // 用固定 secKey 测试加密链 (脱离随机性后才能写确定性断言)
    fn encrypt_with_fixed_seckey(plaintext: &str, sec_key: &[u8; 16]) -> WeapiPayload {
        let enc1 = aes_cbc_pkcs7(plaintext.as_bytes(), PRESET_KEY, IV);
        let enc1_b64 = base64::engine::general_purpose::STANDARD.encode(&enc1);
        let params_bytes = aes_cbc_pkcs7(enc1_b64.as_bytes(), sec_key, IV);
        let params = base64::engine::general_purpose::STANDARD.encode(&params_bytes);
        let sec_key_reversed: Vec<u8> = sec_key.iter().rev().copied().collect();
        let enc_sec_key = rsa_no_pad(&sec_key_reversed, RSA_MODULUS_HEX, RSA_EXP_HEX, RSA_OUT_LEN);
        WeapiPayload {
            params,
            enc_sec_key,
        }
    }

    #[test]
    fn aes_round1_preset_key_known_value() {
        // 固定 plaintext → 固定 enc1 (PRESET_KEY + IV 都固定)
        // 标杆: openssl 命令行独立计算 (跨实现验证算法正确性):
        //   echo -n "hello" | openssl enc -aes-128-cbc \
        //       -K "$(echo -n '0CoJUm6Qyw8W8jud' | xxd -p)" \
        //       -iv "$(echo -n '0102030405060708' | xxd -p)" | base64
        let enc1 = aes_cbc_pkcs7(b"hello", PRESET_KEY, IV);
        let enc1_b64 = base64::engine::general_purpose::STANDARD.encode(&enc1);
        assert_eq!(enc1_b64, "+J9Q3vLzLGFuqlWFQh3T3A==");
    }

    #[test]
    fn aes_round1_empty_string() {
        // 空字符串 PKCS7 后是 16 字节 \x10
        let enc1 = aes_cbc_pkcs7(b"", PRESET_KEY, IV);
        assert_eq!(enc1.len(), 16);
    }

    #[test]
    fn rsa_output_length_fixed() {
        let result = rsa_no_pad(b"0000000000000000", RSA_MODULUS_HEX, RSA_EXP_HEX, RSA_OUT_LEN);
        assert_eq!(result.len(), RSA_OUT_LEN * 2);
        assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn rsa_known_value_matches_python() {
        // 标杆: Python 内置 pow(m, e, n) 独立计算 (跨实现验证 modpow 正确性):
        //   n = int(RSA_MODULUS_HEX, 16)
        //   e = 0x010001
        //   m = int.from_bytes(b"0000000000000000", "big")
        //   c = pow(m, e, n)
        //   print(c.to_bytes(128, "big").hex())
        let result = rsa_no_pad(b"0000000000000000", RSA_MODULUS_HEX, RSA_EXP_HEX, RSA_OUT_LEN);
        assert_eq!(
            result,
            "babc57ca9e9ffb0a879ae290ac6cba6f60620aa9ae3b36a84585e23bbc73d73b\
             13a2ebab4aa2ee80544d255727adc5a04db613d77d02a62a52b3a03134d16f19\
             1d54675f560f797c7f03e3a30c43df8b1b49878fd225b62f5f78041427debc3e\
             95b93582f130618630702621da4eda9c71af91836cc39ab3b760b033643a1889"
                .replace(' ', "")
                .replace('\n', "")
        );
    }

    #[test]
    fn rsa_deterministic() {
        // raw RSA 没有 padding,同 input 必出同 output (跟标准 RSA-OAEP 不同)
        let a = rsa_no_pad(b"abcdefghijklmnop", RSA_MODULUS_HEX, RSA_EXP_HEX, RSA_OUT_LEN);
        let b = rsa_no_pad(b"abcdefghijklmnop", RSA_MODULUS_HEX, RSA_EXP_HEX, RSA_OUT_LEN);
        assert_eq!(a, b);
    }

    #[test]
    fn fixed_seckey_full_chain() {
        // 完整算法链对照 (固定 secKey = "1234567890abcdef")
        let plain = r#"{"ids":"[1234]","level":"standard","csrf_token":""}"#;
        let sec_key: [u8; 16] = *b"1234567890abcdef";
        let p = encrypt_with_fixed_seckey(plain, &sec_key);
        assert_eq!(p.enc_sec_key.len(), 256);
        assert!(!p.params.is_empty());
        // 确定性: 同 input + 同 secKey → 同 output
        let p2 = encrypt_with_fixed_seckey(plain, &sec_key);
        assert_eq!(p.params, p2.params);
        assert_eq!(p.enc_sec_key, p2.enc_sec_key);
    }

    #[test]
    fn random_seckey_charset() {
        for _ in 0..100 {
            let key = random_sec_key();
            for &b in &key {
                assert!(
                    b.is_ascii_alphanumeric(),
                    "secKey 字符必须是 [0-9a-zA-Z]: got 0x{:02x}",
                    b
                );
            }
        }
    }

    #[test]
    fn encrypt_randomness() {
        // 两次 encrypt 同一明文,因 secKey 随机,params 跟 encSecKey 都应不同
        let plain = r#"{"id":1234}"#;
        let a = encrypt(plain);
        let b = encrypt(plain);
        assert_ne!(a.params, b.params);
        assert_ne!(a.enc_sec_key, b.enc_sec_key);
        assert_eq!(a.enc_sec_key.len(), 256);
        assert_eq!(b.enc_sec_key.len(), 256);
    }
}
