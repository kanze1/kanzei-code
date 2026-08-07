# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮完成 R-030、R-050、R-059、R-065、R-067~R-082 及 D-033、D-038、D-043、D-047 的实现、验证和归档：进程/项目会话隔离、工作树生命周期、通知持久化与本机桥接、受控子代理容器、有限重试、前端状态与归档能力均已接入。需求和缺陷清单当前没有开放项；G-002 已达成。长期目标继续作为演进方向保留 active。

## G-002 前端与后端能力对齐 [achieved]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-030/R-050/R-059 及本轮 R-065、R-067~R-082 均已完成实现和验证；前端进程、工作树、通知、测试归档、重试与文档能力已与后端契约对齐。
- 验证: cargo test --workspace；node --check crates/kanzei-app/ui/main.js；git diff --check。
