use anyhow::{Context, Result};
use crossborder_ecom_mcp::config::AppConfig;
use crossborder_ecom_mcp::db::Database;
use crossborder_ecom_mcp::http::HttpClient;
use crossborder_ecom_mcp::model::FetchOptions;
use crossborder_ecom_mcp::platforms::aliexpress::AliExpressDriver;
use crossborder_ecom_mcp::platforms::amazon::AmazonDriver;
use crossborder_ecom_mcp::platforms::shopify::ShopifyDriver;
use crossborder_ecom_mcp::platforms::tiktok::TikTokShopDriver;
use crossborder_ecom_mcp::platforms::PlatformDriver;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

struct McpServer {
    config: AppConfig,
    db: Database,
    http: HttpClient,
    drivers: Vec<Box<dyn PlatformDriver>>,
}

impl McpServer {
    pub fn new(config: AppConfig) -> Result<Self> {
        let db = Database::init(&config.storage.db_path)
            .context("初始化数据库失败")?;
        let http = HttpClient::new(&config.network)
            .context("初始化网络客户端失败")?;

        let drivers: Vec<Box<dyn PlatformDriver>> = vec![
            Box::new(ShopifyDriver::new()),
            Box::new(AmazonDriver::new()),
            Box::new(TikTokShopDriver::new()),
            Box::new(AliExpressDriver::new()),
        ];

        Ok(Self {
            config,
            db,
            http,
            drivers,
        })
    }

    /// 根据 URL 寻找匹配的平台驱动
    fn find_driver(&self, url: &str) -> Option<&dyn PlatformDriver> {
        for driver in &self.drivers {
            if driver.can_handle(url) {
                return Some(driver.as_ref());
            }
        }
        None
    }

    /// 获取商品详情 (支持本地缓存与快照历史沉淀)
    async fn handle_get_product_detail(&self, arguments: &Value) -> Result<Value> {
        let url = arguments
            .get("url")
            .and_then(|v| v.as_str())
            .context("参数缺少 'url'")?;

        let force_refresh = arguments
            .get("force_refresh")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let driver = self.find_driver(url)
            .context("不支持的电商平台链接，当前支持 Amazon, AliExpress (速卖通), Shopify 独立站 与 TikTok Shop")?;

        let item_id = driver.extract_item_id(url)
            .context("无法从链接中解析商品唯一标识")?;

        // 1. 检查本地数据库缓存 (防风控降频)
        if !force_refresh && self.config.cache.enabled {
            if let Ok(Some(cached)) = self.db.get_product(driver.name(), &item_id) {
                if self.db.is_cache_valid(&cached, self.config.cache.default_ttl_hours) {
                    info!("命中本地数据库缓存: [{}] {}", driver.name(), item_id);
                    return Ok(json!({
                        "source": "local_database_cache",
                        "product": cached
                    }));
                }
            }
        }

        // 2. 触发远程网络抓取
        let platform_cookie = match driver.name() {
            "amazon" => self.config.platforms.amazon.cookie.clone(),
            "tiktok" => self.config.platforms.tiktok.cookie.clone(),
            "aliexpress" => self.config.platforms.aliexpress.cookie.clone(),
            _ => None,
        };

        let options = FetchOptions {
            zip_code: Some(self.config.platforms.amazon.default_zip_code.clone()),
            cookie: platform_cookie,
            force_refresh,
        };

        let detail = driver.fetch_product(url, &self.http, &options).await?;

        // 3. 入库并追加快照
        if let Err(e) = self.db.save_product(&detail) {
            error!("保存商品数据至数据库失败: {:?}", e);
        }

        Ok(json!({
            "source": "live_fetch",
            "product": detail
        }))
    }

    /// 获取买家评论列表
    async fn handle_get_product_reviews(&self, arguments: &Value) -> Result<Value> {
        let url = arguments
            .get("url")
            .and_then(|v| v.as_str())
            .context("参数缺少 'url'")?;

        let page = arguments
            .get("page")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        let limit = arguments
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        let driver = self.find_driver(url)
            .context("不支持的电商平台链接")?;

        let item_id = driver.extract_item_id(url)
            .context("无法解析商品 ID")?;

        // 检查本地已有评论
        let cached_reviews = self.db.get_reviews(driver.name(), &item_id, limit).unwrap_or_default();
        if !cached_reviews.is_empty() && page == 1 {
            return Ok(json!({
                "source": "database_cache",
                "count": cached_reviews.len(),
                "reviews": cached_reviews
            }));
        }

        // 远程抓取
        let reviews = driver.fetch_reviews(url, &self.http, page, limit).await?;

        // 保存到数据库
        if let Err(e) = self.db.save_reviews_by_item(driver.name(), &item_id, &reviews) {
            error!("保存买家评论至数据库失败: {:?}", e);
        }

        Ok(json!({
            "source": "live_fetch",
            "count": reviews.len(),
            "reviews": reviews
        }))
    }

