// URL 展开器 — 把一个「集合 URL」展开成 N 个「单项 URL」
//   网易云歌单/收藏夹 (L16c)  → N 个 song URL
//   B 站合集/season    (L16c)  → N 个 BV URL
//   其他                      → passthrough (自己 1 个)
//
// L16b 提供骨架 + passthrough,L16c 填充真实展开逻辑

use crate::error::DownloadError;

pub async fn expand(url: &str) -> Result<Vec<String>, DownloadError> {
    Ok(vec![url.to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn passthrough_returns_single_url() {
        let result = expand("https://example.com/foo").await.unwrap();
        assert_eq!(result, vec!["https://example.com/foo".to_string()]);
    }

    #[tokio::test]
    async fn passthrough_preserves_query() {
        let result = expand("https://music.163.com/song?id=12345").await.unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("?id=12345"));
    }
}
