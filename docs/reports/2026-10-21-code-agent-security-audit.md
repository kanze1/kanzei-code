# Code Agent 安全漏洞修复对照与 Kanzei 同类风险核查

- **审计日期**：2026-10-18（运行环境日期；原稿误写 2026-10-21，保留原文件名以免破坏引用）
- **代码基线**：HEAD `24533867ddc61b442cfd8801c24c8faebe0fa611`。
- **覆盖产品**：OpenAI Codex CLI、Anthropic Claude Code、Cursor、Google Gemini CLI、Moonshot Kimi Code、Cline Kanban Server。
- **名称说明**：本轮将“claudecodebuddy”按 **Claude Code** 理解；没有找到可唯一确认的独立产品“Claude Code Buddy”。如用户指向某个 Buddy 仓库，请提供链接后补查。
- **边界**：外部信息以官方安全公告/项目 changelog 与研究者披露为主；本地审查是源码审阅加定向回归，不是全仓渗透测试。没有执行恶意仓库端到端提示注入、真实浏览器外传或 shell 方言 fuzz。

## 风险排序与状态标记

本文件 **P1/P2/P3 是审计复核顺序，不是 CVSS，也不是 tracker 排期字段**。

- **🔴 P1**：秘密信息可能进入模型上下文，或恶意本地页面可能读取项目敏感文件。
- **🟠 P2**：安全边界/执行预算不够强，影响依赖具体配置、provider 行为或运行场景。
- **🟡 P3**：纵深防御、调用方契约及对抗测试覆盖。
- **状态标记**：`代码路径确认` = 源码清楚支持该行为；`端到端未验证` = 尚未完成攻击性浏览器/provider PoC；`已防护` = 对照路径有实现且定向测试通过；`条件残余` = 需要用户显式宽放授权或特定调用方行为，不表示已发生越权。

## 一、Kanzei 本地发现（按风险排序）

### 🔴 P1-01 — 默认 `read` 可将工作区 `.env` / 私钥等内容送入模型上下文

**状态：代码路径确认；没有运行真实 provider 泄漏 PoC。**

