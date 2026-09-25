# Claude Code / Codex 能力对照与对齐清单(复刻清单 v1)

- 状态:设计基线(三方对照、用户筛选与接口定义已完成;实施条目 R-364~R-367、R-369~R-377 与 D-748、D-751 已登记,实施地图见 cc_codex_alignment_impl_maps.md)
- 日期:2026-09-25
- 关联需求:R-245(工具结果外置)、R-312(上下文减负)、R-322/R-323(模型自治/编排抽象)、R-281(子代理对话读取)
- 关联缺陷:D-662(工具面膨胀)
- 关联决策:A-007(可替代区复刻优先)
- 对照版本:Claude Code v2.1.282(CHANGELOG 最新条目,发布日期未核实)/ OpenAI Codex `rust-v0.157.0`(2026-09-25 发布)
- 来源:
  - CC 工具 schema 取自 2026-09-25 本会话实际加载的 Claude Code 工具定义(一手);其余 CC 机制取自 code.claude.com/docs 与 CHANGELOG。
  - Codex 取自 `openai/codex` 仓库 `rust-v0.157.0` 源码(以源码为准,文档与源码冲突处按源码),辅以 learn.chatgpt.com/docs。
  - kanzei 现状取自本仓只读勘察(file:line 见正文)与 `.kanzei/state.db` 实测。

## 背景与问题

复刻清单 v0(`direction_taste.md`,2026-08-08)只以 CC 为参照,并写明「随 CC 演进复核」。一个半月里两件事变了:

1. CC 与 Codex 都大幅演进,而且**开始互相收敛**——Codex 的 hooks 直接照抄了 CC 的事件名、JSON 契约与 matcher 别名(`Bash`/`Edit`/`Write`/`Agent`),还能 `/import` CC 的 `CLAUDE.md`、agents、commands 与 memory。
2. kanzei 的主力模型是 `codex:gpt-5.6-*`,Codex 的 harness 就是这批模型的训练环境,不再只是「另一个竞品」。

两家都做了的东西是强信号;两家都在撤的东西是反向信号。本文把两家与 kanzei 现状放进同一张表,经用户逐项筛选,落成接口定义。

## 目标与非目标

- 目标:
  - 给出 CC 与 Codex 的差异与 kanzei 现状。
  - 记录用户的筛选结论。
  - 把保留项落成可以直接照着实现的接口定义:模型可见的工具 schema,以及非工具的交互契约。
- 非目标:不重议护城河区(记忆系统、追踪状态机、鞭挞与验收打假、并行线);实施批次在 §七 给草案,登记时再按最新编号落 tracker。

## 筛选判据

沿用 A-007 并叠加本仓已定约束。用户本轮补了一条总口径:**「总的来说还是第一性原理」**——两家都有不等于要做,按他自己实际用不用来定。

| 判据 | 来源 | 作用 |
| --- | --- | --- |
| 用户实际使用频率 | 2026-09-25 定调 | 高频必做;低频即使两家都有也不做 |
| 单人自用 / 桌面+CLI / 中文优先 / 服务自举 | A-007 | 不满足就丢:企业治理、marketplace、云端任务、多人协作一律不做 |
| 常驻工具面不涨 | D-662 | 新能力优先挂在已有工具的参数上,或放进延迟加载层 |
| 不做事前语法闸门 | 威胁模型 | OS 沙箱、命令分类器、审查代理不做,防线放在结果侧的快照与回滚 |
| 任务判断归模型,资源判断归引擎 | R-322 | 对齐时不许借机把任务判断收回引擎 |

## 一、CC 与 Codex 的核心差异

