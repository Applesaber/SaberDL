// weapi 加密(L14b 实现)
//   pub fn encrypt(plaintext: &str) -> (String, String)
//        返回 (params_base64, encSecKey_hex)
//   算法: AES-128-CBC PKCS7 ×2 + RSA-1024 raw modpow