    /// 获取历史价格与指标变动趋势
    async fn handle_get_price_history(&self, arguments: &Value) -> Result<Value> {
        let url = arguments
            .get("url")
            .and_then(|v| v.as_str())
            .context("参数缺少 'url'")?;

        let driver = self.find_driver(url)
            .context("不支持的电商平台链接")?;

        let item_id = driver.extract_item_id(url)
            .context("无法解析商品 ID")?;

        let history = self.db.get_price_history(driver.name(), &item_id)?;

        Ok(json!({
            "platform": driver.name(),
            "item_id": item_id,
            "snapshots_count": history.len(),
            "history": history
        }))
    }

    /// 查询标准商品分类体系（支持关键词搜索或全量浏览）
    async fn handle_list_categories(&self, arguments: &Value) -> Result<Value> {
        let keyword = arguments.get("keyword").and_then(|v| v.as_str()).map(|s| s.to_lowercase());
        let taxonomy = &crossborder_ecom_mcp::taxonomy::TAXONOMY;

        if let Some(kw) = keyword {
            let mut matched = Vec::new();
            for cat in &taxonomy.category {
                let cat_match = cat.c_name.to_lowercase().contains(&kw);
                let matching_subs: Vec<_> = cat.sub.values().filter(|s| s.c_name.to_lowercase().contains(&kw)).cloned().collect();
                if cat_match || !matching_subs.is_empty() {
                    matched.push(json!({
                        "c_code": cat.c_code,
                        "c_name": cat.c_name,
                        "matched_subs": matching_subs
                    }));
                }
            }
            Ok(json!({
                "keyword": kw,
                "total_matched": matched.len(),
                "categories": matched
            }))
        } else {
            Ok(json!({
                "hot_categories": taxonomy.hot_product_category,
                "total_main_categories": taxonomy.category.len(),
                "main_categories": taxonomy.category.iter().map(|c| json!({
                    "c_code": c.c_code,
                    "c_name": c.c_name,
                    "rank": c.rank,
                    "sub_count": c.sub.len()
                })).collect::<Vec<_>>()
            }))
        }
    }

    /// 查询支持的跨境出海国家/地区及业务指标分档规则
    async fn handle_list_supported_countries(&self, _arguments: &Value) -> Result<Value> {
        let taxonomy = &crossborder_ecom_mcp::taxonomy::TAXONOMY;
        Ok(json!({
            "total_countries": taxonomy.country.len(),
            "countries": taxonomy.country,
            "filter_facets": {
                "commission_rate_ranges": taxonomy.commission_rate,
                "price_amount_ranges": taxonomy.price_amount,
                "sold_count_ranges": taxonomy.sold_count,
                "sale_amount_ranges": taxonomy.sale_amount,
                "author_order_rate_ranges": taxonomy.author_order_rate
            }
        }))
    }

