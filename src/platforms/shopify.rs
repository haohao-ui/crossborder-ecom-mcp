use anyhow::{Context, Result};
use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use tracing::info;
use url::Url;

use crate::clean::html_to_clean_markdown;
use crate::http::HttpClient;
use crate::model::{FetchOptions, ProductDetail, ProductReview};
use crate::platforms::PlatformDriver;

pub struct ShopifyDriver;

impl Default for ShopifyDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl ShopifyDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PlatformDriver for ShopifyDriver {
    fn name(&self) -> &'static str {
        "shopify"
    }

    fn can_handle(&self, url: &str) -> bool {
        // Shopify 商品链接通常包含 /products/{handle}
        url.contains("/products/") && !url.contains("amazon.")
    }

    fn extract_item_id(&self, url: &str) -> Option<String> {
        let parsed = Url::parse(url).ok()?;
        let path = parsed.path();
        let re = Regex::new(r"/products/([a-zA-Z0-9_\-]+)").ok()?;
        let caps = re.captures(path)?;
        Some(caps.get(1)?.as_str().to_string())
    }

    async fn fetch_product(
        &self,
        url: &str,
        http: &HttpClient,
        _options: &FetchOptions,
    ) -> Result<ProductDetail> {
        let parsed_url = Url::parse(url).context("无效的 URL")?;
        let host = parsed_url.host_str().context("无法解析域名")?;
        let handle = self.extract_item_id(url).context("无法提取商品 handle")?;

        // 构造官方 .json 端点
        let json_url = format!("https://{}/products/{}.json", host, handle);
        info!("正在获取 Shopify 商品 JSON 数据: {}", json_url);

        let json_text = http.get_text(&json_url, None).await?;
        let data: Value = serde_json::from_str(&json_text)
            .context("Shopify JSON 数据解析失败，可能该独立站关闭了公开接口或使用了非标准架构")?;

        let product = data
            .get("product")
            .context("JSON 响应中缺少 'product' 字段")?;

        let title = product
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("未知商品")
            .to_string();
        let vendor = product
            .get("vendor")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let product_type = product
            .get("product_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // 上架发布时间 (优先 published_at, 降级 created_at)
        let launch_date = product
            .get("published_at")
            .or_else(|| product.get("created_at"))
            .and_then(|v| v.as_str())
            .map(|s| {
                if s.len() >= 10 {
                    s[..10].to_string()
                } else {
                    s.to_string()
                }
            });

        // 详情内容 (HTML 转 Markdown)
        let body_html = product
            .get("body_html")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let description_markdown = html_to_clean_markdown(body_html);

        // 分类与标签
        let tags: Vec<String> = product
            .get("tags")
            .map(|t| {
                if let Some(arr) = t.as_array() {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                } else if let Some(s) = t.as_str() {
                    s.split(',')
                        .map(|item| item.trim().to_string())
                        .filter(|item| !item.is_empty())
                        .collect()
                } else {
                    Vec::new()
                }
            })
            .unwrap_or_default();

        let mut category_path = Vec::new();
        if !product_type.is_empty() {
            category_path.push(product_type.clone());
        }
        category_path.extend(tags.clone());

        let root_category = if !product_type.is_empty() {
            Some(product_type.clone())
        } else {
            tags.first().cloned()
        };
        let leaf_category = tags.last().cloned().or_else(|| root_category.clone());

        // 主图列表
        let mut main_images = Vec::new();
        if let Some(imgs) = product.get("images").and_then(|v| v.as_array()) {
            for img in imgs {
                if let Some(src) = img.get("src").and_then(|v| v.as_str()) {
                    main_images.push(src.to_string());
                }
            }
        }

        // 价格与变体规格
        let mut current_price = 0.0;
        let mut original_price = None;
        let mut specifications = HashMap::new();
        let mut skus = Vec::new();

        if let Some(variants) = product.get("variants").and_then(|v| v.as_array()) {
            specifications.insert("规格总数".to_string(), variants.len().to_string());
            for v in variants {
                let v_title = v
                    .get("title")
                    .and_then(|x| x.as_str())
                    .unwrap_or("默认")
                    .to_string();
                let v_price = v
                    .get("price")
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse::<f64>().ok());
                let v_orig = v
                    .get("compare_at_price")
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse::<f64>().ok());
                let v_sku = v.get("sku").and_then(|x| x.as_str()).map(|s| s.to_string());
                let v_stock = v.get("inventory_quantity").and_then(|x| x.as_i64());

                let mut props = HashMap::new();
                if let Some(opt1) = v.get("option1").and_then(|x| x.as_str()) {
                    props.insert("option1".to_string(), opt1.to_string());
                }
                if let Some(opt2) = v.get("option2").and_then(|x| x.as_str()) {
                    props.insert("option2".to_string(), opt2.to_string());
                }

                skus.push(crate::model::ProductSkuItem {
                    sku_id: v_sku.clone(),
                    name: v_title.clone(),
                    price: v_price,
                    original_price: v_orig,
                    stock: v_stock,
                    image_url: None,
                    properties: props,
                });
            }

            if let Some(first) = variants.first() {
                if let Some(p_str) = first.get("price").and_then(|v| v.as_str()) {
                    current_price = p_str.parse::<f64>().unwrap_or(0.0);
                }
                if let Some(orig_str) = first.get("compare_at_price").and_then(|v| v.as_str()) {
                    original_price = orig_str.parse::<f64>().ok();
                }
                if let Some(sku) = first.get("sku").and_then(|v| v.as_str()) {
                    if !sku.is_empty() {
                        specifications.insert("默认SKU".to_string(), sku.to_string());
                    }
                }
            }

            // 列出前几个变体信息
            let mut variant_summary = Vec::new();
            for (idx, v) in variants.iter().take(5).enumerate() {
                let v_title = v.get("title").and_then(|x| x.as_str()).unwrap_or("默认");
                let v_price = v.get("price").and_then(|x| x.as_str()).unwrap_or("0.00");
                variant_summary.push(format!("{}. {} (${})", idx + 1, v_title, v_price));
            }
            if !variant_summary.is_empty() {
                specifications.insert("主要变体选项".to_string(), variant_summary.join(" | "));
            }
        }

        if let Some(v) = &vendor {
            specifications.insert("品牌/Vendor".to_string(), v.clone());
        }

        let search_text = format!("{} {} {}", title, product_type, tags.join(" "));
        let standard_category = crate::taxonomy::match_category(&search_text);
        let region = crate::taxonomy::match_country_by_url(url);

        let detail = ProductDetail {
            platform: "shopify".to_string(),
            item_id: handle,
            url: url.to_string(),
            title,
            brand: vendor.clone(),
            shop_name: vendor.or_else(|| Some(host.to_string())),
            shop_url: Some(format!("https://{}", host)),
            root_category,
            leaf_category,
            category_path,
            currency: "USD".to_string(),
            current_price,
            original_price,
            sales_text: None, // 独立站通常隐藏真实销量
            launch_date,
            rating_score: None,
            review_count: None,
            main_images,
            specifications,
            skus,
            description_markdown,
            standard_category,
            region,
            updated_at: chrono::Utc::now().timestamp(),
        };

        Ok(detail)
    }

    async fn fetch_reviews(
        &self,
        url: &str,
        http: &HttpClient,
        _page: u32,
        _limit: usize,
    ) -> Result<Vec<ProductReview>> {
        let parsed_url = Url::parse(url).context("无效的 URL")?;
        let host = parsed_url.host_str().context("无法解析域名")?;
        let handle = self.extract_item_id(url).context("无法提取商品 handle")?;

        // 尝试探测主流 Judge.me 公开评论 API
        let judge_me_url = format!(
            "https://judge.me/reviews/reviews_for_widget?url={}&shop_domain={}&platform=shopify",
            handle, host
        );

        if let Ok(resp_text) = http.get_text(&judge_me_url, None).await {
            if let Ok(val) = serde_json::from_str::<Value>(&resp_text) {
                if let Some(html_reviews) = val.get("html").and_then(|v| v.as_str()) {
                    let cleaned = html_to_clean_markdown(html_reviews);
                    if !cleaned.is_empty() {
                        return Ok(vec![ProductReview {
                            review_id: format!("judgeme_{}", handle),
                            author: "Verified Customer".to_string(),
                            rating: 5,
                            review_date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
                            content: cleaned,
                            sku_purchased: None,
                            images: Vec::new(),
                            helpful_votes: None,
                        }]);
                    }
                }
            }
        }

        Ok(Vec::new())
    }
}
