use super::*;
// D-366:检索排序决策在 index,测试里需要"决策排序后 top-k"语义时经检索门面。
use crate::memory::index::{IndexQuery, SqliteMemoryIndex};

fn temp_store() -> (PathBuf, MemoryStore) {
    let dir = std::env::temp_dir().join(format!(
        "kz-memory-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let store = MemoryStore::project(&dir);
    (dir, store)
}

fn add(store: &MemoryStore, category: &str, title: &str, desc: &str, body: &str) -> MemoryEntry {
    // force=true:测试 fixture 刻意构造批量条目,语义闸(R-216)会误拦「改写复述」,
    // 而 fixture 的目的正是造数据验证其它路径;真实 memory_add 不走这里。
    match store
        .add(category, title, desc, body, "user", &[], None, true)
        .unwrap()
    {
        AddOutcome::Added(e) => e,
        AddOutcome::Duplicate(e) => panic!("unexpected duplicate of {}", e.id),
        AddOutcome::SubjectConflict(e) => panic!("unexpected subject conflict with {}", e.id),
        AddOutcome::Uncertain(cands) => panic!("unexpected uncertain: {:?}", cands),
    }
}

fn record_current_recall(store: &MemoryStore, hits: &[SearchHit], injected_ids: &[&str]) {
    let root = store.project_root.as_deref().unwrap();
    let retrieved: Vec<&str> = hits.iter().map(|hit| hit.entry.id.as_str()).collect();
    let injected: Vec<&str> = retrieved
        .iter()
        .copied()
        .filter(|id| injected_ids.contains(id))
        .collect();
    let retrieved_json = serde_json::to_string(&retrieved).unwrap();
    let injected_json = serde_json::to_string(&injected).unwrap();
    let recall_id = format!("test-recall-{}", now_ms());
    let db = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root)).unwrap();
    db.record_recall_event(&kanzei_core::RecallEvent {
        recall_id: &recall_id,
        episode_id: None,
        step_id: None,
        trigger_type: "memory_search",
        trigger_payload: "{}",
        policy_action: "lexical",
        query: "要发版了",
        candidate_ids: &retrieved_json,
        retrieved_ids: &retrieved_json,
        injected_ids: &injected_json,
        lexical_ms: 1,
        embed_ms: 0,
        vector_ms: 0,
        total_ms: 1,
    })
    .unwrap();
}

// 保留原 memory::store::tests::<name> 路径，兼容现有测试过滤器。
include!("admission.rs");
include!("derived_index.rs");
include!("inbox.rs");
include!("integrity.rs");
include!("lifecycle.rs");
include!("merge_reconcile.rs");
include!("migration_refs.rs");
include!("search_recall.rs");
