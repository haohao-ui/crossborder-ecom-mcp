use anyhow::{Context, Result};
use async_trait::async_trait;
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::HashMap;
use tracing::info;
use url::Url;

use crate::clean::html_to_clean_markdown;
use crate::http::HttpClient;
use crate::model::{FetchOptions, ProductDetail, ProductReview};
use crate::platforms::PlatformDriver;

pub struct AmazonDriver;

impl AmazonDriver {
    pub fn new() -> Self {
        Self
    }

    /// 从 HTML 文本中提取上架时间 "Date First Available"
    fn extract_launch_date(&self, html_text: &str, document: &Html) -> Option<String> {
        // 1. 尝试从详情列表区域提取 (detailBullets)
        if let Ok(sel) = Selector::parse("#detailBullets_feature_div li, #detailBulletsWrapper_feature_div li") {
            for li in document.select(&sel) {
                let text = li.text().collect::<Vec<_>>().join(" ");
                if text.contains("Date First Available") {
                    let parts: Vec<&str> = text.split(':').collect();
                    if parts.len() >= 2 {
                        let date_str = parts[1].replace(['\n', '\r'], "").trim().to_string();
                        if !date_str.is_empty() {
                            return Some(date_str);
                        }
                    }
                }
            }
        }

        // 2. 尝试从表格行中提取 (productDetails)
        if let Ok(sel) = Selector::parse("tr") {
            for tr in document.select(&sel) {
                let text = tr.text().collect::<Vec<_>>().join(" ");
                if text.contains("Date First Available") {
                    let th_td: Vec<String> = tr
                        .select(&Selector::parse("th, td").unwrap())
                        .map(|el| el.text().collect::<String>().trim().to_string())
                        .collect();
                    if th_td.len() >= 2 {
                        return Some(th_td[1].clone());
                    }
                }
            }
        }

        // 3. 正则强行匹配
        let re = Regex::new(r"(?i)Date First Available\s*[:\t\n<>/a-z=_-]+\s*([A-Za-z]+ \d{1,2}, \d{4})").ok()?;
        if let Some(caps) = re.captures(html_text) {
            return Some(caps.get(1)?.as_str().to_string());
        }

        None
    }

    /// 提取销量表现（例如 "5K+ bought in past month"）
    fn extract_sales_text(&self, document: &Html) -> Option<String> {
        let selectors = [
            "#social-proofing-faceout-title_feature_div span",
            "#social-proofing-faceout-title_feature_div",
            "span[id*='social-proofing']",
        ];

        for s in selectors {
            if let Ok(sel) = Selector::parse(s) {
                for el in document.select(&sel) {
                    let text = el.text().collect::<String>().trim().to_string();
                    if text.contains("bought in past month") || text.contains("bought in past week") {
                        return Some(text);
                    }
                }
            }
        }
        None
    }

    /// 提取大图链接
    fn extract_images(&self, html_text: &str, document: &Html) -> Vec<String> {
        let mut images = Vec::new();

        // 1. 从主图标签提取
        if let Ok(sel) = Selector::parse("#landingImage, #imgBlkFront") {
            if let Some(el) = document.select(&sel).next() {
                if let Some(src) = el.value().attr("src") {
                    images.push(src.to_string());
                }
                if let Some(hires) = el.value().attr("data-old-hires") {
                    if !hires.is_empty() {
                        images.push(hires.to_string());
                    }
                }
            }
        }

        // 2. 从内嵌 JavaScript 的 colorImages 提取高清图
        let re = Regex::new(r#""hiRes"\s*:\s*"(https://images-na\.ssl-images-amazon\.com/images/I/[^"]+)""#).ok();
        if let Some(re) = re {
            for caps in re.captures_iter(html_text) {
                if let Some(m) = caps.get(1) {
                    let img_url = m.as_str().to_string();
                    if !images.contains(&img_url) {
                        images.push(img_url);
                    }
                }
            }
        }

        images
    }
}

#[async_trait]
impl PlatformDriver for AmazonDriver {
    fn name(&self) -> &'static str {
        "amazon"
    }

    fn can_handle(&self, url: &str) -> bool {
        url.contains("amazon.")
    }

