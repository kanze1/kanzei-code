#[cfg(test)]
mod tool_surface_budget {
    use kanzei_harness::{HarnessSnapshot, KanzeiConfig, ProfileKind, ResolveCtx};
    use std::path::PathBuf;
    use std::sync::Arc;

    // Includes core's task tool; desktop additionally supplies collaboration_status.
    const AUTO_RESIDENT_TOOL_BUDGET: usize = 16;
    const PAIR_RESIDENT_TOOL_BUDGET: usize = 15;
    // Includes the two Markdown skill discovery/read tools.
    const AUTO_DEFERRED_TOOL_BUDGET: usize = 19;
    const PAIR_DEFERRED_TOOL_BUDGET: usize = 20;

    /// readonly 档:只读分析,面应当明显更小。
    ///
    /// **当前 15 不是「合理值」,是「现状值」**——本预算测试立起来时发现 plot/latex/
    /// process/browser 都还在这个面里,且只被判 Ask 而不是硬拒(D-663)。
    /// D-663 修完后这个数应当降到 11 左右,届时把预算一起收紧。
    const READONLY_TOOL_BUDGET: usize = 15;

    fn surface(profile: ProfileKind) -> Arc<HarnessSnapshot> {
        let root = PathBuf::from("C:/kanzei-d662-budget");
        let ctx = ResolveCtx {
            profile,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        kanzei_runtime::build_harness(profile, |_| {}, |_| {})
            .resolve(&ctx)
            .unwrap()
    }

    fn visible_tools(profile: ProfileKind) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = surface(profile)
            .materialize_tools()
            .iter()
            .map(|tool| tool.name())
            .collect();
        names.sort_unstable();
        names
    }

    fn materialized_tool_specs(profile: ProfileKind) -> Vec<kanzei_llm::ToolSpec> {
        surface(profile)
            .materialize_tools()
            .iter()
            .map(|tool| kanzei_llm::ToolSpec {
                name: tool.name().to_string(),
                description: tool.description(),
                input_schema: tool.input_schema(),
            })
            .collect()
    }

    #[test]
    fn chat_tool_catalog_survives_ollama_parameter_projection() {
        fn check_refs(value: &serde_json::Value, root: &serde_json::Value, name: &str) {
            match value {
                serde_json::Value::Object(object) => {
                    if let Some(reference) = object.get("$ref").and_then(|v| v.as_str()) {
                        if let Some(pointer) = reference.strip_prefix('#') {
                            assert!(
                                root.pointer(pointer).is_some(),
                                "{name}: unresolved provider schema reference {reference}"
                            );
                        }
                    }
                    for nested in object.values() {
                        check_refs(nested, root, name);
                    }
                }
                serde_json::Value::Array(array) => {
                    for nested in array {
                        check_refs(nested, root, name);
                    }
                }
                _ => {}
            }
        }
        for profile in [
            ProfileKind::DevAuto,
            ProfileKind::DevPair,
            ProfileKind::Research,
            ProfileKind::Readonly,
        ] {
            let request = kanzei_llm::LlmRequest {
                model: "local-model".into(),
                system: vec![],
                messages: vec![],
                tools: materialized_tool_specs(profile),
                hosted_tools: vec![],
                max_tokens: 64,
                temperature: None,
                reasoning: kanzei_llm::ReasoningEffort::Off,
                service_tier: None,
            };
            let body = kanzei_llm::protocol::openai::build_body(&request);
            for tool in body["tools"].as_array().unwrap() {
                let function = &tool["function"];
                let name = function["name"].as_str().unwrap();
                // Ollama 0.40.6 api.ToolFunctionParameters retains these root
                // fields. In particular, draft-07 definitions is not retained.
                let mut parameters = function["parameters"].clone();
                parameters.as_object_mut().unwrap().retain(|key, _| {
                    matches!(
                        key.as_str(),
                        "type" | "$defs" | "items" | "required" | "properties"
                    )
                });
                assert!(
                    parameters["properties"].is_object(),
                    "{name}: missing properties map"
                );
                check_refs(&parameters, &parameters, name);
            }
        }
    }

