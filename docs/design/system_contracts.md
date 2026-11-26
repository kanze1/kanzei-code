# Kanzei 系统合同

- 身份：live_design
- 核对基线：2026-11-26，精简共享底座与纯中文极简桌面外壳收敛基线。
- 职责：维护当前所有权、不变量和源码入口；不维护实现进度、文件规模、测试数量或版本发布叙事。

## 组件与依赖

| 所有者 | 能力与源码入口 |
| --- | --- |
| [kanzei-base](../../crates/kanzei-base/src/lib.rs) | 原子文件、锁、路径、指纹、写日志和测试原语 |
| [kanzei-llm](../../crates/kanzei-llm/src/lib.rs) | 模型协议、认证、流事件和用量 |
| [kanzei-harness](../../crates/kanzei-harness/src/lib.rs) | agent/tool/skill/context/permission 合同、注册与执行门禁 |
| [kanzei-core](../../crates/kanzei-core/src/lib.rs) | 运行循环、会话与任务事实、恢复、压缩和公开领域服务 |
| [kanzei-memory](../../crates/kanzei-memory/src/lib.rs) | Markdown 记忆、DocStore、检索、准入、生命周期和回放评估 |
| [kanzei-tools](../../crates/kanzei-tools/src/lib.rs) | 文件、Shell/进程、纯净 Git、worktree、沙箱、公网抓取检索、CDP 浏览器、子代理与记忆整理 |
| [kanzei-runtime](../../crates/kanzei-runtime/src/lib.rs) | CLI 与桌面端共享装配；组合能力，不被能力 crate 反向依赖 |
| [kanzei](../../crates/kanzei/src/cli/mod.rs) | kz CLI：参数、项目身份、记忆运维及运行收尾 |
| [kanzei-app](../../crates/kanzei-app/src/main.rs) | 单进程内嵌桌面端：Tauri、强类型 IPC、聚合引导、流式合帧、自动更新与 Vite + TypeScript + React 18 前端 |

调用方向见[系统边界图](../architecture/00_system_boundaries.md)，一轮执行见[运行图](../architecture/01_runtime_loop.md)。完整 crate 图从 Cargo 清单生成。调用者直接依赖能力拥有者；`kanzei-core::store` 私有，对外通过公开领域服务与类型访问。`kanzei-app` 和 `kanzei-runtime` 负责组合，不吸收能力层的业务所有权。

## 状态与持久事实

| 事实 | 真源与约束 |
| --- | --- |
| 当前代码树 | 当前 `cwd` 工作树；文件编辑保存绑定磁盘内容指纹（`expectedHash`） |
| 长期记忆 | `.kanzei/memory/` 与全局目录的 Markdown；检索索引、图（`refgraph`）和目录投影可重建；`.kanzei/memory/*` 受硬 deny 保护 |
| 会话、任务与运行事件 | `kanzei-core` 的持久领域服务与 SQLite 事件；UI 数组不拥有运行事实 |
| Skills | 全局 catalog；按需读取正文，不按项目另建发现目录 |
| 自定义 Agent | 内置模板 → 用户全局 Markdown → 内容已批准的主项目 Markdown；只定义提示词/参数，不增加工具授权 |

所有异步结果携带会话与项目身份。切换视图、展开/收起文件抽屉或查看记忆不改变既有执行归属。关闭、归档、停止和永久删除分别使用其生命周期，不能靠清空界面代替持久操作。

## 档位与工具面

共享装配由 `kanzei-runtime::build_harness` 统一提供，CLI 与桌面端共用同一份能力投影：

- **开发档位（`dev-pair` / `dev-auto`）**：
  - 常驻工具（12 个 + 核心 `task` = 13 个）：`read`、`write`、`edit`、`insert`、`glob`、`grep`、`bash`、`git`、`webfetch`、`websearch`、`memory_search`、`tool_search`。
  - 延迟发现工具（8 个）：`browser`、`memory_note`、`memory_stats`、`process`、`sandbox`、`files`、`skill_list`、`skill_read`。
  - 记忆写工具（`memory_add`/`memory_update`/`memory_merge`/`memory_archive` 等）不进入主 Agent 工具面，保持写读分离。
- **无项目对话（`General`）**：显式加载全局配置与全局偏好，末尾投影裁剪项目级能力，不从进程 `cwd` 自动发现项目。
- **只读档位（`Readonly`）**：依据 `ReadOnlySupport` 统一排除未审计的写/命令工具；写与命令族被整体摘除，不可由运行策略、会话放行或自定义 Agent 正文升级。
- **隔离档位（`Isolated`）**：仅装配独立只读与 `sandbox` 能力，不安装宿主命令、联网、项目自定义来源、全局技能或项目记忆召回。

延迟工具由 `kanzei-harness::tool_search` 提供有界目录（最多 4000 字符）和按需加载合同；已加载 schema 仅在本轮运行内有效，新轮回到档位常驻工具面。

Skills 目录按模型步骤刷新，仅列出已启用且允许模型选用的条目（6000 字符预算）；正文通过 `skill_read(id)` 按需读取，版本变化时显式报错不拼接旧元数据。显式 `$source:name` 技能正文单列为本轮 `skills/invoked` 运行上下文，不拼入持久消息或提升权限。

稳定 system 只包含 Agent 指令、静态能力约定与固定工具目录。运行身份、时间、起点快照、完成契约、任务记忆提示、已调用技能与每步刷新源作为请求级运行上下文投射，不进入持久对话消息。