    /// 处理 JSON-RPC 请求
    async fn handle_request(&self, req: &Value) -> Option<Value> {
        let id = req.get("id").cloned();
        let method = match req.get("method").and_then(|v| v.as_str()) {
            Some(m) => m,
            None => return None,
        };

        match method {
            "initialize" => {
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": self.config.server.name,
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }
                }))
            }
            "notifications/initialized" => None,
            "tools/list" => {
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": [
                            {
                                "name": "get_product_detail",
                                "description": "获取跨境电商商品的深度详情信息（包括上架时间、销量表现、综合评分、店铺与品牌、标准类目代码与名称、国家地区、价格、核心卖点及详情，支持 Amazon, AliExpress (速卖通), Shopify 独立站 与 TikTok Shop，带防风控本地缓存）",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "url": {
                                            "type": "string",
                                            "description": "Amazon, AliExpress (速卖通), Shopify 独立站 或 TikTok Shop 商品完整链接"
                                        },
                                        "force_refresh": {
                                            "type": "boolean",
                                            "description": "是否跳过本地数据库缓存强制从网络最新抓取 (默认 false)"
                                        }
                                    },
                                    "required": ["url"]
                                }
                            },
                            {
                                "name": "get_product_reviews",
                                "description": "获取商品的真实买家评论列表（包含星级评分、评论正文、评论日期、购买变体及晒图，支持分页获取）",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "url": {
                                            "type": "string",
                                            "description": "商品完整链接"
                                        },
                                        "page": {
                                            "type": "integer",
                                            "description": "评论页码 (从 1 开始，默认 1)"
                                        },
                                        "limit": {
                                            "type": "integer",
                                            "description": "单次返回的最大评论条数 (默认 10)"
                                        }
                                    },
                                    "required": ["url"]
                                }
                            },
                            {
                                "name": "get_price_history",
                                "description": "查询该商品在本地数据库中记录的历史价格与动态指标走势（调价记录、销量趋势、评分变化）",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "url": {
                                            "type": "string",
                                            "description": "商品完整链接"
                                        }
                                    },
                                    "required": ["url"]
                                }
                            },
                            {
                                "name": "list_categories",
                                "description": "查询跨境电商标准商品分类体系（支持按关键词搜索细分类目，或查看30个一级分类大类与热门推荐品类）",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "keyword": {
                                            "type": "string",
                                            "description": "可选：搜索分类名称（如'女装'、'美妆'、'手机'、'电脑'）"
                                        }
                                    }
                                }
                            },
                            {
                                "name": "list_supported_countries",
                                "description": "查询系统支持的20个出海国家/地区（美国、英国、德国、日本、越南、印尼等）以及选品筛选分档规则（价格带、销量区间等）",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {}
                                }
                            }
                        ]
                    }
                }))
            }
            "tools/call" => {
                let params = req.get("params");
                let tool_name = params.and_then(|p| p.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.and_then(|p| p.get("arguments")).cloned().unwrap_or(json!({}));

                let call_result = match tool_name {
                    "get_product_detail" => self.handle_get_product_detail(&arguments).await,
                    "get_product_reviews" => self.handle_get_product_reviews(&arguments).await,
                    "get_price_history" => self.handle_get_price_history(&arguments).await,
                    "list_categories" => self.handle_list_categories(&arguments).await,
                    "list_supported_countries" => self.handle_list_supported_countries(&arguments).await,
                    _ => Err(anyhow::anyhow!("未知的工具名称: {}", tool_name)),
                };

                match call_result {
                    Ok(val) => Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [
                                {
                                    "type": "text",
                                    "text": serde_json::to_string_pretty(&val).unwrap_or_default()
                                }
                            ],
                            "isError": false
                        }
                    })),
                    Err(err) => Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("调用失败: {:#}", err)
                                }
                            ],
                            "isError": true
                        }
                    })),
                }
            }
            _ => {
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("方法未找到: {}", method)
                    }
                }))
            }
        }
    }
}

