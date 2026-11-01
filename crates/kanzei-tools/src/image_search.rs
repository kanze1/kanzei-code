//! Public image search uses the same bounded, DNS-pinned transport as webfetch.
use kanzei_harness::ToolCtx;
use serde::{Deserialize, Serialize};

const MAX_SEARCH_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RESULTS: usize = 5;

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageResult {
    pub title: String,
    pub image_url: String,
    pub page_url: String,
}

pub async fn search(query: &str, limit: usize, ctx: &ToolCtx) -> Result<Vec<ImageResult>, String> {
    if query.trim().is_empty() || query.chars().count() > 500 || !(1..=MAX_RESULTS).contains(&limit)
    {
        return Err("图片搜索需要 1–500 字关键词，结果数为 1–5".into());
    }
    let mut url = reqwest::Url::parse("https://www.bing.com/images/async").unwrap();
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("first", "0")
        .append_pair("count", &limit.to_string())
        .append_pair("adlt", "strict");
    let response = crate::webfetch::fetch_bytes(url.as_str(), ctx, MAX_SEARCH_BYTES).await?;
    if response.status != 200 {
        return Err(format!("图片搜索返回 HTTP {}", response.status));
    }
    let html = std::str::from_utf8(&response.body).map_err(|error| error.to_string())?;
    parse(html, limit)
}

fn parse(html: &str, limit: usize) -> Result<Vec<ImageResult>, String> {
    #[derive(Deserialize)]
    struct Metadata {
        t: String,
        murl: String,
        purl: String,
    }
    let document = dom_query::Document::from(html);
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for anchor in document.select("a.iusc").iter() {
        let metadata = anchor.attr("m").ok_or("图片搜索结果缺少来源元数据")?;
        let metadata: Metadata = serde_json::from_str(&metadata)
            .map_err(|error| format!("图片搜索来源元数据无效：{error}"))?;
        if metadata.t.trim().is_empty() {
            return Err("图片搜索结果缺少标题".into());
        }
        for url in [&metadata.murl, &metadata.purl] {
            let parsed = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
            {
                return Err("图片搜索返回了无效来源 URL".into());
            }
        }
        if seen.insert(metadata.murl.clone()) {
            results.push(ImageResult {
                title: metadata.t,
                image_url: metadata.murl,
                page_url: metadata.purl,
            });
        }
        if results.len() == limit {
            break;
        }
    }
    if results.is_empty() {
        return Err("搜索页未提供可验证的图片结果；可能无匹配项、受到访问限制或页面格式已变化，不能据此宣称表情库为空".into());
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_search_decodes_source_metadata_and_rejects_unusable_pages() {
        let html = r#"<a class="iusc" m="{&quot;t&quot;:&quot;开心 &amp; 庆祝&quot;,&quot;murl&quot;:&quot;https://example.com/a.gif?x=1&amp;y=2&quot;,&quot;purl&quot;:&quot;https://example.com/source&quot;}"></a>"#;
        let results = parse(html, 3).unwrap();
        assert_eq!(results[0].title, "开心 & 庆祝");
        assert_eq!(results[0].image_url, "https://example.com/a.gif?x=1&y=2");
        assert_eq!(parse(&format!("{html}{html}"), 3).unwrap().len(), 1);
        assert!(parse("<html>captcha</html>", 3).is_err());
        assert!(parse(r#"<a class="iusc" m="{}"></a>"#, 3).is_err());
    }

    #[tokio::test]
    #[ignore = "requires live Bing image search and a reachable public result image"]
    async fn live_sticker_search_returns_downloadable_pixels() {
        let root = kanzei_base::test_support::TestDir::new("live-sticker-search-");
        let ctx = ToolCtx::new(root.to_path_buf(), root.to_path_buf());
        let results = search("蹲坑 表情包", 3, &ctx).await.unwrap();
        let response = crate::webfetch::fetch_bytes(&results[0].image_url, &ctx, 16 * 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert!(crate::image::media_type(&response.body).is_some());
        println!(
            "Verified {} actual search candidates and original image bytes",
            results.len()
        );
    }
}
