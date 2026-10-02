# Harness 静态分析报告

日期：2026-08-08
范围：`crates/kanzei-harness`、`crates/kanzei-core`、`crates/kanzei-tools` 的权限门禁、工具资源声明、agent 选择、子代理和 runner 调度。

> **时效声明与发现落地映射(2026-08-08 整理时补)**:
> 本报告是 2026-08-08 的一次性静态快照,只记录发现、不保证后续实现状态。以下为已确认落地的映射(以 `.kanzei/project/defects.md` 为准);未列出的发现请按 `subagent_management.md` 的 P0 实施顺序核对是否已登记缺陷:
>
> | 本报告发现 | 对应缺陷 | 状态 |
> |---|---|---|
> | H-003 路径权限未统一解析项目边界 | D-050 | fixed(Windows 大小写/盘符/UNC 规范化) |
> | M-004 bash AlwaysAllow 过度泛化且不含工作目录 | D-051(另 D-122/D-139 后续) | fixed(结构化 bash 资源、取消首词泛化) |
> | M-006 task 并发数没有上限 | D-033 | fixed(每轮最多 8 个) |

## 结论摘要

当前权限体系具备统一的 Ruleset 和 runner 门禁，但“工具声明的资源”仍是信任边界：执行层没有验证资源是否覆盖真实副作用，且部分工具返回空资源或过宽资源。因此后续修复应优先围绕路径边界、工具资源契约和最后一步工具硬拦截展开。

本报告只记录静态发现，不改变运行语义；每项后续修复都应先补回归测试，再单独登记缺陷。

## 高风险发现

### H-001 工具注册键与 action 不一致可能绕过物化阶段 deny

- 位置：`crates/kanzei-harness/src/harness.rs` 的工具物化逻辑、`src/tool.rs` 的 `Tool::action()`。
- 现象：物化工具时按注册表键判断是否整体 deny，运行时却按 `Tool::action()` 做权限判断。自定义工具使用 alias 注册时，两者可能不一致。
- 影响：被 deny 的工具可能仍进入模型工具 schema。
- 建议：注册时强制键等于 `Tool::name()`，或物化阶段统一使用 `tool.action()` 并拒绝重复/错配名称。

### H-002 空 resources 可跳过全部权限询问

- 位置：`crates/kanzei-harness/src/tool.rs`、`crates/kanzei-core/src/runner.rs` 权限循环。
- 现象：runner 只遍历 `Tool::resources()` 返回值；自定义工具返回空数组时不执行 Ruleset 评估。
- 影响：工具可以在没有任何权限评估的情况下执行。
- 建议：空资源默认映射为 `*` 并进入 ask/deny，或注册时拒绝空资源；对高风险工具要求强制资源提取。

### H-003 路径权限未统一解析项目边界

- 位置：`crates/kanzei-tools/src/write.rs`、`edit.rs`、`bash.rs`、`profiles.rs`。
- 现象：工具允许绝对路径和 `..`，权限规则主要使用字符串通配；符号链接真实目标也未统一校验。
- 影响：项目内允许/拒绝规则可能被路径穿越、兄弟目录或符号链接绕过。
- 建议：执行前统一 canonicalize/规范化路径，校验目标位于 `project_root` 或明确允许目录；写入目标额外检查符号链接。

### H-004 glob/grep 的扫描资源不能表达实际 path

- 位置：`crates/kanzei-tools/src/glob.rs`、`grep.rs`、`crates/kanzei-harness/src/tool.rs`。
- 现象：扫描工具没有充分实现 `resources()`，权限层只能看到过宽的资源（或 `*`），无法按实际搜索目录限制。
- 影响：针对路径的 deny 规则不能阻止模型扫描受保护目录。
- 建议：资源声明包含规范化搜索根和路径参数；递归扫描还要校验所有覆盖目录。

## 中风险发现

### M-001 显式选择 agent 未校验 profile/mode

- 位置：`crates/kanzei-harness/src/harness.rs` 的 `select_agent(Some(name))`。
- 现象：显式名称选择只按名称查找，不检查 agent 是否属于当前 profile，也不拒绝 `Subagent` 模式作为主 agent。
- 影响：可选择 research-only 或 subagent agent，造成工具集合和权限边界错配。
- 建议：显式选择与默认选择使用同一 profile 校验；主运行入口拒绝 `AgentMode::Subagent`。

