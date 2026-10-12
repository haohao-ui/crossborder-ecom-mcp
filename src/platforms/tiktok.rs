use anyhow::{Context, Result};
use async_trait::async_trait;
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value;
use std::collections::HashMap;
use tracing::info;
use url::Url;

use crate::clean::html_to_clean_markdown;
use crate::http::HttpClient;
use crate::model::{FetchOptions, ProductDetail, ProductReview, ProductSkuItem};
use crate::platforms::PlatformDriver;
use crate::taxonomy;

pub struct TikTokShopDriver;

impl TikTokShopDriver {
    pub fn new() -> Self {
        Self
    }

    /// 解析价格字段 (支持数值或包含货币符号的字符串如 "$19.24", "19.24")
    fn parse_price(&self, val: Option<&Value>) -> Option<f64> {
        val.and_then(|v| {
            if let Some(n) = v.as_f64() {
                Some(n)
            } else if let Some(s) = v.as_str() {
                let cleaned: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
                cleaned.parse::<f64>().ok()
            } else {
                None
            }
        })
    }

    /// 从页面中查找内嵌的 JSON 状态数据 (如 Modern.js SSR 数据, __UNIVERSAL_DATA_FOR_REHYDRATION__, SIGI_STATE 等)
    fn extract_inline_json(&self, html_text: &str, document: &Html) -> Option<Value> {
        // 1. 优先使用高速正则提取结构化 JSON (避免大型 DOM 树遍历与 CSS 下划线转义问题)
        let json_patterns = [
            r#"(?s)<script[^>]*id=["']__MODERN_ROUTER_DATA__["'][^>]*>(.*?)</script>"#,
            r#"(?s)<script[^>]*id=["']__MODERN_SSR_DATA__["'][^>]*>(.*?)</script>"#,
            r#"(?s)<script[^>]*id=["']__UNIVERSAL_DATA_FOR_REHYDRATION__["'][^>]*>(.*?)</script>"#,
            r#"(?s)<script[^>]*id=["']SIGI_STATE["'][^>]*>(.*?)</script>"#,
            r#"(?s)<script[^>]*id=["']__NEXT_DATA__["'][^>]*>(.*?)</script>"#,
            r#"(?s)<script[^>]*id=["']RENDER_DATA["'][^>]*>(.*?)</script>"#,
            r#"(?s)window\.__INIT_DATA__\s*=\s*(\{.*?\});"#,
            r#"(?s)window\.__UNIVERSAL_DATA_FOR_REHYDRATION__\s*=\s*(\{.*?\});"#,
        ];

        for pat in json_patterns {
            if let Ok(re) = Regex::new(pat) {
                if let Some(caps) = re.captures(html_text) {
                    if let Some(m) = caps.get(1) {
                        if let Ok(val) = serde_json::from_str::<Value>(m.as_str()) {
                            return Some(val);
                        }
                    }
                }
            }
        }

        // 2. 降级回退到 Scraper 属性选择器
        let script_selectors = [
            "script[id='__MODERN_ROUTER_DATA__']",
            "script[id='__MODERN_SSR_DATA__']",
            "script[id='__UNIVERSAL_DATA_FOR_REHYDRATION__']",
            "script[id='SIGI_STATE']",
            "script[id='__NEXT_DATA__']",
        ];

        for s in script_selectors {
            if let Ok(sel) = Selector::parse(s) {
                if let Some(el) = document.select(&sel).next() {
                    let text = el.text().collect::<String>();
                    if let Ok(val) = serde_json::from_str::<Value>(&text) {
                        return Some(val);
                    }
                }
            }
        }

        None
    }

