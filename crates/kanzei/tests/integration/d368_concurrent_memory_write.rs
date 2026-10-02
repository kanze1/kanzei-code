//! D-368 端到端回归:bash 命令窗口与 memory 写入并发时,合法写必须稳定落住并可归因。
//!
//! 根因(D-368):D-364 修复只覆盖固定路径,`.kanzei/memory/` 下的动态条目文件创建前
//! 无法逐个加锁,并发合法写曾被围栏误回滚。R-268 后命令窗口允许专用写者落盘并记
//! 写日志,围栏只在收口拍 after 快照时短暂取 **memory 树共享锁**,与 memory 写入口
//! 的排他树锁互斥;随后按日志吸收合法变化、回滚越界变化。
//!
//! 本文件用**真实 BashTool 管线**(capture → execute → close-out lock → enforce)与
//! **真实 MemoryStore::add** 并发,验证三条可判定形态:
//! ① 窗口内 add 等待后落住:条目文件 + INDEX.md 真在磁盘(验收①③);
//! ② 窗口超过写者锁预算(默认 3s)时,add 明确报错、绝不回 added(验收②);
//! ③ 两个写者并发 add,编号互异、条目齐全(验收① 后写者不得覆盖先写者)。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kanzei_harness::Tool;

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kanzei-{tag}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    dir
}

/// `.kanzei/memory/` 下的条目文件(排除 INDEX.md/inbox.md 两个派生物/草稿)。
fn entry_files(root: &Path) -> Vec<String> {
    let dir = root.join(".kanzei/memory");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n != "INDEX.md" && n != "inbox.md")
        .collect()
}

/// 真实 memory_add 的写者形态:构造 project store 并 add 一条记忆。
/// `force=true` 跳过 R-216 语义/去重闸(双 scope FTS 探测会命中本机 global 记忆,
/// 与本次验证的围栏互斥无关),只走真实写路径:tree_lock → 校验 → write_entry →
/// refresh_derived——围栏互斥点全部保留。
fn add_memory(root: &Path, title: &str, description: &str) -> kanzei_tools::memory::AddOutcome {
    kanzei_tools::memory::MemoryStore::project(root)
        .add(
            "fact",
            title,
            description,
            "正文:D-368 围栏并发写验证。",
            "test",
            &[],
            None,
            true,
        )
        .expect("memory_add 必须成功")
}

/// 验收①③:真围栏形态——真实 BashTool 命令窗口(1.5s)内,并发真实
/// MemoryStore::add(另一线程模拟另一进程的 memory_add)必须落住并留下可归因日志:
/// 条目文件与 INDEX.md 真在磁盘,bash 输出不得出现 [managed-files]。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn 真bash围栏窗口内并发memory_add等待后落盘不被误回滚() {
    let root = unique_dir("d368-fence");
    let ctx = kanzei_harness::ToolCtx {
        cwd: root.clone(),
        project_root: root.clone(),
        ..Default::default()
    };
    // 桌面自举轮形态:bash 命令(1.5s)与专用 memory 写者并发,收口再取共享锁对账。
    let bash = tokio::spawn(async move {
        kanzei_tools::bash::BashTool
            .execute(
                serde_json::json!({ "command": "Start-Sleep -Milliseconds 1500" }),
                &ctx,
            )
            .await
    });
    // 等 bash 进入命令窗口再并发 memory 写者。
    tokio::time::sleep(Duration::from_millis(400)).await;
    let writer_root = root.clone();
    let writer = std::thread::spawn(move || {
        add_memory(
            &writer_root,
            "D-368 围栏窗口内并发 memory_add",
            "窗口内并发写入不被围栏误回滚",
        )
    });

    let output = bash.await.unwrap();
    assert!(!output.is_error, "bash 自身不得报错: {}", output.content);
    assert!(
        !output.content.contains("[managed-files]"),
        "并发合法写入不得被围栏当成 bash 越界: {}",
        output.content
    );

    let entry = match writer.join().expect("写者线程不得 panic") {
        kanzei_tools::memory::AddOutcome::Added(e) => e,
        other => panic!("全新仓库应直接 Added: {other:?}"),
    };
    let file = root
        .join(".kanzei/memory")
        .join(format!("{}.md", entry.file_stem()));
    let text = std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("报 added 的条目必须真的在文件里({}): {e}", file.display()));
    assert!(
        text.contains("D-368 围栏窗口内并发 memory_add"),
        "落盘内容必须是本次写入: {text}"
    );
    assert!(
        root.join(".kanzei/memory/INDEX.md").is_file(),
        "派生物 INDEX.md 必须随写入重建"
    );

    std::fs::remove_dir_all(&root).ok();
}

