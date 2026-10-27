#[test]
fn inbox_batch_respects_note_and_budget_limits() {
    let (dir, store) = temp_store();
    store.append_note("批次 A", "短内容", "fact", &[]).unwrap();
    store.append_note("批次 B", "短内容", "fact", &[]).unwrap();
    store.append_note("批次 C", "短内容", "fact", &[]).unwrap();

    let batch = store.read_inbox_batch(2, usize::MAX, usize::MAX).unwrap();
    assert_eq!(batch.note_count, 2);
    assert!(batch.text.contains("批次 A"));
    assert!(batch.text.contains("批次 B"));
    assert!(!batch.text.contains("批次 C"));
    assert_eq!(batch.bytes, batch.text.len());
    assert!(batch.estimated_tokens > 0);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn inbox_batch_allows_one_oversized_note_and_checkpoint_roundtrips() {
    let (dir, store) = temp_store();
    store
        .append_note("超大首条", &"x".repeat(256), "fact", &[])
        .unwrap();
    store.append_note("后续条目", "y", "fact", &[]).unwrap();

    let batch = store.read_inbox_batch(10, 8, 2).unwrap();
    assert_eq!(batch.note_count, 1, "首条超限也必须被处理，不能饿死队列");
    assert!(batch.text.contains("超大首条"));

    let checkpoint = crate::memory::InboxCheckpoint {
        batch_id: "inbox-test-1".into(),
        status: "completed".into(),
        input_notes: 1,
        input_bytes: batch.bytes,
        success_notes: 1,
        pending_after: 1,
        failure_reason: None,
        consecutive_failures: 0,
        updated_at_ms: 42,
        job: None,
    };
    store.write_inbox_checkpoint(&checkpoint).unwrap();
    assert_eq!(store.read_inbox_checkpoint(), Some(checkpoint));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn inbox_batch_byte_budget_stops_before_next_note() {
    let (dir, store) = temp_store();
    store.append_note("第一条", "a", "fact", &[]).unwrap();
    store.append_note("第二条", "b", "fact", &[]).unwrap();
    let first = store.read_inbox_batch(10, 1, usize::MAX).unwrap();
    assert_eq!(first.note_count, 1);
    assert!(first.text.contains("第一条"));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn inbox_roundtrip_and_clear() {
    let (dir, store) = temp_store();
    store
        .append_note("纯 ui 改动只跑 node 检查", "细节", "habit", &[])
        .unwrap();
    store.append_note("发版走两条通道", "", "sop", &[]).unwrap();
    assert_eq!(store.pending_notes(), 2);
    assert!(store.read_inbox().contains("纯 ui 改动只跑 node 检查"));
    store.clear_inbox().unwrap();
    assert_eq!(store.pending_notes(), 0);
    std::fs::remove_dir_all(dir).ok();
}

/// R-215 验收①:20 条积压逐条 discard 能在数轮内收敛到 0(逐条销账而非整箱清)。
#[test]
fn 二十条积压逐条销账收敛到零() {
    let (dir, store) = temp_store();
    for i in 0..20 {
        store
            .append_note(&format!("积压 note 第 {i} 条"), "", "fact", &[])
            .unwrap();
    }
    assert_eq!(store.pending_notes(), 20);
    // 逐条按指纹销账:每条处理后删该条,不碰其余。
    for i in 0..20 {
        let removed = store.discard_note(&format!("积压 note 第 {i} 条")).unwrap();
        assert!(removed, "第 {i} 条应被销账");
        assert_eq!(store.pending_notes(), 19 - i, "销账后应逐条减少");
    }
    assert_eq!(store.pending_notes(), 0, "20 条应收敛到 0");
    std::fs::remove_dir_all(dir).ok();
}

/// R-215 验收③:消化清空窗口封死——discard 只删已处理指纹,并发 append 的新
/// note 存活;整箱 clear 前先 discard 已见条,新 note 不被吃。
#[test]
fn 逐条销账不吃并发新note_窗口封死() {
    let (dir, store) = temp_store();
    store.append_note("已处理 note A", "", "fact", &[]).unwrap();
    // 模拟处理完 A 后、清空前,并发 append 了 B。
    store.append_note("并发新 note B", "", "fact", &[]).unwrap();
    // 只销账已处理的 A,B 必须存活。
    let removed = store.discard_note("已处理 note A").unwrap();
    assert!(removed);
    assert!(
        store.read_inbox().contains("并发新 note B"),
        "discard A 不得吃掉 B(窗口封死)"
    );
    assert_eq!(store.pending_notes(), 1, "B 应留在箱中待下轮处理");
    std::fs::remove_dir_all(dir).ok();
}

/// R-215 验收②:并发 append 压测零丢 note——多线程同时 append_note,全部落盘。
///
/// D-631:本测试的不变量是**零丢失**,不是尾部写者的等待延迟。共享 CI runner
/// 高 I/O 时 12 写者串行化可能让尾部等待超过统一 3s 锁预算(D-603/D-604 修后
/// 仍复现一次,kanzei-memory 套件整体 259s 的冷环境),那是环境延迟而非丢失;
/// 锁边界不变量(审计不占树锁)由「慢生命周期审计不占用memory树锁」确定性钉住。
/// 因此仅对锁预算超时做有界重试,其它任何错误照旧立即失败。
#[test]
fn 并发append零丢note() {
    let (dir, store) = temp_store();
    let store = std::sync::Arc::new(store);
    let mut handles = Vec::new();
    for i in 0..12 {
        let store = store.clone();
        handles.push(std::thread::spawn(move || {
            let mut budget_retries = 0;
            loop {
                match store.append_note(&format!("并发 note {i}"), "", "fact", &[]) {
                    Ok(_) => break,
                    Err(error) if budget_retries < 10 && error.to_string().contains("仍拿不到") =>
                    {
                        budget_retries += 1;
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                    Err(error) => panic!("并发 append 失败: {error}"),
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(store.pending_notes(), 12, "12 条并发 append 一条都不能丢");
    for i in 0..12 {
        assert!(
            store.read_inbox().contains(&format!("并发 note {i}")),
            "note {i} 应落盘"
        );
    }
    std::fs::remove_dir_all(dir).ok();
}
