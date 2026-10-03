use super::run::recover_cli_prior;
use serde_json::json;

pub(crate) async fn compact_cli(args: &[String]) -> anyhow::Result<()> {
    let focus = match args {
        [] => None,
        [flag, value] if flag == "--focus" => Some(value.clone()),
        _ => anyhow::bail!("usage: kz compact [--focus 文本]"),
    };
    let cwd = std::env::current_dir()?;
    let root = super::main_project_root(super::explicit_main_root(None).as_deref(), &cwd)?;
    let session = kanzei_core::project_session_id(&root);
    let state_path = kanzei_core::project_state_path(&root);
    let source_sequence;
    let mut messages = {
        let store = kanzei_core::SessionStore::open(&state_path)?;
        if store
            .get_session(&session)?
            .is_some_and(|session| session.status == "running")
            || !store.list_pending_inputs(&session)?.is_empty()
        {
            anyhow::bail!("对话正在运行或有排队输入，请结束后再压缩");
        }
        source_sequence = store
            .list_events(&session, 0)?
            .last()
            .map(|e| e.sequence)
            .unwrap_or(0);
        recover_cli_prior(&store, &session)?
    };
    let source_hash = kanzei_core::store::stable_json_hash(&messages);
    let before = kanzei_core::estimate_conversation_tokens(&messages);
    if messages.len() < 3 {
        println!("当前内容很少，无需压缩");
        return Ok(());
    }
    let config = kanzei_harness::KanzeiConfig::load_at_root(&root)?;
    let resolved = config.resolve_model("primary")?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => kanzei_llm::ProxyConfig::Disabled,
        Some("env") | None => kanzei_llm::ProxyConfig::Env,
        Some(value) => kanzei_llm::ProxyConfig::Explicit(value.into()),
    };
    let route = kanzei_core::build_route(&resolved, &proxy).await?;
    let client = kanzei_llm::LlmClient::new(&proxy)?;
    let mut model = kanzei_tools::run::build_digest_model(&config, &proxy, &resolved, &route).await;
    model.archive_root = Some(root.clone());
    model.focus = focus;
    let _publication = kanzei_core::store::artifact_liveness::acquire_publication(&root).await?;
    let dropped = kanzei_core::compact_conversation_with_model(
        &client,
        Some(&model),
        &mut messages,
        before.max(4000),
        &mut Vec::new(),
        config.limits.recent_verbatim_ratio(),
    )
    .await;
    if dropped == 0 {
        println!("当前没有完整、可压缩的历史区间");
        return Ok(());
    }
    let after = kanzei_core::estimate_conversation_tokens(&messages);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let store = kanzei_core::SessionStore::open(&state_path)?;
    if kanzei_core::store::stable_json_hash(&recover_cli_prior(&store, &session)?) != source_hash {
        anyhow::bail!("压缩期间对话已变化，未替换当前上下文，请重试");
    }
    store.append_compaction_transaction_checked(
        &session,
        &format!("manual-cli:{stamp}"),
        &json!({"manual":true,"focus":model.focus,"before":before,"after":after,"dropped":dropped}),
        &json!(messages),
        Some(source_sequence),
    )?;
    println!("已压缩：{before} → {after} token");
    Ok(())
}
