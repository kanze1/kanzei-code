# 桌面命令与数据表索引

从当前源码机械提取。动态 invoke 与间接注册需结合调用链阅读；重复表声明可能来自迁移或测试。

## Tauri 命令

| 命令 | 声明 | main 注册 | 前端字面量调用数 |
|---|---|---|---:|
| agent_directory_get | [crates/kanzei-app/src/agent_directory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/agent_directory.rs:42>) | 是 | 1 |
| agent_directory_open | [crates/kanzei-app/src/agent_directory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/agent_directory.rs:110>) | 是 | 1 |
| auto_state_update | [crates/kanzei-app/src/auto_run.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/auto_run.rs:56>) | 是 | 3 |
| auto_state_reset | [crates/kanzei-app/src/auto_run.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/auto_run.rs:111>) | 是 | 1 |
| collaboration_snapshot | [crates/kanzei-app/src/collaboration.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/collaboration.rs:415>) | 是 | 2 |
| models_list | [crates/kanzei-app/src/commands/models.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/commands/models.rs:50>) | 是 | 4 |
| pending_asks_get | [crates/kanzei-app/src/commands/run.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/commands/run.rs:36>) | 是 | 1 |
| answer_ask | [crates/kanzei-app/src/commands/run.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/commands/run.rs:62>) | 是 | 2 |
| stop_run | [crates/kanzei-app/src/commands/run.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/commands/run.rs:99>) | 是 | 3 |
| stop_task | [crates/kanzei-app/src/commands/run.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/commands/run.rs:195>) | 是 | 2 |
| run_prompt | [crates/kanzei-app/src/commands/run.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/commands/run.rs:224>) | 是 | 4 |
| run_metrics | [crates/kanzei-app/src/commands/run.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/commands/run.rs:483>) | 是 | 1 |
| run_metrics_by_task | [crates/kanzei-app/src/commands/run.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/commands/run.rs:505>) | 是 | 1 |
| run_metrics_by_category | [crates/kanzei-app/src/commands/run.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/commands/run.rs:628>) | 是 | 1 |
| open_delivered_path | [crates/kanzei-app/src/commands/run.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/commands/run.rs:978>) | 是 | 2 |
| summarize_chat | [crates/kanzei-app/src/commands/summarize.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/commands/summarize.rs:51>) | 是 | 1 |
| conversation_clear | [crates/kanzei-app/src/conversation.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/conversation.rs:10>) | 是 | 1 |
| conversation_get | [crates/kanzei-app/src/conversation.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/conversation.rs:150>) | 是 | 1 |
| conversation_shadow_get | [crates/kanzei-app/src/conversation.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/conversation.rs:233>) | 是 | 0 |
| conversation_trace_get | [crates/kanzei-app/src/conversation.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/conversation.rs:260>) | 是 | 2 |
| conversation_list | [crates/kanzei-app/src/conversation.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/conversation.rs:293>) | 是 | 1 |
| conversation_delete | [crates/kanzei-app/src/conversation.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/conversation.rs:448>) | 是 | 1 |
| conversation_cleanup | [crates/kanzei-app/src/conversation.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/conversation.rs:593>) | 是 | 1 |
| verification_cancel | [crates/kanzei-app/src/decisions.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/decisions.rs:8>) | 是 | 1 |
| work_delivery_accept | [crates/kanzei-app/src/decisions.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/decisions.rs:161>) | 是 | 1 |
| decision_review | [crates/kanzei-app/src/decisions.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/decisions.rs:179>) | 是 | 1 |
| git_status | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:141>) | 是 | 1 |
| conventions_read | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:232>) | 是 | 2 |
| conventions_save | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:237>) | 是 | 1 |
| conventions_discard | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:252>) | 是 | 1 |
| conventions_init | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:264>) | 是 | 0 |
| test_runs_snapshot | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:278>) | 是 | 1 |
| test_run_record | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:284>) | 是 | 0 |
| test_runs_init_refs | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:321>) | 是 | 1 |
| docs_snapshot | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:349>) | 是 | 4 |
| research_plan_get | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:528>) | 是 | 1 |
| research_plan_approve | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:545>) | 是 | 1 |
| docs_archive_entries | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:561>) | 是 | 1 |
| docs_update | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:593>) | 是 | 9 |
| research_arxiv_preview | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:708>) | 是 | 2 |
| webfetch_preview | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:780>) | 是 | 2 |
| docs_open | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:848>) | 是 | 1 |
| docs_read | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:858>) | 是 | 2 |
| docs_read_custom | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:877>) | 是 | 2 |
| architecture_snapshot | [crates/kanzei-app/src/docs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/docs.rs:903>) | 是 | 1 |
| fast_model_status | [crates/kanzei-app/src/fast_model.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/fast_model.rs:11>) | 是 | 3 |
| fast_model_setup | [crates/kanzei-app/src/fast_model.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/fast_model.rs:35>) | 是 | 1 |
| file_stat | [crates/kanzei-app/src/files_edit.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/files_edit.rs:614>) | 是 | 1 |
| file_write | [crates/kanzei-app/src/files_edit.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/files_edit.rs:624>) | 是 | 2 |
| files_snapshot | [crates/kanzei-app/src/files_view.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/files_view.rs:32>) | 是 | 1 |
| file_preview | [crates/kanzei-app/src/files_view.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/files_view.rs:97>) | 是 | 3 |
| files_annotate | [crates/kanzei-app/src/files_view.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/files_view.rs:108>) | 是 | 1 |
| memory_overview | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:73>) | 是 | 0 |
| memory_control_plane | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:98>) | 是 | 0 |
| memory_entries | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:188>) | 是 | 2 |
| memory_entry_get | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:219>) | 是 | 1 |
| memory_graph | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:318>) | 是 | 2 |
| memory_note_candidates | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:357>) | 是 | 0 |
| memory_note_discard | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:376>) | 是 | 1 |
| memory_recalls | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:390>) | 是 | 0 |
| memory_value_flags | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:438>) | 是 | 0 |
| memory_entry_save | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:469>) | 是 | 3 |
| memory_entry_delete | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:512>) | 是 | 1 |
| memory_search_page | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:532>) | 是 | 1 |
| memory_context_bill | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:582>) | 是 | 0 |
| memory_consolidate | [crates/kanzei-app/src/memory.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory.rs:601>) | 是 | 2 |
| memory_chat_history | [crates/kanzei-app/src/memory_chat.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory_chat.rs:207>) | 是 | 1 |
| memory_chat_stop | [crates/kanzei-app/src/memory_chat.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory_chat.rs:225>) | 是 | 1 |
| memory_chat_send | [crates/kanzei-app/src/memory_chat.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/memory_chat.rs:245>) | 是 | 1 |
| mobile_service_start | [crates/kanzei-app/src/mobile.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/mobile.rs:703>) | 是 | 1 |
| mobile_device_revoke | [crates/kanzei-app/src/mobile.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/mobile.rs:817>) | 是 | 1 |
| mobile_pair_code_regenerate | [crates/kanzei-app/src/mobile.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/mobile.rs:836>) | 是 | 1 |
| mobile_device_list | [crates/kanzei-app/src/mobile.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/mobile.rs:846>) | 是 | 1 |
| mobile_service_stop | [crates/kanzei-app/src/mobile.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/mobile.rs:876>) | 是 | 1 |
| model_effective | [crates/kanzei-app/src/model_config.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/model_config.rs:520>) | 是 | 1 |
| project_models_get | [crates/kanzei-app/src/model_config.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/model_config.rs:617>) | 是 | 1 |
| project_models_save | [crates/kanzei-app/src/model_config.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/model_config.rs:774>) | 是 | 3 |
| project_config_open | [crates/kanzei-app/src/model_config.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/model_config.rs:788>) | 是 | 1 |
| ui_prefs_get | [crates/kanzei-app/src/prefs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/prefs.rs:216>) | 是 | 1 |
| ui_prefs_set | [crates/kanzei-app/src/prefs.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/prefs.rs:236>) | 是 | 1 |
| preview_open | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:17>) | 是 | 1 |
| preview_set_bounds | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:29>) | 是 | 1 |
| preview_set_visible | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:41>) | 是 | 1 |
| preview_nav | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:50>) | 是 | 1 |
| preview_close | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:55>) | 是 | 2 |
| preview_capture | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:61>) | 是 | 1 |
| preview_console | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:72>) | 是 | 1 |
| preview_console_clear | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:83>) | 是 | 1 |
| preview_device | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:91>) | 是 | 1 |
| preview_pick | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:113>) | 是 | 1 |
| preview_snippet | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:118>) | 是 | 1 |
| preview_dev_urls | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:124>) | 是 | 1 |
| preview_clear_site_data | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:136>) | 是 | 1 |
| preview_open_devtools | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:141>) | 是 | 1 |
| preview_open_external | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:148>) | 是 | 1 |
| tool_image | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:167>) | 是 | 1 |
| delivered_image | [crates/kanzei-app/src/preview/commands.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/preview/commands.rs:175>) | 是 | 1 |
| worktree_gate | [crates/kanzei-app/src/processes/gate.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/gate.rs:122>) | 是 | 1 |
| worktree_post_merge_gate | [crates/kanzei-app/src/processes/gate.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/gate.rs:137>) | 是 | 1 |
| list_pending_inputs | [crates/kanzei-app/src/processes/lifecycle.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/lifecycle.rs:31>) | 是 | 1 |
| cancel_input | [crates/kanzei-app/src/processes/lifecycle.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/lifecycle.rs:48>) | 是 | 1 |
| process_list | [crates/kanzei-app/src/processes/lifecycle.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/lifecycle.rs:76>) | 是 | 1 |
| process_create | [crates/kanzei-app/src/processes/lifecycle.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/lifecycle.rs:103>) | 是 | 2 |
| process_update | [crates/kanzei-app/src/processes/lifecycle.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/lifecycle.rs:402>) | 是 | 2 |
| process_close | [crates/kanzei-app/src/processes/lifecycle.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/lifecycle.rs:497>) | 是 | 1 |
| worktree_create | [crates/kanzei-app/src/processes/workspace.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/workspace.rs:212>) | 是 | 0 |
| worktree_list | [crates/kanzei-app/src/processes/workspace.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/workspace.rs:232>) | 是 | 1 |
| worktree_diff | [crates/kanzei-app/src/processes/workspace.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/workspace.rs:272>) | 是 | 1 |
| worktree_merge_preview | [crates/kanzei-app/src/processes/workspace.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/workspace.rs:291>) | 是 | 1 |
| worktree_merge | [crates/kanzei-app/src/processes/workspace.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/workspace.rs:306>) | 是 | 1 |
| worktree_discard | [crates/kanzei-app/src/processes/workspace.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/workspace.rs:415>) | 是 | 1 |
| worktree_harvest_candidates | [crates/kanzei-app/src/processes/workspace.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/workspace.rs:499>) | 是 | 1 |
| worktree_harvest_writeback | [crates/kanzei-app/src/processes/workspace.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/processes/workspace.rs:520>) | 是 | 1 |
| projects_create | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:156>) | 是 | 1 |
| project_git_init | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:183>) | 是 | 1 |
| project_facts | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:200>) | 是 | 1 |
| projects_get | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:212>) | 是 | 2 |
| projects_init | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:232>) | 是 | 0 |
| projects_rename | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:257>) | 是 | 1 |
| projects_add | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:272>) | 是 | 0 |
| project_root_info | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:316>) | 是 | 1 |
| projects_isolation_report | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:326>) | 是 | 1 |
| project_detach | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:349>) | 是 | 1 |
| projects_pick | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:370>) | 是 | 1 |
| project_files | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:419>) | 是 | 1 |
| export_pick_dir | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:431>) | 是 | 2 |
| export_project_data | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:502>) | 是 | 1 |
| projects_remove | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:561>) | 是 | 1 |
| projects_select | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:573>) | 是 | 1 |
| workspace_snapshot | [crates/kanzei-app/src/projects.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/projects.rs:684>) | 是 | 1 |
| research_workflow_get | [crates/kanzei-app/src/research_auto.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_auto.rs:13>) | 是 | 1 |
| research_workflow_start | [crates/kanzei-app/src/research_auto.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_auto.rs:21>) | 是 | 1 |
| research_workflow_update | [crates/kanzei-app/src/research_auto.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_auto.rs:31>) | 是 | 1 |
| research_latex_templates | [crates/kanzei-app/src/research_latex.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_latex.rs:191>) | 是 | 1 |
| research_latex_create | [crates/kanzei-app/src/research_latex.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_latex.rs:203>) | 是 | 1 |
| research_latex_insert_figure | [crates/kanzei-app/src/research_latex.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_latex.rs:298>) | 是 | 1 |
| research_latex_compile | [crates/kanzei-app/src/research_latex.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_latex.rs:392>) | 是 | 1 |
| research_latex_history | [crates/kanzei-app/src/research_latex.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_latex.rs:456>) | 是 | 1 |
| research_latex_pdf | [crates/kanzei-app/src/research_latex.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_latex.rs:524>) | 是 | 1 |
| research_library_list | [crates/kanzei-app/src/research_library.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_library.rs:229>) | 是 | 1 |
| research_library_create | [crates/kanzei-app/src/research_library.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_library.rs:269>) | 是 | 1 |
| research_library_link_projects | [crates/kanzei-app/src/research_library.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_library.rs:301>) | 是 | 1 |
| research_topic_create | [crates/kanzei-app/src/research_topics.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/research_topics.rs:72>) | 是 | 0 |
| app_info | [crates/kanzei-app/src/run/mod.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/run/mod.rs:50>) | 是 | 1 |
| settings_get | [crates/kanzei-app/src/settings.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/settings.rs:557>) | 是 | 3 |
| settings_save | [crates/kanzei-app/src/settings.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/settings.rs:698>) | 是 | 1 |
| settings_open | [crates/kanzei-app/src/settings.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/settings.rs:772>) | 是 | 1 |
| permission_rules_get | [crates/kanzei-app/src/settings.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/settings.rs:791>) | 是 | 1 |
| permission_rule_delete | [crates/kanzei-app/src/settings.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/settings.rs:807>) | 是 | 1 |
| provider_test | [crates/kanzei-app/src/settings.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/settings.rs:825>) | 是 | 1 |
| ui_probe_result | [crates/kanzei-app/src/state.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/state.rs:114>) | 是 | 1 |
| quick_req | [crates/kanzei-app/src/subagents.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/subagents.rs:21>) | 是 | 1 |
| idea_split | [crates/kanzei-app/src/subagents.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/subagents.rs:176>) | 是 | 1 |
| defect_review | [crates/kanzei-app/src/subagents.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/subagents.rs:661>) | 是 | 1 |
| update_check | [crates/kanzei-app/src/update.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/update.rs:191>) | 是 | 2 |
| update_install | [crates/kanzei-app/src/update.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/update.rs:240>) | 是 | 1 |
| voice_settings_get | [crates/kanzei-app/src/voice.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/voice.rs:45>) | 是 | 1 |
| voice_settings_set | [crates/kanzei-app/src/voice.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/voice.rs:57>) | 是 | 1 |
| voice_status | [crates/kanzei-app/src/voice.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/voice.rs:83>) | 是 | 0 |
| voice_start | [crates/kanzei-app/src/voice.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/voice.rs:109>) | 是 | 2 |
| voice_cancel | [crates/kanzei-app/src/voice.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/voice.rs:171>) | 是 | 1 |
| voice_speak | [crates/kanzei-app/src/voice.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/voice.rs:193>) | 是 | 1 |
| voice_transcribe | [crates/kanzei-app/src/voice.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/src/voice.rs:270>) | 是 | 1 |