| 维度 | Claude Code v2.1.282 | Codex 0.157.0 |
| --- | --- | --- |
| 工具调用形态 | 直接函数调用,可并行 | **code mode**:当前全部模型都是 `code_mode_only`。模型只调一个 freeform 工具 `exec`,在 V8 里写 JS,再由 JS 调 `tools.exec_command()` / `tools.apply_patch()`;嵌套工具以 TypeScript 声明写进 `exec` 的描述;并行靠 `Promise.allSettled`,`parallel_tool_calls=false` |
| 编辑原语 | `Edit` 字符串替换(必须先 Read、`old_string` 唯一)+ `Write` | 只有 `apply_patch`:freeform + Lark 语法,多文件,add/delete/move/update,上下文三遍模糊匹配 |
| Shell | `Bash`/`PowerShell` 一次性执行 + 后台运行 + `Monitor` | `exec_command` + `write_stdin`:PTY 会话,`yield_time_ms` 让出,可向运行中的进程写 stdin;经典的 `shell` 工具已删 |
| 专用读/搜工具 | `Read`/`Grep`/`Glob` 常驻 | **已删**(`read_file`/`list_dir`/`grep_files`),读和搜都走 shell,另有 `view_image` |
| 工具发现 | `ToolSearch` 延迟加载:连 WebFetch/WebSearch 这类工具都只常驻名字 | 原生 `tool_search`(BM25 检索,默认 limit 8),MCP 工具默认延迟加载 |
| 权限与安全 | 权限模式 + 规则 + bash 沙箱 + auto 模式分类器 | **OS 级沙箱为主**(macOS Seatbelt、Linux bwrap+seccomp、Windows 受限令牌或专用沙箱用户+WFP)+ 审批策略 + Starlark `prefix_rule` + Guardian 审查代理 |
| 子代理 | `Agent`(类型、worktree 隔离、默认后台)+ `SendMessage` 续聊 + Workflow 脚本编排 | v2:`spawn_agent`/`send_message`/`followup_task`/`wait_agent`/`interrupt_agent`/`list_agents`,路径式寻址,默认 4 线程 |
| 计划/todo | `TaskCreate` 一族 + plan 模式 | **`update_plan` 自 0.152.0 起默认关**;plan 协作模式 |
| 目标 | `/goal` | `create_goal`/`update_goal`/`get_goal`,带 token 预算 |
| 回退 | `/rewind` 检查点(对话+文件)、`/branch` 分叉 | **`/undo` 与 ghost commit 已删**;Esc Esc 只回退对话;`/fork` 分叉 |
| 上下文 | 自动压缩 + `/compact [焦点]` + `/context` + 1M 窗口 | 服务端加密压缩;实验性 `new_context`;动态状态以 world-state diff **追加**,从不改写已缓存的前缀;请求带 `prompt_cache_key` |
| 网页 | `WebSearch`(服务端)+ `WebFetch{url, prompt}`(小模型按问题提取) | `web.run`:批量查询、recency/domains 过滤、`open` 按行号翻页、`find` 页内查找、`click` 跟链接、PDF 页截图,附严格引用规则 |
| 指令文件 | `CLAUDE.md` 层级 + auto-memory | `AGENTS.md`(根→cwd 逐层,32 KiB 预算,每次请求刷新) |
| 扩展 | hooks(26 事件)、skills、plugins、MCP | hooks(12 事件,照抄 CC)、skills、plugins、MCP |
| 定时 | `/loop`、`CronCreate`、桌面定时任务、云端 routines,四套并存 | 桌面 automations、事件触发任务、`codex queue` |

## 二、收敛信号与撤退信号

**两家收敛的方向**:

- 工具延迟加载。
- hooks。
- commands 并进 skills。
- 子代理可以续聊、后台跑、放进 worktree 隔离。
- 目标驱动 loop。
- worktree。
- 会话分叉。
- 运行中插话。
- 手动压缩。

**撤退的方向**:

- 计划/todo 工具:Codex 已默认关,kanzei 已删。
- 文件级撤销:Codex 删了;CC 仍保留,用户选了 CC 的做法。
- 专用读/搜工具:Codex 删了,kanzei 不跟,见 §六 C 档。
- 把 agent 暴露成 MCP server。
- 自定义 agent 可覆盖的字段:Codex 收窄了。

## 三、工具接口对照(现状)

| 能力 | Claude Code(本会话实载) | Codex(源码) | kanzei 现状 |
| --- | --- | --- | --- |
| 读 | `Read{file_path, offset?, limit?, pages?}`,默认 2000 行,支持图片、PDF、ipynb | 无专用读工具;`view_image{path, detail?}` | `read{path, offset, limit, tail, cells, pages}`,2000 行,256 KiB 上限 |
| 写 | `Write{file_path, content}`;覆盖已有文件前必须先 Read | `apply_patch` 的 Add File | `write{path, content}`,无先读要求 |
| 改 | `Edit{file_path, old_string, new_string, replace_all?}`;必须先 Read | `apply_patch`(多文件、三遍模糊匹配) | `edit{path, old_string, new_string, replace_all, allow_deletion}`,带模糊回退与守卫;`insert{path, anchor, content, position}` |
| 搜 | `Grep{pattern, path, glob, type, output_mode, -A/-B/-C, -i, -n, -o, head_limit=250, offset, multiline}`;`Glob{pattern, path}` | 无;在 shell 里跑 `rg` | `grep{pattern, path, glob, limit=50, files_only, count, case_insensitive, context, multiline}`;`glob`;`files`;`symbols` |
| Shell | `Bash{command, description, timeout≤600s, run_in_background, dangerouslyDisableSandbox}`;内联输出约 3 万字符 | `exec_command{cmd, workdir, tty, yield_time_ms, max_output_tokens, …}` + `write_stdin`;输出 1 万 tokens | `bash{command, timeout_ms, workdir, background, persistent}`;每路流捕获 1 MiB |
| 子代理 | `Agent{description, prompt, subagent_type, model, isolation, run_in_background}`;`SendMessage{to, message}` | `spawn_agent{task_name, message, fork_turns, model, reasoning_effort, agent_type}` + `send_message`/`followup_task`/`wait_agent`/… | `task{prompt, model, schema, agent: explore/plan}`:只读、一次性、不能续聊 |
| 问用户 | `AskUserQuestion{questions[1-4]{question, header≤12, options[2-4]{label, description, preview?}, multiSelect}}` | `request_user_input{questions[{id, header, question, options[{label, description}]}]}` | `question{question, options[], default, multiple}`,一次一个问题 |
| 工具发现 | `ToolSearch{query: "select:A,B" 或关键词, max_results=5}` | `tool_search{query, limit=8}` | 无 |
| 网络 | `WebFetch{url, prompt}`、`WebSearch{query, allowed_domains, blocked_domains}` | `web.run{search_query[], open[], find[], click[], screenshot[], …}` 或托管 `web_search` | `webfetch{url, max_chars}`;`websearch{query, max_results}` 抓 DuckDuckGo HTML 页 |
| 交付文件 | `SendUserFile{files, caption, status, display}` | `send_message_to_user_async` | `deliver{path, caption}`(仅桌面) |

