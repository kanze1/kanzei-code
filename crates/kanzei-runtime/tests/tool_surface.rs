#[cfg(test)]
mod tool_surface_budget {
    use kanzei_harness::{ConfigComponent, KanzeiConfig, ProfileKind, ResolveCtx};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// D-662/R-364 双门禁按工具数计,不是 schema 字符数。
    /// CLI resident=19 个注册工具 + core 单独追加的 task; desktop 再加 collaboration_status。
    const DEV_RESIDENT_TOOL_BUDGET: usize = 20;
    // Packaged skills and explicitly saved preferences are read through deferred tools.
    const DEV_DEFERRED_TOOL_BUDGET: usize = 15;

    /// readonly 档:只读分析,面应当明显更小。
    ///
    /// **当前 15 不是「合理值」,是「现状值」**——本预算测试立起来时发现 plot/latex/
    /// process/browser 都还在这个面里,且只被判 Ask 而不是硬拒(D-663)。
    /// D-663 修完后这个数应当降到 11 左右,届时把预算一起收紧。
    const READONLY_TOOL_BUDGET: usize = 15;

    fn visible_tools(profile: ProfileKind) -> Vec<&'static str> {
        let root = PathBuf::from("C:/kanzei-d662-budget");
        let ctx = ResolveCtx {
            profile,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = kanzei_runtime::build_harness(
            ctx.profile,
            |h| {
                h.add(kanzei_tools::ReadonlyProfile);
            },
            |_| {},
        );
        harness.add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let mut names: Vec<&'static str> = snapshot
            .materialize_tools()
            .iter()
            .map(|tool| tool.name())
            .collect();
        names.sort_unstable();
        names
    }

    fn visible_layer_counts(profile: ProfileKind) -> (usize, usize) {
        let root = PathBuf::from("C:/kanzei-d662-budget");
        let ctx = ResolveCtx {
            profile,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = kanzei_runtime::build_harness(
            ctx.profile,
            |h| {
                h.add(kanzei_tools::ReadonlyProfile);
            },
            |_| {},
        );
        harness.add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        (
            snapshot.resident_tools().len(),
            snapshot.deferred_tools().len(),
        )
    }

    fn materialized_tool_specs(profile: ProfileKind) -> Vec<kanzei_llm::ToolSpec> {
        let root = PathBuf::from("C:/kanzei-r364-b1-schema-bill");
        let ctx = ResolveCtx {
            profile,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = kanzei_runtime::build_harness(
            ctx.profile,
            |h| {
                h.add(kanzei_tools::ReadonlyProfile);
            },
            |_| {},
        );
        harness.add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        snapshot
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
        let resident = [
            "read",
            "write",
            "edit",
            "insert",
            "bash",
            "glob",
            "grep",
            "symbols",
            "git",
            "req",
            "defect",
            "work",
            "test_record",
            "memory_search",
            "memory_note",
            "question",
            "task",
            "websearch",
            "webfetch",
            "tool_search",
        ];
        let deferred = [
            "process",
            "files",
            "incident",
            "conventions",
            "architecture",
            "prior_art",
            "browser",
            "latex",
            "plot",
            "idea",
            "decision",
            "memory_stats",
        ];
        let specs = materialized_tool_specs(ProfileKind::DevAuto);
        let expected_materialized: Vec<&str> = resident
            .iter()
            .copied()
            .chain(deferred.iter().copied())
            .filter(|name| *name != "task")
            .collect();
        for expected in expected_materialized {
            assert!(
                specs.iter().any(|spec| spec.name.as_str() == expected),
                "§5.1 当前应物化工具缺失: {expected}"
            );
        }

        let mut rows: Vec<(&str, usize, usize, &str)> = specs
            .iter()
            .map(|spec| {
                let chars = spec.char_len();
                let bytes =
                    spec.name.len() + spec.description.len() + spec.input_schema.to_string().len();
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
        eprintln!("CLI Dev schema 字符账单:工具\t字符\tbytes/4 估算 tokens\t分层");
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
        let deferred_ratio = deferred_chars as f64 / spec_sum as f64 * 100.0;
        eprintln!(
            "CLI Dev totals: resident={resident_chars}, deferred={deferred_chars} ({deferred_ratio:.2}%), unclassified={unclassified_chars}, all={spec_sum}; core task_spec(未计入)"
        );
    }

    #[test]
    fn dev档常驻与延迟目录分别符合预算() {
        let names = visible_tools(ProfileKind::DevAuto);
        let (resident, deferred) = visible_layer_counts(ProfileKind::DevAuto);
        assert!(
            names.contains(&"tool_search"),
            "tool_search 必须在 Dev 常驻表中"
        );
        assert_eq!(
            resident + 1,
            DEV_RESIDENT_TOOL_BUDGET,
            "resident_tools 的 19 项之外由 core 追加 task_spec"
        );
        assert_eq!(deferred, DEV_DEFERRED_TOOL_BUDGET);
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
