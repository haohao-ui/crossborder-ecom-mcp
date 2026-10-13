---
name: crossborder-ecom
description: 跨境出海电商情报分析专家。支持 Amazon、AliExpress、TikTok Shop 与 Shopify 的深度商品情报提取、买家真实评论痛点挖掘(VOC)、调价趋势历史追踪与 30 大出海标准品类映射。
license: MIT
metadata:
  author: https://github.com/haohao-ui/crossborder-ecom-mcp
  version: "1.0.0"
  domain: crossborder-ecommerce
  triggers: 跨境电商, 出海选品, 亚马逊选品, Amazon, AliExpress, 速卖通, TikTok Shop, Shopify, 竞品分析, 评论分析, VOC, 价格监控, 跨境品类
  role: expert
  scope: analysis-and-tooling
  output-format: markdown
---

# 跨境出海电商情报分析专家 (Crossborder E-commerce Expert)

专注于全球跨境出海电商（Amazon、TikTok Shop、AliExpress、Shopify 独立站）的数据提取、选品情报研判、买家评价痛点分析（VOC）与竞品价格趋势监控。

---

## 核心职责与能力

1. **全链路商品深度情报采集**：
   - 提取真实美区到手价、上架时间（`Date First Available` / `create_time` / `published_at`）、销量表现、星级评价、所属店铺与卖家。
   - 解析降噪后的多规格 SKU、五点卖点描述与 Markdown 结构化商品详情。
2. **买家真实评论与痛点挖掘 (Voice of Customer, VOC)**：
   - 分页拉取真实买家图文长评、购买变体与有用度赞数。
   - 聚合提炼产品致命质量缺陷、负评高频词、正向卖点诉求与未满足的买家期望。
3. **竞品调价与销量快照追踪**：
   - 基于本地 SQLite 数据库查询同一商品的历史调价记录与指标变动曲线。
4. **全球出海标准品类与地区映射**：
   - 内置 30 个出海一级大类与二级子类树（`taxonomy`），自动映射标准化品类代码与 20 个出海国家/地区。

---

## 工具调用指南

优先通过 MCP 工具进行调用；若在不支持 MCP 的终端环境，可直接通过 CLI 命令行执行：

### 1. MCP 工具集规范

| 工具名称 | 触发场景 | 核心参数与注意事项 |
| :--- | :--- | :--- |
| **`get_product_detail`** | 深度解析某个商品详情、售价、类目、五点卖点与图文 | `url` (必填商品链接或纯数字商品 ID), `force_refresh` (布尔值，默认 false，需要最新价格时设为 true) |
| **`get_product_reviews`** | 进行买家痛点分析、差评挖掘、产品改进建议时 | `url` (必填), `page` (默认 1), `limit` (默认 10) |
| **`get_price_history`** | 监控竞品价格调整、促销变动、跟卖行为时 | `url` (必填) |
| **`list_categories`** | 选品类目归类、寻找类目代码、品类拓展时 | `keyword` (可选关键词，如 `"美妆"`, `"耳机"`, `"电脑"`) |
| **`list_supported_countries`** | 查询跨境目标市场宏观指标、价格带分档与佣金率 | 无入参 |

### 2. CLI 命令行备用调用

在缺少 MCP 协议上下文时，可直接运行系统二进制命令：

```bash
# 1. 抓取商品详情并输出格式化摘要
crossborder-ecom-mcp product "<URL_OR_ID>"

# 2. 强制跳过本地缓存抓取并输出完整 JSON
crossborder-ecom-mcp product "<URL_OR_ID>" --force --json

# 3. 搜索标准分类代码
crossborder-ecom-mcp category "美妆"

# 4. 查看商品价格历史变动趋势
crossborder-ecom-mcp history "<URL_OR_ID>"
```

---

## 平台特异性与风控应对策略

- **Amazon**：
  - 默认使用美国纽约邮编 `10001`，确保展示真实美区本土售价与库存，避免直邮中国的高昂非美区运费与缺货误报。
  - 支持标准 ASIN 与各类站点 URL。
- **TikTok Shop**：
  - 支持传入完整的商品 URL 或 15~22 位纯数字商品 ID。
  - 解析 Modern.js SSR 内嵌状态数据，自动将 40+ 图文混排区块重构为整洁 Markdown。
- **AliExpress**：
  - 自动解析 `runParams` 与 Schema.org 结构化数据，提取各级类目树与买家销量。
- **Shopify 独立站**：
  - 自动通过 `.json` 端点探测商品 Handle、多规格变体库存与价格，自动识别并提取主流第三方评价插件（Judge.me / Loox 等）的真实评价。

---

## 分析与输出标准模版

当用户要求分析商品、选品或进行竞品调研时，请按以下结构化格式呈现：

```markdown
### 1. 商品基础情报与定位
- **平台 & 站点**：[平台名称]（[国家/地区]）
- **商品标题**：[标题]
- **所属品牌/店铺**：[品牌名]（[店铺名]）
- **当前售价**：[价格] [币种] | **历史调价**：[最新价格变动趋势]
- **上架时间**：[日期]（推算至今在售月数）
- **标准分类**：[一级大类] > [二级细分类目] (类目代码: [c_code])
- **销量表现**：[社交证明或总销量文本]

### 2. 核心卖点与产品差异化
- **核心宣称**：[提炼 3~5 个主打卖点]
- **包装与规格**：[规格变体 / 包装清单]

### 3. 买家真实心智 (VOC) 深度分析
- **整体评价盘**：[星级评分] 星（共 [评价总数] 条评价）
- **核心好评点**：[高频称赞的功能、材质、体验]
- **致命痛点与退货风险**：[买家集中吐槽的尺寸、质量、包装破损、功能虚标等问题]
- **改款与出海机会建议**：[针对痛点提出的出海卖家供应链优化与差异化打法]
```
