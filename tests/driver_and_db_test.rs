use crossborder_ecom_mcp::clean::html_to_clean_markdown;
use crossborder_ecom_mcp::db::Database;
use crossborder_ecom_mcp::model::{ProductDetail, ProductReview};
use crossborder_ecom_mcp::platforms::amazon::AmazonDriver;
use crossborder_ecom_mcp::platforms::shopify::ShopifyDriver;
use crossborder_ecom_mcp::platforms::PlatformDriver;
use std::collections::HashMap;

#[test]
fn test_html_cleaner() {
    let raw_html = r#"
        <div class="product">
            <h1>Super Laptop 2024</h1>
            <p>This is an <b>amazing</b> product with high performance.</p>
            <ul>
                <li>16GB RAM</li>
                <li>1TB SSD</li>
            </ul>
            <script>alert('bad');</script>
        </div>
    "#;

    let md = html_to_clean_markdown(raw_html);
    assert!(md.contains("### Super Laptop 2024"));
    assert!(md.contains("**amazing**"));
    assert!(md.contains("- 16GB RAM"));
    assert!(!md.contains("alert"));
}

#[test]
fn test_platform_link_parsing() {
    let amazon = AmazonDriver::new();
    let url1 = "https://www.amazon.com/dp/B08N5WRWNW";
    assert!(amazon.can_handle(url1));
    assert_eq!(amazon.extract_item_id(url1), Some("B08N5WRWNW".to_string()));

    let url2 = "https://www.amazon.co.jp/product-name/dp/B09XYZ1234/ref=sr_1_1";
    assert!(amazon.can_handle(url2));
    assert_eq!(amazon.extract_item_id(url2), Some("B09XYZ1234".to_string()));

    let shopify = ShopifyDriver::new();
    let s_url = "https://allbirds.com/products/mens-tree-runners-geyser-light-blue";
    assert!(shopify.can_handle(s_url));
    assert_eq!(
        shopify.extract_item_id(s_url),
        Some("mens-tree-runners-geyser-light-blue".to_string())
    );

    let tiktok = crossborder_ecom_mcp::platforms::tiktok::TikTokShopDriver::new();
    let tt_url = "https://shop.tiktok.com/view/product/1729482847291829182?region=US&locale=en";
    assert!(tiktok.can_handle(tt_url));
    assert_eq!(
        tiktok.extract_item_id(tt_url),
        Some("1729482847291829182".to_string())
    );

    // 测试纯数字 ID 与 PDP 页面识别
    let raw_id = "1730195684975677530";
    assert!(tiktok.can_handle(raw_id));
    assert_eq!(tiktok.extract_item_id(raw_id), Some(raw_id.to_string()));

    let pdp_url = "https://shop.tiktok.com/us/pdp/invisible-sleep-headphones/1730195684975677530";
    assert!(tiktok.can_handle(pdp_url));
    assert_eq!(tiktok.extract_item_id(pdp_url), Some(raw_id.to_string()));

    // 测试 Modern.js HTML 离线样本解析
    if let Ok(sample_html) = std::fs::read_to_string("tiktok_sample.html") {
        let parsed = tiktok
            .parse_html_product(raw_id, pdp_url, &sample_html)
            .expect("解析样本失败");
        assert!(parsed.title.contains("MSHUKCOE Invisible Sleep"));
        assert_eq!(parsed.brand, Some("MSHUKCOE".to_string()));
        assert_eq!(parsed.shop_name, Some("Listening Digital".to_string()));
        assert_eq!(parsed.current_price, 19.24);
        assert_eq!(parsed.original_price, Some(21.97));
        assert_eq!(parsed.skus.len(), 3);
        assert_eq!(parsed.skus[0].name, "【Black】");
        assert_eq!(parsed.skus[0].price, Some(19.24));
        assert_eq!(parsed.skus[0].stock, Some(151));
        assert_eq!(
            parsed
                .standard_category
                .as_ref()
                .map(|c| c.main_name.as_str()),
            Some("手机与数码")
        );
        assert_eq!(
            parsed
                .standard_category
                .as_ref()
                .and_then(|c| c.sub_name.as_deref()),
            Some("影音设备")
        );
        assert_eq!(parsed.main_images.len(), 8);
        assert!(!parsed.description_markdown.is_empty());
        assert!(parsed
            .description_markdown
            .contains("Bluetooth Version: 5.3"));
        assert!(parsed.description_markdown.contains("![详情图]"));
    }

    let ae = crossborder_ecom_mcp::platforms::aliexpress::AliExpressDriver::new();
    let ae_url1 = "https://www.aliexpress.com/item/1005006283920192.html";
    assert!(ae.can_handle(ae_url1));
    assert_eq!(
        ae.extract_item_id(ae_url1),
        Some("1005006283920192".to_string())
    );

    let ae_url2 = "https://aliexpress.us/item/32812345678.html?spm=a2g0o.productlist.main.1.1234";
    assert!(ae.can_handle(ae_url2));
    assert_eq!(ae.extract_item_id(ae_url2), Some("32812345678".to_string()));
}

