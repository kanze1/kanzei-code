# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 继续推进 R-050：发现并修复运行闸门路径规范化缺陷（D-046）。run_prompt、stop_run 与 session_id 现在统一使用 canonical 项目根路径，等价相对路径/子目录不会再错误跨项目；新增回归测试。随后修复 D-031 模式选择刷新丢失与 D-045 长字段展开布局，完成模式偏好持久化和详情换行边界。cargo test -p kanzei-app 6 项通过，前端语法与 diff 检查通过。全局运行闸门仍保留，真实双会话并行测试与运行态隔离仍是下一步。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-050 本轮修复 D-046：运行闸门、停止边界和 session_id 统一 canonical 项目根路径，补充等价路径测试；并完成 D-031、D-045 两项前端缺陷修复。app 6 项测试、tools 12 项测试和前端语法检查通过。R-050 尚未完成真实多会话并行，R-059 继续排队。
