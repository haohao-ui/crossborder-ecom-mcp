use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 原始元数据根结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyResponse {
    pub code: i32,
    pub data: TaxonomyData,
    pub msg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxonomyData {
    pub category: Vec<CategoryItem>,
    pub hot_product_category: Vec<SimpleCategory>,
    pub country: Vec<CountryItem>,
    #[serde(rename = "crate")]
    pub commission_rate: Vec<RangeFilter>,
    pub relate_author: Vec<RangeFilter>,
    pub sold_count: Vec<RangeFilter>,
    pub sale_amount: Vec<RangeFilter>,
    pub day7_sold_count: Vec<RangeFilter>,
    pub day7_sale_amount: Vec<RangeFilter>,
    pub price_amount: Vec<RangeFilter>,
    pub author_order_rate: Vec<RangeFilter>,
}

/// 标准一级分类
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryItem {
    pub c_code: String,
    pub c_name: String,
    pub rank: String,
    #[serde(default)]
    pub sub: HashMap<String, SubCategoryItem>,
}

/// 标准二级分类
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubCategoryItem {
    pub c_code: String,
    pub c_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleCategory {
    pub c_code: String,
    pub c_name: String,
}

/// 标准国家/地区定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountryItem {
    pub region_code: String,
    pub region: String,
    pub region_code_name: String,
    pub region_name: String,
}

/// 筛选与分档区间
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeFilter {
    pub code: String,
    pub name: String,
}

/// 匹配出的标准分类结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchedCategory {
    pub main_code: String,
    pub main_name: String,
    pub sub_code: Option<String>,
    pub sub_name: Option<String>,
}

/// 全局懒加载嵌入式标准元数据
pub static TAXONOMY: Lazy<TaxonomyData> = Lazy::new(|| {
    let raw = include_str!("../data/taxonomy.json");
    let resp: TaxonomyResponse = serde_json::from_str(raw).expect("解析内置 taxonomy.json 失败");
    resp.data
});

/// 判断文本中是否包含指定关键词或词组
/// 对于包含英文字符的词，要求两端满足单词边界（非 ASCII 字母和数字），避免诸如 "1pcs" 误匹配 "pc"
pub fn contains_word_or_phrase(text: &str, kw: &str) -> bool {
    let kw = kw.trim();
    if kw.is_empty() {
        return false;
    }

    let has_ascii = kw.chars().any(|c| c.is_ascii_alphanumeric());
    if !has_ascii {
        return text.contains(kw);
    }

    let kw_len = kw.len();
    let mut search_from = 0;
    while let Some(rel_idx) = text[search_from..].find(kw) {
        let start = search_from + rel_idx;
        let end = start + kw_len;

        let left_boundary_ok = if start == 0 {
            true
        } else {
            let prev_char = text[..start].chars().next_back().unwrap();
            !prev_char.is_ascii_alphanumeric()
        };

        let right_boundary_ok = if end == text.len() {
            true
        } else {
            let next_char = text[end..].chars().next().unwrap();
            !next_char.is_ascii_alphanumeric()
        };

        if left_boundary_ok && right_boundary_ok {
            return true;
        }

        if let Some(ch) = text[start..].chars().next() {
            search_from = start + ch.len_utf8();
        } else {
            break;
        }
    }

    false
}

