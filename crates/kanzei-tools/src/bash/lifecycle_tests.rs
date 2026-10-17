use super::*;
use kanzei_harness::progress::{scope, ProgressHandle};
use serde_json::json;

fn launch_child(root: &Path) -> String {
    let program = detected_shell()
        .program
        .display()
        .to_string()
        .replace('\'', "''");
    let child = root.join("child.ps1").display().to_string();
    format!("$si=[Diagnostics.ProcessStartInfo]::new(); $si.FileName='{program}'; $si.Arguments='-NoProfile -NonInteractive -File \"{child}\"'; $si.UseShellExecute=$false; $si.CreateNoWindow=$true; $p=[Diagnostics.Process]::Start($si); [IO.File]::WriteAllText('child.pid',[string]$p.Id)")
}

async fn assert_cancel_cleanup(drop_caller: bool) {
    let _guard = crate::background::tests::fence_guard();
    let root = super::tests::temp_project("cancel-owner");
    let protected = root.join(".kanzei/project/requirements.md");
    std::fs::write(&protected, "# Requirements\n").unwrap();
    std::fs::write(root.join("child.ps1"), "while (-not (Test-Path release.txt)) { Start-Sleep -Milliseconds 20 }; [IO.File]::WriteAllText('late.txt','late write')").unwrap();
    std::fs::write(root.join("parent.ps1"), format!("{}; [IO.File]::WriteAllText('.kanzei/project/requirements.md','BREACH'); Write-Output OWNER_READY; Start-Sleep -Seconds 300", launch_child(&root))).unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let cancel = CancellationToken::new();
    let stopping = cancel.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let running = tokio::spawn(async move {
        scope(
            ProgressHandle::new("cancel-owner".into(), tx),
            BashTool::default().execute_cancellable(
                json!({"command":"& './parent.ps1'", "timeout_ms":300_000}),
                &ctx,
                Some(&stopping),
            ),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(20), async {
        while let Some((_, chunk)) = rx.recv().await {
            if chunk.contains("OWNER_READY") {
                return;
            }
        }
        panic!("command exited before the cancellation fixture became ready");
    })
    .await
    .unwrap();
    let child: u32 = std::fs::read_to_string(root.join("child.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(std::fs::read_to_string(&protected).unwrap(), "BREACH");
    if drop_caller {
        running.abort();
        assert!(running.await.unwrap_err().is_cancelled());
    } else {
        cancel.cancel();
        let output = running.await.unwrap();
        assert!(
            output.is_error && output.content.contains("cancelled"),
            "{}",
            output.content
        );
    }
    // The owned task is the final sender, so closure proves cleanup completed.
    tokio::time::timeout(Duration::from_secs(20), async {
        while rx.recv().await.is_some() {}
    })
    .await
    .unwrap();
    assert!(
        !crate::shell::process_alive(child),
        "owned child survived cancellation"
    );
    assert_eq!(
        std::fs::read_to_string(&protected).unwrap(),
        "# Requirements\n"
    );
    assert!(!root.join("late.txt").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cooperative_cancel_waits_for_tree_and_file_fence_cleanup() {
    assert_cancel_cleanup(false).await;
}

#[tokio::test]
async fn dropping_the_caller_still_cleans_its_process_tree_and_file_fence() {
    assert_cancel_cleanup(true).await;
}

#[tokio::test]
async fn root_exit_drains_output_without_waiting_for_descendant_pipe_eof() {
    let root = super::tests::temp_project("root-exit");
    std::fs::write(root.join("child.ps1"), "Start-Sleep -Seconds 30").unwrap();
    std::fs::write(root.join("parent.ps1"), format!("{}; [Console]::Out.WriteLine(('x' * 131072)); [Console]::Out.WriteLine('ROOT_TAIL'); [Console]::Error.WriteLine('ROOT_ERR_TAIL'); exit 0", launch_child(&root))).unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let output = BashTool::default()
        .execute(
            json!({"command":"& './parent.ps1'", "timeout_ms":10_000}),
            &ctx,
        )
        .await;
    let child: u32 = std::fs::read_to_string(root.join("child.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        !crate::shell::process_alive(child),
        "foreground completion must join its owned process tree and pipe readers"
    );
    assert!(!output.is_error, "{}", output.content);
    assert!(output.content.contains("exit code: 0"));
    assert!(output.content.contains("ROOT_TAIL"));
    assert!(output.content.contains("ROOT_ERR_TAIL"));
    assert!(output.content.contains(&"x".repeat(131072)));
    std::fs::remove_dir_all(root).unwrap();
}
