use kanzei_harness::{ReadLedger, Tool, ToolCtx};
use serde_json::json;

fn fixture() -> (std::path::PathBuf, ToolCtx) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kz-read-receipt-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone()).with_read_ledger(ReadLedger::default());
    (root, ctx)
}

#[tokio::test]
async fn unread_files_are_unchanged_partial_read_allows_sequential_edits_and_external_changes_are_rejected(
) {
    let (root, ctx) = fixture();
    let path = root.join("file.txt");
    std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
    let write = crate::write::WriteTool;
    let edit = crate::edit::EditTool::default();
    let insert = crate::edit::InsertTool;
    let read = crate::read::ReadTool;
    let result = write
        .execute(json!({"path":"file.txt","content":"lost"}), &ctx)
        .await;
    assert_eq!(result.code, Some("READ_BEFORE_WRITE"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\nthree\n");
    assert!(
        !read
            .execute(json!({"path":"file.txt","limit":1}), &ctx)
            .await
            .is_error
    );
    assert!(
        !edit
            .execute(
                json!({"path":"file.txt","old_string":"one","new_string":"ONE"}),
                &ctx
            )
            .await
            .is_error
    );
    assert!(
        !insert
            .execute(
                json!({"path":"file.txt","anchor":"two","content":"added\n","position":"before"}),
                &ctx
            )
            .await
            .is_error
    );
    std::fs::write(&path, "external\nchange\n").unwrap();
    for input in [
        json!({"path":"file.txt","old_string":"external","new_string":"overwrite"}),
        json!({"path":"file.txt","old_string":"missing","new_string":"overwrite"}),
    ] {
        let result = edit.execute(input, &ctx).await;
        assert_eq!(result.code, Some("FILE_CHANGED_SINCE_READ"));
        assert!(result.content.contains("external"));
    }
    assert_eq!(
        write
            .execute(json!({"path":"file.txt","content":"lost"}), &ctx)
            .await
            .code,
        Some("FILE_CHANGED_SINCE_READ")
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "external\nchange\n"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn new_files_are_allowed_and_receipts_do_not_cross_conversations() {
    let (root, ctx) = fixture();
    let write = crate::write::WriteTool;
    assert!(
        !write
            .execute(json!({"path":"new.txt","content":"first"}), &ctx)
            .await
            .is_error
    );
    assert!(
        !write
            .execute(json!({"path":"new.txt","content":"second"}), &ctx)
            .await
            .is_error
    );
    let child = ToolCtx::new(root.clone(), root.clone()).with_read_ledger(ReadLedger::default());
    assert!(
        !crate::read::ReadTool
            .execute(json!({"path":"new.txt"}), &child)
            .await
            .is_error
    );
    ctx.read_ledger.as_ref().unwrap().clear();
    assert_eq!(
        write
            .execute(json!({"path":"new.txt","content":"third"}), &ctx)
            .await
            .code,
        Some("READ_BEFORE_WRITE")
    );
    let standalone = ToolCtx::new(root.clone(), root.clone());
    assert!(
        !write
            .execute(json!({"path":"new.txt","content":"legacy"}), &standalone)
            .await
            .is_error
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn interactive_command_accepts_input_and_returns_the_actual_output() {
    let (root, ctx) = fixture();
    let ctx = ctx.with_identity(
        "tree".into(),
        "project".into(),
        "run".into(),
        "actor".into(),
    );
    let output = crate::bash::BashTool.execute(json!({
        "command":"Write-Output READY; $line = [Console]::In.ReadLine(); Write-Output \"REPLY:$line\"",
        "interactive":true
    }), &ctx).await;
    assert!(!output.is_error, "{}", output.content);
    let id = output.display.as_ref().unwrap()["processId"]
        .as_str()
        .unwrap();
    let process = crate::process::ProcessTool;
    let sent = process
        .execute(
            json!({"action":"input","id":id,"text":"hello\n","close":true}),
            &ctx,
        )
        .await;
    assert!(!sent.is_error, "{}", sent.content);
    let result = process
        .execute(json!({"action":"wait","id":id,"timeout_secs":10}), &ctx)
        .await;
    assert!(result.content.contains("REPLY:hello"), "{}", result.content);
    assert!(!crate::background::get(id).unwrap().is_running());
    std::fs::remove_dir_all(root).unwrap();
}
