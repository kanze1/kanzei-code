# Goals

## G-001 把 kanzei 打磨成日常主力开发工具:清空 open 缺陷,推进 R-003 持久化与 R-012 子代理 [active]
- 进展: R-007 已完成 Claude OAuth 自动刷新：接入 console.anthropic.com/v1/oauth/token，过期前 5 分钟刷新并回写凭证，cargo test --workspace 全部通过。当前仅剩真实 Claude Code 端到端验证；R-010 继续保持 doing，未新增需求。
- 执行顺序: 按需求编号从小到大，遵循依赖关系；当前继续 R-003，完成后进入后续未完成需求，直至所有可实施需求完成。新增 R-027：需求分析沟通模式与缺陷查找入口，排在现有需求之后。
- 目标调整: 改为按照需求编号顺序推进并完成全部需求：优先完成当前 doing 需求，再按 R-003、R-004……R-027 顺序处理 todo/未完成需求；已 done 保持，仅 dropped 不恢复。