### M-002 frontmatter 非法字段静默回退默认值

- 位置：`crates/kanzei-harness/src/markdown.rs`。
- 现象：非法 profile/mode/steps 被当成缺省值处理，格式错误不会显式暴露。
- 影响：拼写错误可能扩大 agent 可用 profile 或改变 steps 限制。
- 建议：区分字段缺失和字段非法；非法值返回解析错误或至少记录 warning。

### M-003 最后一步未在执行层硬拦截工具调用

- 位置：`crates/kanzei-core/src/runner.rs` 的 `last_step` 分支。
- 现象：最后一步请求移除了工具 schema，但 provider 仍返回 ToolCall 时，后续仍可能进入权限检查和执行。
- 影响：违反“最后一步工具不可用”契约，可能产生写入、命令或联网副作用。
- 建议：`last_step` 下对所有 ToolCall 直接生成错误结果或终止，不得进入 `tool.execute()`。

### M-004 bash AlwaysAllow 过度泛化且不含工作目录

- 位置：`crates/kanzei-tools/src/bash.rs`、`crates/kanzei-harness/src/config.rs`。
- 现象：`git status` 等命令按首词泛化为 `git *`，资源不包含 workdir；解释器/脚本宿主会扩大到任意参数和目录。
- 影响：用户一次确认可能扩大为任意参数、任意目录下的命令执行。
- 建议：资源包含工作目录；解释器、脚本宿主和网络命令默认不自动泛化，必要时只记精确命令。

### M-005 子代理只读约束依赖调用方构造

- 位置：`crates/kanzei-core/src/runner.rs`、`crates/kanzei-tools/src/subagent.rs`。
- 现象：`SubagentRuntime` 可持有外部传入 snapshot，runner 未再次验证工具白名单。
- 影响：未来误用 API 时子代理可能获得写入/bash/联网工具。
- 建议：构造子代理 runtime 时强制过滤为 read/glob/grep，并验证权限结果。

### M-006 task 并发数没有上限

- 位置：`crates/kanzei-core/src/runner.rs` task 并发执行逻辑。
- 现象：同轮返回的大量 task 会全部创建并发请求。
- 影响：连接、内存、模型调用和超时预算可能被耗尽。
- 建议：增加每轮 task 数量上限、全局 semaphore、累计任务预算和超时预算。

## 低风险/设计风险

- Markdown frontmatter 起始分隔符未闭合时缺少明确错误：`crates/kanzei-harness/src/markdown.rs`。
- Ruleset 通配匹配对超长、多星号输入存在 CPU 可用性风险：`crates/kanzei-harness/src/permission.rs`。
- Harness snapshot 保护的是注册结构，不保证外部捕获状态的行为不可变；需要明确快照语义。
- AlwaysAllow 写配置失败时，本轮 session rule 仍可能继续放行；应区分“本轮放行”和“持久化成功”。

## 测试覆盖缺口

现有测试覆盖了基础 Ruleset 通配、last-match-wins、配置层合并、工具输入修复和文档存储，但缺少：

1. 注册键/action 不一致时的拒绝测试；
2. 空 resources 的硬门禁测试；
3. `..`、绝对路径、符号链接和跨项目路径测试；
4. glob/grep 实际搜索根的权限测试；
5. 显式 agent 的 profile/mode 校验测试；
6. runner 最后一步仍返回 ToolCall 时不执行工具的测试；
7. task 数量/并发预算测试；
8. AlwaysAllow 持久化失败和并发写配置测试。

## 建议实施顺序

1. H-002、H-003：先堵空资源和路径边界，补硬门禁回归测试；
2. M-003：最后一步工具调用硬拦截；
3. H-001、M-001：统一工具注册和 agent 选择契约；
4. H-004、M-004、M-005：完善资源声明与子代理隔离；
5. M-006：限制 task 并发与总预算；
6. 最后处理 frontmatter 错误可见性、匹配器性能和 AlwaysAllow 原子持久化。