## 四、三方矩阵与结论

表中 kanzei 的状态分四档:「达标」、「部分」、「缺」、「反超」。

| 领域 | CC | Codex | kanzei | 结论 |
| --- | --- | --- | --- | --- |
| 工具延迟加载 | 有 | 有 | 缺:工具 schema 占每步注入的 48.6% | **做**(§5.1) |
| 工具结果上限 | 约 3 万字符 | 1 万 tokens | 部分:超过 1 MiB 才外置 | **做**(§5.7) |
| 先读后写 | 有 | 由 patch 上下文隐式保证 | 缺 | **做**(§5.6) |
| 运行中插话 | 有 | 有 | 部分:要等 run 结束 | **做**(§5.8) |
| 项目指令文件 | `CLAUDE.md` | `AGENTS.md` | 缺 | **做**(§5.8) |
| 手动压缩 | 有 | 有 | 缺,且压缩依赖子代理 | **做**(§5.8) |
| 缓存稳定 | 追加式注入 | 追加式 + `prompt_cache_key` | 疑似缺 | **做,先测量**(§5.8) |
| 网页搜索 | 服务端搜索 + 提取式抓取 | 富导航 + 引用 | 抓 DuckDuckGo HTML 页 | **做,重点**(§5.5) |
| 回退 | 对话 + 文件 | 只回退对话 | 缺 | **做,照 CC**(§5.9) |
| 子代理 | 全套 | v2 全套 | 部分 | **做四项**(§5.3) |
| 问题批量 | 1–4 个 | 多个 | 一次一个 | **做**(§5.4) |
| 定时任务 | 四套并存,复杂 | automations | 缺 | **做,但一个概念、一个面板**(§5.10) |
| 会话分叉 | 有 | 有 | 缺 | **后做**,轻量版(§5.11) |
| hooks / skills 接线 / MCP | 有 | 有 | 缺 / 只列清单 / 缺 | **不做**:用户很少用;commands 注册表删掉(§5.12) |
| plan 模式 | 有 | 有 | 部分 | **不做**:用户说模型强了之后基本不用 |
| 编辑原语按模型切换 | — | apply_patch | — | **不做**:gpt-5.6-luna 的 edit 未命中率只有 2.9% |
| 无头 JSON 输出 | 有 | 有 | 缺 | 不做(本轮未选) |
| 计划 / todo 工具 | 有 | 默认关 | 已删 | 不恢复 |
| OS 沙箱 / 审查代理 | 有 | 有 | 结果侧快照 | 不做(威胁模型) |
| code mode / Workflow 脚本 | Workflow | exec(JS) | 无 | 不做;记作 R-323 的先行方案 |
| 记忆 / 并行线 / 上下文账单 | — | — | 反超 | 护城河 |

## 五、接口定义

### 5.1 工具面:常驻层与延迟层

依据是近 30 天 1230 个 episode 的工具频次(`episodes.tools_json`)。

| 层 | 工具 | 说明 |
| --- | --- | --- |
| 常驻(dev,20 个) | read、write、edit、insert、bash、glob、grep、symbols、git、req、defect、work、test_record、memory_search、memory_note、question、task、websearch、webfetch、tool_search | 出现率 ≥13% 的全部保留;其余常驻项的理由见下 |
| 常驻(桌面另加) | collaboration_status | 出现率 42% |
| 延迟(只常驻「名称 + 一句话」) | process、files、incident、conventions、architecture、prior_art、browser、latex、plot、idea、decision、memory_stats;桌面另有 ui_dom、ui_console、ui_style、ui_screenshot、frontend_locate、frontend_check、deliver | 出现率都 <5%;latex 与 plot 在 dev 档 30 天调用为 0 |

几个常驻项的理由:

- `question`、`task`:模型得知道自己能问、能派。CC 同样常驻 `AskUserQuestion` 和 `Agent`。
- `memory_note`:记忆写入口属于护城河,不能让它变成「要先搜才想得起来」。
- `websearch`、`webfetch`:用户定调这两个直接影响工作质量;现在的低频很可能是搜索质量差造成的,不能当成「不需要」。

预算与门禁要跟着改:

