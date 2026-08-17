# Findings

## F-001 B4 研究结论可经受限 tracker 权限回流为草稿 [draft]
- V等级: V1
- 文献证据深度: 不适用：本结论属于代码域，不作文学/论文证据主张。
- 结论: B4 回流通道已在 research profile 形成受限的可追溯链路：source/finding 工具可登记研究证据，finding 强制引用 SOURCES；req/defect 仅允许 get 与 add，所有 update/close/archive/reorder/reopen/修复/normalize 等既有条目变更动作被 managed hard deny。research/docs 还把 Sources/Findings 与只读 backlog 注入上下文，并明确 req/defect add 产出 [todo] 草稿。
- 证据域: 代码域
- 证据锚: crates/kanzei-tools/src/profiles.rs:609-629（source/finding 注册与 finding→SOURCES refs）；643-671（source/finding/req/defect 仅 get/add，既有状态变更硬拒绝）；674-685（req/defect 研究工具注册）；715-743（Sources/Findings/backlog 上下文与 [todo] 回流契约）；设计对照 docs/design/research_mode.md:32-37,95-100,103-109,120-130；提交锚 ecfdca5b
- 验收: 研究会话可读取既有 R-/D- 条目并登记一条 [todo] 草稿，同时不能修改既有条目状态；finding 必须有 source refs。
- refs: S-001 S-002 S-003

## F-002 B5 research 记忆统一走 memory 工具并停用历史账本 [draft]
- V等级: V1
- 文献证据深度: 不适用：本结论属于代码域，不作文学/论文证据主张。
- 结论: B5 记忆一元化已在 research profile 的工具面与上下文契约中落地：research 只注册并放行统一 memory_search/memory_note；research/docs 明示历史 .kanzei/research/memory.md 不是记忆来源，research agent 提示词也重复该禁用约束。设计基线同时规定研究结论经 memory_note→manager 晋升，而论文 refs.bib 与记忆晋升分离；本次读码证据确认了入口和禁止历史注入，不宣称已完成 manager 运行时晋升实测。
- 证据域: 代码域
- 证据锚: crates/kanzei-tools/src/profiles.rs:631-642（memory_search/memory_note 注册与放行）；715-743，尤其 735-741（统一记忆指导、历史 memory.md 禁止注入）；746-763（research agent 的 B5 提示词）；设计对照 docs/design/research_mode.md:32-37,95-100,113-130；提交锚 ecfdca5b（R-221 B5 提交标题）
- 验收: research 档存在统一 memory_search/memory_note 入口，历史 research/memory.md 不进入研究上下文；manager 晋升与运行时效果需另行实测，不能由本次 V1 读码替代。
- refs: S-001 S-002 S-003