- 默认权限对 `read:*` 是 `Allow`（[`base.rs`](../../crates/kanzei-tools/src/base.rs#L71-L90)）；只读档也明确 allow `read:*`（[`readonly.rs`](../../crates/kanzei-tools/src/profiles/readonly.rs#L13-L20)）。
- `ReadTool` 将目标作为文件资源交由权限系统，然后调用 `file_access::resolve` / `ScopedFile` 并打开文件；正文读取成功后直接作为 `ToolOutput::ok(text)` 返回（[`read.rs`](../../crates/kanzei-tools/src/read.rs#L65-L88)、[`read.rs`](../../crates/kanzei-tools/src/read.rs#L91-L153)、[`file_access.rs`](../../crates/kanzei-tools/src/file_access.rs#L5-L11)）。`ScopedFile` 限制工作区根和 symlink 越界，但没有按 `.env`、密钥文件名或文件内容分类屏蔽。
- 因而，工作区里存在秘密且模型（包括被仓库提示注入影响的模型）调用 `read` 时，不需要单独获得文件读取批准，秘密会进入后续模型请求。`grep` 等只读工具也需单独审阅，不能将 `ScopedFile` 的路径围栏误认为秘密过滤。
- 现有 `claude_audit_regressions.rs` 的 `private` 测试是显式附加 deny 规则后的路径拒绝，不证明默认会拒绝 `.env`（[`claude_audit_regressions.rs`](../../crates/kanzei/tests/integration/claude_audit_regressions.rs#L247-L307)）。子代理中 `.env` deny 测试也自行添加了该规则（[`subagent.rs`](../../crates/kanzei-tools/src/subagent.rs#L152-L172)）。

**影响边界**：这是“默认可读范围太宽”的秘密暴露风险，不是工作区外路径逃逸，也不单独意味着模型能绕过 `bash`/`webfetch` 的权限闸门。模型供应商是否保留上下文不在本次核验范围内。

**建议优先修复**：对常见密钥/凭据文件（如 `.env*`、私钥、云凭证）增加代码强制的敏感资源策略；拒绝或要求显式批准，并让 `read`、`grep`、`browser` 等所有内容读取路径共享同一策略。需保留用户显式查看文件的可控例外，避免只有提示词中的“不要读密钥”。

### 🔴 P1-02 — 本地项目预览页可同源读取已登记根内的其它文件

**状态：项目文件读取路径与无 CSP 已由源码和 socket 测试确认；页面脚本读取 `.env` 的端到端浏览器 PoC 未运行。**

- 本地浏览器工具权限资源是具体的 `path:<abs>`（[`browser_tool.rs`](../../crates/kanzei-tools/src/browser_tool.rs#L348-L369)），本地 HTML 路径在默认规则中放行（[`base.rs`](../../crates/kanzei-tools/src/base.rs#L91-L105)）。
- `url_for_path` 会把用户打开的 HTML 映射到一个已登记的目录根；普通项目根会包含页面所在项目目录（[`preview_server.rs`](../../crates/kanzei-tools/src/preview_server.rs#L180-L227)）。
- 同一根内任意规范化文件均可由 `serve_file` 返回；它只阻止越出 root，并不按“本次授权打开了哪一个文件”收窄 sibling 文件（[`preview_server.rs`](../../crates/kanzei-tools/src/preview_server.rs#L541-L581)）。项目页面的响应明确没有 CSP，测试也将其作为支持 module/fetch 等页面能力的既定行为（[`preview_server.rs`](../../crates/kanzei-tools/src/preview_server.rs#L988-L1035)）。预览 URL 中的服务 token 和 root id 对页面自身可见，因此同源脚本可构造本根内另一个文件的 URL。
- 对照资源授权只记录单个 `path`、实际 HTTP 服务却暴露整个登记 root，存在“获准打开一个 HTML ⇒ 页面脚本可读取同一项目根其它文件”的授权范围差异。若 `.env` 等在该 root 内，恶意 HTML 有条件读取其内容；把内容发送到外部网络的可行性本次未验证。
- 定向测试 `cargo test -p kanzei-tools preview_server::tests::真实连接_片段响应头带沙箱_项目页面不带` **1/1 通过**，确认线上项目页面响应不带 CSP；这验证了前置条件，不是完整窃取 PoC。

**建议优先修复/评审**：对不可信项目页面使用隔离 origin 或 sandbox CSP，并按文件/目录授权范围而非整个项目根提供读取；如果为了本地开发预览必须保持多文件同源能力，应明确提示其会暴露项目根内文件，并避免在含秘密的目录中预览不可信 HTML。另行确认 WebView 是否允许脚本发起外网请求。

### 🟠 P2-01 — `max_steps` 最后一步只隐藏工具 schema，没有在执行端硬拒 provider 返回的工具调用

**状态：源码路径确认；未构造 mock provider 最小复现；这是既有审计记录 M-003 的再确认。**

- 最后一步向 provider 传空 schema（[`drive.rs`](../../crates/kanzei-core/src/runner/drive.rs#L267-L280)），但若 provider 仍返回 `ToolCall`，`commit_step_messages` 只在 `calls.is_empty()` 时收尾，非空调用继续 Proceed（[`history.rs`](../../crates/kanzei-core/src/runner/drive/history.rs#L78-L118)）。之后 `execute_tool_calls` 按实际工具列表查找并运行，未收到 `last_step` 标记（[`drive.rs`](../../crates/kanzei-core/src/runner/drive.rs#L1199-L1281)；[`serial_tools.rs`](../../crates/kanzei-core/src/runner/drive/serial_tools.rs#L97-L143)）。
- 正常权限闸门仍执行，因此这不是对所有工具权限的绕过；但最后一步的“工具不可用/预算已耗尽”目前不是执行端不变量。若返回的是默认 Allow 的只读工具，可能多执行一步并额外读取数据；对 Ask 工具仍受 Ask 策略控制。
- 旧静态审计报告已将该项列为 M-003（[`harness-static-analysis.md`](2026-08-08-harness-static-analysis.md#L67-L71)），目前代码仍保留该路径。

**建议**：在工具执行入口把 `last_step`/预算终态作为硬拒绝判据，并测试 provider 即使违反空 schema 约束返回工具调用也不会产生副作用。不要仅依赖 system prompt 或 provider 遵守 schema。

### 🟠 P2-02 — 普通 Bash 不提供 OS 级隔离（宽授权下的条件残余）

普通 Bash 直接在 Kanzei 当前进程身份下启动系统 shell（[`bash.rs`](../../crates/kanzei-tools/src/bash.rs#L363-L390)）；本次未发现其具有通用文件系统/网络 OS sandbox。默认 Bash 权限是 Ask，只读档硬拒 Bash；显式 `AskPolicy::AutoAllow` 会自动放行 ASK（[`permissions.rs`](../../crates/kanzei-core/src/runner/drive/permissions.rs#L199-L212)）。所以提示注入在默认交互策略下仍要经过权限决定，但用户启用 AutoAllow/宽规则后，命令可在当前用户权限范围内产生副作用。托管文件快照围栏不是 OS 隔离，也不能撤销任意主机/网络动作。

**判断**：这不是 Codex sandbox escape 的同款权限绕过；是处理恶意仓库时必须明确的运行边界。若目标包含“对不可信仓库自主运行”，应单独评估低权限账户/VM/容器与文件、网络、进程树隔离。

### 🟠 P2-03 — 对第三方 Agent 特殊启动文件没有额外敏感文件闸门

默认 `write` 是 Ask，非交互规则外 Ask 默认拒绝；但在 AutoAllow/宽泛 Allow 下，通用文件写工具没有针对 `.cursor/mcp.json`、`.vscode/settings.json`、`.code-workspace`、`.claude/settings.json`、`.codex/config.toml` 的专项硬保护。Kanzei 本身未发现 MCP server 配置自动加载；风险是共享工作区中其它 Agent 之后可能执行/信任这些配置。此项是条件性跨 Agent 风险，不是默认路径已被攻破。

**建议**：评估创建/修改这些敏感配置是否应在 AutoAllow 下仍强制额外确认；新建与改写必须同一策略。

### 🟡 P3 — 未确认但应持续跟踪的边界

- **子代理快照依赖调用方**：当前 `SubagentBase` 装配只读工具，但 `SubagentRuntime.snapshot` 与 `writable` 是公开字段；runner 使用传入 snapshot。当前应用内没有发现仓库内容能构造不安全 snapshot 的路径，故不是已确认应用漏洞。建议加 `writable=false` 但 snapshot 含获准 Bash 的负向测试，或收紧 runtime 构造边界。
- **网页提示注入不是代码隔离**：`webfetch` 页面内容会作为普通工具结果进入模型上下文；本次未发现它能自动跳过后续工具权限闸门。应继续把页面视为不可信输入，不把 system prompt 的提醒当作安全边界。
- **本地服务认证**：移动服务默认 loopback，LAN 需显式开启；敏感接口要求 Bearer token，设备撤销会使 token 失效（[`mobile.rs`](../../crates/kanzei-app/src/mobile.rs#L111-L130)、[`mobile.rs`](../../crates/kanzei-app/src/mobile.rs#L253-L271)、[`mobile.rs`](../../crates/kanzei-app/src/mobile.rs#L684-L712)）。桌面 bridge 也绑定 loopback 并要求 invoke key（[`desktop_bridge.rs`](../../crates/kanzei-app/src/desktop_bridge.rs#L172-L203)）。静态审阅没有发现 Cline 无认证 WebSocket 同款结构；未运行真实恶意网页跨源 PoC。
- **Git 元数据命令注入**：Git 命令执行器用 `Command::new("git").args(args)`，不是把 Git 配置拼进 shell（[`commands.rs`](../../crates/kanzei-tools/src/git/commands.rs#L42-L79)）；未发现 Claude Git email 注入同款实现，审阅范围仅覆盖公共 Git runner。

## 二、与外部漏洞修复的对照（按风险类别排序）

### 🔴 P1 — 权限判据/命令解析不一致导致工具执行绕过

- **Claude Code 命令确认绕过**：`<1.0.20` 受影响，`1.0.20` 修复；引入不可信上下文后命令解析错误可绕过确认。 [GHSA-x56v-x2h6-7j34](https://github.com/anthropics/claude-code/security/advisories/GHSA-x56v-x2h6-7j34)
- **Claude Code `$IFS`/短参数只读验证绕过**：`<1.0.93` 受影响，`1.0.93` 修复；命令解析差异绕过只读校验执行任意代码。 [GHSA-xq4m-mc3c-vvg3](https://github.com/anthropics/claude-code/security/advisories/GHSA-xq4m-mc3c-vvg3)
- **Gemini CLI headless trust/allowlist**：`<0.39.1` 与 `0.40.0-preview.2` 受影响，分别在 `0.39.1` / `0.40.0-preview.3` 修复，Action 修复版 `0.1.22`；headless 自动信任仓库配置、`--yolo` 忽略细粒度 allowlist。 [GHSA-wpqr-6v78-jr5g](https://github.com/advisories/GHSA-wpqr-6v78-jr5g)

**修复模式**：最终执行动作必须和授权时解析的结构化动作一致；所有模式均走同一策略；不能只靠工具 schema、提示词或前端隐藏。

### 🔴 P1 — 仓库配置、Git 元数据和特殊文件持久化执行

- **Claude Code Git email 命令注入**：`<1.0.105` 受影响，`1.0.105` 修复；不可信 `git config user.email` 进入启动命令，且发生在 workspace trust 确认前。 [GHSA-j4h9-wv2m-wrf7](https://github.com/anthropics/claude-code/security/advisories/GHSA-j4h9-wv2m-wrf7)
- **Claude Code 缺失 settings 文件注入 hook**：`<2.1.2` 受影响，`2.1.2` 修复；sandbox 内创建 `.claude/settings.json` 并植入 `SessionStart` hook，后续宿主权限启动执行。 [GHSA-ff64-7w26-62rf](https://github.com/advisories/GHSA-ff64-7w26-62rf)
- **Cursor MCP special-file 创建审批缺口**：`≤1.2.1` 受影响，公告列出修复 `1.3.9`；提示注入新建 `.cursor/mcp.json` 启动攻击者 MCP server。 [GHSA-4cxx-hrm3-49rm](https://github.com/cursor/cursor/security/advisories/GHSA-4cxx-hrm3-49rm)
- **Cursor 编辑器/工作区配置注入**：`.vscode/settings.json` 链修复 `1.3.9`；`.code-workspace` 链影响 `<1.7`，`1.7` 修复；修复将敏感配置列入额外审批。 [GHSA-vqv7-vq92-x87f](https://github.com/cursor/cursor/security/advisories/GHSA-vqv7-vq92-x87f) · [GHSA-xg6w-rmh5-r77r](https://github.com/cursor/cursor/security/advisories/GHSA-xg6w-rmh5-r77r)
- **OpenAI Codex `.env`/`CODEX_HOME` 配置执行**：Check Point 描述仓库 `.env` 将 `CODEX_HOME` 指向本地 `.codex/config.toml`，启动时执行其中 MCP command；该报告称 CLI `0.23.0` 修复。但 GitHub Advisory `CVE-2025-61260 / GHSA-xrxf-jgv3-qmrm` 标示受影响 `≤0.23.0` 且未列 patched version，公开版本边界冲突；本报告不把 `0.23.0` 当成已由所有来源确认的安全下界。 [Check Point](https://research.checkpoint.com/2025/openai-codex-cli-command-injection-vulnerability/) · [GitHub Advisory](https://github.com/advisories/GHSA-xrxf-jgv3-qmrm)

**修复模式**：项目文件不自动等于可信配置；启动前显式审批并将批准绑定内容指纹；敏感文件创建和编辑同等检查；所有不可信元数据以 argv/结构化数据传递，不拼 shell。

### 🔴 P1 — workspace/sandbox 路径根或 symlink 与真实 I/O 不一致

- **OpenAI Codex sandbox cwd 根混淆**：CLI `0.2.0–0.38.0` 受影响，`0.39.0` 修复；模型生成的 cwd 可被误当成 sandbox 可写根。 [GHSA-w5fx-fh39-j5rw / CVE-2025-59532](https://github.com/openai/codex/security/advisories/GHSA-w5fx-fh39-j5rw)
- **Claude Code symlink sandbox escape**：`<2.1.64` 受影响，`2.1.64` 修复；sandbox 创建指向外部的 symlink，宿主后续写入跟随 symlink 越界。 [GHSA-vp62-r36r-9xqp / CVE-2026-39861](https://github.com/advisories/GHSA-vp62-r36r-9xqp)

**修复模式**：按规范化物理路径限制实际 I/O；防止检查后父目录被替换；跨 sandbox/host 两阶段不得信任攻击者可控路径。

### 🟠 P2 — URL fetch SSRF、DNS rebinding 与 redirect

- **Moonshot Kimi Code FetchURL SSRF**：`<0.27.0` 受影响，`0.27.0` 修复；hostname DNS 解析可指向 loopback/private，redirect 未逐跳复核。修复包括解析并拒绝非公网地址、逐跳复查与连接固定。 [JFrog CVE-2026-17534](https://research.jfrog.com/vulnerabilities/kimi-code-is-vulnerable-to-a-fetchurl-ssrf-protection-bypass-via-dns-resolving-hostnames-and-redirects-cve-2026-17534/) · [Kimi changelog](https://www.kimi.com/code/docs/en/kimi-code-cli/changelog.html)

### 🟠 P2 — 本机 Agent 控制服务缺少认证/来源限制

- **Cline Kanban Server WebSocket hijack**：公告影响 `≤2.13.0`，未列出修复版；恶意网页可连接 loopback WebSocket、读取运行数据并向 PTY 注入。 [GHSA-5c57-rqjx-35g2 / CVE-2026-44211](https://github.com/advisories/GHSA-5c57-rqjx-35g2)

## 三、Kanzei 已验证防护与验证范围

- **文件工具工作区边界**：`ScopedFile` 限定 `cwd`/`project_root` 物理目标并用能力目录完成 I/O（[`scoped_file.rs`](../../crates/kanzei-harness/src/permission/scoped_file.rs#L25-L111)）。`cargo test -p kanzei-harness scoped_file::tests`：**2 passed**，覆盖外部路径/链接拒绝及检查后父目录 symlink 替换。
- **项目配置/Agent 信任**：`.kanzei/kanzei.toml` 和项目 Agent 需按 root+内容指纹显式批准，未批准来源不加载（[`project_trust.rs`](../../crates/kanzei-harness/src/project_trust.rs#L35-L102)、[`markdown.rs`](../../crates/kanzei-harness/src/markdown.rs#L140-L166)）。`cargo test -p kanzei-harness approvals_bind_root_and_source_content_and_do_not_approve_future_agents`：**1 passed**。
- **权限默认项**：`cargo test -p kanzei-tools primary_base_exposes_structured_search_and_git`：**1 passed**。它验证 Base 组件装配，不是 `.env` 专项防护测试；`read:*` 默认 Allow 由源码确认。
- **WebFetch SSRF**：DNS 地址校验、连接固定和逐跳 redirect 复查（[`public_http.rs`](../../crates/kanzei-tools/src/public_http.rs#L21-L60)、[`webfetch.rs`](../../crates/kanzei-tools/src/webfetch.rs#L524-L567)）。`cargo test -p kanzei-tools public_fetch_blocks_private_targets_and_redirects_including_raw_downloads`：**1 passed**。
- **本地预览服务**：`cargo test -p kanzei-tools preview_server::tests::真实连接_片段响应头带沙箱_项目页面不带`：**1 passed**。测试证明片段有 sandbox CSP、项目文件响应没有；该行为本身触发 P1-02 的同源风险审查。
- **命令资源一致性**：已有 `bash_action_literal.rs` 回归测试将结构化 Bash 资源与权限 action 绑定；这不等同于对所有 shell 语法做 fuzz。

## 四、建议整改顺序与结论

1. **🔴 P1：秘密文件统一读取策略**——覆盖 `read`、`grep`、本地预览/浏览器等所有能把内容交给模型或页面脚本的路径；默认保护，明确审批才能例外。
2. **🔴 P1：预览不可信页面隔离**——收窄单文件授权的实际服务范围，或将项目页面置于不能同源读取项目秘密的隔离 origin/sandbox；再做真实浏览器读取与外传测试。
3. **🟠 P2：执行端强制 max_steps**——补 provider 违反空 schema 的 mock 测试并在 dispatch 硬拦截最后一步工具。
4. **🟠 P2：明确 AutoAllow/Bash 的非沙箱边界**——处理不可信仓库时避免宽权限；若产品目标包含无人值守恶意仓库执行，另行设计 OS 级隔离。
5. **🟠 P2：第三方 Agent 敏感配置创建/修改策略**——评估在宽授权模式下保留额外确认。
6. **🟡 P3：子代理快照与网页不可信数据**——补调用方不变量和对抗回归，不把待验证问题误写成已确认绕过。

**结论**：本地复核发现两条 P1 级敏感数据暴露路径（默认可读工作区秘密；本地项目 HTML 同源可读登记根内 sibling 文件），以及一条 P2 执行预算未在 dispatch 端强制的已有缺口。前两项是代码行为/权限范围确认，不是对外网泄漏完成端到端攻击的宣称；相关的 browser 外传 PoC 尚未运行。其他与外部案例对照的路径中，文件工具 symlink/越界、项目配置信任、WebFetch SSRF 和已审阅的 loopback 服务认证均有对应保护证据。普通 Bash 没有 OS sandbox 是另一个条件性架构边界，不应误报为已发现的授权绕过。

**Tracker 说明**：本轮按用户要求将发现写入本报告并按 P1→P3 排序；没有在未做浏览器/provider PoC、且没有用户明确要求实施的情况下擅自修改源码或创建整改条目。

## 参考来源

### 官方安全公告/项目 changelog

- [OpenAI Codex sandbox path boundary — GHSA-w5fx-fh39-j5rw](https://github.com/openai/codex/security/advisories/GHSA-w5fx-fh39-j5rw)
- [OpenAI Codex project config — GHSA-xrxf-jgv3-qmrm](https://github.com/advisories/GHSA-xrxf-jgv3-qmrm)
- [Claude Code command confirmation bypass — GHSA-x56v-x2h6-7j34](https://github.com/anthropics/claude-code/security/advisories/GHSA-x56v-x2h6-7j34)
- [Claude Code read-only validation bypass — GHSA-xq4m-mc3c-vvg3](https://github.com/anthropics/claude-code/security/advisories/GHSA-xq4m-mc3c-vvg3)
- [Claude Code Git email injection — GHSA-j4h9-wv2m-wrf7](https://github.com/anthropics/claude-code/security/advisories/GHSA-j4h9-wv2m-wrf7)
- [Claude Code settings injection — GHSA-ff64-7w26-62rf](https://github.com/advisories/GHSA-ff64-7w26-62rf)
- [Claude Code symlink escape — GHSA-vp62-r36r-9xqp](https://github.com/advisories/GHSA-vp62-r36r-9xqp)
- [Cursor MCP special-file injection — GHSA-4cxx-hrm3-49rm](https://github.com/cursor/cursor/security/advisories/GHSA-4cxx-hrm3-49rm)
- [Cursor editor settings injection — GHSA-vqv7-vq92-x87f](https://github.com/cursor/cursor/security/advisories/GHSA-vqv7-vq92-x87f)
- [Cursor `.code-workspace` injection — GHSA-xg6w-rmh5-r77r](https://github.com/cursor/cursor/security/advisories/GHSA-xg6w-rmh5-r77r)
- [Google Gemini CLI trust/allowlist — GHSA-wpqr-6v78-jr5g](https://github.com/advisories/GHSA-wpqr-6v78-jr5g)
- [Cline Kanban Server WebSocket advisory — GHSA-5c57-rqjx-35g2](https://github.com/advisories/GHSA-5c57-rqjx-35g2)
- [Kimi Code official changelog](https://www.kimi.com/code/docs/en/kimi-code-cli/changelog.html)

### 独立研究者披露

- [Check Point Research — OpenAI Codex CLI command injection](https://research.checkpoint.com/2025/openai-codex-cli-command-injection-vulnerability/)
- [JFrog Research — Kimi Code FetchURL SSRF, CVE-2026-17534](https://research.jfrog.com/vulnerabilities/kimi-code-is-vulnerable-to-a-fetchurl-ssrf-protection-bypass-via-dns-resolving-hostnames-and-redirects-cve-2026-17534/)
