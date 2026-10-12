# Crossborder E-commerce MCP Server (`crossborder-ecom-mcp`)

[![CI](https://github.com/haohao-ui/crossborder-ecom-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/haohao-ui/crossborder-ecom-mcp/actions/workflows/ci.yml)
[![Release](https://github.com/haohao-ui/crossborder-ecom-mcp/actions/workflows/release.yml/badge.svg)](https://github.com/haohao-ui/crossborder-ecom-mcp/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

基于 Rust 开发的高性能跨境出海电商情报与产品信息获取 MCP (Model Context Protocol) 服务端。

专门针对 **Amazon（亚马逊）**、**AliExpress（速卖通）**、**Shopify（品牌独立站）** 与 **TikTok Shop（跨境短视频电商）** 设计，提供包含产品上架时间、销量表现、综合评价、所属店铺、双轨分类体系、买家长评、五点卖点及图文详情在内的深度情报提取能力。

内置 **嵌入式 SQLite 存储**，具备**防封降频本地缓存**与**商品价格/指标历史趋势追踪**功能。

---

## 核心特性

1. **深度情报覆盖**：
   - **支持平台**：Amazon（全球各分站 ASIN）、AliExpress（速卖通多站点商品 ID）、Shopify（全球独立站 Handle）、TikTok Shop（美区与多国短视频商品 ID）。
   - **产品上架时间**：精准提取 Amazon 的 `Date First Available`、Shopify 的 `published_at` / `created_at`、TikTok Shop 的 `create_time`。
   - **真实销量表现**：提取 TikTok Shop 的 `"Sold 10k"`、AliExpress 的 `"10,000+ sold"`、Amazon 社交证明（如 `"5K+ bought in past month"`）。
   - **双轨分类体系**：保留原生类目全路径树（`category_path`），并自动提取一级大类（`root_category`）与叶子类目（`leaf_category`）。
   - **所属店铺与品牌**：解析官方旗舰店、品牌方、卖家链接与发货方。
   - **评价大盘与买家评论**：支持星级评分、评价总数提取，并提供独立的买家评论获取工具（支持图文正文、购买规格 SKU、晒图与有用点赞）。
   - **Markdown 降噪详情**：自动剔除冗余脚本样式，输出大模型友好的结构化 Markdown 卖点与参数表。

2. **嵌入式 SQLite 存储（方案 B）**：
   - **防反爬降频保护**：同一商品在有效期（默认 12 小时）内直接读库响应，保护 IP 避免触发 Amazon 验证码。
   - **指标自动快照**：每次抓取自动沉淀 `(价格, 销量表现, 评分, 评价数)` 时间快照，天然支持调价追踪与竞品监测。

3. **结构化配置分类系统 (`config.toml`)**：
   - `[server]`：服务标识与日志。
   - `[storage]`：SQLite 数据库路径。
   - `[cache]`：缓存生效时间 (TTL)。
   - `[network]`：出海代理支持（HTTP / SOCKS5）、超时与重试。
   - `[platforms.amazon]`：默认美区邮编（如 `10001` 纽约），确保呈现真实美区售价与库存。

---

## 一键安装与快速启动

### 方式一：Shell 一键安装（推荐，自动匹配操作系统与架构）

```bash
curl -fsSL https://raw.githubusercontent.com/haohao-ui/crossborder-ecom-mcp/main/install.sh | bash
```

> **说明**：该脚本会自动识别您的系统（macOS / Linux / Windows）与 CPU 架构（x86_64 / arm64），自动下载最新预编译二进制并安装至 `~/.local/bin`。若未检测到 Release 包且本地有 Rust 环境，将自动回退到源码编译。

### 方式二：通过 Rust Cargo 一键安装

```bash
cargo install --git https://github.com/haohao-ui/crossborder-ecom-mcp.git
```

### 方式三：直接下载 GitHub Releases 预编译包

访问 [GitHub Releases 页面](https://github.com/haohao-ui/crossborder-ecom-mcp/releases) 下载对应平台的二进制压缩包（`.tar.gz` 或 `.zip`），解压后将可执行文件移动至您的系统 PATH 路径即可。

---

## CLI 命令行模式使用指南

除了作为 MCP 协议服务端运行，程序同时提供了一流的 CLI 命令行交互体验，可直接在终端快速检索商品、调价历史与标准分类：

```bash
# 1. 抓取并展示商品详情
crossborder-ecom-mcp product "https://allbirds.com/products/womens-allbirds-flip-flop-dusty-pink"

# 2. 强制跳过缓存刷新抓取并输出完整 JSON
crossborder-ecom-mcp product "<URL>" --force --json

# 3. 搜索标准分类大类与二级子类
crossborder-ecom-mcp category "电脑"

# 4. 查看商品价格变动历史快照
crossborder-ecom-mcp history "<URL>"

# 5. 查看支持的 20 个出海国家/地区
crossborder-ecom-mcp countries

# 6. 带参数覆盖运行服务 (如指定自定义代理与数据库)
crossborder-ecom-mcp --proxy "http://127.0.0.1:7890" --db "data/custom.db" serve
```

---

## 提供的 MCP Tools

| 工具名称 | 功能描述 | 核心入参 |
| :--- | :--- | :--- |
| **`get_product_detail`** | 获取商品全量详情（店铺、标准类目代码与名称、国家地区、上架时间、价格、销量、评分、核心卖点、Markdown 详情） | `url` (必填), `force_refresh` (可选布尔值，默认 false) |
| **`get_product_reviews`** | 分页获取真实买家评论列表（买家、评分、日期、评论正文、购买 SKU、晒图） | `url` (必填), `page` (默认 1), `limit` (默认 10) |
| **`get_price_history`** | 查询该商品在本地数据库中记录的历史价格与指标变动趋势 | `url` (必填) |
| **`list_categories`** | 查询系统内置的 30 个一级分类大类、二级子分类与热门推荐品类（支持关键词搜索） | `keyword` (可选字符串，如 "美妆"、"女装"、"电脑") |
| **`list_supported_countries`** | 查询支持的 20 个跨境出海国家/地区代码与业务指标分档规则（价格带、销量、佣金率等） | 无入参 |

---

## 内置标准分类与地区元数据体系

服务内置了完整的出海电商标准化元数据资产（[`data/taxonomy.json`](file:///Users/yang/tools/code/Rust/crossborder-ecom-mcp/data/taxonomy.json)）：
1. **30 个标准一级分类大类与完整二级子类树**：
   - 涵盖 美妆个护、女装与女士内衣、手机与数码、电脑办公、居家日用、食品饮料、运动与户外、珠宝、宠物等 30 个一级大类及数百个细分二级类目代码 (`c_code`)。
2. **20 个全球核心出海国家与地区**：
   - 美国 (`US`)、印度尼西亚 (`ID`)、英国 (`GB`)、越南 (`VN`)、泰国 (`TH`)、马来西亚 (`MY`)、菲律宾 (`PH`)、西班牙 (`ES`)、墨西哥 (`MX`)、德国 (`DE`)、法国 (`FR`)、意大利 (`IT`)、巴西 (`BR`)、日本 (`JP`)、新加坡 (`SG`)、奥地利 (`AT`)、比利时 (`BE`)、荷兰 (`NL`)、波兰 (`PL`)、葡萄牙 (`PT`)。
3. **选品多维分档规范**：
   - 包含佣金率 (`crate`)、价格带 (`price_amount`)、总销量 (`sold_count`)、7天销量与销售额 (`day7_sold_count` / `day7_sale_amount`) 等标准化区间划分。
4. **智能匹配与映射**：
   - 抓取商品时，系统会自动根据标题与原始类目智能匹配对应的标准一级大类与二级子类 (`standard_category`)，并根据域名站点自动映射对应的出海国家/地区 (`region`)。

---

## 目录结构

```text
crossborder-ecom-mcp/
├── Cargo.toml
├── config.example.toml           # 结构化分类配置模板
├── src/
│   ├── main.rs                   # MCP stdio JSON-RPC 2.0 入口
│   ├── lib.rs                    # 库导出
│   ├── config.rs                 # 分类配置系统 (server, storage, cache, network, platforms)
│   ├── model.rs                  # 统一商品、快照、评论数据结构
│   ├── http.rs                   # 请求客户端 (Header 伪装、SOCKS5/HTTP 代理、邮编 Cookie)
│   ├── clean.rs                  # HTML 降噪转 Markdown 清洗器
│   ├── db/                       # SQLite 嵌入式存储 (商品、快照、评论、TTL 校验)
│   │   └── mod.rs
│   └── platforms/                # 平台插件化驱动
│       ├── mod.rs                # PlatformDriver Trait
│       ├── amazon.rs             # 亚马逊驱动 (ASIN、Date First Available、月销、面包屑)
│       ├── aliexpress.rs         # AliExpress 驱动 (阿里滑块拦截告警、runParams 与 Schema.org 解析)
│       ├── tiktok.rs             # TikTok Shop 驱动 (安全拦截告警、内嵌状态数据解析)
│       └── shopify.rs            # Shopify 驱动 (.json 接口、Handle 解析、变体规格)
└── tests/                        # 单元与集成测试套件
```

---

## 编译与构建

```bash
# 进入项目目录
cd /Users/yang/tools/code/Rust/crossborder-ecom-mcp

# 编译测试
cargo test

# 构建 Release 高性能二进制
cargo build --release
```

---

## 配置与接入（以 Claude Desktop 为例）

在 `claude_desktop_config.json` 中配置：

```json
{
  "mcpServers": {
    "crossborder-ecom": {
      "command": "/Users/yang/tools/code/Rust/crossborder-ecom-mcp/target/release/crossborder-ecom-mcp",
      "args": []
    }
  }
}
```