- D-662 的预算门禁改成两个数:常驻面预算(dev 20,桌面 21)与延迟目录。
- 提示词里点名的工具(D-190 同源测试)必须是常驻工具,或者提示词里写明先 `tool_search select:<名>`。`frontend_inspection_guidance()` 按后者改写。
- `harness.rs:53-73` 的「硬拒名单点名的替代工具必须存在」校验,覆盖范围扩到延迟层。
- bash 以 `background=true` 返回时,结果里附一句「查看输出用 `tool_search select:process`」。

### 5.2 新增 `tool_search`

```text
tool_search {
  query: string,   // "select:a,b" 按名称精确加载;否则按关键词检索名称与描述
  limit?: int      // 默认 5,最大 10
}
→ 每个命中工具返回 name、description、input_schema。加载后本 run 内可以直接调用,压缩后仍然可用。
  没有命中时,列出延迟目录的全部名称。
```

- **provider 机制**:Anthropic 与 Responses 走原生延迟加载(`defer_loading`),保住提示词缓存。字段名在实现首批核对两家 API 的当前版本。Chat Completions 与 DeepSeek 退化成「加载后追加到 tools 末尾」,每加载一次缓存失效一次,并在上下文账单里记一笔。
- **并发契约**:`Shared`(只读)。

### 5.3 变更 `task`(子代理四项全做,不加新工具)

```text
task {
  prompt: string,
  agent?: string,        // explore | plan | writer | 自定义 agent(.kanzei/agents/*.md 且 mode: subagent)
  model?: string,        // fast | primary | provider:model;默认取 agent 定义
  schema?: object,       // 已有:结构化答案
  background?: bool,     // 默认 false。true:立即返回 task_id,完成时以通知注入主对话
  isolation?: "worktree",// 可写:在独立 worktree 与分支上改;结束返回分支名、diff 统计与摘要,由主 agent 决定合并
  resume?: string        // 续聊:传 task_id,带着该子代理原来的上下文继续,prompt 作为新消息
}
```

- **自定义 agent**:frontmatter 增加 `description`,写进 `task` 的描述里,供模型选择;可选 `writable: true`,只在 `isolation: worktree` 下生效。现在人格列表硬编码为 `[plan_agent()]`(`kanzei-tools/src/run.rs:138`),这里放开。
- **后台**:
  - 完成通知是一条 user 角色消息 `<task-notification>{task_id, status, 摘要}`,对齐 CC 的 task-notification 与 Codex 的 subagent_notification。
  - 模型不需要等待工具,结束本轮即可;停止用桌面端现有的 `stop_task`。
- **可写**:
  - 复用现成的 `WritableSubagentBase` / writer / 写租约(目前只在测试里接线,`run.rs:155-162`)。
  - worktree 复用 `worktree.rs` 的创建与合并预检。
  - 子代理自己不 commit,留待主 agent 合并。
- **续聊**:依赖子代理 transcript 持久化,R-281 已有前置。已结束的子代理用新 prompt 续一轮;还在运行中的,消息排到它的下一步。
- **嵌套**:子代理仍然拿不到 `task`,不允许嵌套。

### 5.4 变更 `question`(一次问多个)

```text
question {
  questions: [                              // 1-4 个
    { question: string,
      header?: string,                      // ≤12 字,UI 标签
      options?: [{ label: string, description?: string }],   // 2-4 个;UI 自动附加「其他」
      multiple?: bool,
      default?: string }
  ]
}
→ 按顺序返回每个问题的答案;「其他」返回用户的原文
```

- **兼容**:旧的单问题形态(顶层 `question`/`options`/`default`/`multiple`)继续接受,在反序列化层归一成单元素数组,不走报错回路。
- **非交互运行**:行为不变,返回 `QUESTION_PENDING`。

### 5.5 网页搜索与抓取(重点)

总体分工:

- **搜索源**按通道选:codex 与 claude 通道用**模型自带搜索**;其余通道(DeepSeek、本地模型、子代理的 fast 模型)用 kanzei 的 `websearch`,底层是改进后的 DuckDuckGo 抓取。
- **抓取**全部走 kanzei 的 `webfetch`,因为证据落盘、外置、research 存证都需要结果在 kanzei 手里。
- 集两家之长:
  - 取 CC 的「按问题提取」和「域名过滤」。
  - 取 Codex 的「批量查询、时间过滤、行号翻页、页内查找、跟链接、引用编号」。

**模型自带搜索**(不是 kanzei 工具,是请求里声明的服务端工具):

