# 桌面服务前端和工程交付

## 前端真实组织方式

Tauri 配置直接将 `ui` 作为 frontendDist，没有打包构建步骤。index.html 以 `type=module` 加载入口；JS 模块用显式 import/export，同时仍有过渡兼容桥。package.json 是 eslint 和浏览器验收依赖壳，不是业务服务端。

`01-core`、`03-shell`、chat、events、compose、sessions 等存在相互 import，初始化顺序需要 defer，部分可变绑定靠显式 setter 更新。ESM 化已经发生，模块之间职责独立尚不能仅凭文件拆分认定完成。

| 模块族 | 当前职责 |
|---|---|
| 00-surface、00-frame、brand | 窗口表面、对话框、toast、布局和品牌 |
| 01-core、02-i18n、03-* | Tauri 桥、偏好、翻译、项目与研究空间、视图导航 |
| 04-* | Markdown、Mermaid、结构化输出和错误解析 |
| 05-* | 对话、思考、子代理 transcript、工具摘要和交付 |
| 06-* | 活动、任务侧面板、运行状态和侧面展示策略 |
| 07-events、08-* | 事件归约、发送、自动续跑、模型选择 |
| 09、20 | 会话和开发线、worktree、并列运行状态 |
| 10 至 15 | tracker、工作单元、概览、决策复核、记忆和规范 |
| 16、17、18 | 设置、按需手机/记忆/语音加载器、Monaco 文件编辑、启动恢复和更新检查 |
| 19-* | 研究路线图、课题、AUTO、引用、LaTeX 和架构浏览 |
| 21 | 科研调色板 |
| 22-* | 星座/神经流/OC 动画、性能、可见性和偏好 |
| 23-* | 语音控制、录音、播放与交互状态 |
| 24-* | 记忆图谱和应用内网页预览 |
| gallery、oc-studio | 素材/伴随角色展示入口，属于表现层与素材调试 |

index.html 中主要导航包括 chat、lines、documents、research、memory、files、arch、metrics、workspace、settings。research 按工作空间显示；不同空间允许的视图由 workspaces 控制。

## 事件怎样进入 UI

后端 `RunEvent` 先归约到 UI、typed、trace、metrics 四个 sink；UI 经 kz:* 事件投影到对话、活动、子代理和状态。事件必须携带正确 session 身份，后台会话结束与当前可见会话分别处理。

`kz:experience` 提供记忆/研究/运行体验事件统一词表；兼容的 kz:* 事件仍存在。图谱和动画是事件消费者，不应承担“该不该派工作”的判断。

IPC 的 Rust 结构、手工 JSON、JS 字面量和 scripts/ipc-contract.json 共同构成契约面。移除三个容器清单命令后，全量提取匹配 163 声明与注册；计数相等只证明 command 接线存在，不证明参数/返回类型全正确。

一个具体断点：`ui/06-activity.js:413` 调 `run_tool_process_stop`，main 无注册且源码无实现。这不是更新命令 rename 的情况；update_check/update_install 的 Rust 函数有明确 rename，已被索引识别。该活动区后台进程停止按钮需要修复接线并验收。

## 文件浏览和编辑

文件扫描来自 tools/files，而不是 UI 另扫一套。file_preview、file_stat、file_write 共用路径规范化，拒绝穿越、绝对路径和解析出根外的目标。文件编辑保存校验 expected hash，并创建 file checkpoint，避免覆盖外部修改。

托管路径列表由 MANAGED_ROOTS 提供；需求、缺陷、测试、架构、规范和记忆通过专用页面/工具编辑。Monaco 是第三方编辑器资源，不应纳入手写业务模块重构。

用途标注来自 AI 结果；文件存在、大小、hash 来自文件系统。这两层不能混作同一可靠性。

## 应用内网页预览

preview_open 创建 Tauri child WebView。Windows 通过进程内 WebView2 CallDevToolsProtocolMethod 驱动 DOM、输入、console 和截图，不需要生产远程调试端口。面板对当前会话可见且匹配时 browser 使用 pane，否则使用 headless 辅进程。

本地 HTML/代码片段由 `preview_server` 在 loopback 随机端口和 token 路径提供。CDP、面板生命周期、主窗口焦点、DPI 与 bounds、设备尺寸、控制台缓存分别放在 preview 子模块。console 环形缓冲保留 seq、时间、等级、URL 和行列。

截图分预览 browser 截图和桌面 ui_screenshot：后者获取真实运行窗口像素。返回工具图像或交付图像都有受限路径/类型/大小校验。

当前 main 仍保留 `KANZEI_E2E_CDP` 的条件调试口入口；生产面板本身走进程内 CDP。这两件事不能用一句“已废除 CDP”概括。

## 手机桥和 PWA

桌面 mobile.rs 提供本地 HTTP 桥，默认 loopback，可切 LAN。桌面生成一次性配对码，设备换取独立 token；单设备撤销后立即失效。PWA 的 index/app/sw/manifest 处理手机端显示、消息、审批与离线表面。

手机消息写入对应会话输入队列，桌面用 kz:mobile-message 刷新。审批和通知按 delivery cursor 拉取，避免各设备重复消费。电脑是实际 Agent 执行端，手机是遥控界面。

