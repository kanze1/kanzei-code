//! Explicit connections never share an implicit current page with the preview pane.
use kanzei_harness::ToolCtx;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

type Connections = HashMap<(String, String), String>;
static CONNECTIONS: LazyLock<Mutex<Connections>> = LazyLock::new(Mutex::default);

pub fn uses_connection(input: &Value) -> bool {
    input
        .get("connection_id")
        .is_some_and(|value| !value.is_null())
        || matches!(
            action(input),
            Some(
                super::BrowserAction::Connect
                    | super::BrowserAction::Pages
                    | super::BrowserAction::Disconnect
            )
        )
}

fn action(input: &Value) -> Option<super::BrowserAction> {
    input["action"]
        .as_str()
        .and_then(|value| super::BrowserAction::parse(value).ok())
}

pub fn connection_endpoint(value: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(value).map_err(|error| error.to_string())?;
    let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
    let local = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if url.scheme() != "http"
        || !local
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "CDP endpoint must be a local HTTP origin without credentials, path, query or fragment"
                .into(),
        );
    }
    Ok(url.to_string())
}

pub fn connection_resources(input: &Value, ctx: &ToolCtx) -> Vec<String> {
    if action(input) == Some(super::BrowserAction::Connect) {
        return vec![input["endpoint"]
            .as_str()
            .and_then(|endpoint| connection_endpoint(endpoint).ok())
            .map(|endpoint| format!("cdp:{endpoint}"))
            .unwrap_or_else(|| "connection:invalid-endpoint".into())];
    }
    let id = input["connection_id"].as_str().unwrap_or("");
    let endpoint = CONNECTIONS
        .lock()
        .expect("browser connection registry")
        .get(&(super::owner_key(ctx), id.into()))
        .cloned();
    if matches!(
        action(input),
        Some(super::BrowserAction::Pages | super::BrowserAction::Disconnect)
    ) {
        return vec![endpoint
            .map(|endpoint| format!("cdp:{endpoint}"))
            .unwrap_or_else(|| "connection:unavailable".into())];
    }
    let mut resources = super::resources_for(input, Some(&ctx.cwd), input["expected_url"].as_str());
    if let Some(url) = input["expected_frame_url"].as_str() {
        resources.push(super::url_resource(url));
    }
    if let Some(url) = input["expected_url"].as_str() {
        let resource = super::url_resource(url);
        if !resources.contains(&resource) {
            resources.push(resource);
        }
    } else {
        resources.push("connection:missing-page-identity".into());
    }
    resources
}

pub(super) fn remember(ctx: &ToolCtx, value: &Value) -> Result<(), String> {
    let id = value["connection_id"]
        .as_str()
        .ok_or("connected browser did not return its identity")?;
    let endpoint = connection_endpoint(
        value["endpoint"]
            .as_str()
            .ok_or("connected browser endpoint missing")?,
    )?;
    CONNECTIONS
        .lock()
        .map_err(|_| "browser connection registry unavailable")?
        .insert((super::owner_key(ctx), id.into()), endpoint);
    Ok(())
}

pub(super) fn forget(ctx: &ToolCtx, id: &str) {
    CONNECTIONS
        .lock()
        .expect("browser connection registry")
        .remove(&(super::owner_key(ctx), id.into()));
}

pub(super) fn clear() {
    CONNECTIONS
        .lock()
        .expect("browser connection registry")
        .clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connection_endpoint_grants_only_a_local_http_origin() {
        for valid in [
            "http://127.0.0.1:9222",
            "http://[::1]:9222/",
            "http://localhost:9222/",
        ] {
            assert!(connection_endpoint(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "https://127.0.0.1:9222/",
            "http://example.com:9222/",
            "http://127.0.0.1:9222/path",
            "http://127.0.0.1:9222/?token=secret",
            "http://user@127.0.0.1:9222/",
            "http://tauri.localhost:9222/",
        ] {
            assert!(connection_endpoint(invalid).is_err(), "{invalid}");
        }
    }
    #[test]
    fn attached_actions_bind_the_page_and_url_for_both_permission_and_execution() {
        let ctx = ToolCtx::new("worktree".into(), "project".into());
        let raw = serde_json::json!({"action":"type","connection_id":"connection","page_id":"page","expected_url":"https://example.com/form","selector":"#field","text":"new"});
        let input = super::super::parse_browser_input(raw.clone()).unwrap();
        assert!(super::super::validate(&input, super::super::BrowserAction::Type).is_ok());
        assert!(connection_resources(&raw, &ctx).contains(&"url:example.com/form".into()));
        let mut frame = raw.clone();
        frame["frame_id"] = "frame".into();
        frame["expected_frame_url"] = "https://frames.example.org/embedded".into();
        assert!(
            connection_resources(&frame, &ctx).contains(&"url:frames.example.org/embedded".into())
        );
        assert!(super::super::validate(
            &super::super::parse_browser_input(frame).unwrap(),
            super::super::BrowserAction::Type
        )
        .is_ok());
        let mut missing = input.clone();
        missing.page_id = None;
        assert!(super::super::validate(&missing, super::super::BrowserAction::Type).is_err());
        let mut privileged = input;
        privileged.expected_url = Some("http://tauri.localhost/".into());
        assert!(super::super::validate(&privileged, super::super::BrowserAction::Type).is_err());
    }
}