- codex 通道:声明 Responses 托管 `web_search` 工具。流里的 `web_search_call` 项要解析、渲染、按原样回放;回答里的 `url_citation` 渲染成引用。
- claude 通道:声明 Anthropic 服务端 web search 工具(版本号实现时核对)。`server_tool_use` 与 `web_search_tool_result` 块(含 encrypted_content)要按协议回放。
- 开关:`[web] native_search = true`(默认开),可按 profile 关闭。
- 权限:服务端搜索无法逐次拦截,只能做 profile 级开关。research 档的 topic/task_id 绑定改为约束「引用前必须经 `webfetch` 存证」。
- **可行性风险**:Codex 自己在 gpt-5.6 这类模型上用的是客户端执行的 `web.run`,不是托管 `web_search`。订阅后端对 gpt-5.6 接不接受托管搜索,必须在实现首批用探针验证;claude 订阅通道同理。
- **codex 通道探针失败时的退路(用户 2026-09-25 选定):复用 Codex 的搜索接口。**
  - 做法:仿照 Codex `web.run` 的客户端执行路径,用订阅 token 调用 ChatGPT 后端的搜索端点(源码见 `codex-api/src/search.rs` 与 `ext/web-search`),作为 `websearch` 在 codex 通道上的后端。
  - 好处:结果落在 kanzei 手里,存证比托管搜索更方便。
  - 代价:这是未公开接口。每次调用都要识别失效(非 2xx、结构不符),失效就自动退回 DuckDuckGo,并在结果里注明「已退回 DuckDuckGo」,不静默。
  - 端点路径与请求结构在实现首批从 `rust-v0.157.0` 源码核对,不凭记忆写。

**`websearch`**(非原生通道用;在原生通道上也保留,供子代理和回退使用):

```text
websearch {
  queries: [ { q: string,
               recency?: "day"|"week"|"month"|"year",   // DuckDuckGo 走 df=d/w/m/y
               domains?: [string] } ],                    // 转成 site: 运算符
  max_results?: int,          // 每个查询的结果数,默认 5,最大 10
  prior_art_topic?: string    // 保留(R-248)
}
→ 每条结果:ref(如 "s2r3")、title、url、snippet
```

- `queries` 1–4 个。旧的单 `query` 形态继续接受。
- 同一次调用里的多个查询并发执行;DuckDuckGo 限流时逐条标注失败,不静默重试(沿用现有文案)。

**`webfetch`**:

```text
webfetch {
  url?: string,          // url 与 ref 二选一
  ref?: string,          // websearch 结果的 ref
  prompt?: string,       // CC 式:给了就由 web_extract 角色模型按问题提取,只回相关段落,每段带行号锚点
  from_line?: int,       // Codex 式:从第 N 行起返回(长页翻页)
  find?: string,         // Codex 式:页内查找,返回命中行及前后各 3 行
  links?: bool,          // 附带页内链接清单(带编号),便于顺着链接继续抓
  max_chars?: int        // 默认 20000
}
```

- **行为**:
  - 页面转成带行号的 markdown;全文落盘到 `.kanzei/artifacts/web/`,可以用 `read` 回取。
  - 同一会话内同一 URL 缓存 15 分钟。
  - 跨域重定向显式告知,不自动跟随(与 CC 一致)。
  - PDF 落盘后提示用 `read pages=…` 按页读,复用现有 PDF 管线。
- **提取模型**:`[models].web_extract` 默认取 fast。本地 4B 模型的提取质量存疑,因此不给 `prompt` 时永远返回原文分页,提取只是可选项。
- **引用纪律**:dev 与 research 的系统提示增加一条——写出来的网页事实必须附 URL。

### 5.6 `edit` / `write` / `insert` 语义变更:先读后写

不新增工具,只改语义:

- **读取账本**:每个会话维护一份账本,记录「规范化路径 → 最后一次读到或写出的内容 hash」,由 `read` 和写类工具的写后结果更新。`read` 读一部分也算读过。
- **规则**:
  - 对已存在的文件做 `edit`/`write`/`insert`,账本里没有这个路径 → `needs_correction: READ_BEFORE_WRITE`。
  - 账本里有,但磁盘 hash 不一致 → `needs_correction: FILE_CHANGED_SINCE_READ`,附当前目标附近的上下文,或 diff 摘要。
  - 新建文件不受限。
- **bash 改文件**不进账本,下次 edit 会因 hash 不一致被拦下来重读。这正好覆盖 bash 与其它并行线的改动。
- **模糊回退**(CRLF/空白)照常保留。前提变了:模型看到的内容已被保证是新鲜的。

### 5.7 工具结果外置阈值

- `TOOL_RESULT_SPILL_THRESHOLD` 从 1 MiB 降到 32 KiB(`tool_exec.rs:148`),与影子遥测阈值合一。
- 预览保留头部 8 KiB + 尾部 4 KiB,并附原始字节数与 `read` 回取路径。bash 的 1 MiB 捕获不变,由外置兜住。
- 并入 R-245 的收尾;磁盘配额的决策一并拍板。

### 5.8 非工具接口:插话、指令文件、压缩、缓存

- **运行中插话**:
  - `steer` 输入在工具边界注入:每步请求前查 inbox,把 steer 作为一条 user 消息追加,前缀 `[运行中插话]`;UI 标记「已送达」。
  - `queue` 仍在 run 边界处理。
- **项目指令文件**:
  - 新增 context source `project/instructions`:从仓库根到 cwd 逐层,优先取 `AGENTS.md`,没有就取 `CLAUDE.md`。
  - 预算 32 KiB,截断时显式注明截掉了多少。dev、research、readonly 三档都注入。
- **手动压缩**:
  - 入口:桌面输入框内置 `/compact [焦点]`,CLI 加 `kz compact [--focus 文本]`。焦点写进 L1 纪要的指令。
  - 摘要改为直接调用 compact 角色模型,不再经过子代理运行时,修掉 `compaction.rs:181-194` 的退化问题。
