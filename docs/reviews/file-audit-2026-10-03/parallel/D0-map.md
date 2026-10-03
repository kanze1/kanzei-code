# D0 依赖与边界地图

审查基线：`dev b42b284f57f4f06e2bb93defdd734b65bd2b9c6e`。初始只读审查；主代理核准3个P1后，本包仅修改 home.rs/project_root.rs/markdown.rs 及 D0 文档。主代理的 mobile.rs/inbox.rs/M2.md 与原有问题.MD 不属本包。

先读 dependency-map.md、harness/Cargo.toml、harness/lib.rs，然后按 defs → home → project_root → markdown → refs 全文检查，包括文件内测试。Cargo 清单确认 harness 生产内部依赖只有 kanzei-base；lib.rs 将五个模块公开，defs/home/MarkdownComponent 在 crate 根再导出，config 再导出项目根 API。

| 文件 | 依赖与调用方 | 身份、运行状态、持久边界 |
| --- | --- | --- |
| defs.rs | serde；harness 快照、config、profiles、CLI/app 装配、core runner、team/task；effective_agent_steps 直接调用在 core/runner/drive/assembly.rs、tools/team/mod.rs、app/agent_directory.rs | 纯枚举/定义与预算函数；0 由运行角色解释，主代理不限步，子代理默认32；无文件写入 |
| home.rs | dirs/env、base::content_hash；config、markdown、memory、app prefs/settings/voice/research_library/runtime_service/general_chat、tools profiles/git/base/team/work/schedules、harness ToolCtx | KANZEI_HOME 是全局目录来源；general 对话身份决定项目工具和每会话 artifacts 的隔离；只构造路径，不读取真实全局配置 |
| project_root.rs | home、permission::normalize_resource、base::path_form；config load/re-export、ToolCtx::discovering、CLI main_project_root、app projects/docs/agent_directory、tools memory_consolidation | 入口取根后传递项目身份；canonicalize 仅内部比较；发现结果决定配置和项目 state.db/文档/记忆落点；本文件不写这些数据 |
| markdown.rs | defs、harness、registry、home、serde_json；tools/run.rs build_harness/build_subagent_runtime、tools/team/mod.rs、app agent_directory 的 parse_frontmatter | 全局→项目、同名后者覆盖；贡献 agent/skill 到运行期不可变快照；技能正文路径按需读取；仅检查代码，不实际扫描用户 agents/skills |
| refs.rs | 纯 std；tools/refgraph/memory_graph.rs 唯一生产直接消费者 | 分词不判存在性；RefRel::as_str 成图谱关系；tokens/dirty 分离；caller 的节点存在性与 warnings 是外层边界，无持久写入 |

公开 API 的仓内全部 Rust 搜索命中（含测试/再导出/同名项）保存在 D0-verification.json 的 api_search_matches；调用方只审相关切片，不记成全文。Frontmatter::get 由 markdown 内部扫描与 app/agent_directory 调用；SplitRefs/RefToken/RANGE_LIMIT 仅类型/结果或内部常量使用，没有独立生产消费者；discover_project_config 搜索仅定义与再导出。

已重点跟踪三条链：CLI 根解析→项目持久身份；general_chat::open→规范根→run/assembly/GeneralChatProfile；agent_directory 状态→markdown 注册→select_agent→有效预算。PASS 判断、3个P1修复和回归代码见 D0.md。只读验证 git diff --check 已通过；未运行 Cargo、测试、Clippy 或 fmt；整合后的执行验证由主代理完成。
