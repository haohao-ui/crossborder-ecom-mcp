use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 跨境电商商品完整详情模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductDetail {
    pub platform: String,          // 平台标识: "amazon", "shopify"
    pub item_id: String,           // 商品唯一键 (ASIN 或 handle)
    pub url: String,               // 原始访问链接
    pub title: String,             // 商品标题
    pub brand: Option<String>,     // 品牌
    pub shop_name: Option<String>, // 所属店铺/卖家
    pub shop_url: Option<String>,  // 店铺链接

    // 分类体系（双轨设计：原始路径 + 归一化类目）
    pub root_category: Option<String>, // 一级大类 (如 "Electronics")
    pub leaf_category: Option<String>, // 叶子末级分类 (如 "Laptops")
    pub category_path: Vec<String>,    // 完整分类面包屑树

    // 销售表现与上架指标
    pub currency: String,            // 货币种类 (USD, EUR, GBP 等)
    pub current_price: f64,          // 当前售价
    pub original_price: Option<f64>, // 划线价/原价
    pub sales_text: Option<String>,  // 销量表现 (如 "5K+ bought in past month")
    pub launch_date: Option<String>, // 上架时间 (如 "2024-03-15" 或 "Date First Available")

    // 评价大盘
    pub rating_score: Option<f64>, // 综合评分 (如 4.6)
    pub review_count: Option<u64>, // 评价总数

    // 详情内容与规格变体
    pub main_images: Vec<String>,                // 主图与轮播图列表
    pub specifications: HashMap<String, String>, // 技术规格与参数表
    #[serde(default)]
    pub skus: Vec<ProductSkuItem>, // 结构化 SKU 规格与变体列表
    pub description_markdown: String,            // 清洗后的详情内容/卖点列表 (Markdown)

    // 标准化类目与出海地区映射
    pub standard_category: Option<crate::taxonomy::MatchedCategory>,
    pub region: Option<crate::taxonomy::CountryItem>,

    // 时间戳
    pub updated_at: i64, // 最后抓取时间戳 (Unix 秒)
}

/// 商品具体变体 SKU 项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductSkuItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku_id: Option<String>,
    pub name: String, // 变体名称，如 "8mm 100CM White"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>, // 变体价格
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_price: Option<f64>, // 原价
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stock: Option<i64>, // 可售库存
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>, // 变体专属预览图
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<String, String>, // 属性键值对，如 {"长度": "100CM", "粗细": "8mm"}
}

/// 历史指标快照（用于价格变动、销量趋势与评分追踪）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductSnapshot {
    pub id: Option<i64>,
    pub product_id: i64,
    pub price: f64,
    pub currency: String,
    pub sales_text: Option<String>,
    pub rating_score: Option<f64>,
    pub review_count: Option<u64>,
    pub recorded_at: i64,
}

/// 买家评论模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductReview {
    pub review_id: String,             // 评论唯一 ID
    pub author: String,                // 评论者
    pub rating: u8,                    // 星级评分 (1~5)
    pub review_date: String,           // 评论日期 (如 "March 15, 2024")
    pub content: String,               // 评论正文 (已清洗)
    pub sku_purchased: Option<String>, // 购买的具体变体/规格
    pub images: Vec<String>,           // 买家晒图
    pub helpful_votes: Option<u32>,    // 有用点赞数
}

/// 统一的平台抓取接口入参
#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub zip_code: Option<String>,
    pub cookie: Option<String>,
    #[allow(dead_code)]
    pub force_refresh: bool,
}
