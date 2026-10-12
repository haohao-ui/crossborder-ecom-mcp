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
use crate::model::{FetchOptions, ProductDetail, ProductReview};
use crate::platforms::PlatformDriver;
use crate::taxonomy;

pub struct AliExpressDriver;

impl AliExpressDriver {
    pub fn new() -> Self {
        Self
    }

    /// 从页面中提取内嵌的 JSON 数据 (如 window.runParams, window._init_data_, application/ld+json)
    fn extract_inline_data(&self, html_text: &str, document: &Html) -> Option<Value> {
        // 1. 尝试解析 window.runParams = { ... };
        let run_params_patterns = [
            r#"window\.runParams\s*=\s*(\{.*?\});\s*(?:window\.|var|<)"#,
            r#"window\.runParams\s*=\s*(\{.*?\});"#,
            r#"data:\s*(\{.*?"titleModule".*?\}),\s*csrfToken"#,
            r#"window\._init_data_\s*=\s*(\{.*?\});"#,
            r#"window\.__AER_DATA__\s*=\s*(\{.*?\});"#,
        ];

        for pat in run_params_patterns {
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

        // 2. 尝试从 script[type="application/ld+json"] 结构化数据解析
        if let Ok(sel) = Selector::parse(r#"script[type="application/ld+json"]"#) {
            for el in document.select(&sel) {
                let text = el.text().collect::<String>();
                if let Ok(val) = serde_json::from_str::<Value>(&text) {
                    let schema_type = val.get("@type").and_then(|t| t.as_str()).unwrap_or("");
                    if schema_type.eq_ignore_ascii_case("Product") {
                        return Some(val);
                    }
                }
            }
        }

        None
    }

    /// 在嵌套的 JSON 中递归查找产品相关节点
    fn find_nested_node<'a>(&self, val: &'a Value, keys: &[&str]) -> Option<&'a Value> {
        if let Some(obj) = val.as_object() {
            for key in keys {
                if let Some(found) = obj.get(*key) {
                    return Some(found);
                }
            }
            for v in obj.values() {
                if let Some(found) = self.find_nested_node(v, keys) {
                    return Some(found);
                }
            }
        } else if let Some(arr) = val.as_array() {
            for v in arr {
                if let Some(found) = self.find_nested_node(v, keys) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// 检测目标出海国家/地区
    fn detect_region(&self, url_str: &str) -> Option<taxonomy::CountryItem> {
        if let Ok(parsed) = Url::parse(url_str) {
            for (k, v) in parsed.query_pairs() {
                if k.eq_ignore_ascii_case("country") || k.eq_ignore_ascii_case("locale") || k.eq_ignore_ascii_case("shipTo") {
                    let code = v.to_uppercase();
                    let prefix = if code.len() >= 2 { &code[..2] } else { &code };
                    if let Some(c) = taxonomy::TAXONOMY.country.iter().find(|c| c.region_code == prefix) {
                        return Some(c.clone());
                    }
                }
            }
        }

        taxonomy::match_country_by_url(url_str)
    }
}

#[async_trait]
impl PlatformDriver for AliExpressDriver {
    fn name(&self) -> &'static str {
        "aliexpress"
    }

    fn can_handle(&self, url: &str) -> bool {
        let lower = url.to_lowercase();
        (lower.contains("aliexpress.com") || lower.contains("aliexpress.us") || lower.contains("aliexpress.ru"))
            && (lower.contains("item") || lower.contains("/i/") || lower.contains("product"))
    }

    fn extract_item_id(&self, url: &str) -> Option<String> {
        // 支持类似:
        // /item/1005006283920192.html
        // /i/1005006283920192.html
        // productId=1005006283920192
        // item_id=1005006283920192
        let patterns = [
            r"/(?:item|i)/(\d+)\.html",
            r"/(?:item|i)/(\d+)",
            r"[?&](?:productId|item_id|id)=(\d+)",
        ];

        for pat in patterns {
            if let Ok(re) = Regex::new(pat) {
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
        let item_id = self.extract_item_id(url).context("无法识别 AliExpress 商品 ID")?;
        info!("正在拉取 AliExpress 商品: ID = {}, URL = {}", item_id, url);

        let cookie_str = options.cookie.as_deref();
        let html_text = http.get_text(url, cookie_str).await?;

        // 阿里巴巴集团天网/滑块验证安全拦截检测 (rgv587_flag, _____tmd_____, action: captcha)
        if html_text.contains("_____tmd_____")
            || html_text.contains("rgv587_flag")
            || html_text.contains(r#""action":"captcha""#)
            || html_text.contains("login_jump")
        {
            anyhow::bail!(
                "触发 AliExpress (速卖通) 阿里系安全风控拦截 (rgv587_flag / _____tmd_____ 滑块验证)。\n\
                 原因分析：AliExpress 对未经浏览器认证的纯代码 HTTP 请求触发了阿里天网反爬拦截。\n\
                 解决方案：\n\
                 1. 请在浏览器中打开该 AliExpress 商品链接，完成验证后按 F12 复制网络请求中的 Cookie；\n\
                 2. 在 config.toml 中的 [platforms.aliexpress] 下添加: cookie = \"你的Cookie\"；\n\
                 3. 若为海外特定地区（如美区、欧区），可在 [network] proxy 配置对应区域代理。"
            );
        }

        let document = Html::parse_document(&html_text);
        let inline_data = self.extract_inline_data(&html_text, &document);

        let mut title = format!("AliExpress Product {}", item_id);
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

        // 1. 优先提取 Meta 结构化标头 (AliExpress 全球站最稳定的高保真数据源)
        if let Ok(sel) = Selector::parse(r#"meta[property="og:title"], meta[name="twitter:title"]"#) {
            for el in document.select(&sel) {
                if let Some(content) = el.value().attr("content") {
                    let cleaned = clean_aliexpress_title(content);
                    if !cleaned.is_empty() && !cleaned.eq_ignore_ascii_case("aliexpress") {
                        title = cleaned;
                        break;
                    }
                }
            }
        }

        if let Ok(sel) = Selector::parse(r#"meta[property="og:image"], meta[name="twitter:image"]"#) {
            for el in document.select(&sel) {
                if let Some(img_url) = el.value().attr("content") {
                    let full_img = if img_url.starts_with("//") {
                        format!("https:{}", img_url)
                    } else {
                        img_url.to_string()
                    };
                    if !main_images.contains(&full_img) {
                        main_images.push(full_img);
                    }
                }
            }
        }

        if let Ok(sel) = Selector::parse(r#"meta[property="og:description"], meta[name="description"]"#) {
            for el in document.select(&sel) {
                if let Some(content) = el.value().attr("content") {
                    let desc = content.trim();
                    if !desc.is_empty() && !desc.contains("Smarter Shopping, Better Living") {
                        description_markdown = html_to_clean_markdown(desc);
                        break;
                    }
                }
            }
        }

        // 2. 尝试提取 window._d_c_.DCData 高清原图集
        let dcdata_re = Regex::new(r#"window\._d_c_\.DCData\s*=\s*(\{.*?\});"#).unwrap();
        if let Some(caps) = dcdata_re.captures(&html_text) {
            if let Some(m) = caps.get(1) {
                if let Ok(val) = serde_json::from_str::<Value>(m.as_str()) {
                    if let Some(imgs) = val.get("imagePathList").and_then(|v| v.as_array()) {
                        for img in imgs {
                            if let Some(src) = img.as_str() {
                                let full_src = if src.starts_with("//") { format!("https:{}", src) } else { src.to_string() };
                                if !main_images.contains(&full_src) {
                                    main_images.push(full_src);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3. 如果有结构化/内联数据 (runParams 或 Schema.org Product)，进一步丰富
        if let Some(ref data) = inline_data {
            // 兼容 Schema.org Product
            if data.get("@type").and_then(|t| t.as_str()).map(|t| t.eq_ignore_ascii_case("Product")).unwrap_or(false) {
                if let Some(n) = data.get("name").and_then(|v| v.as_str()) {
                    let cleaned = clean_aliexpress_title(n);
                    if !cleaned.is_empty() && !cleaned.eq_ignore_ascii_case("aliexpress") {
                        title = cleaned;
                    }
                }
                if let Some(b) = data.get("brand").and_then(|v| v.get("name")).and_then(|v| v.as_str()) {
                    brand = Some(b.to_string());
                }
                if let Some(desc) = data.get("description").and_then(|v| v.as_str()) {
                    if description_markdown.is_empty() {
                        description_markdown = html_to_clean_markdown(desc);
                    }
                }
                if let Some(date) = data.get("releaseDate").or_else(|| data.get("datePublished")).and_then(|v| v.as_str()) {
                    launch_date = Some(date.to_string());
                }
                if let Some(img) = data.get("image") {
                    if let Some(arr) = img.as_array() {
                        for i in arr {
                            if let Some(s) = i.as_str() {
                                let full = if s.starts_with("//") { format!("https:{}", s) } else { s.to_string() };
                                if !main_images.contains(&full) {
                                    main_images.push(full);
                                }
                            }
                        }
                    } else if let Some(s) = img.as_str() {
                        let full = if s.starts_with("//") { format!("https:{}", s) } else { s.to_string() };
                        if !main_images.contains(&full) {
                            main_images.push(full);
                        }
                    }
                }
                if let Some(offers) = data.get("offers") {
                    if let Some(p) = offers.get("price").or_else(|| offers.get("lowPrice")) {
                        if let Some(p_f) = p.as_f64() {
                            current_price = p_f;
                        } else if let Some(p_s) = p.as_str() {
                            current_price = p_s.parse().unwrap_or(0.0);
                        }
                    }
                    if let Some(hp) = offers.get("highPrice") {
                        if let Some(hp_f) = hp.as_f64() {
                            original_price = Some(hp_f);
                        } else if let Some(hp_s) = hp.as_str() {
                            original_price = hp_s.parse().ok();
                        }
                    }
                    if let Some(cur) = offers.get("priceCurrency").and_then(|v| v.as_str()) {
                        currency = cur.to_string();
                    }
                }
                if let Some(agg) = data.get("aggregateRating") {
                    if let Some(r) = agg.get("ratingValue") {
                        if let Some(rf) = r.as_f64() {
                            rating_score = Some(rf);
                        } else if let Some(rs) = r.as_str() {
                            rating_score = rs.parse().ok();
                        }
                    }
                    if let Some(c) = agg.get("reviewCount") {
                        if let Some(cu) = c.as_u64() {
                            review_count = Some(cu);
                        } else if let Some(cs) = c.as_str() {
                            review_count = cs.parse().ok();
                        }
                    }
                }
            }

            // 兼容 AliExpress runParams 模块 (titleModule, priceModule, storeModule, etc.)
            if let Some(title_mod) = self.find_nested_node(data, &["titleModule", "title_module"]) {
                if let Some(subj) = title_mod.get("subject").or_else(|| title_mod.get("title")).and_then(|v| v.as_str()) {
                    let cleaned = clean_aliexpress_title(subj);
                    if !cleaned.is_empty() && !cleaned.eq_ignore_ascii_case("aliexpress") {
                        title = cleaned;
                    }
                }
            }

            if let Some(store_mod) = self.find_nested_node(data, &["storeModule", "store_module", "seller"]) {
                if let Some(sn) = store_mod.get("storeName").or_else(|| store_mod.get("name")).and_then(|v| v.as_str()) {
                    shop_name = Some(sn.to_string());
                    if brand.is_none() {
                        brand = Some(sn.to_string());
                    }
                }
                if let Some(su) = store_mod.get("storeURL").or_else(|| store_mod.get("url")).and_then(|v| v.as_str()) {
                    shop_url = Some(su.to_string());
                }
            }

            if let Some(price_mod) = self.find_nested_node(data, &["priceModule", "price_module"]) {
                if let Some(cur) = price_mod.get("currencyCode").or_else(|| price_mod.get("currency")).and_then(|v| v.as_str()) {
                    currency = cur.to_string();
                }
                if let Some(act) = price_mod.get("minActivityAmount").or_else(|| price_mod.get("minAmount")) {
                    if let Some(v) = act.get("value").and_then(|v| v.as_f64()) {
                        current_price = v;
                    }
                }
                if current_price == 0.0 {
                    if let Some(f_price) = price_mod.get("formatedActivityPrice").or_else(|| price_mod.get("formatedAmount")).and_then(|v| v.as_str()) {
                        let re = Regex::new(r"[\d.]+").unwrap();
                        if let Some(m) = re.find(f_price) {
                            current_price = m.as_str().parse().unwrap_or(0.0);
                        }
                    }
                }
                if let Some(max_act) = price_mod.get("maxActivityAmount").or_else(|| price_mod.get("maxAmount")) {
                    if let Some(v) = max_act.get("value").and_then(|v| v.as_f64()) {
                        original_price = Some(v);
                    }
                }
            }

            if let Some(trade_mod) = self.find_nested_node(data, &["tradeModule", "trade_module"]) {
                if let Some(fmt_trade) = trade_mod.get("formatTradeCount").or_else(|| trade_mod.get("tradeCount")).and_then(|v| v.as_str()) {
                    sales_text = Some(format!("{} sold", fmt_trade));
                }
            }

            if let Some(fb_mod) = self.find_nested_node(data, &["feedbackModule", "feedback_module"]) {
                if let Some(star) = fb_mod.get("averageStar").or_else(|| fb_mod.get("eExpressFeedbackStar")).and_then(|v| v.as_f64()) {
                    rating_score = Some(star);
                }
                if let Some(num) = fb_mod.get("totalValidNum").and_then(|v| v.as_u64()) {
                    review_count = Some(num);
                }
            }

            if let Some(img_mod) = self.find_nested_node(data, &["imageModule", "image_module"]) {
                if let Some(list) = img_mod.get("imagePathList").and_then(|v| v.as_array()) {
                    for i in list {
                        if let Some(src) = i.as_str() {
                            let full = if src.starts_with("//") { format!("https:{}", src) } else { src.to_string() };
                            if !main_images.contains(&full) {
                                main_images.push(full);
                            }
                        }
                    }
                }
            }

            if let Some(specs_mod) = self.find_nested_node(data, &["specsModule", "specs_module"]) {
                if let Some(props) = specs_mod.get("props").and_then(|v| v.as_array()) {
                    for prop in props {
                        let k = prop.get("attrName").and_then(|v| v.as_str()).unwrap_or("");
                        let val = prop.get("attrValue").and_then(|v| v.as_str()).unwrap_or("");
                        if !k.is_empty() && !val.is_empty() {
                            specifications.insert(k.to_string(), val.to_string());
                        }
                    }
                }
            }

            // 深度解析 skuModule 结构化变体数据 (若有 Cookie/SSR 数据)
            if let Some(sku_mod) = self.find_nested_node(data, &["skuModule", "sku_module"]) {
                let mut prop_value_map: HashMap<String, (String, String)> = HashMap::new();
                if let Some(prop_list) = sku_mod.get("productSKUPropertyList").and_then(|v| v.as_array()) {
                    for p in prop_list {
                        let prop_name = p.get("skuPropertyName").and_then(|v| v.as_str()).unwrap_or("");
                        let prop_id = p.get("skuPropertyId").map(|v| v.to_string()).unwrap_or_default();
                        if let Some(vals) = p.get("skuPropertyValues").and_then(|v| v.as_array()) {
                            for val in vals {
                                let val_id = val.get("propertyValueId").map(|v| v.to_string()).unwrap_or_default();
                                let val_name = val.get("propertyValueDisplayName").or_else(|| val.get("propertyValueName")).and_then(|v| v.as_str()).unwrap_or("");
                                prop_value_map.insert(format!("{}:{}", prop_id, val_id), (prop_name.to_string(), val_name.to_string()));
                            }
                        }
                    }
                }

                if let Some(price_list) = sku_mod.get("skuPriceList").and_then(|v| v.as_array()) {
                    for item in price_list {
                        let sku_id = item.get("skuId").map(|v| v.to_string());
                        let sku_val = item.get("skuVal").cloned().unwrap_or(Value::Null);
                        let price = sku_val.get("skuActivityAmount").or_else(|| sku_val.get("skuAmount"))
                            .and_then(|v| v.get("value")).and_then(|v| v.as_f64());
                        let orig_price = sku_val.get("skuAmount").and_then(|v| v.get("value")).and_then(|v| v.as_f64());
                        let stock = sku_val.get("availQuantity").and_then(|v| v.as_i64());
                        let sku_attr = item.get("skuAttr").and_then(|v| v.as_str()).unwrap_or("");

                        let mut props = HashMap::new();
                        let mut name_parts = Vec::new();
                        for part in sku_attr.split(';') {
                            if let Some((p_name, v_name)) = prop_value_map.get(part) {
                                props.insert(p_name.clone(), v_name.clone());
                                name_parts.push(v_name.clone());
                            }
                        }
                        let name = if !name_parts.is_empty() { name_parts.join(" ") } else { format!("SKU {}", sku_id.as_deref().unwrap_or("默认")) };
                        skus.push(crate::model::ProductSkuItem {
                            sku_id,
                            name,
                            price,
                            original_price: orig_price,
                            stock,
                            image_url: None,
                            properties: props,
                        });
                    }
                }
            }
        }

        // 4. DOM 兜底抽取（必须排除包含 logo 的顶级 h1）
        if title.starts_with("AliExpress Product") || title.eq_ignore_ascii_case("aliexpress") {
            if let Ok(sel) = Selector::parse("[data-pl='product-title'], .product-title-text, h1.product-title") {
                if let Some(el) = document.select(&sel).next() {
                    let t = el.text().collect::<String>().trim().to_string();
                    let cleaned = clean_aliexpress_title(&t);
                    if !cleaned.is_empty() && !cleaned.eq_ignore_ascii_case("aliexpress") {
                        title = cleaned;
                    }
                }
            }
        }

        if current_price == 0.0 {
            if let Ok(sel) = Selector::parse(".product-price-current, .uniform-banner-box-price, span[class*='Price']") {
                if let Some(el) = document.select(&sel).next() {
                    let text = el.text().collect::<String>();
                    let re = Regex::new(r"[\d.]+").unwrap();
                    if let Some(m) = re.find(&text) {
                        current_price = m.as_str().parse().unwrap_or(0.0);
                    }
                    if text.contains('$') {
                        currency = "USD".to_string();
                    } else if text.contains('€') {
                        currency = "EUR".to_string();
                    } else if text.contains('£') {
                        currency = "GBP".to_string();
                    }
                }
            }
        }

        if sales_text.is_none() {
            if let Ok(sel) = Selector::parse(".product-reviewer-sold, span[class*='sold'], span[class*='Sold']") {
                for el in document.select(&sel) {
                    let t = el.text().collect::<String>().trim().to_string();
                    if t.to_lowercase().contains("sold") || t.contains("件") {
                        sales_text = Some(t);
                        break;
                    }
                }
            }
        }

        if shop_name.is_none() {
            if let Ok(sel) = Selector::parse("a.shop-name-link, a[class*='store-name'], a[class*='shop-name']") {
                if let Some(el) = document.select(&sel).next() {
                    let sn = el.text().collect::<String>().trim().to_string();
                    if !sn.is_empty() {
                        shop_name = Some(sn.clone());
                        if brand.is_none() {
                            brand = Some(sn);
                        }
                    }
                }
            }
        }

        if description_markdown.is_empty() {
            if let Ok(sel) = Selector::parse("#product-description, .product-description, [data-pl='product-description']") {
                if let Some(el) = document.select(&sel).next() {
                    description_markdown = html_to_clean_markdown(&el.html());
                }
            }
        }

        // 5. CSR 渲染模式检测与智能多规格/变体推导展开
        let is_csr = html_text.contains("isCSR = true") || html_text.contains(r#""isCSR":true"#);
        if is_csr && current_price == 0.0 {
            info!(
                "提示: 当前商品页面为 AliExpress CSR 骨架屏渲染模式。已提取高保真标题、主图图集与出海分类体系。如需获取实时售价与完整动态库存，请在 config.toml 的 [platforms.aliexpress] 中配置 Cookie。"
            );
        }

        // 针对未内联 skuModule 的情况，从标题及页面智能提取多变体规格（例如鞋带的 100/120/140/160CM 尺寸列表）
        if skus.is_empty() {
            let size_re = Regex::new(r"(\d+(?:/\d+)+(?:CM|cm|MM|mm|M|m|Inch|inch))").unwrap();
            let mut thickness = String::new();
            if let Ok(th_re) = Regex::new(r"(\d+(?:\.\d+)?\s*(?:mm|MM|cm|CM))") {
                if let Some(caps) = th_re.captures(&title) {
                    if let Some(m) = caps.get(1) {
                        thickness = m.as_str().to_string();
                        specifications.insert("粗细规格".to_string(), thickness.clone());
                    }
                }
            }

            if let Some(caps) = size_re.captures(&title) {
                if let Some(m) = caps.get(1) {
                    let full_match = m.as_str();
                    let unit = full_match.chars().filter(|c| c.is_alphabetic()).collect::<String>();
                    let nums_part = full_match.trim_end_matches(&unit);
                    let sub_sizes: Vec<&str> = nums_part.split('/').collect();
                    for (idx, s) in sub_sizes.iter().enumerate() {
                        let size_label = format!("{}{}", s, unit);
                        let mut props = HashMap::new();
                        props.insert("长度尺寸".to_string(), size_label.clone());
                        if !thickness.is_empty() {
                            props.insert("粗细规格".to_string(), thickness.clone());
                        }
                        if title.to_lowercase().contains("pair") {
                            props.insert("包装单位".to_string(), "1Pair (双/对)".to_string());
                        }

                        let name_label = if !thickness.is_empty() {
                            format!("{} {} (1Pair)", thickness, size_label)
                        } else {
                            format!("{} (1Pair)", size_label)
                        };

                        skus.push(crate::model::ProductSkuItem {
                            sku_id: Some(format!("{}_{}", item_id, idx + 1)),
                            name: name_label,
                            price: if current_price > 0.0 { Some(current_price) } else { None },
                            original_price: None,
                            stock: None,
                            image_url: main_images.first().cloned(),
                            properties: props,
                        });
                    }
                    specifications.insert("可选规格列表".to_string(), sub_sizes.iter().map(|s| format!("{}{}", s, unit)).collect::<Vec<_>>().join(" / "));
                    specifications.insert("变体规格数".to_string(), skus.len().to_string());
                }
            }

            if title.to_lowercase().contains("pair") && !specifications.contains_key("包装单位") {
                specifications.insert("包装单位".to_string(), "1Pair (双/对)".to_string());
            }
        } else {
            specifications.insert("变体规格数".to_string(), skus.len().to_string());
        }

        // 6. 关联标准出海分类体系与国家地区
        let search_text = format!("{} {}", title, description_markdown);
        let standard_category = taxonomy::match_category(&search_text);
        let region = self.detect_region(url);

        let root_category = standard_category.as_ref().map(|c| c.main_name.clone());
        let leaf_category = standard_category.as_ref().and_then(|c| c.sub_name.clone());
        let mut category_path = Vec::new();
        if let Some(r) = &root_category {
            category_path.push(r.clone());
        }
        if let Some(l) = &leaf_category {
            category_path.push(l.clone());
        }

        let detail = ProductDetail {
            platform: "aliexpress".to_string(),
            item_id,
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

    async fn fetch_reviews(
        &self,
        url: &str,
        http: &HttpClient,
        _page: u32,
        limit: usize,
    ) -> Result<Vec<ProductReview>> {
        let item_id = self.extract_item_id(url).context("无法提取 AliExpress 商品 ID")?;
        info!("正在获取 AliExpress 买家评论: ID = {}", item_id);

        let html_text = http.get_text(url, None).await?;
        let document = Html::parse_document(&html_text);

        let mut reviews = Vec::new();

        // 尝试从内联数据提取
        if let Some(ref data) = self.extract_inline_data(&html_text, &document) {
            if let Some(fb_mod) = self.find_nested_node(data, &["feedbackList", "reviews", "reviewList"]) {
                if let Some(arr) = fb_mod.as_array() {
                    for (idx, r) in arr.iter().take(limit).enumerate() {
                        let author = r.get("buyerName").or_else(|| r.get("author")).and_then(|v| v.as_str()).unwrap_or("AliExpress Buyer").to_string();
                        let content = r.get("buyerFeedback").or_else(|| r.get("content")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let rating = r.get("buyerEval").or_else(|| r.get("rating")).and_then(|v| v.as_u64()).unwrap_or(5) as u8;
                        let date = r.get("evalDate").and_then(|v| v.as_str()).unwrap_or("").to_string();

                        if !content.is_empty() {
                            reviews.push(ProductReview {
                                review_id: format!("ae_{}_{}", item_id, idx + 1),
                                author,
                                rating,
                                review_date: if date.is_empty() { chrono::Utc::now().format("%Y-%m-%d").to_string() } else { date },
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

/// 清洗 AliExpress 页面标题，去除末尾附加的电商站名和数字编号
fn clean_aliexpress_title(raw: &str) -> String {
    let mut title = raw.trim();
    if let Some(pos) = title.rfind(" - AliExpress") {
        title = &title[..pos];
    } else if let Some(pos) = title.rfind(" | AliExpress") {
        title = &title[..pos];
    } else if let Some(pos) = title.rfind(" - Aliexpress") {
        title = &title[..pos];
    } else if let Some(pos) = title.rfind(" | Aliexpress") {
        title = &title[..pos];
    }
    title.trim().to_string()
}
