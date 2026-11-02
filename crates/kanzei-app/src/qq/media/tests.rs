use super::*;

#[test]
fn searched_original_stickers_are_sendable_and_keep_existing_collection() {
    let root = tempfile::tempdir().unwrap();
    let source = kanzei_tools::image_search::ImageResult {
        title: "开心".into(),
        image_url: "https://example.com/original.gif".into(),
        page_url: "https://example.com/source".into(),
    };
    let bytes = b"GIF89a-original-animation";
    let sticker = cache_sticker(root.path(), &source, bytes).unwrap();
    let other = kanzei_tools::image_search::ImageResult {
        title: "庆祝".into(),
        ..source
    };
    cache_sticker(root.path(), &other, b"GIF89a-other-animation").unwrap();
    assert_eq!(stickers(root.path()).unwrap().len(), 2);
    let offered = serde_json::to_value(sticker.offer()).unwrap();
    let input: ReplyInput = serde_json::from_value(offered["reply"].clone()).unwrap();
    let message = input.prepare(root.path()).unwrap();
    assert_eq!(
        STANDARD
            .decode(
                message.wire[0]["data"]["file"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("base64://")
                    .unwrap()
            )
            .unwrap(),
        bytes
    );
    assert!(stickers(root.path()).unwrap()[0]
        .description
        .contains("https://example.com/source"));
    assert!(!std::fs::read_to_string(root.path().join(MANIFEST))
        .unwrap()
        .contains("\"reply\""));
    assert!(cache_sticker(root.path(), &other, b"<html>not pixels</html>").is_err());
    let oversize = kanzei_tools::image_search::ImageResult {
        title: "x".repeat(MAX_MANIFEST_BYTES as usize),
        ..other
    };
    assert!(cache_sticker(root.path(), &oversize, b"GIF89a-budget-overflow").is_err());
    assert_eq!(stickers(root.path()).unwrap().len(), 2);
}

#[test]
fn sandbox_files_and_gif_stickers_use_original_bytes_and_byte_free_records() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("stickers")).unwrap();
    let gif = b"GIF89a\0original-animation-frames";
    std::fs::write(root.path().join("stickers/happy.gif"), gif).unwrap();
    std::fs::write(root.path().join(MANIFEST), r#"{"version":1,"stickers":[{"id":"happy","file":"happy.gif","name":"开心","description":"开心庆祝"}]}"#).unwrap();
    std::fs::write(root.path().join("报告.pdf"), b"%PDF-original").unwrap();
    let input: ReplyInput = serde_json::from_value(json!({"text":"**完成**","attachments":[{"type":"sticker","id":"happy"},{"type":"file","path":"/workspace/报告.pdf"},{"type":"face","id":0},{"type":"market_face","emoji_package_id":123,"emoji_id":"actual-id","key":"actual-key"}]})).unwrap();
    let message = input.prepare(root.path()).unwrap();
    assert_eq!(message.wire[0]["data"]["text"], "完成");
    assert_eq!(message.wire[1]["data"]["sub_type"], 1);
    assert_eq!(
        STANDARD
            .decode(
                message.wire[1]["data"]["file"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("base64://")
                    .unwrap()
            )
            .unwrap(),
        gif
    );
    assert_eq!(message.wire[2]["data"]["name"], "报告.pdf");
    assert_eq!(message.wire[3]["data"]["id"], 0);
    assert_eq!(message.wire[4]["type"], "mface");
    assert!(!message.record.to_string().contains("base64://"));
    assert!(!message.record.to_string().contains("original-animation"));
    assert_eq!(
        message.record[1]["asset"]["hash"],
        kanzei_base::content_hash(gif)
    );
    assert_eq!(stickers(root.path()).unwrap()[0].description, "开心庆祝");
}

#[test]
fn image_only_reply_is_valid_and_invalid_paths_fail_before_send() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("image.png"), b"image-bytes").unwrap();
    let message = |path| {
        serde_json::from_value::<ReplyInput>(json!({"attachments":[{"type":"image","path":path}]}))
            .unwrap()
            .prepare(root.path())
    };
    assert_eq!(message("image.png").unwrap().wire[0]["type"], "image");
    for path in [
        "../secret",
        "/etc/passwd",
        "C:/secret",
        "folder\\file",
        "/workspace/../secret",
        "image.png:stream",
        "./image.png",
    ] {
        assert!(message(path).is_err(), "{path}");
    }
    assert!(message("missing.png").is_err());
    assert!(message("/workspace/").is_err());
    assert!(serde_json::from_value::<ReplyInput>(json!({}))
        .unwrap()
        .prepare(root.path())
        .is_err());
    assert!(PreparedMessage::text("---").is_none());
}

#[test]
fn attachment_budget_is_aggregate_and_bad_libraries_are_reported() {
    let root = tempfile::tempdir().unwrap();
    assert!(stickers(root.path()).unwrap().is_empty());
    std::fs::create_dir(root.path().join("stickers")).unwrap();
    std::fs::write(root.path().join(MANIFEST), "invalid JSON").unwrap();
    assert!(stickers(root.path()).is_err());
    let file = std::fs::File::create(root.path().join("large.bin")).unwrap();
    file.set_len(MAX_REPLY_BYTES / 2 + 1).unwrap();
    let input: ReplyInput = serde_json::from_value(json!({"attachments":[{"type":"file","path":"large.bin"},{"type":"file","path":"large.bin"}]})).unwrap();
    assert!(input.prepare(root.path()).is_err());
    assert!(serde_json::from_value::<ReplyInput>(
        json!({"attachments":[{"type":"image","url":"http://127.0.0.1/secret"}]})
    )
    .is_err());
}

#[test]
fn symbolic_links_cannot_export_files_outside_the_owned_workspace() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "OUTSIDE_CANARY").unwrap();
    let link = root.path().join("escape");
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(outside.path())
        .output()
        .unwrap()
        .status
        .success());
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    assert!(read_file(root.path(), "escape/secret", MAX_REPLY_BYTES).is_err());
}