## 执行、并发与安全边界

运行装配进入 [runner](../../crates/kanzei-core/src/runner/drive.rs)。拒绝、需要修正、等待用户和执行失败保持严格区分，不通过重试或吞错伪造成功。上下文压缩只改变模型可见投影，原始事件与可追溯引用完整保留。

消息来源独立于供应商的 `user/assistant` 角色，贯穿准入事件、输入队列、实时运行、历史事实与导出。持久输入队列同时承载人类输入与后台回传；排队计数与撤销控件只作用于明确标记为 Human 的记录。

会话生命周期由 `kanzei-core::ConversationSupervisor` 唯一裁决：运行中发送默认插入当前任务，显式排队等待下一轮；停止先发 `kz:stopping`，待后台写入任务、所属资源和会话执行锁全部结清后才公布 `kz:idle`。`process_list.runtime` 与 `kz:lifecycle` 共享 `actor_epoch/revision`。

并行写任务通过独立 Git worktree 隔离，主项目资产（如项目记忆与代理配置）指向主根。纯净 Git 工具仅保留代码卫生门禁（`cargo check --workspace --all-targets`、`cargo fmt --all -- --check`、`cargo clippy --workspace -- -D warnings`）。

同一进程内的本地推理调用按服务地址共享准入：交互运行优先，后台维护请求（如记忆整理）主动让路并等待交互结束后继续，不重放已完成工具调用。

## 界面、单进程生命周期与文件上下文抽屉

桌面端采用**单进程内嵌**架构（无后台独立守护进程与跨进程回环 TCP 桥接）：窗口与应用运行时共享同一进程，通过 `app_bootstrap` 单次聚合返回首屏引导配置，关闭窗口或安装更新前经 `app_lifecycle::app_shutdown` / `shutdown_for_update` 优雅静止全部活跃会话与后台终端后退出。

前端采用 **Vite + TypeScript + React 18**（`crates/kanzei-app/web/`，产物 `crates/kanzei-app/dist-web/`）：
- 命令签名由 `crates/kanzei-app/build/ipc.rs` 从 `command_registry.json` 与 Rust 函数签名生成，经 `scripts/sync-ipc-contract.mjs` 同步至 `scripts/ipc-commands.json` 与 `crates/kanzei-app/web/src/ipc/generated.ts`，实现编译期与运行期双重参数校验；
- 对话流使用 `conversation_page_messages` 尾窗分页（默认最近 100 条，触顶向前翻页）与 `@tanstack/react-virtual` 虚拟滚动；后端 `UiEventSink` 对高频 `kz:text` / `kz:reasoning` 做 16ms 合帧并在控制事件与 `Drop` 前强制刷新保序；
- 工作台总览、记忆管理、Skills 技能库与全局设置四个二级视图均通过 `React.lazy` 按需分包加载，KaTeX、Mermaid、ForceGraph 静态离线资源按需加载；
- 移除独立整页文件树/Monaco 编辑器视图，所有代码阅读、Diff 对照与轻编辑统一收口至对话右侧伴生抽屉（`ContextDrawer.tsx`）：
  - 聚合本轮触及文件与当前工作树 Git 改动文件；
  - 对话内 `path:line` 链接与输入框上下文带改动条点击直接打开抽屉定位原文或 Diff，不切离对话视图；
  - 单文件 Diff 由后端 `file_git_diff` 提供，经 `files_edit::resolve_in_root` 校验路径边界防止目录逃逸；
  - 轻量编辑保存统一复用 `file_write` 通道，强制校验磁盘内容哈希（`expectedHash`），外部修改时显式报错不覆盖，复杂重度编辑一键交由外部 IDE 打开（`open_with` / `reveal_path`）。

外部浏览器工具（`browser`）统一通过外部 Playwright CDP 连接执行，页面观察与截图保留实际来源范围。

## 记忆与公网传输

外部记忆系统完整保留：
- 候选、已确认、失效与归档记忆分别管理；来源、内容修订、使用机会、实际读取、采用/纠正反馈和保留/归档作为独立事实记录，反馈不跨修订套用。
- `core/preferences` 按步骤刷新已保存偏好（项目库取无区域绑定 active preference；全局库仅取 `source=user` 显式偏好），整条装入或整条省略。
- 任务记忆提示与失败指纹召回按有界字符预算整条列出；读取错误、超时和检索不可用明确记为 `unavailable`，与零命中 `miss` 区分。
- 自动记忆整理仅提出候选并按精确 `note_id` 核销收件箱草稿，不提供整箱清空工具。

公网抓取与检索统一通过 `public_http`：域名使用配置的 HTTPS JSON DNS（默认 AliDNS）解析 A/AAAA，引导连接与源站连接（含代理 CONNECT）均固定到已验证公网 IP，重定向逐跳复核，禁止回环、内网与云元数据地址。`webfetch` 与本地 `read` 共用按字节识别的 PNG/JPEG/GIF/WebP 判定，原始图片字节保存为证据并经视觉通道交付（动图仅取首帧并显式告知）。

## 演进与验证

新能力先修改拥有者及调用方，再同步更新本文与[使用手册](../使用手册.md)。本地验证与发布门禁见[贡献指南](../../CONTRIBUTING.md)，工程约束见 [AGENTS](../../AGENTS.md)。
