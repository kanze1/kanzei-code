//! Online candidates become scoped, verified assets before they can be sent.
use kanzei_harness::{ExternalSourceKind, ToolCtx, ToolOutput};
use serde_json::json;
use std::path::Path;

pub(super) async fn search(
    query: &str,
    limit: usize,
    workspace: &Path,
    ctx: &ToolCtx,
) -> anyhow::Result<ToolOutput> {
    let candidates = kanzei_tools::image_search::search(&format!("{query} 表情包"), limit, ctx)
        .await
        .map_err(anyhow::Error::msg)?;
    let mut results = Vec::new();
    let mut images = Vec::new();
    let mut errors = Vec::new();
    let mut remaining = usize::try_from(super::media::MAX_REPLY_BYTES)?;
    // Each returned ID has original bytes, not an untested URL or a thumbnail.
    for source in candidates {
        let downloaded =
            kanzei_tools::webfetch::fetch_bytes(&source.image_url, ctx, remaining).await;
        let result = match downloaded {
            Ok(response) => {
                let bytes = response.body.clone();
                match super::image::image(response) {
                    Ok(image) => {
                        let workspace = workspace.to_owned();
                        let cached = tokio::task::spawn_blocking(move || {
                            super::media::cache_sticker(&workspace, &source, &bytes)
                                .map(|sticker| (sticker, image, bytes.len()))
                        })
                        .await??;
                        Ok(cached)
                    }
                    Err(error) => Err(error.to_string()),
                }
            }
            Err(error) => Err(error),
        };
        match result {
            Ok((sticker, image, size)) => {
                remaining -= size;
                results.push(json!({"sticker":sticker,"preview_index":images.len()}));
                images.push(image);
            }
            Err(error) => errors.push(error),
        }
    }
    anyhow::ensure!(
        !results.is_empty(),
        "搜索候选均未能下载为图片：{}",
        errors.join("；")
    );
    Ok(ToolOutput::ok(json!({"source":"web","stickers":results,"download_errors":errors,"manifest":"/workspace/stickers/manifest.json"}).to_string())
        .with_external_source(ExternalSourceKind::Web, "Bing image search; linked original sources")
        .with_images(images))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires live Bing image search and reachable original sticker images"]
    async fn live_search_caches_original_stickers_and_prepares_a_reply() {
        let root = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(root.path().into(), root.path().into());
        let workspace = root.path().join("workspace");
        let output = search("蹲坑", 3, &workspace, &ctx).await.unwrap();
        let content: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        let id = content["stickers"][0]["sticker"]["id"].as_str().unwrap();
        let input: super::super::media::ReplyInput =
            serde_json::from_value(json!({"attachments":[{"type":"sticker","id":id}]})).unwrap();
        let message = input.prepare(&workspace).unwrap();
        assert_eq!(
            message.wire[0]["data"]["file"],
            format!("base64://{}", output.images[0].data)
        );
        assert_eq!(message.wire[0]["data"]["sub_type"], 1);
        assert!(!message.record.to_string().contains("base64://"));
        println!("Verified {} downloaded, attributed stickers; original bytes prepared for QQ without sending",output.images.len());
    }
}
