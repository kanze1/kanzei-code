# Goals

## G-001 把 kanzei 打磨成日常主力开发工具:清空 open 缺陷,推进 R-003 持久化与 R-012 子代理 [active]
- 进展: 完成 R-003 queue/steer drain 的一个关键收尾：修复多个 pending steer 被批量标记 promoted 却只消费第一条的问题，改为每次逐条 FIFO 提升，并新增回归测试。cargo test --workspace 全部通过。已提交 91d3f2b；同时将此前已通过验证的 SubagentRuntime timeout_secs 改动提交为 d59ad72。当前 R-003 仍剩运行中 admission/drain 竞态与端到端覆盖，未提前标记完成。
- 执行顺序: 按需求编号从小到大，遵循依赖关系；当前继续 R-003，完成后进入后续未完成需求，直至所有可实施需求完成。新增 R-027：需求分析沟通模式与缺陷查找入口，排在现有需求之后。
- 目标调整: 改为按照需求编号顺序推进并完成全部需求：优先完成当前 doing 需求，再按 R-003、R-004……R-027 顺序处理 todo/未完成需求；已 done 保持，仅 dropped 不恢复。