use clap::Parser;
use crossborder_ecom_mcp::cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. 解析命令行参数
    let cli = Cli::parse();

    // 2. 日志统一输出至 stderr
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_writer(io::stderr)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);

    // 3. 加载配置文件并应用命令行参数覆盖
    let mut config = AppConfig::load_or_default(&cli.config);
    if let Some(db_path) = cli.db {
        config.storage.db_path = db_path;
    }
    if let Some(proxy) = cli.proxy {
        config.network.proxy = Some(proxy);
    }

    let server = Arc::new(McpServer::new(config)?);

    // 4. 判断命令行为: CLI 独立模式 vs MCP 服务模式
    match cli.command {
        Some(Commands::Product { url, force, json }) => {
            let args = json!({ "url": url, "force_refresh": force });
            match server.handle_get_product_detail(&args).await {
                Ok(resp) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&resp)?);
                    } else if let Some(prod) = resp.get("product") {
                        let title = prod.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        let price = prod.get("current_price").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let currency = prod.get("currency").and_then(|v| v.as_str()).unwrap_or("USD");
                        let launch = prod.get("launch_date").and_then(|v| v.as_str()).unwrap_or("未知");
                        let shop = prod.get("shop_name").and_then(|v| v.as_str()).unwrap_or("未知");
                        let source = resp.get("source").and_then(|v| v.as_str()).unwrap_or("");
                        
                        println!("==================================================");
                        println!("商品标题: {}", title);
                        println!("当前售价: {} {}", price, currency);
                        println!("上架时间: {}", launch);
                        println!("所属店铺: {}", shop);
                        println!("数据来源: {}", source);
                        if let Some(cat) = prod.get("standard_category") {
                            let main_n = cat.get("main_name").and_then(|v| v.as_str()).unwrap_or("");
                            let sub_n = cat.get("sub_name").and_then(|v| v.as_str()).unwrap_or("");
                            println!("标准分类: {} > {}", main_n, sub_n);
                        }
                        if let Some(reg) = prod.get("region") {
                            let r_name = reg.get("region_name").and_then(|v| v.as_str()).unwrap_or("");
                            let r_code = reg.get("region_code").and_then(|v| v.as_str()).unwrap_or("");
                            println!("出海地区: {} ({})", r_name, r_code);
                        }
                        if let Some(skus) = prod.get("skus").and_then(|v| v.as_array()) {
                            if !skus.is_empty() {
                                println!("--------------------------------------------------");
                                println!("SKU变体 (共 {} 种):", skus.len());
                                for (idx, s) in skus.iter().take(6).enumerate() {
                                    let s_name = s.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                    let s_price = s.get("price").and_then(|v| v.as_f64()).map(|p| format!(" [价格: {} {}]", p, currency)).unwrap_or_default();
                                    println!("  * 变体 {}: {}{}", idx + 1, s_name, s_price);
                                }
                                if skus.len() > 6 {
                                    println!("  ... 另有 {} 个变体", skus.len() - 6);
                                }
                            }
                        }
                        if let Some(imgs) = prod.get("main_images").and_then(|v| v.as_array()) {
                            if !imgs.is_empty() {
                                println!("--------------------------------------------------");
                                println!("主图列表 (共 {} 张):", imgs.len());
                                for (idx, img) in imgs.iter().take(4).enumerate() {
                                    let img_url = img.as_str().unwrap_or("");
                                    println!("  [{}] {}", idx + 1, img_url);
                                }
                                if imgs.len() > 4 {
                                    println!("  ... 另有 {} 张高清主图 (添加 --json 查看完整列表)", imgs.len() - 4);
                                }
                            }
                        }
                        if let Some(desc) = prod.get("description_markdown").and_then(|v| v.as_str()) {
                            if !desc.trim().is_empty() {
                                println!("--------------------------------------------------");
                                println!("详细页内容 (Markdown 预览):");
                                let preview_lines: Vec<&str> = desc.lines().filter(|l| !l.trim().is_empty()).take(8).collect();
                                for line in preview_lines {
                                    println!("  {}", line);
                                }
                                if desc.lines().count() > 8 {
                                    println!("  ... 详细内容共 {} 字符 (添加 --json 查看完整富文本详情)", desc.len());
                                }
                            }
                        }
                        println!("==================================================");
                    }
                }
                Err(e) => eprintln!("获取商品失败: {:#}", e),
            }
        }
        Some(Commands::Reviews { url, page, limit }) => {
            let args = json!({ "url": url, "page": page, "limit": limit });
            match server.handle_get_product_reviews(&args).await {
                Ok(resp) => println!("{}", serde_json::to_string_pretty(&resp)?),
                Err(e) => eprintln!("获取评论失败: {:#}", e),
            }
        }
        Some(Commands::History { url }) => {
            let args = json!({ "url": url });
            match server.handle_get_price_history(&args).await {
                Ok(resp) => println!("{}", serde_json::to_string_pretty(&resp)?),
                Err(e) => eprintln!("查询价格历史失败: {:#}", e),
            }
        }
        Some(Commands::Category { keyword }) => {
            let args = json!({ "keyword": keyword });
            match server.handle_list_categories(&args).await {
                Ok(resp) => println!("{}", serde_json::to_string_pretty(&resp)?),
                Err(e) => eprintln!("查询分类失败: {:#}", e),
            }
        }
        Some(Commands::Countries) => {
            let args = json!({});
            match server.handle_list_supported_countries(&args).await {
                Ok(resp) => println!("{}", serde_json::to_string_pretty(&resp)?),
                Err(e) => eprintln!("查询出海国家失败: {:#}", e),
            }
        }
        None | Some(Commands::Serve) => {
            // 默认启动 MCP stdio JSON-RPC 2.0 服务
            let stdin = io::stdin();
            let mut stdout = io::stdout();
            let reader = stdin.lock();

            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                if let Ok(req) = serde_json::from_str::<Value>(trimmed) {
                    let server_clone = Arc::clone(&server);
                    if let Some(resp) = server_clone.handle_request(&req).await {
                        let resp_str = serde_json::to_string(&resp)?;
                        let _ = writeln!(stdout, "{}", resp_str);
                        let _ = stdout.flush();
                    }
                }
            }
        }
    }

    Ok(())
}
