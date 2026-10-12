use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "crossborder-ecom-mcp",
    version,
    about = "跨境出海电商产品情报 MCP 服务 & CLI 工具 (Amazon, AliExpress, Shopify, TikTok Shop)",
    long_about = "支持作为标准 MCP (Model Context Protocol) 服务运行于 Claude Desktop / Cursor，亦可作为独立 CLI 命令行工具直接查询商品详情、买家评论、价格历史与标准分类体系。"
)]
pub struct Cli {
    /// 配置文件路径
    #[arg(short, long, default_value = "config.toml")]
    pub config: PathBuf,

    /// 覆盖数据库文件路径 (如: data/custom.db)
    #[arg(long)]
    pub db: Option<PathBuf>,

    /// 覆盖网络代理 (如: http://127.0.0.1:7890 或 socks5://127.0.0.1:1080)
    #[arg(long)]
    pub proxy: Option<String>,

    /// 子命令 (若未指定子命令，则默认启动 MCP stdio 服务模式)
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 启动 MCP 服务 (stdio JSON-RPC 2.0 模式，默认行为)
    Serve,

    /// 获取商品深度详情 (价格、上架时间、店铺、双轨分类、规格与详情)
    Product {
        /// 商品 URL (Amazon, AliExpress, Shopify 独立站 或 TikTok Shop 链接)
        url: String,

        /// 是否跳过本地缓存强制从网络最新拉取
        #[arg(short, long)]
        force: bool,

        /// 是否仅输出紧凑格式或纯 JSON
        #[arg(long)]
        json: bool,
    },

    /// 分页获取买家真实评论
    Reviews {
        /// 商品 URL
        url: String,

        /// 评论页码 (从 1 开始)
        #[arg(short, long, default_value_t = 1)]
        page: u32,

        /// 获取评论数量上限
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },

    /// 查询该商品在本地数据库中记录的历史价格与指标变动
    History {
        /// 商品 URL
        url: String,
    },

    /// 查询 30 个标准商品分类大类与细分类目
    Category {
        /// 搜索关键词 (如: 美妆、女装、手机、电脑)
        keyword: Option<String>,
    },

    /// 查询支持的 20 个出海国家/地区及业务指标分档规则
    Countries,
}
