use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub platforms: PlatformsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            storage: StorageConfig::default(),
            cache: CacheConfig::default(),
            network: NetworkConfig::default(),
            platforms: PlatformsConfig::default(),
        }
    }
}

/// 基础服务配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub log_level: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "crossborder-ecom-mcp".to_string(),
            log_level: "info".to_string(),
        }
    }
}

/// 本地数据库与存储配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub db_path: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("data/ecom.db"),
        }
    }
}

/// 缓存与频控策略配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub enabled: bool,
    pub default_ttl_hours: u64,
    pub review_ttl_hours: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_ttl_hours: 12,
            review_ttl_hours: 48,
        }
    }
}

/// 网络、代理与反爬配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub timeout_seconds: u64,
    pub retry_count: usize,
    pub proxy: Option<String>,
    pub user_agent: Option<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 15,
            retry_count: 2,
            proxy: None,
            user_agent: None,
        }
    }
}

/// 各大出海电商平台专属配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformsConfig {
    #[serde(default)]
    pub amazon: AmazonConfig,
    #[serde(default)]
    pub shopify: ShopifyConfig,
    #[serde(default)]
    pub tiktok: TikTokConfig,
    #[serde(default)]
    pub aliexpress: AliExpressConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AliExpressConfig {
    pub cookie: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TikTokConfig {
    pub cookie: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmazonConfig {
    pub default_zip_code: String,
    pub preferred_currency: String,
    pub cookie: Option<String>,
}

impl Default for AmazonConfig {
    fn default() -> Self {
        Self {
            default_zip_code: "10001".to_string(),
            preferred_currency: "USD".to_string(),
            cookie: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShopifyConfig {
    pub fetch_reviews: bool,
}

impl Default for ShopifyConfig {
    fn default() -> Self {
        Self {
            fetch_reviews: true,
        }
    }
}

impl AppConfig {
    /// 尝试从本地加载配置文件，如果不存在则返回默认值
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Self {
        let p = path.as_ref();
        if p.exists() {
            if let Ok(content) = std::fs::read_to_string(p) {
                if let Ok(config) = toml::from_str::<AppConfig>(&content) {
                    return config;
                }
            }
        }
        AppConfig::default()
    }
}
