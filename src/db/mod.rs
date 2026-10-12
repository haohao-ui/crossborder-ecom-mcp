use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::info;

use crate::model::{ProductDetail, ProductReview, ProductSnapshot};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// 初始化 SQLite 数据库与建表
    pub fn init<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let path = db_path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建数据库目录失败: {:?}", parent))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("打开 SQLite 数据库失败: {:?}", path))?;

        // 启用 WAL 模式提高读写并发性能
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.create_tables()?;
        info!("SQLite 数据库初始化完成: {:?}", path);
        Ok(db)
    }

    fn create_tables(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // 1. 商品基础信息表（包含双轨分类与核心详情）
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS products (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                platform             TEXT NOT NULL,
                item_id              TEXT NOT NULL,
                url                  TEXT NOT NULL,
                title                TEXT NOT NULL,
                brand                TEXT,
                shop_name            TEXT,
                shop_url             TEXT,
                root_category        TEXT,
                leaf_category        TEXT,
                category_path        TEXT,
                currency             TEXT NOT NULL,
                current_price        REAL NOT NULL,
                original_price       REAL,
                sales_text           TEXT,
                launch_date          TEXT,
                rating_score         REAL,
                review_count         INTEGER,
                main_images          TEXT,
                specifications       TEXT,
                skus                 TEXT,
                description_markdown TEXT,
                standard_category    TEXT,
                region               TEXT,
                updated_at           INTEGER NOT NULL,
                UNIQUE(platform, item_id)
            );
            CREATE INDEX IF NOT EXISTS idx_products_platform_item ON products(platform, item_id);
            CREATE INDEX IF NOT EXISTS idx_products_categories ON products(root_category, leaf_category);
            "#,
            [],
        )?;

        // 兼容已有数据库自动增加 skus 字段
        let _ = conn.execute("ALTER TABLE products ADD COLUMN skus TEXT;", []);

        // 2. 价格与动态指标快照表
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS product_snapshots (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                product_id   INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
                price        REAL NOT NULL,
                currency     TEXT NOT NULL,
                sales_text   TEXT,
                rating_score REAL,
                review_count INTEGER,
                recorded_at  INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_snapshots_product_time ON product_snapshots(product_id, recorded_at);
            "#,
            [],
        )?;

        // 3. 买家评论表
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS product_reviews (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                product_id     INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
                review_id      TEXT NOT NULL,
                author         TEXT,
                rating         INTEGER,
                review_date    TEXT,
                content        TEXT,
                sku_purchased  TEXT,
                images         TEXT,
                helpful_votes  INTEGER,
                created_at     INTEGER NOT NULL,
                UNIQUE(product_id, review_id)
            );
            CREATE INDEX IF NOT EXISTS idx_reviews_product ON product_reviews(product_id);
            "#,
            [],
        )?;

        Ok(())
    }

    /// 查询商品详情
    pub fn get_product(&self, platform: &str, item_id: &str) -> Result<Option<ProductDetail>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT platform, item_id, url, title, brand, shop_name, shop_url,
                   root_category, leaf_category, category_path, currency,
                   current_price, original_price, sales_text, launch_date,
                   rating_score, review_count, main_images, specifications, skus,
                   description_markdown, standard_category, region, updated_at
            FROM products
            WHERE platform = ?1 AND item_id = ?2
            "#,
        )?;

        let mut rows = stmt.query(params![platform, item_id])?;
        if let Some(row) = rows.next()? {
            let category_path_raw: String = row.get(9)?;
            let category_path: Vec<String> =
                serde_json::from_str(&category_path_raw).unwrap_or_default();

            let main_images_raw: String = row.get(17)?;
            let main_images: Vec<String> =
                serde_json::from_str(&main_images_raw).unwrap_or_default();

            let specs_raw: String = row.get(18)?;
            let specifications: HashMap<String, String> =
                serde_json::from_str(&specs_raw).unwrap_or_default();

            let skus_raw: Option<String> = row.get(19)?;
            let skus: Vec<crate::model::ProductSkuItem> = skus_raw
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

            let standard_cat_raw: Option<String> = row.get(21)?;
            let standard_category = standard_cat_raw.and_then(|s| serde_json::from_str(&s).ok());

            let region_raw: Option<String> = row.get(22)?;
            let region = region_raw.and_then(|s| serde_json::from_str(&s).ok());

            let detail = ProductDetail {
                platform: row.get(0)?,
                item_id: row.get(1)?,
                url: row.get(2)?,
                title: row.get(3)?,
                brand: row.get(4)?,
                shop_name: row.get(5)?,
                shop_url: row.get(6)?,
                root_category: row.get(7)?,
                leaf_category: row.get(8)?,
                category_path,
                currency: row.get(10)?,
                current_price: row.get(11)?,
                original_price: row.get(12)?,
                sales_text: row.get(13)?,
                launch_date: row.get(14)?,
                rating_score: row.get(15)?,
                review_count: row.get(16)?,
                main_images,
                specifications,
                skus,
                description_markdown: row.get(20)?,
                standard_category,
                region,
                updated_at: row.get(23)?,
            };
            return Ok(Some(detail));
        }

        Ok(None)
    }

    /// 保存或更新商品，并自动追加历史快照
    pub fn save_product(&self, detail: &ProductDetail) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let category_path_json = serde_json::to_string(&detail.category_path)?;
        let main_images_json = serde_json::to_string(&detail.main_images)?;
        let specs_json = serde_json::to_string(&detail.specifications)?;
        let skus_json = serde_json::to_string(&detail.skus)?;
        let standard_category_json = detail
            .standard_category
            .as_ref()
            .and_then(|c| serde_json::to_string(c).ok());
        let region_json = detail
            .region
            .as_ref()
            .and_then(|r| serde_json::to_string(r).ok());

        // UPSERT 到 products 表
        conn.execute(
            r#"
            INSERT INTO products (
                platform, item_id, url, title, brand, shop_name, shop_url,
                root_category, leaf_category, category_path, currency,
                current_price, original_price, sales_text, launch_date,
                rating_score, review_count, main_images, specifications, skus,
                description_markdown, standard_category, region, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                ?8, ?9, ?10, ?11,
                ?12, ?13, ?14, ?15,
                ?16, ?17, ?18, ?19, ?20,
                ?21, ?22, ?23, ?24
            )
            ON CONFLICT(platform, item_id) DO UPDATE SET
                url = excluded.url,
                title = excluded.title,
                brand = excluded.brand,
                shop_name = excluded.shop_name,
                shop_url = excluded.shop_url,
                root_category = excluded.root_category,
                leaf_category = excluded.leaf_category,
                category_path = excluded.category_path,
                currency = excluded.currency,
                current_price = excluded.current_price,
                original_price = excluded.original_price,
                sales_text = excluded.sales_text,
                launch_date = excluded.launch_date,
                rating_score = excluded.rating_score,
                review_count = excluded.review_count,
                main_images = excluded.main_images,
                specifications = excluded.specifications,
                skus = excluded.skus,
                description_markdown = excluded.description_markdown,
                standard_category = excluded.standard_category,
                region = excluded.region,
                updated_at = excluded.updated_at
            "#,
            params![
                detail.platform,
                detail.item_id,
                detail.url,
                detail.title,
                detail.brand,
                detail.shop_name,
                detail.shop_url,
                detail.root_category,
                detail.leaf_category,
                category_path_json,
                detail.currency,
                detail.current_price,
                detail.original_price,
                detail.sales_text,
                detail.launch_date,
                detail.rating_score,
                detail.review_count,
                main_images_json,
                specs_json,
                skus_json,
                detail.description_markdown,
                standard_category_json,
                region_json,
                detail.updated_at,
            ],
        )?;

        // 获取内部产品主键 ID
        let product_id: i64 = conn.query_row(
            "SELECT id FROM products WHERE platform = ?1 AND item_id = ?2",
            params![detail.platform, detail.item_id],
            |row| row.get(0),
        )?;

        // 插入价格与动态指标快照
        conn.execute(
            r#"
            INSERT INTO product_snapshots (
                product_id, price, currency, sales_text, rating_score, review_count, recorded_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                product_id,
                detail.current_price,
                detail.currency,
                detail.sales_text,
                detail.rating_score,
                detail.review_count,
                detail.updated_at,
            ],
        )?;

        Ok(product_id)
    }

    /// 获取历史价格与指标变动快照
    pub fn get_price_history(&self, platform: &str, item_id: &str) -> Result<Vec<ProductSnapshot>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT s.id, s.product_id, s.price, s.currency, s.sales_text, s.rating_score, s.review_count, s.recorded_at
            FROM product_snapshots s
            JOIN products p ON s.product_id = p.id
            WHERE p.platform = ?1 AND p.item_id = ?2
            ORDER BY s.recorded_at ASC
            "#,
        )?;

        let rows = stmt.query_map(params![platform, item_id], |row| {
            Ok(ProductSnapshot {
                id: Some(row.get(0)?),
                product_id: row.get(1)?,
                price: row.get(2)?,
                currency: row.get(3)?,
                sales_text: row.get(4)?,
                rating_score: row.get(5)?,
                review_count: row.get(6)?,
                recorded_at: row.get(7)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// 保存买家评论列表
    pub fn save_reviews(&self, product_id: i64, reviews: &[ProductReview]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        for r in reviews {
            let images_json = serde_json::to_string(&r.images)?;
            conn.execute(
                r#"
                INSERT INTO product_reviews (
                    product_id, review_id, author, rating, review_date,
                    content, sku_purchased, images, helpful_votes, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(product_id, review_id) DO UPDATE SET
                    content = excluded.content,
                    helpful_votes = excluded.helpful_votes
                "#,
                params![
                    product_id,
                    r.review_id,
                    r.author,
                    r.rating,
                    r.review_date,
                    r.content,
                    r.sku_purchased,
                    images_json,
                    r.helpful_votes,
                    now,
                ],
            )?;
        }
        Ok(())
    }

    /// 根据平台和商品 ID 保存买家评论
    pub fn save_reviews_by_item(
        &self,
        platform: &str,
        item_id: &str,
        reviews: &[ProductReview],
    ) -> Result<()> {
        let product_id_opt: Option<i64> = {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT id FROM products WHERE platform = ?1 AND item_id = ?2",
                params![platform, item_id],
                |row| row.get(0),
            )
            .ok()
        };

        if let Some(pid) = product_id_opt {
            return self.save_reviews(pid, reviews);
        }
        Ok(())
    }

    /// 获取缓存的买家评论
    pub fn get_reviews(
        &self,
        platform: &str,
        item_id: &str,
        limit: usize,
    ) -> Result<Vec<ProductReview>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT r.review_id, r.author, r.rating, r.review_date, r.content,
                   r.sku_purchased, r.images, r.helpful_votes
            FROM product_reviews r
            JOIN products p ON r.product_id = p.id
            WHERE p.platform = ?1 AND p.item_id = ?2
            ORDER BY r.id DESC
            LIMIT ?3
            "#,
        )?;

        let rows = stmt.query_map(params![platform, item_id, limit as i64], |row| {
            let images_raw: String = row.get(6)?;
            let images: Vec<String> = serde_json::from_str(&images_raw).unwrap_or_default();
            Ok(ProductReview {
                review_id: row.get(0)?,
                author: row.get(1)?,
                rating: row.get(2)?,
                review_date: row.get(3)?,
                content: row.get(4)?,
                sku_purchased: row.get(5)?,
                images,
                helpful_votes: row.get(7)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// 根据 TTL 判断缓存是否依然有效
    pub fn is_cache_valid(&self, detail: &ProductDetail, ttl_hours: u64) -> bool {
        let now = chrono::Utc::now().timestamp();
        let age_seconds = now.saturating_sub(detail.updated_at);
        age_seconds < (ttl_hours as i64 * 3600)
    }
}