/// 验收②:窗口超过写者锁预算(默认 3s)时,memory_add 必须明确报错、绝不回 added——
/// 宁可失败也不能假成功(与 D-364 的 CLI 预算用例同一哲学)。
#[test]
fn 窗口超过写者锁预算时memory_add明确报错() {
    let root = unique_dir("d368-refuse");
    // 模拟围栏:命令执行期间持有 memory 树锁。
    let fence = kanzei_tools::atomic_file::lock_exclusive(&root.join(".kanzei/memory"))
        .expect("主进程应拿到树锁");
    let writer_root = root.clone();
    let writer = std::thread::spawn(move || {
        // 返回 Result:预算失败必须可断言,不在这里 unwrap 成假成功。
        kanzei_tools::memory::MemoryStore::project(&writer_root).add(
            "fact",
            "D-368 预算外写入",
            "窗口超过预算必须报错",
            "正文:D-368 验收②。",
            "test",
            &[],
            None,
            true,
        )
    });
    // 持锁 3.2s > 写者 3s 预算:add 拿不到树锁,必须明确失败。
    std::thread::sleep(Duration::from_millis(3200));
    drop(fence);

    let error = writer
        .join()
        .expect("写者线程不得 panic")
        .expect_err("超过锁预算 memory_add 必须失败");
    assert!(
        error.to_string().contains("写锁") || error.to_string().contains("lock"),
        "错误必须点名锁: {error}"
    );
    assert!(
        entry_files(&root).is_empty(),
        "失败路径不得留下半截条目: {:?}",
        entry_files(&root)
    );

    std::fs::remove_dir_all(&root).ok();
}

/// 验收①:两个写者真并发 add(同一进程内两条线程各自落盘),编号必须互异、两条都在
/// 磁盘——后写者不得覆盖先写者(D-364 同款保证扩展到 memory 条目)。
#[test]
fn 两个并发memory_add编号互异条目齐全() {
    let root = unique_dir("d368-concurrent");
    let ra = root.clone();
    let rb = root.clone();
    let a = std::thread::spawn(move || add_memory(&ra, "并发甲", "并发写者甲"));
    let b = std::thread::spawn(move || add_memory(&rb, "并发乙", "并发写者乙"));
    let id_of = |outcome: kanzei_tools::memory::AddOutcome| match outcome {
        kanzei_tools::memory::AddOutcome::Added(e) => e.id,
        other => panic!("应 Added: {other:?}"),
    };
    let (ida, idb) = (id_of(a.join().unwrap()), id_of(b.join().unwrap()));
    assert_ne!(ida, idb, "两个并发 add 不得拿到同一编号");

    let files = entry_files(&root);
    assert_eq!(files.len(), 2, "两条都必须落盘: {files:?}");
    let mut bodies = String::new();
    for file in &files {
        bodies.push_str(
            &std::fs::read_to_string(root.join(".kanzei/memory").join(file)).unwrap_or_default(),
        );
    }
    assert!(
        bodies.contains("并发甲") && bodies.contains("并发乙"),
        "两条内容都应在: {bodies}"
    );

    std::fs::remove_dir_all(&root).ok();
}
