use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, USER_AGENT};
use reqwest::{Client, Proxy};
use std::time::Duration;
use tracing::info;

use crate::config::NetworkConfig;

#[derive(Clone)]
pub struct HttpClient {
    inner: Client,
    pub retry_count: usize,
}

impl HttpClient {
    pub fn new(config: &NetworkConfig) -> Result<Self> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .cookie_store(true);

        // 如果配置了代理 (HTTP / SOCKS5)
        if let Some(proxy_url) = &config.proxy {
            if !proxy_url.trim().is_empty() {
                let proxy = Proxy::all(proxy_url)
                    .with_context(|| format!("无效的代理地址: {}", proxy_url))?;
                builder = builder.proxy(proxy);
                info!("已启用网络代理: {}", proxy_url);
            }
        }

        // 默认浏览器请求头伪装
        let ua = config.user_agent.clone().unwrap_or_else(|| {
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36".to_string()
        });

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_str(&ua)?);
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"),
        );
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("en-US,en;q=0.9"),
        );
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("max-age=0"));
        headers.insert("sec-ch-ua", HeaderValue::from_static("\"Chromium\";v=\"128\", \"Not;A=Brand\";v=\"24\", \"Google Chrome\";v=\"128\""));
        headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
        headers.insert("sec-ch-ua-platform", HeaderValue::from_static("\"macOS\""));
        headers.insert("sec-fetch-dest", HeaderValue::from_static("document"));
        headers.insert("sec-fetch-mode", HeaderValue::from_static("navigate"));
        headers.insert("sec-fetch-site", HeaderValue::from_static("none"));
        headers.insert("sec-fetch-user", HeaderValue::from_static("?1"));
        headers.insert("upgrade-insecure-requests", HeaderValue::from_static("1"));

        builder = builder.default_headers(headers);

        let inner = builder.build().context("构建 HTTP 客户端失败")?;

        Ok(Self {
            inner,
            retry_count: config.retry_count,
        })
    }

    /// 发起带重试机制的 GET 请求获取文本响应
    pub async fn get_text(&self, url: &str, extra_cookie: Option<&str>) -> Result<String> {
        let mut attempts = 0;
        let mut last_err = anyhow::anyhow!("请求未知错误");

        while attempts <= self.retry_count {
            attempts += 1;
            let mut req = self.inner.get(url);
            if let Some(c) = extra_cookie {
                if !c.is_empty() {
                    req = req.header("Cookie", c);
                }
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let text = resp.text().await.context("解析响应文本失败")?;
                        return Ok(text);
                    } else if status.as_u16() == 404 {
                        anyhow::bail!("页面不存在 (404 Not Found): {}", url);
                    } else if status.as_u16() == 503 {
                        // 亚马逊常见的人机验证或临时限流
                        last_err = anyhow::anyhow!("服务暂不可用或触发反爬拦截 (HTTP 503): {}", url);
                    } else {
                        last_err = anyhow::anyhow!("HTTP 错误状态码: {}", status);
                    }
                }
                Err(e) => {
                    last_err = anyhow::anyhow!("网络请求失败: {:?}", e);
                }
            }

            if attempts <= self.retry_count {
                tokio::time::sleep(Duration::from_millis(1000 * attempts as u64)).await;
            }
        }

        Err(last_err)
    }
}