    fn extract_item_id(&self, url: &str) -> Option<String> {
        // 匹配 10 位 ASIN 码
        let patterns = [
            r"/dp/([A-Z0-9]{10})",
            r"/gp/product/([A-Z0-9]{10})",
            r"/ASIN/([A-Z0-9]{10})",
            r"/product/([A-Z0-9]{10})",
        ];

        for pattern in patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(url) {
                    if let Some(m) = caps.get(1) {
                        return Some(m.as_str().to_string());
                    }
                }
            }
        }

        None
    }

    async fn fetch_product(
        &self,
        url: &str,
        http: &HttpClient,
        options: &FetchOptions,
    ) -> Result<ProductDetail> {
        let asin = self.extract_item_id(url).context("无法从链接中识别 Amazon ASIN 码")?;
        let parsed_url = Url::parse(url).context("无效的 URL")?;
        let host = parsed_url.host_str().unwrap_or("www.amazon.com");

        // 构建规范的商品访问 URL
        let canonical_url = format!("https://{}/dp/{}", host, asin);
        info!("正在请求亚马逊商品页: ASIN = {}, URL = {}", asin, canonical_url);

        // 注入美区邮编 Cookie (sp-cdn, ubid-main 等)，确保以美区视角展示价格与库存
        let zip = options.zip_code.as_deref().unwrap_or("10001");
        let cookie_str = format!("sp-cdn=\"L5Z9:US\"; lc-main=en_US; i18n-prefs=USD; session-id-time=2082787201l; ubid-main=134-{}-{}", zip, asin);

        let html_text = http.get_text(&canonical_url, Some(&cookie_str)).await?;
        let document = Html::parse_document(&html_text);

        // 1. 标题
        let title_sel = Selector::parse("#productTitle").unwrap();
        let title = document
            .select(&title_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_else(|| format!("Amazon Product {}", asin));

        // 2. 品牌与店铺
        let mut brand = None;
        let mut shop_name = None;
        let mut shop_url = None;
        if let Ok(sel) = Selector::parse("#bylineInfo") {
            if let Some(el) = document.select(&sel).next() {
                let text = el.text().collect::<String>().trim().to_string();
                if let Some(href) = el.value().attr("href") {
                    shop_url = Some(format!("https://{}{}", host, href));
                }
                if text.starts_with("Brand: ") {
                    brand = Some(text.replace("Brand: ", "").trim().to_string());
                } else if text.starts_with("Visit the ") {
                    let s = text.replace("Visit the ", "").replace(" Store", "").trim().to_string();
                    shop_name = Some(s.clone());
                    brand = Some(s);
                } else {
                    shop_name = Some(text);
                }
            }
        }

        // 3. 分类面包屑
        let mut category_path = Vec::new();
        if let Ok(sel) = Selector::parse("#wayfinding-breadcrumbs_feature_div ul li span a") {
            for el in document.select(&sel) {
                let cat = el.text().collect::<String>().trim().to_string();
                if !cat.is_empty() {
                    category_path.push(cat);
                }
            }
        }
        let root_category = category_path.first().cloned();
        let leaf_category = category_path.last().cloned();

        // 4. 价格
        let mut current_price = 0.0;
        let mut original_price = None;
        let mut currency = "USD".to_string();

        if let Ok(sel) = Selector::parse(".a-price .a-offscreen, #priceblock_ourprice, #priceblock_dealprice") {
            if let Some(el) = document.select(&sel).next() {
                let price_text = el.text().collect::<String>().trim().to_string();
                if price_text.starts_with('$') {
                    currency = "USD".to_string();
                    let num_str = price_text.replace(['$', ','], "");
                    current_price = num_str.parse::<f64>().unwrap_or(0.0);
                } else if price_text.starts_with('€') {
                    currency = "EUR".to_string();
                    let num_str = price_text.replace(['€', ','], "");
                    current_price = num_str.parse::<f64>().unwrap_or(0.0);
                } else {
                    let re = Regex::new(r"[\d.]+").unwrap();
                    if let Some(m) = re.find(&price_text) {
                        current_price = m.as_str().parse::<f64>().unwrap_or(0.0);
                    }
                }
            }
        }

        if let Ok(sel) = Selector::parse(".basisPrice .a-offscreen, .a-text-price .a-offscreen") {
            if let Some(el) = document.select(&sel).next() {
                let orig_text = el.text().collect::<String>().trim().to_string();
                let re = Regex::new(r"[\d.]+").unwrap();
                if let Some(m) = re.find(&orig_text) {
                    original_price = m.as_str().parse::<f64>().ok();
                }
            }
        }

        // 5. 销量表现与上架时间
        let sales_text = self.extract_sales_text(&document);
        let launch_date = self.extract_launch_date(&html_text, &document);

        // 6. 评分与评价总数
        let mut rating_score = None;
        let mut review_count = None;
        if let Ok(sel) = Selector::parse("#acrPopover .a-icon-alt, span[data-hook='rating-out-of-text']") {
            if let Some(el) = document.select(&sel).next() {
                let text = el.text().collect::<String>();
                let re = Regex::new(r"([\d.]+)\s*out of").unwrap();
                if let Some(caps) = re.captures(&text) {
                    if let Some(m) = caps.get(1) {
                        rating_score = m.as_str().parse::<f64>().ok();
                    }
                }
            }
        }

        if let Ok(sel) = Selector::parse("#acrCustomerReviewText") {
            if let Some(el) = document.select(&sel).next() {
                let text = el.text().collect::<String>();
                let re = Regex::new(r"([\d,]+)").unwrap();
                if let Some(m) = re.find(&text) {
                    let count_str = m.as_str().replace(',', "");
                    review_count = count_str.parse::<u64>().ok();
                }
            }
        }

        // 7. 主图列表
        let main_images = self.extract_images(&html_text, &document);

        // 8. 技术规格与五点卖点描述
        let mut specifications = HashMap::new();
        if let Ok(sel) = Selector::parse("#productDetails_techSpec_section_1 tr, #technicalSpecifications_section_1 tr") {
            for tr in document.select(&sel) {
                let th = tr.select(&Selector::parse("th").unwrap()).next().map(|el| el.text().collect::<String>().trim().to_string());
                let td = tr.select(&Selector::parse("td").unwrap()).next().map(|el| el.text().collect::<String>().trim().to_string());
                if let (Some(k), Some(v)) = (th, td) {
                    if !k.is_empty() && !v.is_empty() {
                        specifications.insert(k, v);
                    }
                }
            }
        }

        // 五点描述作为 Markdown 卖点列表
        let mut bullet_points = Vec::new();
        if let Ok(sel) = Selector::parse("#feature-bullets ul li span") {
            for el in document.select(&sel) {
                let text = el.text().collect::<String>().trim().to_string();
                if !text.is_empty() && !text.contains("Make sure this fits") {
                    bullet_points.push(format!("- {}", text));
                }
            }
        }

        // 商品介绍详情
        let mut desc_text = String::new();
        if let Ok(sel) = Selector::parse("#productDescription") {
            if let Some(el) = document.select(&sel).next() {
                desc_text = html_to_clean_markdown(&el.html());
            }
        }

        let mut description_markdown = String::new();
        if !bullet_points.is_empty() {
            description_markdown.push_str("### 核心卖点 (Key Features)\n\n");
            description_markdown.push_str(&bullet_points.join("\n"));
            description_markdown.push_str("\n\n");
        }
        if !desc_text.is_empty() {
            description_markdown.push_str("### 商品详细介绍\n\n");
            description_markdown.push_str(&desc_text);
        }

        let search_text = format!("{} {}", title, category_path.join(" "));
        let standard_category = crate::taxonomy::match_category(&search_text);
        let region = crate::taxonomy::match_country_by_url(url);

        let detail = ProductDetail {
            platform: "amazon".to_string(),
            item_id: asin,
            url: canonical_url,
            title,
            brand,
            shop_name,
            shop_url,
            root_category,
            leaf_category,
            category_path,
            currency,
            current_price,
            original_price,
            sales_text,
            launch_date,
            rating_score,
            review_count,
            main_images,
            specifications,
            skus: Vec::new(),
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
        page: u32,
        limit: usize,
    ) -> Result<Vec<ProductReview>> {
        let asin = self.extract_item_id(url).context("无法提取 ASIN 码")?;
        let parsed_url = Url::parse(url).context("无效的 URL")?;
        let host = parsed_url.host_str().unwrap_or("www.amazon.com");

        let reviews_url = format!(
            "https://{}/product-reviews/{}?pageNumber={}&sortBy=recent",
            host, asin, page
        );
        info!("正在拉取 Amazon 买家评论页: {}", reviews_url);

        let html_text = http.get_text(&reviews_url, None).await?;
        let document = Html::parse_document(&html_text);

        let mut list = Vec::new();
        let review_block_sel = Selector::parse("div[data-hook='review']").unwrap();

        for el in document.select(&review_block_sel).take(limit) {
            let review_id = el.value().attr("id").unwrap_or("unknown").to_string();

            // 评价作者
            let author = el
                .select(&Selector::parse(".a-profile-name").unwrap())
                .next()
                .map(|a| a.text().collect::<String>().trim().to_string())
                .unwrap_or_else(|| "Amazon Customer".to_string());

            // 评分
            let rating = el
                .select(&Selector::parse("i[data-hook*='star-rating'] span").unwrap())
                .next()
                .and_then(|r| {
                    let text = r.text().collect::<String>();
                    let re = Regex::new(r"(\d)").ok()?;
                    re.find(&text)?.as_str().parse::<u8>().ok()
                })
                .unwrap_or(5);

            // 评论日期
            let review_date = el
                .select(&Selector::parse("span[data-hook='review-date']").unwrap())
                .next()
                .map(|d| d.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            // 购买规格 SKU
            let sku_purchased = el
                .select(&Selector::parse("a[data-hook='format-strip']").unwrap())
                .next()
                .map(|s| s.text().collect::<String>().trim().to_string());

            // 评论正文
            let content = el
                .select(&Selector::parse("span[data-hook='review-body'] span").unwrap())
                .next()
                .map(|c| c.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            // 晒图
            let mut images = Vec::new();
            if let Ok(img_sel) = Selector::parse("img[data-hook='review-image-tile']") {
                for img in el.select(&img_sel) {
                    if let Some(src) = img.value().attr("src") {
                        images.push(src.to_string());
                    }
                }
            }

            // 有用点赞数
            let helpful_votes = el
                .select(&Selector::parse("span[data-hook='helpful-vote-statement']").unwrap())
                .next()
                .and_then(|h| {
                    let text = h.text().collect::<String>();
                    let re = Regex::new(r"(\d+)").ok()?;
                    re.find(&text)?.as_str().parse::<u32>().ok()
                });

            if !content.is_empty() {
                list.push(ProductReview {
                    review_id,
                    author,
                    rating,
                    review_date,
                    content,
                    sku_purchased,
                    images,
                    helpful_votes,
                });
            }
        }

        Ok(list)
    }
}
