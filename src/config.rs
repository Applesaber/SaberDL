// SaberDL 全局配置 (~/.config/saber-dl/config.toml)
//   load() 永远返回有效 Config — 文件不存在自动创建模板,解析失败 fallback 到 default
//   字段优先级: CLI 参数 > config.toml > 内置默认值

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub download: DownloadConfig,
    pub netease: NeteaseConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct DownloadConfig {
    pub default_jobs: usize,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self { default_jobs: 8 }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct NeteaseConfig {
    pub default_level: String,
}

impl Default for NeteaseConfig {
    fn default() -> Self {
        Self {
            default_level: "exhigh".into(),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("saber-dl").join("config.toml"))
}

const DEFAULT_CONFIG_TEMPLATE: &str = r#"# SaberDL 配置文件
# 改完保存即生效,所有字段都可省略 (用默认值)
# CLI 参数永远优先于本文件

[download]
# 默认下载并发数 (-j N 会覆盖)
default_jobs = 8

[netease]
# 默认音质: standard (128K) | higher (192K) | exhigh (320K) | lossless (FLAC) | hires (Hi-Res)
default_level = "exhigh"
"#;

pub async fn load() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };

    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::write(&path, DEFAULT_CONFIG_TEMPLATE).await;
        return Config::default();
    }

    match tokio::fs::read_to_string(&path).await {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("[WARN] config.toml 解析失败 ({}),用默认值", e);
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let c = Config::default();
        assert_eq!(c.download.default_jobs, 8);
        assert_eq!(c.netease.default_level, "exhigh");
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let toml_text = r#"
            [download]
            default_jobs = 16
        "#;
        let c: Config = toml::from_str(toml_text).unwrap();
        assert_eq!(c.download.default_jobs, 16);
        assert_eq!(c.netease.default_level, "exhigh");
    }

    #[test]
    fn empty_toml_all_defaults() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.download.default_jobs, 8);
        assert_eq!(c.netease.default_level, "exhigh");
    }
}
