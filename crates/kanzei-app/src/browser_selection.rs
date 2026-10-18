//! Browser choice belongs to the conversation, rather than the selected UI tab.
use crate::MutexPoisonExt;
use kanzei_harness::ToolCtx;
use kanzei_tools::browser::Backend;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

const EVENT: &str = "browser.target_selected";
static TEMPORARY: LazyLock<Mutex<HashMap<String, Selection>>> = LazyLock::new(Mutex::default);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Selection {
    pub(crate) owner: String,
    pub(crate) backend: Backend,
    pub(crate) page_id: Option<String>,
}

pub(crate) fn selected(ctx: &ToolCtx) -> Result<Option<Selection>, String> {
    let owner = kanzei_tools::browser::owner_key(ctx);
    let Some(session) = &ctx.session_id else {
        return Ok(TEMPORARY.lock_or_recover().get(&owner).cloned());
    };
    let store = kanzei_core::SessionStore::open_read_only(&kanzei_core::project_state_path(
        &ctx.project_root,
    ))
    .map_err(|e| format!("读取浏览器归属失败: {e}"))?;
    store
        .latest_matching_event(session, EVENT, "$.owner", &owner)
        .map_err(|e| e.to_string())?
        .map(|event| serde_json::from_value(event.payload).map_err(|e| e.to_string()))
        .transpose()
}

pub(crate) fn remember(
    ctx: &ToolCtx,
    backend: Backend,
    page_id: Option<String>,
) -> Result<(), String> {
    let selection = Selection {
        owner: kanzei_tools::browser::owner_key(ctx),
        backend,
        page_id,
    };
    if selected(ctx)?.as_ref() == Some(&selection) {
        return Ok(());
    }
    let Some(session) = &ctx.session_id else {
        TEMPORARY
            .lock_or_recover()
            .insert(selection.owner.clone(), selection);
        return Ok(());
    };
    let store =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&ctx.project_root))
            .map_err(|e| e.to_string())?;
    if store
        .get_session(session)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err("浏览器所属会话已删除".into());
    }
    store
        .append_event(session, EVENT, &serde_json::json!(selection))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_survives_reopen_and_isolated_by_session_and_child() {
        let root = tempfile::tempdir().unwrap();
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root.path())).unwrap();
        store
            .create_session("a", &root.path().display().to_string(), None)
            .unwrap();
        store
            .create_session("b", &root.path().display().to_string(), None)
            .unwrap();
        let mut ctx = ToolCtx::new(root.path().into(), root.path().into());
        ctx.session_id = Some("a".into());
        remember(&ctx, Backend::Headless, None).unwrap();
        drop(store);
        assert_eq!(selected(&ctx).unwrap().unwrap().backend, Backend::Headless);
        ctx.browser_owner = Some("child".into());
        assert!(selected(&ctx).unwrap().is_none());
        ctx.browser_owner = None;
        ctx.session_id = Some("b".into());
        assert!(selected(&ctx).unwrap().is_none());
        remember(&ctx, Backend::Pane, Some("pane-incarnation".into())).unwrap();
        assert_eq!(
            selected(&ctx).unwrap().unwrap().page_id.as_deref(),
            Some("pane-incarnation")
        );
    }
}
