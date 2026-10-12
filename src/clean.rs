use regex::Regex;

/// 将富文本 HTML 清洗为精简、高可读性的 Markdown 文本，去除冗余标签与空行
pub fn html_to_clean_markdown(html: &str) -> String {
    if html.trim().is_empty() {
        return String::new();
    }

    let mut text = html.to_string();

    // 1. 去除 <script>, <style>, <noscript>, <iframe> 及其内容
    let remove_tags = [
        r"(?is)<script[^>]*>.*?</script>",
        r"(?is)<style[^>]*>.*?</style>",
        r"(?is)<noscript[^>]*>.*?</noscript>",
        r"(?is)<iframe[^>]*>.*?</iframe>",
        r"(?is)<!--.*?-->",
    ];
    for pattern in remove_tags {
        if let Ok(re) = Regex::new(pattern) {
            text = re.replace_all(&text, "").to_string();
        }
    }

    // 2. 结构标签转为换行与 Markdown 标识
    let replacements = [
        (r"(?i)<br\s*/?>", "\n"),
        (r"(?i)</?p[^>]*>", "\n\n"),
        (r"(?i)<h[1-3][^>]*>", "\n\n### "),
        (r"(?i)</h[1-3]>", "\n"),
        (r"(?i)<h[4-6][^>]*>", "\n\n#### "),
        (r"(?i)</h[4-6]>", "\n"),
        (r"(?i)<li[^>]*>", "\n- "),
        (r"(?i)</li>", ""),
        (r"(?i)</?(ul|ol)[^>]*>", "\n"),
        (r"(?i)<strong[^>]*>|<b[^>]*>", "**"),
        (r"(?i)</strong>|</b>", "**"),
    ];

    for (pattern, rep) in replacements {
        if let Ok(re) = Regex::new(pattern) {
            text = re.replace_all(&text, rep).to_string();
        }
    }

    // 3. 剥除所有剩余 HTML 标签
    if let Ok(re) = Regex::new(r"<[^>]+>") {
        text = re.replace_all(&text, "").to_string();
    }

    // 4. 常见 HTML 实体字符还原
    let entities = [
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&bull;", "•"),
        ("&trade;", "™"),
        ("&copy;", "©"),
    ];
    for (entity, real) in entities {
        text = text.replace(entity, real);
    }

    // 5. 压缩多余连续空格与多余空行
    let mut cleaned_lines = Vec::new();
    let mut last_was_empty = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !last_was_empty {
                cleaned_lines.push("");
                last_was_empty = true;
            }
        } else {
            cleaned_lines.push(trimmed);
            last_was_empty = false;
        }
    }

    cleaned_lines.join("\n").trim().to_string()
}