    /// 在嵌套的 JSON 树中递归深度搜索可能包含产品信息的子对象
    fn find_product_node<'a>(&self, val: &'a Value) -> Option<&'a Value> {
        if let Some(obj) = val.as_object() {
            if (obj.contains_key("product_id") || obj.contains_key("productId")) && (obj.contains_key("title") || obj.contains_key("skus") || obj.contains_key("price")) {
                return Some(val);
            }
            for v in obj.values() {
                if let Some(found) = self.find_product_node(v) {
                    return Some(found);
                }
            }
        } else if let Some(arr) = val.as_array() {
            for v in arr {
                if let Some(found) = self.find_product_node(v) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// 当直连遇到 TLS 阻断或风控且本地运行有 Chrome 调试实例时，智能复用已加载的 DOM
    async fn try_fetch_from_local_chrome(&self, item_id: &str) -> Option<String> {
        for port in [9333, 9222] {
            let list_url = format!("http://127.0.0.1:{}/json/list", port);
            if let Ok(client) = reqwest::Client::builder().timeout(std::time::Duration::from_millis(600)).build() {
                if let Ok(resp) = client.get(&list_url).send().await {
                    if let Ok(pages) = resp.json::<Vec<Value>>().await {
                        for page in pages {
                            let page_url = page.get("url").and_then(|v| v.as_str()).unwrap_or("");
                            if page_url.contains(item_id) {
                                if let Some(ws_url) = page.get("webSocketDebuggerUrl").and_then(|v| v.as_str()) {
                                    let script = format!(
                                        r#"const ws = new WebSocket("{ws_url}");
ws.onopen = () => ws.send(JSON.stringify({{ id: 1, method: "Runtime.evaluate", params: {{ expression: "document.documentElement.outerHTML" }} }}));
ws.onmessage = (e) => {{
  const d = JSON.parse(e.data);
  if (d.id === 1) {{
    process.stdout.write(d.result.result.value);
    ws.close();
    process.exit(0);
  }}
}};"#,
                                        ws_url = ws_url
                                    );
                                    if let Ok(output) = std::process::Command::new("node")
                                        .arg("-e")
                                        .arg(&script)
                                        .output()
                                    {
                                        if output.status.success() && !output.stdout.is_empty() {
                                            if let Ok(html) = String::from_utf8(output.stdout) {
                                                if html.len() > 1000 {
                                                    info!("已从本地浏览器调试端口 {} 实时获取 TikTok 商品 DOM 内容", port);
                                                    return Some(html);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// 解析 URL 中的出海地区代码 (如 region=US, region=GB, region=ID 等)
    fn detect_region(&self, url_str: &str) -> Option<taxonomy::CountryItem> {
        if let Ok(parsed) = Url::parse(url_str) {
            for (k, v) in parsed.query_pairs() {
                if k.eq_ignore_ascii_case("region") || k.eq_ignore_ascii_case("locale") {
                    let code = v.to_uppercase();
                    let prefix = if code.len() >= 2 { &code[..2] } else { &code };
                    if let Some(c) = taxonomy::TAXONOMY.country.iter().find(|c| c.region_code == prefix) {
                        return Some(c.clone());
                    }
                }
            }
        }

        if url_str.contains("/us/") || url_str.contains("region=US") {
            return taxonomy::TAXONOMY.country.iter().find(|c| c.region_code == "US").cloned();
        }

        // 降级使用域名规则
        taxonomy::match_country_by_url(url_str)
    }
}

#[async_trait]
impl PlatformDriver for TikTokShopDriver {
    fn name(&self) -> &'static str {
        "tiktok"
    }

    fn can_handle(&self, url: &str) -> bool {
        let trimmed = url.trim();
        // 允许直接传入 15~22 位纯数字商品 ID
        if trimmed.chars().all(|c| c.is_ascii_digit()) && trimmed.len() >= 15 && trimmed.len() <= 22 {
            return true;
        }

        let lower = trimmed.to_lowercase();
        (lower.contains("tiktok.com") || lower.contains("tiktokshop.com"))
            && (lower.contains("product") || lower.contains("shop") || lower.contains("pdp") || lower.contains("/item/"))
    }

    fn extract_item_id(&self, url: &str) -> Option<String> {
        let trimmed = url.trim();
        if trimmed.chars().all(|c| c.is_ascii_digit()) && trimmed.len() >= 15 && trimmed.len() <= 22 {
            return Some(trimmed.to_string());
        }

        let patterns = [
            r"/pdp/[^/?#]+/(\d{15,22})",
            r"/pdp/(\d{15,22})",
            r"/product/(\d{15,22})",
            r"product_id=(\d{15,22})",
            r"/item/(\d{15,22})",
            r"/p/(\d{15,22})",
            r"/(\d{17,22})(?:[/?#]|$)",
        ];

        for pattern in patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(trimmed) {
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
        let item_id = self.extract_item_id(url).context("无法识别 TikTok Shop 商品 ID")?;
        
        // 规范化访问 URL
        let fetch_url = if url.trim().chars().all(|c| c.is_ascii_digit()) {
            format!("https://shop.tiktok.com/view/product/{}?region=US&locale=en", item_id)
        } else {
            url.to_string()
        };

        info!("正在拉取 TikTok Shop 商品: ID = {}, URL = {}", item_id, fetch_url);

        let cookie_str = options.cookie.as_deref();
        let html_text = match http.get_text(&fetch_url, cookie_str).await {
            Ok(text) => text,
            Err(e) => {
                // 如果网络请求遭遇 TLS 阻断或连接错误，尝试从本地 Chrome 获取已渲染页面
                if let Some(local_html) = self.try_fetch_from_local_chrome(&item_id).await {
                    local_html
                } else {
                    return Err(e);
                }
            }
        };

        // 核心风控拦截检测 (页面包含真实 SSR 路由数据时表示正常加载，忽略页面中引用的 captcha 依赖库)
        if (html_text.contains("<title>Security Check</title>") || html_text.contains("<title>Access Denied</title>"))
            && !html_text.contains("__MODERN_ROUTER_DATA__")
        {
            // 尝试自愈：检查本地浏览器
            if let Some(local_html) = self.try_fetch_from_local_chrome(&item_id).await {
                return self.parse_html_product(&item_id, &fetch_url, &local_html);
            }

            anyhow::bail!(
                "触发 TikTok Shop 人机安全验证拦截 (Security Check / 滑块验证码)。\n\
                 原因分析：TikTok 对未经认证的纯代码 HTTP 请求强制触发风控验证页。\n\
                 解决方案：\n\
                 1. 请在浏览器中打开该商品链接，完成验证后按 F12 复制网络请求的 Cookie；\n\
                 2. 在 config.toml 中的 [platforms.tiktok] 下添加: cookie = \"你的Cookie\"；\n\
                 3. 若为海外区域（如美区/泰国/印尼），可在 [network] proxy 配置对应区域代理。"
            );
        }

        self.parse_html_product(&item_id, &fetch_url, &html_text)
    }

    async fn fetch_reviews(
        &self,
        url: &str,
        http: &HttpClient,
        _page: u32,
        limit: usize,
    ) -> Result<Vec<ProductReview>> {
        let item_id = self.extract_item_id(url).context("无法提取商品 ID")?;
        info!("正在获取 TikTok Shop 买家评论: ID = {}", item_id);

        let html_text = match http.get_text(url, None).await {
            Ok(t) => t,
            Err(e) => {
                if let Some(local_html) = self.try_fetch_from_local_chrome(&item_id).await {
                    local_html
                } else {
                    return Err(e);
                }
            }
        };

        let document = Html::parse_document(&html_text);
        let mut reviews = Vec::new();

        if let Some(root_data) = self.extract_inline_json(&html_text, &document) {
            // Modern.js review_info 或 通用 reviews
            if let Some(rev_arr) = root_data.pointer("/loaderData").and_then(|ld| {
                if let Some(obj) = ld.as_object() {
                    for (_k, v) in obj {
                        if let Some(reviews) = v.pointer("/page_config/components_map/3/component_data/review_info/product_reviews").and_then(|r| r.as_array()) {
                            return Some(reviews);
                        }
                    }
                }
                None
            }) {
                for (idx, r) in rev_arr.iter().take(limit).enumerate() {
                    let author = r.get("user_name").or_else(|| r.get("nick_name")).and_then(|v| v.as_str()).unwrap_or("TikTok Shopper").to_string();
                    let content = r.get("content").or_else(|| r.get("review_text")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let rating = r.get("rating").or_else(|| r.get("rating_star")).and_then(|v| v.as_u64()).unwrap_or(5) as u8;
                    let date = r.get("post_time").or_else(|| r.get("create_time")).and_then(|v| v.as_i64()).and_then(|ct| {
                        chrono::DateTime::from_timestamp(ct, 0).map(|d| d.format("%Y-%m-%d").to_string())
                    }).unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
                    let sku_info = r.get("sku_specification").and_then(|v| v.as_str()).map(|s| s.to_string());

                    if !content.is_empty() {
                        reviews.push(ProductReview {
                            review_id: format!("tt_{}_{}", item_id, idx + 1),
                            author,
                            rating,
                            review_date: date,
                            content,
                            sku_purchased: sku_info,
                            images: Vec::new(),
                            helpful_votes: None,
                        });
                    }
                }
            } else if let Some(prod) = self.find_product_node(&root_data) {
                if let Some(rev_arr) = prod.get("reviews").or_else(|| prod.get("review_list")).and_then(|v| v.as_array()) {
                    for (idx, r) in rev_arr.iter().take(limit).enumerate() {
                        let author = r.get("user_name").or_else(|| r.get("author")).and_then(|v| v.as_str()).unwrap_or("TikTok User").to_string();
                        let content = r.get("content").or_else(|| r.get("comment")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let rating = r.get("rating").or_else(|| r.get("score")).and_then(|v| v.as_u64()).unwrap_or(5) as u8;
                        let date = r.get("create_time").and_then(|v| v.as_i64()).and_then(|ct| {
                            chrono::DateTime::from_timestamp(ct, 0).map(|d| d.format("%Y-%m-%d").to_string())
                        }).unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());

                        if !content.is_empty() {
                            reviews.push(ProductReview {
                                review_id: format!("tt_{}_{}", item_id, idx + 1),
                                author,
                                rating,
                                review_date: date,
                                content,
                                sku_purchased: None,
                                images: Vec::new(),
                                helpful_votes: None,
                            });
                        }
                    }
                }
            }
        }

        Ok(reviews)
    }
}

impl TikTokShopDriver {
    /// 解析抓取到的 HTML 页面生成结构化商品数据
    pub fn parse_html_product(&self, item_id: &str, url: &str, html_text: &str) -> Result<ProductDetail> {
        let document = Html::parse_document(html_text);
        let inline_json = self.extract_inline_json(html_text, &document);

        let mut title = format!("TikTok Shop Product {}", item_id);
        let mut brand = None;
        let mut shop_name = None;
        let mut shop_url = None;
        let mut current_price = 0.0;
        let mut original_price = None;
        let mut currency = "USD".to_string();
        let mut sales_text = None;
        let mut launch_date = None;
        let mut rating_score = None;
        let mut review_count = None;
        let mut main_images = Vec::new();
        let mut specifications = HashMap::new();
        let mut skus = Vec::new();
        let mut description_markdown = String::new();
        let mut category_path = Vec::new();

        // 1. 如果有内联 JSON，优先解析
        if let Some(root_data) = &inline_json {
            // A. 尝试从 Modern.js 路由数据提取 (TikTok Shop 2024+ 统一架构)
            let mut modern_handled = false;
            if let Some(loader_data) = root_data.get("loaderData").and_then(|v| v.as_object()) {
                for (_route_key, page_obj) in loader_data {
                    if let Some(comp_map) = page_obj.get("page_config").and_then(|v| v.get("components_map")) {
                        let comp_items: Vec<&Value> = if let Some(arr) = comp_map.as_array() {
                            arr.iter().collect()
                        } else if let Some(obj) = comp_map.as_object() {
                            obj.values().collect()
                        } else {
                            Vec::new()
                        };

                        for comp_wrapper in comp_items {
                            if let Some(comp_data) = comp_wrapper.get("component_data") {
                                if let Some(p) = comp_data.get("product_info") {
                                    // 标题
                                    if let Some(t) = p.get("title").and_then(|v| v.as_str()).or_else(|| comp_data.pointer("/aigc_info/aigc_title").and_then(|v| v.as_str())) {
                                        title = t.to_string();
                                    }

                                    // 价格
                                    if let Some(price_obj) = p.get("price") {
                                        if let Some(cur) = price_obj.get("currency").and_then(|v| v.as_str()) {
                                            currency = cur.to_string();
                                        }
                                        if let Some(cp) = self.parse_price(price_obj.get("min_sku_price")).or_else(|| self.parse_price(price_obj.get("real_price"))) {
                                            current_price = cp;
                                        }
                                        if let Some(op) = self.parse_price(price_obj.get("min_sku_original_price")).or_else(|| self.parse_price(price_obj.get("original_price"))) {
                                            original_price = Some(op);
                                        }
                                    }

                                    // 店铺信息
                                    if let Some(shop_info) = comp_data.get("shop_info") {
                                        if let Some(sn) = shop_info.get("shop_name").and_then(|v| v.as_str()) {
                                            shop_name = Some(sn.to_string());
                                        }
                                        if let Some(sl) = shop_info.get("shop_link").and_then(|v| v.as_str()) {
                                            shop_url = Some(sl.to_string());
                                        }
                                    } else if let Some(seller) = p.get("seller") {
                                        if let Some(sn) = seller.get("shop_name").and_then(|v| v.as_str()) {
                                            shop_name = Some(sn.to_string());
                                        }
                                    }

                                    // 销量表现
                                    if let Some(sold) = p.get("sold_count") {
                                        if let Some(s_num) = sold.as_u64() {
                                            sales_text = Some(format!("Sold {}", s_num));
                                        } else if let Some(s_str) = sold.as_str() {
                                            sales_text = Some(format!("Sold {}", s_str));
                                        }
                                    }

                                    // 评分与评价数
                                    if let Some(rev) = p.get("review") {
                                        if let Some(rating_val) = rev.get("product_rating_str").or_else(|| rev.get("product_rating")) {
                                            rating_score = self.parse_price(Some(rating_val));
                                        }
                                        if let Some(rc_val) = rev.get("review_count") {
                                            if let Some(rc_num) = rc_val.as_u64() {
                                                review_count = Some(rc_num);
                                            } else if let Some(rc_str) = rc_val.as_str() {
                                                let cleaned: String = rc_str.chars().filter(|c| c.is_ascii_digit()).collect();
                                                review_count = cleaned.parse::<u64>().ok();
                                            }
                                        }
                                    }

                                    // 主图列表
                                    if let Some(img_arr) = p.get("images").and_then(|v| v.as_array()) {
                                        for img in img_arr {
                                            if let Some(src) = img.pointer("/url_list/0").or_else(|| img.get("url")).or_else(|| img.get("uri")).and_then(|u| u.as_str()) {
                                                main_images.push(src.to_string());
                                            }
                                        }
                                    }

                                    // 属性规格
                                    if let Some(spec_arr) = p.get("specifications").and_then(|v| v.as_array()) {
                                        for s in spec_arr {
                                            if let (Some(k), Some(v)) = (s.get("name").and_then(|x| x.as_str()), s.get("value").and_then(|x| x.as_str())) {
                                                specifications.insert(k.to_string(), v.to_string());
                                                if k.eq_ignore_ascii_case("brand") && brand.is_none() {
                                                    brand = Some(v.to_string());
                                                }
                                            }
                                        }
                                    }

                                    // 面包屑分类
                                    if let Some(cats) = comp_data.get("categories").and_then(|v| v.as_array()) {
                                        for cat in cats {
                                            if let Some(c_name) = cat.get("category_name").and_then(|v| v.as_str()) {
                                                category_path.push(c_name.to_string());
                                            }
                                        }
                                    }

                                    // SKU 变体
                                    if let Some(sku_arr) = p.get("skus").and_then(|v| v.as_array()) {
                                        specifications.insert("变体规格数".to_string(), sku_arr.len().to_string());
                                        for sku in sku_arr {
                                            let sku_id = sku.get("sku_id").map(|v| v.to_string());
                                            let mut sku_names = Vec::new();
                                            if let Some(props) = sku.get("sku_sale_props").and_then(|v| v.as_array()) {
                                                for prop in props {
                                                    if let Some(pv) = prop.get("prop_value").and_then(|v| v.as_str()) {
                                                        sku_names.push(pv);
                                                    }
                                                }
                                            }
                                            let sku_name = if !sku_names.is_empty() {
                                                sku_names.join(" ")
                                            } else {
                                                sku.get("name").or_else(|| sku.get("sku_name")).and_then(|v| v.as_str()).unwrap_or("默认").to_string()
                                            };

                                            let sku_price = sku.get("price").and_then(|pr| {
                                                self.parse_price(pr.get("sale_price_decimal")).or_else(|| self.parse_price(pr.get("sale_price_format")))
                                            });

                                            let sku_orig_price = sku.get("price").and_then(|pr| {
                                                self.parse_price(pr.get("origin_price_decimal")).or_else(|| self.parse_price(pr.get("origin_price_format")))
                                            });

                                            let sku_stock = sku.get("stock").and_then(|v| v.as_i64());
                                            let sku_img = sku.get("image").or_else(|| sku.get("pic")).and_then(|v| v.as_str()).map(|s| s.to_string());

                                            skus.push(ProductSkuItem {
                                                sku_id,
                                                name: sku_name,
                                                price: sku_price,
                                                original_price: sku_orig_price,
                                                stock: sku_stock,
                                                image_url: sku_img,
                                                properties: HashMap::new(),
                                            });
                                        }
                                    }

                                    // 详情页图文内容 (TikTok desc_blocks: 包含参数文本与长图)
                                    let mut desc_lines = Vec::new();
                                    if let Some(blocks) = p.get("desc_blocks").and_then(|v| v.as_array()) {
                                        for b in blocks {
                                            let b_type = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                            if b_type == "text" {
                                                if let Some(txt) = b.get("text").and_then(|v| v.as_str()) {
                                                    let trimmed = txt.trim();
                                                    if !trimmed.is_empty() {
                                                        desc_lines.push(trimmed.to_string());
                                                    }
                                                }
                                            } else if b_type == "image" {
                                                if let Some(img_url) = b.pointer("/image/url_list/0").or_else(|| b.pointer("/image/url")).and_then(|v| v.as_str()) {
                                                    desc_lines.push(format!("![详情图]({})", img_url));
                                                }
                                            }
                                        }
                                    }
                                    if !desc_lines.is_empty() {
                                        description_markdown = desc_lines.join("\n\n");
                                    }

                                    // 详情视频
                                    if let Some(video_url) = p.pointer("/desc_video/video_infos/0/main_url").and_then(|v| v.as_str()) {
                                        specifications.insert("详情视频".to_string(), video_url.to_string());
                                    }

                                    modern_handled = true;
                                    break;
                                }
                            }
                        }
                    }
                    if modern_handled {
                        break;
                    }
                }
            }

            // B. 降级：通用递归搜索 product_node
            if !modern_handled {
                if let Some(prod_data) = self.find_product_node(root_data) {
                    if let Some(t) = prod_data.get("title").or_else(|| prod_data.get("product_name")).or_else(|| prod_data.get("name")).and_then(|v| v.as_str()) {
                        title = t.to_string();
                    }

                    if let Some(seller) = prod_data.get("seller").or_else(|| prod_data.get("shop")).or_else(|| prod_data.get("seller_info")) {
                        if let Some(sn) = seller.get("name").or_else(|| seller.get("shop_name")).and_then(|v| v.as_str()) {
                            shop_name = Some(sn.to_string());
                            if brand.is_none() {
                                brand = Some(sn.to_string());
                            }
                        }
                        if let Some(su) = seller.get("url").or_else(|| seller.get("shop_url")).and_then(|v| v.as_str()) {
                            shop_url = Some(su.to_string());
                        }
                    }

                    if let Some(price_obj) = prod_data.get("price").or_else(|| prod_data.get("product_price")) {
                        if let Some(cur) = price_obj.get("currency").and_then(|v| v.as_str()) {
                            currency = cur.to_string();
                        }
                        if let Some(cp) = self.parse_price(price_obj.get("real_price")).or_else(|| self.parse_price(price_obj.get("min_price"))).or_else(|| self.parse_price(price_obj.get("price"))) {
                            current_price = cp;
                        }
                        if let Some(op) = self.parse_price(price_obj.get("original_price")) {
                            original_price = Some(op);
                        }
                    }

                    if let Some(sold) = prod_data.get("sold_count").or_else(|| prod_data.get("sales")).or_else(|| prod_data.get("sold")) {
                        if let Some(s_num) = sold.as_u64() {
                            sales_text = Some(format!("Sold {}", s_num));
                        } else if let Some(s_str) = sold.as_str() {
                            sales_text = Some(format!("Sold {}", s_str));
                        }
                    }

                    if let Some(rating_obj) = prod_data.get("rating").or_else(|| prod_data.get("review")) {
                        if let Some(score) = self.parse_price(rating_obj.get("score")).or_else(|| self.parse_price(rating_obj.get("product_rating"))) {
                            rating_score = Some(score);
                        }
                        if let Some(rc) = rating_obj.get("count").or_else(|| rating_obj.get("review_count")).and_then(|v| v.as_u64()) {
                            review_count = Some(rc);
                        }
                    }

                    if let Some(ct) = prod_data.get("create_time").and_then(|v| v.as_i64()) {
                        if let Some(dt) = chrono::DateTime::from_timestamp(ct, 0) {
                            launch_date = Some(dt.format("%Y-%m-%d").to_string());
                        }
                    }

                    if let Some(imgs) = prod_data.get("images").or_else(|| prod_data.get("image_list")).and_then(|v| v.as_array()) {
                        for img in imgs {
                            if let Some(src) = img.as_str().or_else(|| img.get("url").and_then(|u| u.as_str())) {
                                main_images.push(src.to_string());
                            }
                        }
                    }

                    if let Some(sku_items) = prod_data.get("skus").or_else(|| prod_data.get("sku_list")).and_then(|v| v.as_array()) {
                        specifications.insert("变体规格数".to_string(), sku_items.len().to_string());
                        for sku_item in sku_items {
                            let sku_name = sku_item.get("name").or_else(|| sku_item.get("sku_name")).and_then(|v| v.as_str()).unwrap_or("默认").to_string();
                            let sku_price = self.parse_price(sku_item.get("price"));
                            let sku_id = sku_item.get("id").or_else(|| sku_item.get("sku_id")).map(|v| v.to_string());
                            let sku_stock = sku_item.get("stock").or_else(|| sku_item.get("inventory")).and_then(|v| v.as_i64());
                            let sku_img = sku_item.get("image").or_else(|| sku_item.get("pic")).and_then(|v| v.as_str()).map(|s| s.to_string());

                            skus.push(ProductSkuItem {
                                sku_id,
                                name: sku_name,
                                price: sku_price,
                                original_price: None,
                                stock: sku_stock,
                                image_url: sku_img,
                                properties: HashMap::new(),
                            });
                        }
                    }

                    if let Some(desc) = prod_data.get("description").or_else(|| prod_data.get("desc")).and_then(|v| v.as_str()) {
                        description_markdown = html_to_clean_markdown(desc);
                    }
                }
            }
        }

        // 2. 回退：从 HTML DOM 补充可能缺失的字段
        if title.starts_with("TikTok Shop Product") {
            if let Ok(sel) = Selector::parse("h1, [data-testid='product-title'], .product-title") {
                if let Some(el) = document.select(&sel).next() {
                    let t = el.text().collect::<String>().trim().to_string();
                    if !t.is_empty() {
                        title = t;
                    }
                }
            }
        }

        if current_price == 0.0 {
            if let Ok(sel) = Selector::parse(".price, [data-testid='product-price'], span[class*='Price']") {
                if let Some(el) = document.select(&sel).next() {
                    let text = el.text().collect::<String>();
                    let re = Regex::new(r"[\d.]+").unwrap();
                    if let Some(m) = re.find(&text) {
                        current_price = m.as_str().parse::<f64>().unwrap_or(0.0);
                    }
                    if text.contains('$') {
                        currency = "USD".to_string();
                    } else if text.contains('£') {
                        currency = "GBP".to_string();
                    }
                }
            }
        }

        if description_markdown.is_empty() {
            if let Ok(sel) = Selector::parse("[data-testid='product-description'], .product-desc, #product-description") {
                if let Some(el) = document.select(&sel).next() {
                    description_markdown = html_to_clean_markdown(&el.html());
                }
            }
        }

        // 3. 关联标准出海分类体系与国家地区
        let search_text = format!("{} {} {}", title, category_path.join(" "), description_markdown);
        let standard_category = taxonomy::match_category(&search_text);
        let region = self.detect_region(url);

        let root_category = category_path.first().cloned().or_else(|| standard_category.as_ref().map(|c| c.main_name.clone()));
        let leaf_category = category_path.last().cloned().or_else(|| standard_category.as_ref().and_then(|c| c.sub_name.clone()));

        let detail = ProductDetail {
            platform: "tiktok".to_string(),
            item_id: item_id.to_string(),
            url: url.to_string(),
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
            skus,
            description_markdown,
            standard_category,
            region,
            updated_at: chrono::Utc::now().timestamp(),
        };

        Ok(detail)
    }
}