- **缓存**:
  1. episodes 按步落盘 `cache_read`/`cache_write`。
  2. Responses 请求带上 `prompt_cache_key = session_id`。
  3. 看数据决定要不要把每步刷新的段落(`dev/control`、`runtime/collaboration`)从 system 末尾(`drive.rs:248-255`)挪到最新消息末尾。

### 5.9 回退(照 CC:对话 + 代码)

- **检查点**:
  - 每条用户消息,也就是每个 run 的开始,是一个检查点。
  - 在同一检查点窗口内,edit/write/insert 第一次触碰某个文件时,保存该文件的前像;新建的文件记为「原本不存在」。
  - 前像按内容寻址,存进 `.kanzei/artifacts/checkpoints/`,索引放在 state.db。
- **回退入口**:只在 UI 里,不给模型工具。鼠标悬停在用户消息上出现「回退到这里」,可选三项:
  - 对话 + 代码(默认)。
  - 只回退对话。
  - 只回退代码。
- **对话回退**:追加 `conversation.rewind{to_sequence}` 事件,与现有的 `conversation.reset` 同一套事件溯源,历史按它重建。被回退掉的那段仍可以查看,不物理删除。原消息回填到输入框,便于改完再发。
- **代码回退**:
  - 把检查点之后改过的文件恢复成前像;检查点之后新建的文件删掉。
  - **偏离 CC 一处**:某个文件在 kanzei 最后一次写入之后又被外部改过(并行线、codex、用户手改),默认跳过并列出来,确认后才覆盖。理由是 kanzei 有并行线与共用工作树的场景,CC 没有。
- **预览与警示**:回退前列出将恢复的文件,同时列出「不会还原」的内容:这段时间里跑过的 bash 命令、做过的 git 提交、追踪文档的写入。行为契约与 CC 一致:只还原编辑工具改过的文件。
- **范围**:检查点按会话与线隔离,只作用于本线的工作树。

### 5.10 定时任务(触发器 → 固定流程 → 可选回写)

用户要的形态「倾向于定时任务那样子」,同时明确说 CC 的定时任务「偏复杂不好用,前端呈现不直观」。CC 那边有四套机制并存:`/loop`、`CronCreate`、桌面定时任务、云端 routines。kanzei 只做**一个概念、一个面板**。

**定义**:存为 `.kanzei/schedules/<名称>.md`(项目级)或 `~/.kanzei/schedules/`(全局)。主要编辑入口是 UI 表单,文件是存储格式,也可以手改。

```yaml
---
name: arxiv-每日
enabled: true
when: 每天 09:00            # 人话频率。UI 用选择器生成,不让用户写 cron
                             # 支持:每 N 分钟/小时、每天 HH:MM、工作日 HH:MM、每周一..日 HH:MM
agent: research              # prompt 步用哪个 agent
model: fast                  # fast | primary | provider:model
timeout: 30m
catch_up: once               # 错过的触发(应用没开):下次启动补跑一次 | skip
steps:
  - run: curl -s "https://export.arxiv.org/api/query?search_query=..."   # 命令步:输出传给下一步
  - prompt: 从上一步结果里挑出与持续学习相关的论文,每篇两句话摘要
writeback:                   # 可选,可多选;不写就只在任务面板里留结果
  - notify                   # 桌面 + 手机通知
  - memory_inbox
  - file: .kanzei/research/daily/{date}.md    # 追加
  - idea                     # 生成 idea 草稿,走 tracker 通道
---
(正文:这个任务是做什么的,给人看)
```

**执行**:

- 每次运行是一条独立的线:自己的会话、自己的上下文、事件溯源,不污染主对话。
- 无人值守时权限按 `rules_only` 处理:需要问你的动作被拒并记录,再加上资源兜底(超时、步数、ZeroOutput)。三种触发位置一律如此。

**触发位置**:每个任务单独选,用户 2026-09-25 要求「提供几个选项」。frontmatter 增加 `host:` 字段。

| `host` | 触发方 | 应用关着时 | 结果如何回到面板 | 前提与限制 |
| --- | --- | --- | --- | --- |
| `app`(默认) | kzapp 内置调度器 | 不跑;按 `catch_up` 下次启动补跑一次,并在面板上注明 | 直接写入本地 state.db | 无 |
| `system` | Windows 任务计划程序,到点调用 `kz schedule run <名称>` | **照跑** | CLI 写入同一个项目 state.db,下次打开应用即可看到 | 前提:`kz` 要支持无头执行定时任务。限制:应用关着时 `notify` 只能发系统通知,手机桥在 kzapp 里,推不了;其余回写照常。注册与注销由面板开关控制,关掉任务同时删除计划程序条目 |
| `server:<环境名>` | 你登记的服务器上的 cron 或 systemd timer,调用远端 `kz schedule run` | **照跑** | kzapp 下次连接时经 SSH 拉回运行记录与产物,并入面板 | 服务器沿用 research `environments.md` 的 SSH 登记(A-016/A-018);凭据只存 secret 引用,不写进 Markdown;远端要装 kz 并配好模型通道。远端运行时只执行 `file` 回写(写远端文件),`notify`/`memory_inbox`/`idea` 等拉回时由本地补做 |

