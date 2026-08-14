//! D-364 端到端回归:bash 围栏持锁窗口内,外部 `kz req add` 必须稳定落住。
//!
//! 根因(D-364):bash 工具的 D-173 结果侧围栏(managed.rs::enforce_managed_files)在命令
//! 执行前后拍托管树快照,命令窗口内**任何**变化(含另一进程经 tracker 合法写入
//! requirements.md)都被当成 bash 越界整体回滚——桌面自举轮跑 bash 时,外部 add 报
//! added 但条目被抹掉、同 id 二次分配。修复:围栏命令执行期间经专用线程持有全部
//! 已知托管文档的写锁,把并发写者挡在窗口之外;命令结束(快照比对后)才释放。
//!
//! 本文件用真 OS 进程(免 LLM 的 `kz req add` 直通路径)验证三条可判定形态:
//! ① 窗口内 add 等待后落住、编号唯一、条目真的在文件里(验收①③④);
//! ② 窗口超过写者锁预算时,add 明确报错、退出码非 0、绝不回 added(验收②);
//! ③ 两个 CLI 进程真并发 add,编号互异、条目齐全(验收① 后写者不得覆盖先写者)。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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

fn requirements_path(root: &Path) -> PathBuf {
    root.join(".kanzei").join("project").join("requirements.md")
}

/// 在 `cwd` 里跑 `kz req add <title>`,`KANZEI_PROJECT_ROOT` 显式指向主根。
/// 登记必填字段走开关传(R-191 B3 硬门禁:缺复杂度/优先级/标签即拒)。
fn spawn_req_add(cwd: &Path, main_root: &Path, title: &str) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_kz"))
        .args([
            "req",
            "add",
            title,
            "--priority",
            "P3",
            "--complexity",
            "小",
            "--tag",
            "流程",
        ])
        .current_dir(cwd)
        .env("KANZEI_PROJECT_ROOT", main_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("kz 起不来")
}

fn entry_ids(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// 验收①③④:主进程模拟 bash 围栏在命令执行期间持有 requirements.md 写锁(修复后的
/// 围栏行为),并发 `kz req add` 必须在窗口内等锁、窗口结束落住,条目真在文件里。
#[test]
fn 围栏持锁窗口内cli登记等待后落住编号唯一() {
    let root = unique_dir("d364-window");
    std::fs::write(requirements_path(&root), "# Requirements\n").unwrap();

    let req = requirements_path(&root);
    // 模拟围栏:命令执行期间持有托管文档写锁(D-364 修复后 bash 的真实行为)。
    let lock = kanzei_tools::atomic_file::lock_exclusive(&req).expect("主进程应拿到锁");
    let child = spawn_req_add(&root, &root, "围栏窗口内的登记");
    // 命令窗口 1.5s(< CLI 锁预算 3s):窗口内 CLI 应等待而非失败。
    std::thread::sleep(Duration::from_millis(1500));
    drop(lock);

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "窗口内 add 必须等待后成功\nstdout={stdout}\nstderr={stderr}"
    );
    let id = stdout
        .split_whitespace()
        .find(|word| word.starts_with("R-"))
        .unwrap_or_else(|| panic!("stdout 里没有编号: {stdout}"))
        .to_string();

    let ids = entry_ids(&requirements_path(&root));
    assert!(
        ids.contains(&id),
        "报 added 的条目必须真的在文件里: {ids:?}"
    );
    assert_eq!(
        ids.len(),
        1,
        "既无条目 + 一次 add = 恰好一条,不得多不得少: {ids:?}"
    );

    std::fs::remove_dir_all(&root).ok();
}

/// 验收②:窗口超过写者锁预算(CLI 3s)时,add 必须明确报错、退出码非 0、绝不回
/// added——宁可失败也不能假成功。
#[test]
fn 窗口超过锁预算时cli明确报错绝不回added() {
    let root = unique_dir("d364-refuse");
    std::fs::write(requirements_path(&root), "# Requirements\n").unwrap();

    let req = requirements_path(&root);
    let lock = kanzei_tools::atomic_file::lock_exclusive(&req).expect("主进程应拿到锁");
    let child = spawn_req_add(&root, &root, "预算外的登记");
    // 持锁 3.8s > CLI 3s 预算:CLI 拿不到锁,必须明确失败。
    std::thread::sleep(Duration::from_millis(3800));
    drop(lock);

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        !output.status.success(),
        "超过锁预算 CLI 必须失败\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(!stdout.contains("added"), "禁止假成功: stdout={stdout}");
    assert!(
        stderr.contains("写锁") || stderr.contains("cannot lock"),
        "错误必须点名锁: stderr={stderr}"
    );
    assert!(
        entry_ids(&requirements_path(&root)).is_empty(),
        "失败路径不得留下半截条目"
    );

    std::fs::remove_dir_all(&root).ok();
}

