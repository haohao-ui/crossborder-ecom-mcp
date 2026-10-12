use crossborder_ecom_mcp::config::AppConfig;
use crossborder_ecom_mcp::db::Database;
use crossborder_ecom_mcp::http::HttpClient;
use crossborder_ecom_mcp::platforms::amazon::AmazonDriver;
use crossborder_ecom_mcp::platforms::shopify::ShopifyDriver;
use crossborder_ecom_mcp::platforms::PlatformDriver;

#[test]
fn test_mcp_tools_declaration() {
    let drivers: Vec<Box<dyn PlatformDriver>> = vec![
        Box::new(ShopifyDriver::new()),
        Box::new(AmazonDriver::new()),
        Box::new(crossborder_ecom_mcp::platforms::tiktok::TikTokShopDriver::new()),
    ];

    assert_eq!(drivers.len(), 3);
    assert_eq!(drivers[0].name(), "shopify");
    assert_eq!(drivers[1].name(), "amazon");
    assert_eq!(drivers[2].name(), "tiktok");
}

#[tokio::test]
async fn test_config_and_db_initialization() {
    let config = AppConfig::default();
    assert_eq!(config.server.name, "crossborder-ecom-mcp");
    assert_eq!(config.cache.default_ttl_hours, 12);
    assert_eq!(config.platforms.amazon.default_zip_code, "10001");

    let temp_db = format!("data/test_mcp_{}.db", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let _db = Database::init(&temp_db).expect("DB 初始化失败");
    let http = HttpClient::new(&config.network).expect("HTTP 初始化失败");

    assert_eq!(http.retry_count, 2);

    let _ = std::fs::remove_file(&temp_db);
}