/// 根据文本（标题、面包屑、关键词）智能推荐匹配的标准一级与二级分类
pub fn match_category(text: &str) -> Option<MatchedCategory> {
    if text.trim().is_empty() {
        return None;
    }
    let lower_text = text.to_lowercase();

    // 1. 优先在二级分类中做关键词匹配 (更精准)
    for cat in &TAXONOMY.category {
        for sub in cat.sub.values() {
            let sub_name_lower = sub.c_name.to_lowercase();
            if contains_word_or_phrase(&lower_text, &sub_name_lower) {
                return Some(MatchedCategory {
                    main_code: cat.c_code.clone(),
                    main_name: cat.c_name.clone(),
                    sub_code: Some(sub.c_code.clone()),
                    sub_name: Some(sub.c_name.clone()),
                });
            }
        }
    }

    // 2. 尝试英文与行业关键词同义词映射
    let keyword_mappings: &[(&[&str], &str, Option<&str>)] = &[
        // (关键词组, 一级code, 二级code)
        // 居家日用细分：节庆派对用品与家居装饰
        (
            &[
                "christmas",
                "xmas",
                "nativity",
                "halloween",
                "easter",
                "thanksgiving",
                "party decor",
                "party decora",
                "party decoration",
                "party supplies",
                "jesus scene",
                "festival",
                "carnival",
                "balloon",
                "balloons",
                "confetti",
                "圣诞",
                "节庆",
                "派对用品",
                "节日装饰",
            ],
            "10",
            Some("852488"),
        ),
        (
            &[
                "statue",
                "sculpture",
                "figurine",
                "desktop decoration",
                "home decoration",
                "home decor",
                "office decor",
                "office decora",
                "table decor",
                "wall decor",
                "wall art",
                "ornament",
                "vase",
                "candle holder",
                "artificial flower",
                "摆件",
                "雕像",
                "装饰品",
                "家居装饰",
                "桌面摆件",
            ],
            "10",
            Some("852104"),
        ),
        (
            &[
                "storage box",
                "storage bag",
                "organizer",
                "clothes rack",
                "hanger",
                "收纳盒",
                "收纳袋",
                "收纳整理",
                "衣架",
            ],
            "10",
            Some("851848"),
        ),
        (
            &[
                "shower curtain",
                "towel",
                "bath mat",
                "toothbrush holder",
                "毛巾",
                "浴巾",
                "浴帘",
                "浴室用品",
            ],
            "10",
            Some("851976"),
        ),
        // 鞋靴配件优先于基础鞋靴，避免误判
        (
            &[
                "shoelace",
                "shoe lace",
                "shoelaces",
                "laces",
                "insole",
                "shoe horn",
                "shoe tree",
                "shoe accessories",
                "shoes accessories",
                "鞋带",
                "鞋垫",
                "鞋配件",
                "鞋靴配件",
            ],
            "6",
            Some("900744"),
        ),
        (
            &[
                "men shoe",
                "men's shoe",
                "men sneaker",
                "men boot",
                "男鞋",
                "男士鞋",
            ],
            "6",
            Some("900616"),
        ),
        (
            &[
                "women shoe",
                "women's shoe",
                "women sneaker",
                "high heel",
                "pump",
                "女鞋",
                "女士鞋",
                "高跟鞋",
            ],
            "6",
            Some("900488"),
        ),
        (
            &[
                "sneaker",
                "boot",
                "sandal",
                "slipper",
                "shoe",
                "footwear",
                "shoes",
                "boots",
                "sneakers",
                "鞋",
                "运动鞋",
                "凉鞋",
                "拖鞋",
            ],
            "6",
            None,
        ),
        // 美妆个护
        (
            &[
                "beauty", "cosmetic", "skincare", "lipstick", "perfume", "makeup", "facial",
                "lotion", "shampoo", "美妆", "护肤", "口红", "香水",
            ],
            "14",
            Some("848776"),
        ),
        // 服装：避免使用孤立的 "women" 或 "men" 以免误判配件/鞋靴
        (
            &[
                "women clothing",
                "women's clothing",
                "women clothes",
                "dress",
                "skirt",
                "bra",
                "blouse",
                "lingerie",
                "underwear",
                "leggings",
                "女装",
                "裙",
                "连衣裙",
                "文胸",
            ],
            "2",
            Some("842504"),
        ),
        (
            &[
                "men clothing",
                "men's clothing",
                "shirt",
                "suit",
                "jacket",
                "hoodie",
                "t-shirt",
                "pants",
                "trousers",
                "jeans",
                "男装",
                "男士",
            ],
            "3",
            Some("839944"),
        ),
        // 电脑办公：避免单字母简写 "pc" 误匹配 "1pcs" 或其它词汇
        (
            &[
                "laptop",
                "computer",
                "notebook",
                "desktop computer",
                "desktop pc",
                "gaming pc",
                "pc computer",
                "keyboard",
                "mouse",
                "monitor",
                "电脑",
                "笔记本",
                "台式机",
                "显示器",
            ],
            "15",
            Some("824840"),
        ),
        // 手机数码与影音设备
        (
            &[
                "earbuds",
                "headphone",
                "headset",
                "earphones",
                "bluetooth earbuds",
                "speaker",
                "soundbar",
                "audio",
                "hifi",
                "耳机",
                "蓝牙耳机",
                "音箱",
                "音响",
                "影音",
            ],
            "16",
            Some("909320"),
        ),
        (
            &[
                "phone",
                "smartphone",
                "iphone",
                "phone case",
                "charger",
                "cable",
                "power bank",
                "screen protector",
                "数码",
                "手机",
                "手机壳",
                "充电器",
                "数据线",
            ],
            "16",
            Some("909064"),
        ),
        // 运动户外与厨房
        (
            &[
                "sport", "fitness", "yoga", "outdoor", "camping", "hiking", "cycling", "运动",
                "户外", "健身", "瑜伽",
            ],
            "9",
            Some("835336"),
        ),
        (
            &[
                "kitchen",
                "coffee",
                "knife",
                "cookware",
                "frying pan",
                "tableware",
                "cutlery",
                "厨房",
                "餐具",
                "茶具",
                "刀具",
            ],
            "11",
            Some("858504"),
        ),
        // 箱包与首饰
        (
            &[
                "bag",
                "backpack",
                "wallet",
                "handbag",
                "tote bag",
                "luggage",
                "suitcase",
                "箱包",
                "背包",
                "女包",
                "钱包",
                "旅行箱",
            ],
            "7",
            Some("902408"),
        ),
        (
            &[
                "jewelry", "necklace", "ring", "earring", "bracelet", "diamond", "gold", "silver",
                "珠宝", "首饰", "项链", "戒指", "耳环",
            ],
            "28",
            Some("955016"),
        ),
        (
            &[
                "watch",
                "smartwatch",
                "glasses",
                "sunglasses",
                "wig",
                "hair extension",
                "手表",
                "眼镜",
                "太阳镜",
                "假发",
            ],
            "8",
            Some("905352"),
        ),
        // 玩具与宠物与家电
        (
            &[
                "toy",
                "puzzle",
                "doll",
                "action figure",
                "game",
                "plush",
                "玩具",
                "手办",
                "拼图",
                "公仔",
            ],
            "19",
            Some("859656"),
        ),
        (
            &[
                "pet",
                "dog",
                "cat",
                "aquarium",
                "pet leash",
                "pet collar",
                "宠物",
                "猫",
                "狗",
            ],
            "17",
            Some("812168"),
        ),
        (
            &[
                "appliance",
                "refrigerator",
                "vacuum",
                "air purifier",
                "blender",
                "家电",
                "吸尘器",
                "冰箱",
            ],
            "13",
            Some("844808"),
        ),
    ];

    for (kws, main_code, sub_code) in keyword_mappings {
        for kw in *kws {
            if contains_word_or_phrase(&lower_text, kw) {
                if let Some(cat) = TAXONOMY.category.iter().find(|c| c.c_code == *main_code) {
                    let sub_name = sub_code
                        .and_then(|code| cat.sub.get(code))
                        .map(|s| s.c_name.clone());
                    return Some(MatchedCategory {
                        main_code: cat.c_code.clone(),
                        main_name: cat.c_name.clone(),
                        sub_code: sub_code.map(|s| s.to_string()),
                        sub_name,
                    });
                }
            }
        }
    }

    // 3. 匹配一级分类中文名
    for cat in &TAXONOMY.category {
        let cat_name_lower = cat.c_name.to_lowercase();
        if contains_word_or_phrase(&lower_text, &cat_name_lower) {
            return Some(MatchedCategory {
                main_code: cat.c_code.clone(),
                main_name: cat.c_name.clone(),
                sub_code: None,
                sub_name: None,
            });
        }
    }

    None
}

/// 根据 URL 自动匹配其出海目标国家/地区
pub fn match_country_by_url(url: &str) -> Option<CountryItem> {
    let lower_url = url.to_lowercase();

    let domain_mappings: &[(&str, &str)] = &[
        (".co.uk", "GB"),
        (".de", "DE"),
        (".fr", "FR"),
        (".co.jp", "JP"),
        (".es", "ES"),
        (".it", "IT"),
        (".vn", "VN"),
        (".th", "TH"),
        (".my", "MY"),
        (".ph", "PH"),
        (".sg", "SG"),
        (".com.br", "BR"),
        (".com.mx", "MX"),
        (".at", "AT"),
        (".be", "BE"),
        (".nl", "NL"),
        (".pl", "PL"),
        (".pt", "PT"),
        (".co.id", "ID"),
        (".com", "US"), // 默认美国
    ];

    for (domain_suffix, region_code) in domain_mappings {
        if lower_url.contains(domain_suffix) {
            if let Some(c) = TAXONOMY
                .country
                .iter()
                .find(|item| item.region_code == *region_code)
            {
                return Some(c.clone());
            }
        }
    }

    // 降级返回 US
    TAXONOMY
        .country
        .iter()
        .find(|item| item.region_code == "US")
        .cloned()
}
