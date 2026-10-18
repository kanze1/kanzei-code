//! Engine-owned identity and clock facts, committed before a provider sees them.

use kanzei_core::SessionStore;
use kanzei_harness::config::ResolvedModel;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct RunFacts<'a> {
    schema_version: u32,
    run_id: &'a str,
    input_id: &'a str,
    session_id: &'a str,
    provider: &'a str,
    model: &'a str,
    started_at_utc: String,
    local_date: String,
    utc_offset: String,
    app_version: &'static str,
    build: &'static str,
}

impl<'a> RunFacts<'a> {
    fn capture(
        run_id: &'a str,
        input_id: &'a str,
        session_id: &'a str,
        model: &'a ResolvedModel,
    ) -> Self {
        let now = chrono::Local::now();
        Self {
            schema_version: 1,
            run_id,
            input_id,
            session_id,
            provider: &model.provider_name,
            model: &model.model,
            started_at_utc: now.with_timezone(&chrono::Utc).to_rfc3339(),
            local_date: now.format("%Y-%m-%d").to_string(),
            utc_offset: now.format("%:z").to_string(),
            app_version: env!("CARGO_PKG_VERSION"),
            build: option_env!("KANZEI_BUILD_INFO").unwrap_or("dev"),
        }
    }
}

pub(super) fn prepare(
    store: &SessionStore,
    session_id: &str,
    run_id: &str,
    input_id: &str,
    model: &ResolvedModel,
    system: &mut String,
) -> anyhow::Result<()> {
    let facts = serde_json::to_value(RunFacts::capture(run_id, input_id, session_id, model))?;
    store.append_event(session_id, "run.facts", &facts)?;
    system.push_str(&format!(
        "\n\n<engine_run_facts>\n{facts}\n</engine_run_facts>\n这些事实属于当前主会话运行，由引擎生成；仓库起点版本以 run.baseline 为准。日期使用本地日期和 UTC 偏移。验证结论必须引用实际执行记录及其版本；源码存在、准备好命令、工具返回成功，均不自动代表测试或发布完成。build=dev 表示开发构建。"
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_are_committed_with_the_same_identity_and_model_that_enter_context() {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_session("session", "fixture", None).unwrap();
        let config: kanzei_harness::config::KanzeiConfig = toml::from_str(
            "[models]\nprimary='stub:actual-model'\n[providers.stub]\nprotocol='openai'\nbase_url='http://127.0.0.1:1/v1'\n"
        ).unwrap();
        let model = config.resolve_model("stub:actual-model").unwrap();
        let mut system = String::new();
        prepare(&store, "session", "run", "input", &model, &mut system).unwrap();
        let facts = store
            .latest_event("session", "run.facts")
            .unwrap()
            .unwrap()
            .payload;
        assert_eq!(facts["run_id"], "run");
        assert_eq!(facts["input_id"], "input");
        assert_eq!(facts["provider"], "stub");
        assert_eq!(facts["model"], "actual-model");
        assert!(
            chrono::DateTime::parse_from_rfc3339(facts["started_at_utc"].as_str().unwrap()).is_ok()
        );
        assert!(system.contains(&facts.to_string()));
    }
}