## 前端字面量调用未匹配后端声明

| 调用 | 位置 |
|---|---|
| run_tool_process_stop | [crates/kanzei-app/ui/06-activity.js](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/06-activity.js:413>) |

## SQL 表声明

| 表名 | 位置 |
|---|---|
| schema_meta | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:13>) |
| sessions | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:49>) |
| session_events | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:57>) |
| session_inputs | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:78>) |
| agent_notifications | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:91>) |
| delivery_cursors | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:101>) |
| mobile_devices | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:110>) |
| episodes | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:116>) |
| recall_events | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:143>) |
| memory_recoveries | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:164>) |
| memory_sources | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:173>) |
| memory_eval | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:181>) |
| memory_eval_agg | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:202>) |
| processes | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:220>) |
| retired_processes | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:237>) |
| work_events | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:244>) |
| work_surfaces | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:256>) |
| research_runs | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:267>) |
| research_environment_leases | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:293>) |
| research_run_events | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:302>) |
| file_checkpoints | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:313>) |
| session_inputs | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:407>) |
| processes | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:892>) |
| processes | [crates/kanzei-core/src/store/schema.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-core/src/store/schema.rs:941>) |
| memory_vectors | [crates/kanzei-memory/src/memory/index.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/index.rs:167>) |
| memory_hits | [crates/kanzei-memory/src/memory/store.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/store.rs:1095>) |
| memory_recalls | [crates/kanzei-memory/src/memory/store.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/store.rs:1101>) |
| novelty_events | [crates/kanzei-memory/src/memory/store.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/store.rs:1119>) |
| manager_decisions | [crates/kanzei-memory/src/memory/store.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/store.rs:1126>) |
| recurrence_counts | [crates/kanzei-memory/src/memory/store.rs](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-memory/src/memory/store.rs:1134>) |
