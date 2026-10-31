use super::*;
use kanzei_harness::{QqConversation, QqPeerKind};

#[test]
fn qq_memory_namespaces_isolate_accounts_friends_groups_and_global_lifecycle() {
    test_home::run("memory::qq_tests::qq_memory_namespaces_isolate_accounts_friends_groups_and_global_lifecycle", |home| {
        let identities = [
            QqConversation::new("1",QqPeerKind::Friend,"2").unwrap(),
            QqConversation::new("1",QqPeerKind::Friend,"3").unwrap(),
            QqConversation::new("1",QqPeerKind::Group,"2").unwrap(),
            QqConversation::new("9",QqPeerKind::Group,"2").unwrap(),
        ];
        let global = MemoryStore::global().unwrap();
        global.add("fact","GLOBAL_PRIVATE","private scope","PRIVATE_SENTINEL","memory-manager",&[],None,true).unwrap();
        let mut stores = Vec::new();
        for (index, identity) in identities.iter().enumerate() {
            let root = identity.root_at(home);
            std::fs::create_dir_all(&root).unwrap();
            let store = MemoryStore::project(&root);
            assert_eq!(store.scope, if identity.kind == QqPeerKind::Friend {MemoryScope::QqFriend} else {MemoryScope::QqGroup});
            let entry = match store.add("fact",&format!("QQ_SENTINEL_{index}"),"QQ_SENTINEL recall","QQ_SENTINEL scoped body","user",&[],None,true).unwrap() { AddOutcome::Added(entry)=>entry, other=>panic!("{other:?}") };
            assert!(entry.id.starts_with(store.scope.prefix()));
            let ctx=ToolCtx::new(root.clone(),root.clone());
            assert!(validate_source_refs(&ctx,std::slice::from_ref(&entry.id)).is_ok());
            assert!(validate_source_refs(&ctx,&[format!("{}-999999",store.scope.prefix())]).is_err());
            assert!(MemoryStore::global_for_project(&root).is_none());
            store.write_source_artifact("source.json","{\"source\":\"QQ_SENTINEL\"}").unwrap();
            let hits=SqliteMemoryIndex::new(&root).search_entries(&IndexQuery::text("QQ_SENTINEL"),None,Some("active"),20).unwrap();
            assert_eq!(hits.len(),1);
            assert_eq!(hits[0].entry.title,entry.title);
            let revision=kanzei_base::content_hash(render_entry(&entry).as_bytes());
            store.update(&entry.id,None,None,Some("corrected scoped body"),None,Some(&revision),false).unwrap();
            assert!(store.update(&entry.id,None,None,Some("stale change"),None,Some(&revision),false).is_err());
            let report = reconcile_candidates(&root,None,30).unwrap();
            assert_eq!(report.global_reviewed,0);
            stores.push(store);
        }
        for (index,store) in stores.iter().enumerate() {
            let entries=store.load_all_checked().unwrap();
            assert_eq!(entries.len(),1);
            assert_eq!(entries[0].1.title,format!("QQ_SENTINEL_{index}"));
        }
        assert_eq!(global.load_all_checked().unwrap().len(),1);
        assert!(global.recalls(100).is_empty(),"QQ maintenance must not inspect global candidates");
    });
}