#[test]
fn test_database_crud_and_snapshot() {
    let temp_db_path = format!(
        "data/test_{}.db",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let db = Database::init(&temp_db_path).expect("初始化测试 DB 失败");

    let mut specs = HashMap::new();
    specs.insert("Color".to_string(), "Space Gray".to_string());
    specs.insert("RAM".to_string(), "32GB".to_string());

    let detail = ProductDetail {
        platform: "amazon".to_string(),
        item_id: "B0TEST1234".to_string(),
        url: "https://www.amazon.com/dp/B0TEST1234".to_string(),
        title: "Test Laptop 16-inch".to_string(),
        brand: Some("Apple".to_string()),
        shop_name: Some("Apple Store".to_string()),
        shop_url: Some("https://www.amazon.com/stores/Apple".to_string()),
        root_category: Some("Electronics".to_string()),
        leaf_category: Some("Laptops".to_string()),
        category_path: vec![
            "Electronics".to_string(),
            "Computers".to_string(),
            "Laptops".to_string(),
        ],
        currency: "USD".to_string(),
        current_price: 1999.99,
        original_price: Some(2199.99),
        sales_text: Some("1K+ bought in past month".to_string()),
        launch_date: Some("October 24, 2023".to_string()),
        rating_score: Some(4.8),
        review_count: Some(542),
        main_images: vec!["https://m.media-amazon.com/images/I/test.jpg".to_string()],
        specifications: specs,
        skus: vec![],
        description_markdown: "### Great test laptop".to_string(),
        standard_category: Some(crossborder_ecom_mcp::taxonomy::MatchedCategory {
            main_code: "15".to_string(),
            main_name: "电脑办公".to_string(),
            sub_code: Some("824840".to_string()),
            sub_name: Some("电脑整机".to_string()),
        }),
        region: Some(crossborder_ecom_mcp::taxonomy::CountryItem {
            region_code: "US".to_string(),
            region: "US".to_string(),
            region_code_name: "美国".to_string(),
            region_name: "美国".to_string(),
        }),
        updated_at: chrono::Utc::now().timestamp(),
    };

    // 保存商品
    let pid = db.save_product(&detail).expect("保存商品失败");
    assert!(pid > 0);

    // 查询商品
    let fetched = db
        .get_product("amazon", "B0TEST1234")
        .expect("查询失败")
        .expect("未找到记录");
    assert_eq!(fetched.title, "Test Laptop 16-inch");
    assert_eq!(fetched.current_price, 1999.99);
    assert_eq!(fetched.root_category, Some("Electronics".to_string()));
    assert_eq!(fetched.launch_date, Some("October 24, 2023".to_string()));

    // 再次保存（模拟调价），验证快照增加
    let mut updated_detail = detail.clone();
    updated_detail.current_price = 1899.99; // 降价 $100
    updated_detail.updated_at += 3600;
    db.save_product(&updated_detail).expect("再次更新商品失败");

    let history = db
        .get_price_history("amazon", "B0TEST1234")
        .expect("获取价格历史失败");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].price, 1999.99);
    assert_eq!(history[1].price, 1899.99);

    // 测试买家评论保存与查询
    let reviews = vec![ProductReview {
        review_id: "REV001".to_string(),
        author: "John Doe".to_string(),
        rating: 5,
        review_date: "2024-01-10".to_string(),
        content: "Awesome machine!".to_string(),
        sku_purchased: Some("32GB Space Gray".to_string()),
        images: vec![],
        helpful_votes: Some(12),
    }];
    db.save_reviews(pid, &reviews).expect("保存评论失败");

    let cached_reviews = db
        .get_reviews("amazon", "B0TEST1234", 10)
        .expect("查询评论失败");
    assert_eq!(cached_reviews.len(), 1);
    assert_eq!(cached_reviews[0].author, "John Doe");

    // 清理临时测试数据库
    let _ = std::fs::remove_file(&temp_db_path);
}