手机设置由 18-optional-ui.js 首次展开时加载 16-mobile.js，服务仍需主动启动。系统通知默认关闭；勾选后启动桥前检查 KDE Connect CLI，桥停止后关闭通知，普通运行事件不探测其依赖。没有实现任意公网推送/跨互联网服务，不能把 LAN 可配对写成云端移动托管。

## 本地语音

首次检查或开启语音时才加载界面与 controller；默认启动不读取语音设置、不启动服务、不轮询。AudioWorklet 采集，voice/controller 管会话、转写、朗读、取消和状态，关闭后停止归属轮询。Rust 只连接配置 loopback service，验证设置，按 session/request identity 管 CancellationToken，取消 A 会话不终止 B 会话。

voice_service 读取 `voice-launcher.json`，核对 port、绝对 program、cwd 和参数，隐藏启动已登记 runtime，等待 ASR 与 TTS 都 ready。缺登记或模型未就绪就返回诊断，不能仅凭 spawn 成功显示语音可用。

scripts/voice 包含环境准备、模型下载、CUDA/doctor、launcher 登记、service、启停与测试。服务代码提供 health、transcribe 和合成代理等接口，桌面不是自己实现语音模型。是否在此机可用需要实时 health 与音频验收，本次未检查。

## 视觉和素材

星座、神经流、OC companion、clip director/renderer、detail shader、layout 和 performance 都在 22 模块族。轻量状态反馈始终可用；复杂视觉由统一开关控制且默认关闭，开启才 import 22-visual-runtime.js，关闭时销毁动画循环、观察器和监听。它们将运行状态映射到动画，不决定运行事实。

OC H3 的 59 个素材制作文件已移到 extras/visual-authoring/oc-h3；其他素材辅助脚本在 scripts/oc-*；参考 prompt、PNG/MP4 是资源，代码逻辑在 renderer/director。品牌生成脚本和 SVG/安装图标属于独立资产链，不与核心执行状态机合并。全部资源路径和现有文档在 [11](11-documents-assets.md)。

## 设置与认证

settings.rs 管全局配置、provider、认证状态、权限和开关；model_config 管项目及本线模型覆盖。TOML patch 应保留未知键，schema default 保持旧配置兼容。Agent 目录是只读来源投影，实际解析仍由 Harness 完成。

Codex 登录复用 CLI；Claude 订阅路径当前明确停用，需要 API Key。Ollama fast 模型就绪探测和服务保活有独立 fast_model 模块。模型列表探测不是一次模型实际推理验证。

## 构建更新和发布

桌面启动顺序先识别 verification worker，再处理 pending update、同步 bundled CLI、清孤儿 WebView、建窗口、装 UI 探针/预览/验证 monitor、启动 fast service 保活。worker 隐藏入口不会先创建一个普通应用窗口。

update 检查 GitHub Release，下载和 pending 接力更新；桌面唯一安装位是 `%LOCALAPPDATA%/kanzei/kzapp.exe`，cargo bin 的 kzapp 是转发启动器，CLI 独立同步。运行中能落 pending，不应把安装器 exit 0 直接当作 exe 已替换。

release.ps1 负责本机测试、构建和安装。package.ps1 负责核对提交范围 Ack、证据、打包和可选 Publish，发行资产含 NSIS。它们是不同交付通道；本轮九项整理已交付 `build-59f17d67`，范围和验收见 [整理验收](../2026-10-02-web-cleanup.md)。

## 当前验证政策

verify-policy 有 15 个检查键。默认 targeted 按改动路径裁剪 Rust、前端和图；`-Full` 或 KANZEI_VERIFY_FULL 走完整模式。package 校验 commit 精确等于 HEAD、all_pass、full_verify、mode=full、skipped_steps 空及所有必需键存在，targeted 证据不能直接打包。

verify.ps1 的 clippy 当前不带 all-targets，test 编译测试代码；CI clippy 带 all-targets。项目规范里“所有提交固定全量 all-targets”和“十步全绿”的口径已落后于代码，应按当前政策改写并明确测试代码 lint 谁执行。

CI `.github/workflows/ci.yml:3` 只有 workflow_dispatch。当前不存在每次 push 自动全量兜底；CI 设计文档、A-009/A-010 已于 2026-10-02 校正为手动事实。CI 的 ui runtime 调用仍未带 verify.ps1 使用的 `--experimental-vm-modules`；smoke 使用 vm.SourceTextModule，源码中未见自动补参数重启。此处需要统一命令，再用 CI 日志确认。

已有 smoke 分布在 UI runtime/lint/a11y/i18n/markdown/connectivity/diagram、parallel lines、IPC、memory graph、voice、preview、constellation、decision console、verification worker、release policy 等脚本。它们各自覆盖静态、模拟、浏览器或 worker 层，不能统称为已安装桌面端到端验收。

## 仓库中的交互原型和审计辅助代码

`docs/prototypes/workbench` 用内存中的 projects/reviews/preferences 和按项目隔离的草稿演示工作台；`decision_console` 演示项目筛选、决定复核和偏好关系；`softwire` 演示运行组件、归属和工具详情。各目录 serve.mjs 提供本机静态服务。它们不接真实 Tauri 执行接口，刷新会重置示例状态。

`docs/reports/2026-09-26-*` 的 Python/Rust 是上下文导出和 harness 回放辅助代码；`docs/assets/icon-concepts/*.html` 是品牌概念展示。它们同样纳入 08/10 的 docs-auxiliary 域，避免在整理时误当作生产模块或遗漏可用脚本。