/// 验收④ 的真围栏形态:走**真实** `BashTool` 管线(含 acquire_managed_locks →
/// capture → 执行 → enforce 的完整围栏),命令窗口内并发 `kz req add` 必须稳定落住、
/// 且围栏不得把它误判成 bash 越界。修掉「bash 围栏在命令窗口内不持锁」的回归(有人
/// 删掉 acquire 调用)时,本测试会红:CLI 的条目会被围栏当越界回滚、bash 报
/// [managed-files]。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn 真bash围栏窗口内并发cli登记不被误回滚() {
    let root = unique_dir("d364-real-fence");
    std::fs::write(requirements_path(&root), "# Requirements\n").unwrap();
    let ctx = kanzei_harness::ToolCtx {
        cwd: root.clone(),
        project_root: root.clone(),
        ..Default::default()
    };
    // 桌面自举轮形态:bash 命令(1.5s)在跑,围栏持有托管文档写锁。
    let bash = tokio::spawn(async move {
        kanzei_tools::bash::BashTool
            .execute(
                serde_json::json!({ "command": "Start-Sleep -Milliseconds 1500" }),
                &ctx,
            )
            .await
    });
    // 等 bash 进入命令窗口(锁已持有)再并发 CLI 登记。
    tokio::time::sleep(Duration::from_millis(400)).await;
    let child = spawn_req_add(&root, &root, "真围栏窗口内的登记");

    let output = bash.await.unwrap();
    assert!(!output.is_error, "bash 自身不得报错: {}", output.content);
    assert!(
        !output.content.contains("[managed-files]"),
        "并发合法写入不得被围栏当成 bash 越界: {}",
        output.content
    );

    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.success(),
        "窗口内 add 必须落住\nstdout={stdout}\nstderr={stderr}"
    );
    let id = stdout
        .split_whitespace()
        .find(|word| word.starts_with("R-"))
        .unwrap_or_else(|| panic!("stdout 里没有编号: {stdout}"))
        .to_string();
    let ids = entry_ids(&requirements_path(&root));
    assert!(ids.contains(&id), "条目必须真的在文件里: {ids:?}");
    assert_eq!(ids.len(), 1, "恰好一条: {ids:?}");

    std::fs::remove_dir_all(&root).ok();
}

/// 验收①:两个 CLI 进程真并发 add(同时 spawn、各自落盘),编号必须互异、两条都在
/// 文件里——后写者不得覆盖先写者。
#[test]
fn 两个cli进程真并发add编号互异条目齐全() {
    let root = unique_dir("d364-concurrent");
    std::fs::write(requirements_path(&root), "# Requirements\n").unwrap();

    let a = spawn_req_add(&root, &root, "并发甲");
    let b = spawn_req_add(&root, &root, "并发乙");
    let oa = a.wait_with_output().unwrap();
    let ob = b.wait_with_output().unwrap();
    let id_of = |output: &std::process::Output| {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        assert!(
            output.status.success(),
            "并发 add 必须成功\nstdout={stdout}\nstderr={stderr}"
        );
        stdout
            .split_whitespace()
            .find(|word| word.starts_with("R-"))
            .unwrap_or_else(|| panic!("stdout 里没有编号: {stdout}"))
            .to_string()
    };
    let (ida, idb) = (id_of(&oa), id_of(&ob));
    assert_ne!(ida, idb, "两个并发 add 不得拿到同一编号");

    let ids = entry_ids(&requirements_path(&root));
    assert_eq!(ids.len(), 2, "两条都必须落盘: {ids:?}");
    assert!(ids.contains(&ida), "第一条在文件里: {ids:?}");
    assert!(ids.contains(&idb), "第二条在文件里: {ids:?}");

    std::fs::remove_dir_all(&root).ok();
}