#[test]
fn test_taxonomy_matching() {
    use crossborder_ecom_mcp::taxonomy::{match_category, match_country_by_url, TAXONOMY};

    assert_eq!(TAXONOMY.category.len(), 30);
    assert_eq!(TAXONOMY.country.len(), 20);

    // 1. 测试分类匹配
    let match1 = match_category("Women's Casual Summer Dress").expect("应匹配女装");
    assert_eq!(match1.main_code, "2");
    assert_eq!(match1.main_name, "女装与女士内衣");

    let match2 = match_category("Gaming Laptop with 32GB RAM").expect("应匹配电脑");
    assert_eq!(match2.main_code, "15");
    assert_eq!(match2.main_name, "电脑办公");

    let match3 = match_category("Natural Skincare Cleanser & Lipstick").expect("应匹配美妆个护");
    assert_eq!(match3.main_code, "14");
    assert_eq!(match3.main_name, "美妆个护");

    let match_shoelace = match_category("1Pair Round Shoelaces Quality 8mm Thicker Shoelace Sneakers Rope Laces 100/120/140/160CM Boots Shoe laces Shoes Accessories").expect("应匹配鞋靴配件");
    assert_eq!(match_shoelace.main_code, "6");
    assert_eq!(match_shoelace.main_name, "鞋靴");
    assert_eq!(match_shoelace.sub_code, Some("900744".to_string()));
    assert_eq!(match_shoelace.sub_name, Some("鞋靴配件".to_string()));

    // 重点验证：带 1PCS 和 Desktop Decoration 的耶稣/圣诞装饰摆件不能误判为电脑办公，必须归入居家日用 > 节庆及派对用品
    let match_nativity = match_category("1PCS Nativity Desktop Decoration Statue Home Decoration Office Decora Art Theme Party Decora Christmas Decora Jesus Scene Decora").expect("应匹配居家日用/节庆");
    assert_eq!(match_nativity.main_code, "10");
    assert_eq!(match_nativity.main_name, "居家日用");
    assert_eq!(match_nativity.sub_code, Some("852488".to_string()));
    assert_eq!(match_nativity.sub_name, Some("节庆及派对用品".to_string()));

    // 验证桌面雕像装饰匹配到 居家日用 > 装饰
    let match_statue =
        match_category("Nordic Modern Statue Ceramic Desktop Decoration For Home Living Room")
            .expect("应匹配家居装饰");
    assert_eq!(match_statue.main_code, "10");
    assert_eq!(match_statue.main_name, "居家日用");
    assert_eq!(match_statue.sub_code, Some("852104".to_string()));
    assert_eq!(match_statue.sub_name, Some("装饰".to_string()));

    // 验证包含 1PCS 的非电脑商品不会因为子串 "pc" 误判为电脑
    let match_storage =
        match_category("1PCS Foldable Fabric Storage Box Organizer").expect("应匹配家居收纳");
    assert_eq!(match_storage.main_code, "10");
    assert_eq!(match_storage.main_name, "居家日用");
    assert_eq!(match_storage.sub_code, Some("851848".to_string()));
    assert_eq!(match_storage.sub_name, Some("家居收纳".to_string()));

    // 2. 测试国家匹配
    let c_us = match_country_by_url("https://www.amazon.com/dp/B08N5WRWNW").expect("应匹配美国");
    assert_eq!(c_us.region_code, "US");

    let c_jp = match_country_by_url("https://www.amazon.co.jp/dp/B08N5WRWNW").expect("应匹配日本");
    assert_eq!(c_jp.region_code, "JP");

    let c_gb = match_country_by_url("https://www.amazon.co.uk/dp/B08N5WRWNW").expect("应匹配英国");
    assert_eq!(c_gb.region_code, "GB");
}
