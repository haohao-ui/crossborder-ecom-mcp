pub mod aliexpress;
pub mod amazon;
pub mod shopify;
pub mod tiktok;

use anyhow::Result;
use async_trait::async_trait;

use crate::http::HttpClient;
use crate::model::{FetchOptions, ProductDetail, ProductReview};

#[async_trait]
pub trait PlatformDriver: Send + Sync {
    /// 平台英文标识 (如 "amazon", "shopify")
    fn name(&self) -> &'static str;

    /// 判断链接是否属于该平台驱动管辖
    fn can_handle(&self, url: &str) -> bool;

    /// 提取该链接对应的唯一标识 (如 ASIN, Handle)
    fn extract_item_id(&self, url: &str) -> Option<String>;

    /// 抓取商品详情
    async fn fetch_product(
        &self,
        url: &str,
        http: &HttpClient,
        options: &FetchOptions,
    ) -> Result<ProductDetail>;

    /// 抓取买家评论
    async fn fetch_reviews(
        &self,
        url: &str,
        http: &HttpClient,
        page: u32,
        limit: usize,
    ) -> Result<Vec<ProductReview>>;
}