    #[test]
    fn 逐工具schema字符账单() {
        for profile in [ProfileKind::DevPair, ProfileKind::DevAuto] {
            let snapshot = surface(profile);
            let resident: Vec<_> = snapshot
                .resident_tools()
                .iter()
                .map(|tool| tool.name())
                .collect();
            let deferred: Vec<_> = snapshot
                .deferred_tools()
                .iter()
                .map(|tool| tool.name())
                .collect();
            let specs = materialized_tool_specs(profile);
            let mut rows: Vec<(&str, usize, usize, &str)> = specs
                .iter()
                .map(|spec| {
                    let chars = spec.char_len();
                    let bytes = spec.name.len()
                        + spec.description.len()
                        + spec.input_schema.to_string().len();
                    let layer = if resident.contains(&spec.name.as_str()) {
                        "常驻"
                    } else if deferred.contains(&spec.name.as_str()) {
                        "延迟"
                    } else {
                        "未分类"
                    };
                    (spec.name.as_str(), chars, bytes / 4, layer)
                })
                .collect();
            rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
            eprintln!("CLI {profile:?} schema 字符账单:工具\t字符\tbytes/4 估算 tokens\t分层");
            for (name, chars, token_estimate, layer) in &rows {
                eprintln!("{name}\t{chars}\t{token_estimate}\t{layer}");
            }
            let resident_chars: usize = rows
                .iter()
                .filter(|row| row.3 == "常驻")
                .map(|row| row.1)
                .sum();
            let deferred_chars: usize = rows
                .iter()
                .filter(|row| row.3 == "延迟")
                .map(|row| row.1)
                .sum();
            let unclassified_chars: usize = rows
                .iter()
                .filter(|row| row.3 == "未分类")
                .map(|row| row.1)
                .sum();
            let row_sum: usize = rows.iter().map(|row| row.1).sum();
            let spec_sum: usize = specs.iter().map(kanzei_llm::ToolSpec::char_len).sum();
            assert_eq!(row_sum, spec_sum, "账单总数必须等于逐项之和");
            assert_eq!(unclassified_chars, 0, "每个已注册工具必须计入实际装载层");
            let resident_bytes: usize = snapshot.resident_tools().iter().map(|tool| {
            serde_json::json!({"name":tool.name(), "description":tool.description(), "input_schema":tool.input_schema()}).to_string().len()
        }).sum();
            eprintln!("CLI {profile:?}: resident_count={}, resident_definition_bytes={resident_bytes}, catalog_chars={}", resident.len(), snapshot.deferred_catalog().unwrap().chars().count());
            let deferred_ratio = deferred_chars as f64 / spec_sum as f64 * 100.0;
            eprintln!(
            "CLI {profile:?} totals: resident={resident_chars}, deferred={deferred_chars} ({deferred_ratio:.2}%), unclassified={unclassified_chars}, all={spec_sum}; core task_spec(未计入)"
        );
        }
    }

    #[test]
    fn dev档常驻与延迟目录分别符合预算() {
        for (profile, resident_budget, deferred_budget) in [
            (
                ProfileKind::DevPair,
                PAIR_RESIDENT_TOOL_BUDGET,
                PAIR_DEFERRED_TOOL_BUDGET,
            ),
            (
                ProfileKind::DevAuto,
                AUTO_RESIDENT_TOOL_BUDGET,
                AUTO_DEFERRED_TOOL_BUDGET,
            ),
        ] {
            let snapshot = surface(profile);
            assert!(snapshot
                .resident_tools()
                .iter()
                .any(|tool| tool.name() == "tool_search"));
            assert_eq!(
                snapshot.resident_tools().len() + 1,
                resident_budget,
                "{profile:?}: core adds task separately"
            );
            assert_eq!(
                snapshot.deferred_tools().len(),
                deferred_budget,
                "{profile:?}"
            );
        }
    }

    #[test]
    fn paired_management_tools_are_discoverable_without_loading_their_schemas() {
        for profile in [ProfileKind::DevPair, ProfileKind::DevAuto] {
            let snapshot = surface(profile);
            let resident: Vec<_> = snapshot.resident_tools().iter().map(|t| t.name()).collect();
            let catalog = snapshot.deferred_catalog().unwrap();
            for name in ["req", "defect", "test_record", "memory_note"] {
                assert!(
                    !resident.contains(&name),
                    "{profile:?} eagerly loads {name}"
                );
                assert!(snapshot.is_deferred(name), "{name} must remain available");
                assert!(catalog.contains(&format!("- {name} —")));
            }
            assert_eq!(resident.contains(&"work"), profile == ProfileKind::DevAuto);
            assert_eq!(
                snapshot.is_deferred("work"),
                profile == ProfileKind::DevPair
            );
            for name in [
                "read",
                "write",
                "edit",
                "bash",
                "grep",
                "git",
                "tool_search",
            ] {
                assert!(resident.contains(&name), "missing coding primitive {name}");
            }
        }
    }

    #[test]
    fn readonly档工具面明显更小() {
        let names = visible_tools(ProfileKind::Readonly);
        assert!(
            names.len() <= READONLY_TOOL_BUDGET,
            "readonly 档可见工具 {} 个,超出预算 {READONLY_TOOL_BUDGET};当前清单: {names:?}",
            names.len()
        );
        // 档位契约的正面断言:写与命令族必须整体摘除(fully_denied),不是判 Ask。
        // 这几条现在是**通过**的;D-663 记的是 plot/latex/process/browser 没进这个名单。
        for denied in ["write", "edit", "insert", "bash", "sandbox"] {
            assert!(
                !names.contains(&denied),
                "readonly 档不得看见 {denied}(应被 action_fully_denied 整体摘除)"
            );
        }
    }

    /// 记忆写路径必须**不在**主 agent 的工具面里(写读分离,R-105)。
    ///
    /// 这条是 D-662 的护栏:memory 一族在仓库里有 10 个工具,主面只该看见 3 个。
    /// 哪天有人图省事把 MemoryManagerComponent 挂进主装配,主面会一次涨 7 个,
    /// 而且**记忆的唯一写路径**这条设计约束会同时失效——两件事一条测试拦住。
    #[test]
    fn 记忆写工具不得进入主agent工具面() {
        let names = visible_tools(ProfileKind::DevAuto);
        for leaked in [
            "memory_add",
            "memory_promote",
            "memory_update",
            "memory_merge",
            "memory_stale",
            "memory_inbox_clear",
        ] {
            assert!(
                !names.contains(&leaked),
                "{leaked} 泄漏进主 agent 工具面:记忆的唯一写路径是 memory-manager 子代理"
            );
        }
        for expected in ["memory_note", "memory_search", "memory_stats"] {
            assert!(names.contains(&expected), "主 agent 应保留 {expected}");
        }
    }
}