`server` 这一档是按「托管到你自己的服务器」理解的,待用户确认,见 §八。

**面板**:侧边栏「定时任务」,每个任务一张卡片,包含:

- 名称。
- 人话频率,如「每天 09:00」。
- 下次运行倒计时。
- 上次结果:✓/✗、一句话摘要、耗时。
- 开关、「立即运行」「历史」;点开可以看那次运行的完整对话。

编辑用表单:频率选择器、步骤列表、回写多选框。

**不给模型工具**。v1 只由用户定义,常驻工具面不变。事件触发(提交后、run 结束后、文件变化)留作 v2,沿用同一份定义格式,只把 `when` 换成 `on: commit` 这类写法。

### 5.11 会话分叉(后做)

- 入口:用户消息菜单里的「从这里分叉」,新建一条线,历史复制到该消息为止,原线不动。
- 分叉不带代码状态;要带代码,就配合 worktree 线一起用。
- 用户定调:用得少(上下文管理做得好,就不太需要分叉),优先级排在回退之后。

### 5.12 删除:commands 注册表

- 删除 `commands/*.md` 的扫描与提示词清单(`markdown.rs:30-41, 152-173`)。它从不展开参数,也没有消费方。v0 判定「接线或删,不许半吊」,用户本轮不要 skills/commands 接线。
- skills 维持现状:清单进提示词,由模型自己 `read`,与 Codex 的本地 skills 一致。

## 六、不做(含用户否决)

| 项 | 理由 |
| --- | --- |
| hooks | 用户:「很少用,不用的原因是不熟悉工作原理」。他真正想要的是触发器 + 固定流程,已由 §5.10 承接 |
| skills 接线、MCP | 用户:基本不用;「比起 MCP 更想定义触发器和固定 pipeline 的回写」 |
| plan 模式 | 用户:「模型能力强了,我基本也用的少了」 |
| 编辑原语按模型切换(apply_patch) | 实测不支持:gpt-5.6-luna 3217 次 edit 中未命中只有 2.9%。12.1% 的被拒来自 kanzei 自己的守卫,另行分析 |
| 计划 / todo 工具 | Codex 自己在撤,kanzei 已删 |
| OS 沙箱、Guardian、auto 分类器、Starlark 规则 | 威胁模型;D-267 已砍 bash 中间档 |
| 删专用读 / 搜工具(Codex 方向) | kanzei 的 read/grep 承载了图片、PDF、外置回取和权限资源;Windows 上 shell 的引号是真实痛点 |
| code mode、Workflow 脚本 | 记作 R-323 的先行方案;要嵌 JS 引擎,与空闲 RSS <50 MB 的目标冲突 |
| 无头 JSON 输出 | 本轮未选;回放评测真需要时再提 |
| plugins、企业托管、云端任务、机器人、OTEL | A-007 判据丢弃 |

## 七、登记结果与实施地图

2026-09-25 用户说「直接登记就行」,已按下表登记(登记前重查了最新编号)。每条的行号地图、批次、陷阱与裁决见 `cc_codex_alignment_impl_maps.md` 对应小节。

| 条目 | 主题 | 规格 | 地图 | 优先级 | 批次 |
| --- | --- | --- | --- | --- | --- |
| R-364 | 工具延迟加载与 tool_search | §5.1 / §5.2 | §1 | P1 | 4 |
| R-365 | 网页搜索与抓取 | §5.5 | §2 | P1 | 5 |
| R-366 | 回退(对话 + 代码) | §5.9 | §3 | P1 | 4 |
| R-367 | 先读后写 | §5.6 | §4 | P1 | 3 |
| R-369 | 子代理补齐 | §5.3 | §5 | P2 | 3 |
| R-370 | 定时任务 | §5.10 | §6 | P2 | 6 |
| R-371 | 运行中插话 | §5.8 | §7 | P2 | 3 |
| R-372 | 手动压缩 | §5.8 | §8 | P2 | 3 |
| R-373 | 缓存测量与 prompt_cache_key | §5.8 | §9 | P2 | 3 |
| R-374 | question 批量化 | §5.4 | §10 | P2 | 2 |
| R-375 | 项目指令文件 | §5.8 | §11 | P2 | 1 |
| R-376 | 工具结果外置阈值(配额仍归 R-245) | §5.7 | §12 | P2 | 1 |
| R-377 | 会话分叉(依赖 R-366) | §5.11 | §13 | P3 | 1 |
| D-748 | 删除 commands 注册表 | §5.12 | §14 | P3 | 1 |
| D-751 | 手机端 question 只能批准/拒绝(勘察中发现的既有缺陷) | — | — | P2 | 1 |

同日另登记的文档引用功能 R-368 与两条相关缺陷 D-749、D-750,设计见 `doc_reference_graph.md`,不属于本文范围。

