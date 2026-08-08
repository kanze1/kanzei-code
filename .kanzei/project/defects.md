# Defects

## D-156 加了 OpenAI 兼容 provider 却选不出任何模型 [fixed] (high)
- 复现: 2026-08-08 用户按指引在设置页添加 deepseek(protocol=openai, base_url=https://api.deepseek.com/v1, api_key_env=DEEPSEEK_API_KEY),顶栏「模型」下拉里一个 deepseek 模型都没有,只有 primary/fast 两个角色项。
- 根因: `models_list` 只硬编码枚举四种情况——primary/fast 角色、`auth="codex"`(3 个写死型号)、`auth="claude"`(3 个写死型号)、`base_url` 含 11434 的 Ollama(查 /api/tags)。**其余 provider 直接落到分支尾部,贡献 0 个模型**。而配置层是完全开放的:任何 OpenAI 兼容端点都能配进去。于是"能配 provider"与"能用 provider"之间断了一环,DeepSeek/OpenRouter/Kimi/自建 vLLM 全中招。
- 影响: provider 配置形同虚设——配好了、连通性测试也过,就是没法在界面上选中它的模型。用户只能去改 kanzei.toml 的 `[models]` 硬指,顶栏下拉这条主路径不通。
- 验收: ①protocol 为 openai / openai-responses 的 provider 走标准 `GET {base_url}/models` 探测,带上 api_key(直填优先于环境变量),遵循全局代理设置;②探测失败静默跳过,不阻断其余 provider 的列举——端点可能没实现 /models,或 key 尚未配好;③提供手填兜底「＋ 手填模型…」,输入 `provider:model` 直指,校验格式后落盘并持久留在下拉里;④Ollama 仍走原生 /api/tags(它的 /v1/models 不全),抽成 `push_ollama_models` 避免两处重复。
- 优先级: P1
- 阶段: 3
- 不变量: 配置:能配进来的 provider 就必须能在界面上用起来
- 证据等级: E2
- 备注: 落地位置 crates/kanzei-app/src/main.rs(models_list 新增 openai 分支 + push_ollama_models)、ui/main.js(手填入口与持久化)。冒烟新增 4 项断言:手填入口存在、落盘、回到下拉、非法格式被挡。
- refs: R-115
- 标签: 模型

## D-159 memory-manager 忽略前置 pathspec fatal 并把 commit 症状误记为根因 [open] (medium)
- refs: R-105
- 优先级: P2
- 复现: 一次 `git add` 因文件名大小写/截断不匹配报 pathspec，随后 `git commit` 因无暂存内容退出 1。自动 memory-manager 生成 M-013，标题断言“Changes not staged 表示没有暂存内容”，正文进一步把根因泛化为忘记 git add；但本次真实根因是前置 git add 的 pathspec 不存在。
- 影响: 记忆把症状误当根因，未来遇到同类输出会错误建议再次 git add，而不检查前置 add 是否因 pathspec/权限失败；属于会诱导重复失败的错误长期事实。
- 标签: 核心
- 根因: 失败归纳只消费了批次末尾 `git commit` 输出，没有关联同一 bash 调用前面的 `fatal: pathspec ... did not match any files`，跨命令因果被截断。
- 证据等级: E1
- 验收: M-013 被更正或标 stale，不再声称本次根因是忘记暂存；失败提炼能优先保留同一 bash 调用中更早的 fatal/pathspec 根因，或在无法判定时只记录症状不下根因结论；有回归覆盖。

- 进展: 错误 M-013 仍处于未提交状态；已向 memory inbox 投递具名更正说明，后续修复需让 failure harvest 保留同批前置 `fatal: pathspec` 根因并补回归。本轮不把错误记忆混入 R-069 提交。

