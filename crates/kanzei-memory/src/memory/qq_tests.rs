use super::*;
use kanzei_harness::{QqConversation, QqPeerKind};
#[test]
fn personal_channels_share_memory_while_each_group_retains_its_own_owner() {
    test_home::run(
        "memory::qq_tests::personal_channels_share_memory_while_each_group_retains_its_own_owner",
        |home| {
            let general = home.join("conversations/general");
            let personal = [
                general.clone(),
                QqConversation::new("1", QqPeerKind::Friend, "2")
                    .unwrap()
                    .root_at(home),
                QqConversation::new("9", QqPeerKind::Friend, "3")
                    .unwrap()
                    .root_at(home),
                kanzei_harness::bilibili_conversation_root("10").unwrap(),
            ];
            let global = MemoryStore::global().unwrap();
            global
                .add(
                    "fact",
                    "GLOBAL_PRIVATE",
                    "private scope",
                    "PRIVATE_SENTINEL",
                    "memory-manager",
                    &[],
                    None,
                    true,
                )
                .unwrap();
            for root in &personal {
                std::fs::create_dir_all(root).unwrap();
            }
            let first = MemoryStore::project(&personal[1]);
            let entry = match first
                .add(
                    "fact",
                    "PERSONAL_SENTINEL",
                    "PERSONAL_SENTINEL recall",
                    "PERSONAL_SENTINEL body",
                    "user",
                    &[],
                    None,
                    true,
                )
                .unwrap()
            {
                AddOutcome::Added(e) => e,
                other => panic!("{other:?}"),
            };
            assert!(entry.id.starts_with("P-"));
            for root in &personal {
                let store = MemoryStore::project(root);
                assert_eq!(store.scope, MemoryScope::Personal);
                assert_eq!(store.root, first.root);
                assert!(MemoryStore::global_for_project(root).is_some());
                assert_eq!(
                    SqliteMemoryIndex::new(root)
                        .search_entries(
                            &IndexQuery::text("PERSONAL_SENTINEL"),
                            None,
                            Some("active"),
                            20
                        )
                        .unwrap()
                        .len(),
                    1
                );
                assert!(validate_source_refs(
                    &ToolCtx::new(root.clone(), root.clone()),
                    std::slice::from_ref(&entry.id)
                )
                .is_ok());
            }
            for (account, peer) in [("1", "2"), ("1", "3"), ("9", "2")] {
                let root = QqConversation::new(account, QqPeerKind::Group, peer)
                    .unwrap()
                    .root_at(home);
                std::fs::create_dir_all(&root).unwrap();
                let store = MemoryStore::project(&root);
                assert_eq!(store.scope, MemoryScope::QqGroup);
                assert_ne!(store.root, first.root);
                assert!(store.load_all_checked().unwrap().is_empty());
                assert!(MemoryStore::global_for_project(&root).is_none());
                assert!(validate_source_refs(
                    &ToolCtx::new(root.clone(), root.clone()),
                    std::slice::from_ref(&entry.id)
                )
                .is_err());
                store
                    .add(
                        "fact",
                        "GROUP_SENTINEL",
                        "GROUP_SENTINEL recall",
                        "GROUP_SENTINEL body",
                        "user",
                        &[],
                        None,
                        true,
                    )
                    .unwrap();
                assert_eq!(store.load_all_checked().unwrap().len(), 1);
                assert_eq!(
                    reconcile_candidates(&root, None, 30)
                        .unwrap()
                        .global_reviewed,
                    0
                );
            }
            assert_eq!(first.load_all_checked().unwrap().len(), 1);
        },
    );
}
#[test]
fn legacy_friend_entries_migrate_once_with_cross_references_and_original_source_retained() {
    test_home::run("memory::qq_tests::legacy_friend_entries_migrate_once_with_cross_references_and_original_source_retained", |home| {
        let root=QqConversation::new("1",QqPeerKind::Friend,"2").unwrap().root_at(home);std::fs::create_dir_all(&root).unwrap();
        let legacy=MemoryStore::open(MemoryScope::QqFriend,root.join(".kanzei/memory"));
        let first=match legacy.add("fact","OLD_FRIEND","OLD_FRIEND recall","OLD_FRIEND fact","user",&[],None,true).unwrap(){AddOutcome::Added(e)=>e,other=>panic!("{other:?}")};
        legacy.add("fact","OLD_REFERENCE","OLD_REFERENCE recall","OLD_REFERENCE fact","user",std::slice::from_ref(&first.id),None,true).unwrap();
        assert_eq!(legacy.load_all_checked().unwrap().len(),2);
        legacy.append_note("待整理内容","旧的线索","fact",std::slice::from_ref(&first.id)).unwrap();
        for _ in 0..2 { let shared=MemoryStore::project(&root);let all=shared.load_all_checked().unwrap();assert_eq!(all.len(),2);assert_eq!(shared.pending_notes(),1);assert!(all.iter().all(|(_,e)|e.scope=="personal" && e.status=="active"));let mapped=all.iter().find(|(_,e)|e.title=="OLD_FRIEND").unwrap();let reference=all.iter().find(|(_,e)|e.title=="OLD_REFERENCE").unwrap();assert!(reference.1.extras.iter().any(|(k,v)|k=="refs" && v==&mapped.1.id)); }
        let shared=MemoryStore::project(&root); shared.clear_inbox().unwrap(); assert_eq!(MemoryStore::project(&root).pending_notes(),0);
        assert_eq!(legacy.load_all_checked().unwrap().len(),2);
    });
}