### 实施地图对本文规格的补充与偏离

核对勘察时发现,下面几处照 §五 原文实现会踩到既有不变量或自相矛盾,所以在地图里作了裁决。以地图为准,理由在对应小节。

| 规格处 | 地图裁决 | 理由 |
| --- | --- | --- |
| §5.3 续聊「运行中的消息排到它的下一步」 | v1 对运行中的任务返回稳定 code `task_resume_running` | 排到下一步依赖 R-371 的 runner 钩子;R-371 落地后再改 |
| §5.3 可写子代理 | 只对 dev 档开放;跳过 subagent-write 前置询问;合并走 `git merge_task`(squash)加常规 commit 门禁 | research 档对 bash 与 git 写动作是硬拒;生产装配里 ask_router 恒为 None,不跳过询问则一次都写不了 |
| §5.4 question | 单弹窗一次展示全部问题并一次提交 | 用户「少打断」;逐题弹窗仍是 N 次打断 |
| §5.5 原生搜索 | research 档默认关闭;自主轮在非交互拒绝策略下不声明;托管条目带通道标识 | 守 R-217、D-571、R-248;同协议不同厂商不能互相回放 |
| §5.7 外置阈值 | read 与 task 结果豁免;外置时保留终端块显示 | read 会形成外置→read→再外置的死循环;task 报告被截成头尾会丢结论;D-237 的长输出查看不能退化 |
| §5.10 定时任务 | v1 只做项目级任务;缺省 agent 为 readonly;无人值守运行中 websearch/webfetch 放行 | 全局任务会被每个项目重复触发;readonly 可避开写租约互等,回写本来就由引擎完成 |

## 八、待确认

1. 定时任务 `server` 档按「托管到登记服务器,远端定时跑 kz,结果拉回本地」实现:用户 2026-09-25 原话「暂定这样以后要改再改就行」。若以后改成外部事件触发,只重写 R-370 的 B6。
2. R-245 的磁盘配额:用户 2026-09-25 拍板「2 GiB,超了退回截断。先这样」,即上限 2 GiB、超限降级为 Inline 截断;实现归 R-245,R-376 不含这一项。

已决:
- 原生搜索探针失败时的退路 = 复用 Codex 搜索接口(§5.5)。
- 定时任务提供 `app` / `system` / `server` 三种触发位置(§5.10)。
- 登记:用户「直接登记就行」。

## 变更记录

- 2026-09-25:建档。三路只读调研(CC 文档与本会话实载工具定义、Codex `rust-v0.157.0` 源码、kanzei 全仓勘察)+ state.db 实测(30 天工具频次、按模型的 edit 未命中率)。
- 2026-09-25:两轮用户筛选,落成 §五 接口定义。用户定调:「第一性原理」按实际使用取舍。
  - 保留:A 档全收、子代理四项、question 批量。
  - 升级:网页搜索升为重点、回退照 CC。
  - 否决:hooks、skills、MCP、plan 模式。
  - 新增:定时任务(一个概念、一个面板、回写可选)。
- 2026-09-25:第三轮。
  - 搜索退路定为复用 Codex 搜索接口,失效自动退回 DuckDuckGo。
  - 定时任务增加 `host: app | system | server` 三种触发位置。
  - 用户要求先审阅文档再登记。
- 2026-09-25:用户「直接登记就行」。
  - 7 组代码勘察各配一个对抗核对者,产出实施地图 `cc_codex_alignment_impl_maps.md`。
  - 登记 R-364~R-367、R-369~R-377、D-748、D-751。
  - §七 记录地图对规格的补充与偏离。

## 验证证据

- 工具频次:对 `episodes.tools_json` 做 30 天聚合,共 1230 个 episode。
- edit 未命中率与被拒率:对 `episodes.metrics_json` 按 provider:model 聚合。
- 缓存:主代理每步的 `cache_read` 未落盘,只有子代理的 trace 带 usage,所以 §5.8 缓存第 3 步是假设。
- 源码锚点:
  - 每步刷新段拼进 system:`drive.rs:248-255`
  - Anthropic 缓存断点:`anthropic.rs:23-50`
  - Responses 请求体:`openai_responses.rs:92-97`,无 `prompt_cache_key`
  - 外置阈值:`tool_exec.rs:148-149`
  - DuckDuckGo 端点:`websearch.rs:9`
  - `webfetch`:`webfetch.rs:10-20`

## TODO 与后续风险

- 条目已登记(见 §七)。`direction_taste.md` 的复刻清单 v0 已加指针指向本文;A-007 决策条目的备注仍指向 v0,下次修订决策时同步。
- 风险一:原生搜索依赖订阅后端的非公开行为,探针结论要写进条目,不能假设可用。
- 风险二:延迟加载在 Chat Completions 类 provider 上没有原生机制,缓存失效只能做到可见,消除不了。
- 风险三:回退的「外部改动跳过」要和并行线的合并流程对齐,避免回退与合并互相踩。
- 风险四:定时任务的无人值守运行要走与自主档相同的资源兜底,不能因为「只是定时任务」就放宽。
