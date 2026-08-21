# Requirements

## R-283 自举二期系统升级编排:research/memory/运行体验/动画/voice 依赖和联合验收 [doing]
- 优先级: P1
- 复杂度: 大
- 标签: 架构 流程 自举
- 来源: 2026-08-17 用户确认「自举一期应该差不多可以算结束,需要对已有系统全面升级改造」并要求详细拆解需求依赖加入当前自举清单。
- refs: R-221 R-276 R-277 R-284 R-285 R-286 R-287 docs/design/phase2_system_upgrade.md
- 内容: 以 docs/design/phase2_system_upgrade.md 为二期真源维护五批:批1 设计/依赖/需求映射;批2 P0 事实恢复(D-409 与 memory backlog);批3 research+memory 引擎 E2;批4 animation+voice E3/E4;批5 真实联合闭环(语音研究请求→research 报告→dev 实施→memory 晋升→动画可见→定制声音复述)。
- 边界: 本条只做跨条目编排、依赖和结项门禁,不重复实现 R-221/R-277/R-286/R-287;一期 loop/dev 主链进入稳定维护,存量缺陷仍按原条目处理;每个边界分别报告静态、测试、WebView2、provider、安装和工作树状态。
- 验收: ①所有二期子条目有明确依赖、批次、风险、数据边界和证据等级;②Wave 0～4 各有 Go/No-Go 记录;③联合闭环按 session/topic/memory id 可回溯;④二期结项时 requirements/defects/tests/实现无相互矛盾状态。
- 批次: 3/5
- 进展: 2026-08-17 用户确认：R-283 是二期系统升级总控条目，具体工作必须按不同任务展开为独立条目。现有子条目为 research(R-221/R-276/R-277)、记忆晋升与遥测(R-286)、统一事件契约(R-284)、金色神经流(R-285)、voice(R-287)；Wave 0 先修当前 dev 缺失的 D-428。R-283 保持 3/5，只在子条目形成真实证据后更新门禁，不再被取活为实现任务。
- 状态: doing
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-283
- observed_head: 148386f3d467b701f334932b2bfc85bbcfcea475
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786925390809
- 依赖: R-284 R-287
- 阻塞: 
- 对账: 2026-08-18 对账:原依赖 D-428/R-221/R-277/R-286 已全部关闭归档,依赖收缩为 R-284 R-287;phase2 §6 Wave 门禁记录引用的状态已过期,由 R-303 文档批次订正后再更新门禁

## R-284 运行体验事件契约:统一 session/tool/memory/research/voice 的事实投影和瞬时表现事件 [doing]
- 优先级: P1
- 复杂度: 大
- 标签: 核心 后端 前端
- 来源: 2026-08-17 二期升级依赖分析;主对话已有 kz:* 事件,但 memory/research/voice 无共同事件词表,动画若直接从 DOM 推断会形成影子状态。
- 依赖: R-242
- refs: R-277 R-285 R-286 R-287 docs/design/phase2_system_upgrade.md docs/design/session_state_and_line_runtime.md
- 内容: 按 phase2_system_upgrade.md §5.4 分四批。批1 定义 snake_case 事件包络、持久事实/瞬时表现/高频 delta 三类边界与词表。批2 memory/research/voice 后端生产者接线,统一 project/session/run/topic/entity 归属。批3 前端按归属先归并 store 再分发 animation/audio/工作台,重放幂等。批4 高频 delta 合并、未知事件诊断、重连恢复和跨会话回归。
- 边界: 不改变 R-242 的 session 真源;瞬时表现事件允许丢帧且不写长期数据库;动画和音频不得反向决定业务状态;未知事件不崩 UI;事件字段不混用 camelCase,第三方/旧事件在适配层转换。
- 验收: ①词表和 JSON 契约有 schema/单测;②同一 memory promote/research verify/voice state 事实可重放且不重复副作用;③后台会话事件不驱动当前会话动画;④text delta 压缩后长回复无事件风暴;⑤重连从持久事实恢复状态,不依赖错过的表现事件。
- 批次: 4/4
- 状态: todo
- 进展: B4 已实现并提交 `605c6413`，按验收逐条对账：① 既有 B1 `crates/kanzei-core/src/experience_events.rs:20-41,97-119` 提供 snake_case JSON schema/validate，`T-1786922726586` 覆盖 schema 与 legacy 归一化；② memory/research 事实由 B2 `crates/kanzei-core/src/experience_events.rs:126-164` 幂等落库/回放，`crates/kanzei-app/ui/13-memory.js:18` 消费 `experience_facts` 恢复前端投影，`T-1786922726586` 覆盖 memory/research；验收降级：voice state 原文→当前仓库无真实 ASR/TTS/VAD 生产者，不能伪造 voice 证据，保留 R-287 缺口；③ B3 `crates/kanzei-app/ui/01-core.js:78-100,180-213` 按 session/topic/entity 归并并隔离后台 session，`T-1786922726593` 回归通过；④ B4 `01-core.js:102-150,197-200` 在一帧内合并 text delta、携带 delta_count，未知事件只诊断，`scripts/ui-runtime-smoke.mjs:1447-1481` 断言三段文本合为一次表现事件，`T-1786922726593` 通过；⑤ B4 `01-core.js:152-160` + `13-memory.js:18` 从持久 facts 恢复且 event_id 幂等，`scripts/ui-runtime-smoke.mjs:1483-1495` 断言首次恢复/重复恢复，`T-1786922726593` 通过。最终验证：`T-1786922726593` 六条前端冒烟全通过，`T-1786922726594` cargo test -p kanzei-app 231 passed。当前仍保持 doing：voice state 生产者缺口未满足，不能关闭 R-284。
- observed_head: 605c64135451bae1bd3128ef2a20666b98d57504
- observed_worktree_hash: fnv1a64:abf42289ad631ab3
- recorded_at: 1787246367339
- 批次表: B1 契约与事件包络：snake_case、持久事实/瞬时表现/high-frequency delta、归属字段和 schema；B2 后端生产者：memory/research/voice 事件接线与真实持久事实映射；B3 前端归并：按 session/topic/memory 归属入 store，再分发动画/音频/工作台；B4 压缩与恢复：delta 合并、未知事件诊断、重连回放和跨会话回归。
- 停车: B1-B4 已完成并通过验证；剩余 voice state 生产者属于 R-287 的真实 ASR/TTS/VAD 范围，本条主动让位 R-287，待 voice 生产者落地后恢复验收②；恢复人:agent

## R-285 金色神经流:主对话与记忆层的真实事件驱动动画 [doing]
- 优先级: P2
- 复杂度: 中
- 标签: 前端 视觉
- 来源: 2026-08-17 用户愿望「记忆层有流动的黄色神经网络,结合主对话运行触发动态效果」;同轮明确由当前主会话完成,自举模型不负责视觉 taste。
- 执行者: 主会话(SOL)。自举循环只可运行机械检查、补明确回归测试或修已登记缺陷,不得自行重做构图、颜色、运动节奏和视觉层级。
- 依赖: R-284(仅批3/4硬依赖;批1/2可使用现有 kz:* 与记忆页真实操作先行)
- refs: R-276 R-286 R-287 docs/design/phase2_system_upgrade.md docs/design/app_icon.md
- 内容: 按 phase2_system_upgrade.md §5.5 分四批。批1 Canvas 2D 金色神经场、主题 token、ResizeObserver、DPR 上限、后台暂停和 reduced-motion。批2 接现有 turn/text/tool/done/error 与 memory refresh/search/consolidate/cleanup,完成呼吸/流动/结晶三种运动。批3 接 R-284 的 recall retrieved/injected、candidate promoted、research verified、voice listening/speaking。批4 设置开关、质量档位、WebView2 E3、窗口/长对话/性能/视觉回归。
- 视觉主张: 深暖近黑工作台中的低亮金色生命体;常态静息,真实运行才增强;主对话内容与操作状态始终高于动画。静态网络是运行场,不冒充知识图谱或伪造记忆关系。
- 验收: ①主对话 run→text→tool→done/error 的动画状态与真实事件一致;②记忆页真实搜索/整理/清理触发对应流动且失败不播放成功结晶;③后台会话不串到当前动画;④Canvas 不拦点击/选择/滚动;⑤reduced-motion 下无连续动画仍保留静态层;⑥暗/亮主题、800/1024/1280 三档可读;⑦WebView2 长回复和记忆页实测无明显帧率/CPU 回退并留截图或录屏工件。
- 批次: 2/4
- 进展: 2026-08-17 主会话完成批1+批2。新增 22-neural-flow.js:确定性神经拓扑、单 RAF 调度、活动/静息节流、ResizeObserver、DPR≤1.75、窗口隐藏暂停、reduced-motion 静态降级,实现呼吸/流动/结晶与失败阻塞四类表现;index.html 接主对话 Canvas 与记忆流舞台,style.css 使用 app 金色 token 且 Canvas pointer-events:none。真实接线:07-events.js 的 turn/text/reasoning/tool start/end/compacted/stopped/done/error;13-memory.js 的 snapshot/search/consolidate/candidate discard/cleanup,失败事件独立且不播放成功结晶。测试:T-1786922726035 前端全冒烟通过;T-1786922726036 真实 Chromium 1440/800 视觉验收通过。剩余批3=依赖 R-284 的原生 recall injected/candidate promoted/research/voice 事件;批4=设置/质量档/真实 WebView2 长会话性能与录屏。
- 状态: doing

## R-287 voice 语音交互:流式 ASR/VAD、语音回复、定制声音与打断 [todo]
- 优先级: P2
- 复杂度: 大
- 标签: 后端 前端 语音 集成
- 来源: 2026-08-17 用户要求接入语音功能并优先支持定制语音;技术调研结论为 chained voice:音频→ASR→现有文本 Agent→TTS。
- 依赖: R-284(voice 状态投影;设备/模型基准可先行)
- refs: R-285 docs/design/phase2_system_upgrade.md
- 内容: 按 phase2_system_upgrade.md §5.6 分五批。批1 Rust cpal/WASAPI 设备枚举、录音播放和 50 条中英/代码术语基准,sherpa-onnx 与 whisper.cpp 对照。批2 push-to-talk+partial/final 字幕回填输入框。批3 TTS provider adapter 与播放/暂停/停止。批4 经授权的 voice profile:CosyVoice 本地 sidecar、OpenAI Custom Voice、ElevenLabs 可替换。批5 VAD 自动收尾、barge-in、流式首包和 R-285 动画联动。
- 安全: 默认不保存原始录音;参考音频/consent 放应用数据目录且不进 Git/memory/普通日志;provider key 只在 Rust/后端;删除 voice 同时报告本地与云端结果;无明确授权不得克隆声音。
- 边界: 第一阶段不改为端到端 speech-to-speech,保留文本、工具、权限、审计和记忆链;模型下载/长时间 GPU 基准先由用户确认执行;任何 provider 激活必须有真实认证输出,配置或容器日志不算完成。
- 验收: ①真麦克风设备切换、录音和播放;②50 条基准报告 partial/final 延迟、CER/术语修正率、实时率与资源;③ASR 文本进入现有输入和权限链;④TTS 失败不阻断文本回复;⑤真实授权定制声音输出;⑥播放中插话能中断并恢复会话状态;⑦原始音频、consent、key 的存储与删除边界有测试。
- 批次: 0/5
- 状态: todo

## R-101 桌面端/前端 E2 测试 harness 与延期 E2 清单 [doing]
- 复杂度: 大
- 优先级: P2
- 归属: kanzei
- 背景: 多条缺陷按 conventions §1.2「可用即关闭」关闭,其验证增强项收拢至此,不再阻塞缺陷与需求推进;此前反复出现的阻塞原因是仓库无 package.json、无浏览器测试 harness,无法安全启动真实 Tauri UI。
- 验收: 按新路线保留完整延期范围，不在本条缩小平台或场景：①建立可启动真实 Tauri UI 的 Windows UIA E2 基座，失败非零退出、截图与断言；②逐项覆盖权限弹窗、pending ask、切项目复位、手写内容保留、run_task 收尾、停止与长会话响应。R-302 只完成路线选定与最小 E2，以上行为清单仍由本条后续批次交付。
- 拆批(2026-08-08 用户定调「拆出能先做的部分」): **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。**留待 R-086**——依赖会话事件路由的三条:D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位。基座 + 四条 E2 交付即可关闭本条,剩余三条并入 R-086 验收。
- refs: R-086
- 阶段: 3

- 标签: 流程

- 拆批: 2026-08-08 用户定调「拆出能先做的部分」: **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。基座 + 四条 E2 交付即可关闭本条;R-086 已于本轮按 §1.2 可用即关闭关闭,原「并入 R-086 验收」的三条桌面 E2(D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位)留在本条目验收清单执行。

- 进展: R-101 B3 已提交 d1cc0006:scripts/ui-desktop-uia.ps1:106-131 每轮按生产 AutomationId 查找 send/stop,失败回退真实名称并重新读取 UIA 节点;:213/:216 使用显式参数。T-1786922726472 默认真实 UIA 回归通过。 || 2026-08-20 B3 收口:用户关闭 kzapp 窗口后真实执行 -RunStopTest 通过——send→stop→settle 全链路 stop_requested=true/stop_settled=true,process_owned_by_test=true(冷启动自拉起,顺带修 D-564 冷启动 prompt 轮询),截图 464972 bytes;D-552/D-556/D-564 均已关闭。下一步:B4 按验收清单继续(权限弹窗、pending ask、切项目复位、run_task 收尾等桌面 E2)
- 状态纠正(2026-08-09): doing→todo。用户已挂起本条,实际不在推进中,却按旧 §1.1 口径占用 doing 名额,与 R-148 一起把 R-153 拒之门外(见 D-219)。恢复推进时再转 doing;挂起前提的小缺陷中 D-185/D-184 仍 open。

- 阻塞: 

- 批次: 3/6
- 技术路线: Windows 原生 UI Automation/真实 WebView2 用户路径已选定。基座通过 `scripts/ui-desktop-uia.ps1` 以 UIA 附着真实安装位 kzapp.exe、断言顶层 Window、通过生产 prompt 的 ValuePattern 写入/回读，并保存真实窗口截图；CDP/connectOverCDP 不再作为路线或验收条件。
- observed_head: a8e75106b629441cc19963dd5667aee07a74339a
- observed_worktree_hash: fnv1a64:00ea97ae7b316f67
- recorded_at: 1787168115069
- 取活依据: engine:唯一可执行 WIP 是 R-101，必须先恢复它
- 停车: 排队:B4 桌面 E2 体量大,排在收口类条目(D-504/R-242/R-296/R-299)之后恢复;恢复人:agent

## R-245 Tool Result Spill 与显式空间整理：完整 artifact、可恢复引用、无自动过期 [doing]
- refs: D-209 R-180 D-297 D-298 R-242 docs/design/deepseek_harness_upgrade.md
- 依赖: R-242
- 内容: 统一工具结果为 Inline 或 Spilled{preview,artifact_id,bytes,sha256,retrieval_hint}；read 优先指向原文件 offset/limit，bash、git、test_record、web 等完整原文进入与 state.db 同生命周期的 Git 忽略运行目录。提供存储与整理入口，按类别、会话、日期、大小预览占用，支持清理无引用 artifact；经风险确认后，用可恢复失败的删除计划物理删除已选会话的事件、投影和引用 artifact；并支持 SQLite checkpoint、VACUUM 与迁移备份管理。默认不自动过期。
- 复杂度: 大
- 批次: 2/5
- 来源: DeepSeek Harness spill policy、本地 state.db 输出分布统计，以及用户确认“不自动过期但需要显式整理入口”。
- 标签: 核心
- 边界: 任何事件仍引用的 artifact 不得被静默清理；整理前显示预计释放空间和不可恢复范围，执行后给清单与实际释放量。32 KiB 先做 shadow telemetry。普通会话删除保证产品不可检索且重启不复生；安全整理才处理 SQLite freelist、WAL 和含旧正文备份。当前库为 WAL、secure_delete=OFF、auto_vacuum=NONE，不能把 DELETE 行等同磁盘字节已擦除。弹窗必须区分仅删除与删除并安全整理，取消零写入；显式整理不是定时任务。
- 迁移与回滚: artifact 原子写入后再提交引用事件；任一步失败不得留下有效事件指向缺失文件。删除使用引用图和事务清单，失败可重试；schema 迁移前备份。关闭 Spill 可回到 Inline，但已有引用仍必须可读。
- 阻塞: 
- 验收: ①32 KiB shadow telemetry 不改变模型输入并产出按工具分布；②Spill 原文 sha256 与工具原输出一致，重启后可取回；③事件提交与 artifact 写入故障注入无悬空引用；④明确无自动过期任务；⑤整理入口列出总占用、数据库、WAL、freelist、artifact、无引用文件和迁移备份并支持 dry-run；⑥清理引用中 artifact 被拒，清理无引用 artifact 成功且释放量可核对；⑦删除弹窗列出会话事件、轨迹、草稿与 artifact，仅删除和删除并安全整理差异明确，取消零写入；⑧确认删除后事件、投影和引用 artifact 产品层不可检索且重启不复生，删除计划任一点失败可恢复重试；⑨安全整理仅在运行静止时执行，成功后 checkpoint、VACUUM 与备份处置可核对，busy 或失败不静默；⑩权限、路径逃逸、不可预测文件名和磁盘配额有测试。
- 优先级: P1
- 进展: B2 已实现、验证并提交 `112b2dc4`：① 既有 D-349 spill 与 B1 telemetry 保持不变，证据 `T-1786922726595`；② 新增只读 `SessionStore::open_read_only` 与 `storage_report`（`crates/kanzei-core/src/store/session.rs:338-427`），统计 state.db/WAL/SHM/page_count/freelist、tool-result artifact/shadow telemetry、state.db.v*.bak；`crates/kanzei-core/src/store/mod.rs:56-69` 定义 `StorageReport`，`crates/kanzei-core/src/lib.rs:34` 对外导出；③ 新增真实 CLI `crates/kanzei/src/cli/artifacts.rs:1-112`，经 `crates/kanzei/src/cli/mod.rs:14,54,89` 接入 `kz artifacts stats [--json]`，只读输出并明确不做 expiry/delete/checkpoint/VACUUM；④ `T-1786922726596` 覆盖 storage_report 单测、CLI 单测和真实命令，实测 state.db 171495424 bytes、WAL 3052952、SHM 32768、freelist 8771、迁移备份 1/93528064 bytes。验收⑤阶段性完成：数据库/WAL/freelist/artifact/迁移备份已列出；无引用 artifact 与 dry-run 尚未实现。D-616 已 fixed。下一步 B3：建立 artifact 引用图并提供无引用扫描与 dry-run 清理计划，禁止删除仍被引用 artifact。
- observed_head: 112b2dc40273584bba3d61ee65701d8bb99124e2
- observed_worktree_hash: fnv1a64:abf42289ad631ab3
- recorded_at: 1787247404126
- 停车: 本轮 defect-first 先收口 D-604；R-245 B2 已完成，B3 待 WIP 槽释放后恢复。恢复人:agent

## R-249 工具结果可返回图片:ToolOutput 承载 image part,打通图片读取与 UI 截图 [doing]
- refs: R-014 R-101 R-244 R-245
- 依赖: R-245
- 内容: 现状 `ToolOutput.content` 只有 String(kanzei-harness/src/tool.rs:178),任何工具都无法把图片交给模型;`Part::Image` 的三协议映射早在 R-014 交付,但入口只有桌面端用户附件(kanzei-app/src/state.rs:29)。本条把 ToolOutput 扩成可携带 image part,并打通两个消费点:①`read` 读图片文件(PNG/JPEG/WebP/GIF)按 media_type 编码返回;②UI 自检补截图通道——现有 `ui_probe` 窗口通道加 `screenshot`,让 ui_dom/ui_style 的结构读数配上真实渲染画面。
- 复杂度: 大
- 批次: 0/4
- 来源: 2026-08-14 三系统工具面对照(DeepSeek harness / Claude Code / kanzei):read_image 是唯一的能力硬缺口。桌面端 ui_dom/ui_console/ui_style 能读结构与数值但看不见渲染结果,对齐、遮挡、观感一类问题无法自查。
- 标签: 核心
- 边界: ToolOutput 是 harness 核心契约,R-244 明确要冻结「ToolOutput 公共契约」、R-245 要把它改成 Inline/Spilled 二态——本条**不得抢在 R-244 之前改这个结构**,否则必然返工。图片体积走 R-245 的 spill 口径,不在 ToolOutput 内联大 base64。不实现 UI 点击/输入/滚动(那是 R-101 的 E2 harness 范围),本条只做「看得见」不做「动得了」。deepseek_responses 协议当前丢弃 Image part,本条不负责补齐该 provider,但要在 provider 不支持时给出显式降级提示,不静默丢弃。
- 进展: 2026-08-14 批1 交付(1831239)。勘察修正了原条目的一处前提:`Part::Image` 的三协议映射早在 R-014 就通了,缺的只是**工具侧出口**,协议层零改动即可打通——不必等 R-244。实现:①ToolOutput 增 images 载荷(空 vec 与既有行为逐字节一致,53 处 `ToolOutput {` 里只有 4 个真构造点,其余是解构模式);②read 按 magic bytes 而非扩展名识图(PNG/JPEG/WebP/GIF),扩展名撒谎会让 media_type 与真实字节不符、provider 400 且报错指向请求体;③图片 Part 只能追加在所有 ToolResult 之后——Anthropic 要求 tool_result 块在 user 消息最前,而 results[i]↔calls[i] 由 note_step 的 debug_assert 锁着,中间也不能插;④provider 不支持时**在进 messages 前**降级为显式文本说明,判据收敛为 Route::supports_images() 与 client.rs 硬拒绝共用一处。新增 10 条测试。 || 2026-08-14 批2 交付:新增 ui_screenshot 工具(kanzei-app/src/screenshot.rs)。实窗验证三轮才对,两次假绿都值得记——①未声明 DPI 感知时 GetWindowRect 返回虚拟化坐标(2582px 的窗口报成 1295px),抓到的是横跨多个窗口的错误区域;②改用正确矩形后,屏幕 DC 抓取拿到的是压在上面那个应用的界面(kzapp 被完全遮挡),内容丰富所以 looks_blank 一路放行。两次都是「测试通过但抓的不是那个窗口」。最终改用 PrintWindow+PW_RENDERFULLCONTENT 离屏渲染,免疫遮挡,在完全被盖住的状态下抓到 kzapp 完整界面并经人眼与用户实拍逐项比对一致;屏幕 DC 仅在 PrintWindow 失效且本窗口为前台时作回退,不是前台宁可报错——返回别人的界面比返回错误坏得多。测试记录 T-1786705800。 || 2026-08-16 复核:批1 已解除;批3 的依赖 R-244 已 done 并归档(Tool Pipeline 契约已冻结),只余 R-245 确定图片类 artifact 的 spill 落点,而 R-245 自身仍等 R-242。当前 park 的唯一原因是 WIP 槽由 R-195 持有(用户 2026-08-16 指定)。解除动作: R-195 关闭后清本字段直接续做批2。解除人: agent(批2)/ 依赖自然解除(批3 等 R-245)。 || 2026-08-16 让位:本轮按队列顺序取 R-186(P0 队首),本条 doing→todo 让位,待 R-186 交付后按队列轮转;批1/批2(ui_screenshot/read 识图)已交付,剩余批3 等 R-245(R-242 完成后才解)。
- 阻塞: 
- 验收: ①read 读 PNG/JPEG/WebP/GIF 各有定向测试,media_type 正确,非图片文件走原文本路径无回归;②ui_probe screenshot 返回的图片能被模型消费,桌面端实测有轨迹;③provider 不支持图片时有显式降级诊断;④图片 artifact 走 R-245 spill,ToolOutput 不内联超阈值 base64;⑤R-014 既有附件路径逐条无回归;⑥ToolOutput 结构变更后既有全部工具返回路径编译通过且行为不变(机械核验)。
- 优先级: P1
- observed_head: 98d7a586f38a09f5b449b75b7a3c93c62d01852f
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786835811870
- 状态: todo

## R-264 前端迁移原生 ESM(勘察已完成,方案见 docs/design/ui_esm_migration.md) [todo]
- refs: docs/design/ui_esm_migration.md R-142 R-154
- 优先级: P2
- 复杂度: 大
- 标签: 前端 流程
- 内容: ui/*.js 现为 21 个经典 script 共享全局作用域,靠 gen-ui-lint-globals.mjs 生成的白名单补 no-undef。迁到原生 ESM(<script type="module">,仍零构建步骤)可得真模块作用域并删掉整套补偿机制。**动工前必须先重建测试 harness**,顺序不可颠倒——详见设计文档 §二/§四。
- 前置: 无(但内部三道前置 B1/B2/B3 必须先于任何前端文件改动完成)
- 为什么是这个形态: 不上打包器。打包器的收益(minify/tree-shake 业务代码)在本仓不成立,而代价是 devDependencies 从 3 个变几百个、cargo build 经 beforeBuildCommand 依赖 npm 构建、六个冒烟脚本的加载模型全部重做。原生 ESM 拿到全部核心收益且仍无构建步骤。
- 边界: 不删 vendor/monaco/basic-languages(独立决策,与本条无关);不引入打包器/TypeScript;不借机重构业务逻辑,迁移期间只改模块边界。
- 来源: 2026-08-15 用户提出「前端改成打包呢」。勘察(21 文件逐文件审计 + index.html 专项 + 外部依赖专项)结论:前端本身不是障碍(587 个真顶层符号、零重名冲突、零内联事件处理器),阻塞全在测试 harness——ui-runtime-smoke.mjs 的 6799 行断言建立在 vm.runInContext 逐文件跑经典脚本之上,ESM 下整体作废;且 ui-sources.mjs 修好正则后会出现「三个冒烟静默变绿」的失效模式。同轮用户问「做了对自举有收益吗」,结论是没有:ESM 不影响 cargo 任何耗时,前端六个冒烟合计约 4 秒;唯一收益(模型读代码时 import 自带溯源)已被 20467db 修好白名单后大体覆盖。故降为 P3 留档。
- 验收: ①B1 ui-sources.mjs 改为遍历 ui/*.js 目录并带文件数下限断言,不再解析 HTML 取清单;②B2 ui-runtime-smoke.mjs 换用可跑 ESM 的执行模型,且保住「逐文件执行以复刻浏览器多 script TDZ 语义」这一能力(设计文档 §二 B2 说明为何不能丢);③B3 __kzTest 钩子改为 08-compose.js 显式 export,冒烟改 import 取用;④以上三条完成且 6799 行断言全绿之后,才开始逐文件迁移,每迁一个文件跑一次全套六个冒烟;⑤迁移完成后删除 gen-ui-lint-globals.mjs、ui-lint-globals.json 及 ui-lint-smoke.mjs 的清单同步校验,eslint.config.js 改 sourceType: "module";⑥设计文档 §三 表格里 10 处顶层跨文件读与 6 处 typeof 守卫逐条改为显式 import 并在验收中点名。
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-264
- 批次: 3/4
- 进展: 批2 完成(6c90aba)+工具链(ef89d20/e80114a/66098a6/634c9dc/b9ac558)。**批3 跨模块写全覆盖推进(2026-08-16,已提交 98d7a58 并 push)**:①renderProjects 完整 setter 化(currentProject/activeProcessId/activeSessionId,03-shell setter 定义+09 import 固化);②14-docs-actions documentsKind/dependencyViewOpen setter 化(12-docs-pages setter 定义固化);③工具链特殊修复点完善(languageSelect const→let+defer/value 并入 defer/09 setter import+赋值/03-shell setter 定义/12-docs-pages setter 定义,迁移重跑不丢失);④冒烟时序适配:DOMContentLoaded 触发后 await flush(20) 让 async 初始化推进,classic 路径不受影响(六冒烟全绿)。**新定位**:withSessionRender(R-267,01-core)写 5 个跨模块状态(currentAssistant/currentReasoning/currentReasoningHead/renderingBackground/activePane)——设计文档 §三 只列了读没列写,是深层持续工程。已回退 ui 到 HEAD(批2 稳定六冒烟全绿)。 || 2026-08-16 让位:本轮按队列顺序取 R-186(P0 队首),本条 doing→todo 让位,待 R-186 交付后按队列轮转,届时做剩余批4(withSessionRender setter 化、B3、defer 时序与冒烟断言适配、删补偿)。P3 留档(设计文档自述对自举无收益)。
- observed_head: 98d7a586f38a09f5b449b75b7a3c93c62d01852f
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786835812161
- 状态: todo
- 阻塞: 排期停车:等待 R-242 主线完成后按队列恢复批4;原点名的 D-428/R-221/R-277 已于 2026-08-17 前后全部关闭(2026-08-18 对账)。解除人:agent 按队列优先级恢复。
- 对账: 2026-08-18 用户拍板 ESM 收尾「做完」,原 P3 留档提级 P2;剩余工作=批4(withSessionRender 等 5 处跨模块写 setter 化、B3 __kzTest 显式 export、defer 时序与冒烟断言适配、删除 gen-ui-lint-globals 补偿机制);动工前先修 D-498(冒烟执行顺序与浏览器不一致),否则逐文件迁移的冒烟证据不可信;设计文档状态过期由 R-303 订正

## R-281 子代理面板重做成完整对话读取器:看到子代理自己说的话,而不只是工具轨迹 [doing]
- 优先级: P1
- 复杂度: 中
- 标签: 前端 后端
- 来源: 2026-08-17 用户「左侧的子代理按钮弹出的东西太简单了,没啥用,我们要的是能看到子代理的运行状态、可以看到内容」;同轮用户拍板形态=完整对话读取器(留在右侧面板,不做独立主视图)。
- 背景: 面板看不到内容是**后端没发**,不是前端没渲染。crates/kanzei-core/src/runner/subagent.rs:593 一行 `RunEvent::AssistantMessageCommitted { .. } | RunEvent::ToolResultsCommitted { .. } => None` 把子代理正文显式丢弃,其余事件走 `_ => None`;面板能拿到的只有轮次、ToolStart 原始 JSON 入参、ToolEnd preview、StepEnd usage 四类。最终答案也只以 preview 到达:编排派发路径 phase_pipeline.rs:390 取 `text.lines().next()`(只有首行)。前端侧 06-agent-panel.js:243 每次进度强制 `detail.classList.remove("hidden")`,renderAgentTranscript 直接 `JSON.stringify(input)` —— 于是右栏被原始 JSON 糊满(用户 2026-08-17 截图)。
- 内容: 数据真源已经就位——R-279 把子代理完整消息历史落 `subagent.transcript` 事件,crates/kanzei-core/src/store/typed.rs:770 有 recover_subagent_transcript(session, call_id),projection_gate.rs:24 已缺省启用。缺的是三件:批1 后端补正文——run_subagent 的 on_event 把子代理每轮文本折成 TaskTrace(新 phase="text")上抛,终态把**完整答案原文**带给面板(编排路径同步改掉 lines().next() 截断);批2 读取通道——新增 Tauri 命令按 (session_id, call_id) 取 subagent.transcript 的完整消息历史,复用 projection_gate 既有开关;批3 阅读器 UI——面板条目点开进完整对话读取器,子代理每轮正文按主对话同一 markdown 渲染,工具调用**默认折叠**(一行名称+摘要,展开才看入参与输出),运行中实时追加、结束后从事件重放(含跨重启)。
- 边界: 不做独立主视图(用户拍板留右侧面板读取器形态);不改 subagent.transcript 事件格式(R-279 是真源,本条只读它);不给子代理开思考(RunnerConfig reasoning 仍 Off,subagent.rs:490);不重做活动面板。
- 验收: ①运行中能逐轮看到子代理**自己说的话**,不只是工具名(实测一轮真实 task,事件轨迹或截图为证);②结束后点开条目看到的是完整最终答案原文,不是被截成首行的 preview(反向断言:编排路径不得再用 lines().next() 当终态文本);③工具调用默认折叠、展开才显示入参与输出,进度事件不得强制展开 detail(反向断言 06-agent-panel.js:243 那行 remove("hidden") 已消失);④重启后打开历史子代理条目仍能读到完整对话——数据来自 subagent.transcript 事件,不依赖进程内 TranscriptStore(跨进程实测);⑤编排派发的角色(architecture_scout 等)与模型自派的 task 走同一个读取器,不出现第二套渲染;⑥六条前端冒烟全绿(node --check 不算证据)。
- refs: R-279 R-174 R-175 D-419
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-281
- 批次: 1/3
- 进展: 批1代码已完成并通过：crates/kanzei-core/src/runner/event.rs 增加 TaskTrace.text；runner/subagent.rs 上抛 AssistantMessageCommitted 完整 Text parts 并有 assistant_message_text_keeps_full_text_parts 测试；crates/kanzei-app/src/run/events/mod.rs 转发 text/usage；phase_pipeline.rs 两处终态文本改为完整原文、不再使用 lines().next()；ui/06-agent-panel.js 使用 renderMarkdown 渲染正文且工具/正文默认折叠。T-1786922726071、T-1786922726072、T-1786922726077 均通过，六条前端冒烟与 workspace 全量覆盖已完成。当前 39 文件 staged，最近实际 hash 6eb4f03c4de88cb4；已按该 hash 执行 R-281 B1 提交，仍被结构化 git 的旧 source_test_gate 拒绝：门禁仍选 R-285 Playwright 记录，未读取当前源码指纹记录。源码侧 crates/kanzei-tools/src/git.rs:746 已有 last_passed_for_fingerprint 修复且对应测试通过，但当前 git 工具运行态尚未加载。批1 已提交(ed305ae8)。被拦的真实原因是旧 kzapp(2026-08-09 安装版)把 13 位毫秒测试 id 当秒比较，恒选无收尾的 R-285 Playwright 记录，详见 D-349 进展。下一步做批2 transcript Tauri 读取通道。
- observed_head: 148386f3d467b701f334932b2bfc85bbcfcea475
- observed_worktree_hash: fnv1a64:6aa6fbd939a238f6
- recorded_at: 1786933041284
- 停车: 2026-08-18 对账:原停车原因(让位 R-221)已消失,R-221 已于 2026-08-17 关闭;待 WIP 槽空闲按队列恢复批2(transcript Tauri 读取通道)。恢复人:agent。

## R-288 Android 真机 E3 验收:移动端 PWA 通知与双向消息真实链路 [todo]
- refs: R-059 R-270 R-271 D-389
- 内容: 在同一 LAN 下使用 Android 真机打开 PWA，完成 bearer 配对，验证主/次代理通知展示、SSE 更新与消息发送；只补真实设备证据，不重做桥接或 PWA。
- 复杂度: 小
- 批次: 0/1
- 来源: R-059 拆分；R-270/R-271 已完成服务端与 PWA 实现，剩余仅是真实 Android 设备验收。
- 标签: 流程
- 阻塞: 需要 Android 真机与当前机器位于同一 LAN 后执行 E3；这是设备验收，不阻塞二期 P0/P1 主线。解除人:用户提供设备窗口或后续人工验收。
- 验收: ①Android 真机可访问并完成鉴权；②收到真实运行成功/失败通知；③从手机发送消息后服务端产生可追溯事件；④保存截图、端口/设备与 session 证据；⑤失败时明确网络、权限或设备边界。
- 优先级: P3

## R-299 IPC 与事件契约机械比对扩面 [doing]
- refs: R-284
- 内容: scripts/ipc-contract.json 仅锁 docs_snapshot 一个顶层键(1/104 command),而该机制自述正是 30+ 命令手搓 JSON 两侧各写一遍字符串(crates/kanzei-app/src/ipc_contract.rs:1-19);后端 emit 事件集合(kz:compacted/kz:meta/kz:reasoning/kz:step 等)与前端 on() 订阅集合无任何机械求差;ui-runtime 冒烟的多会话/记忆页 fixture 是前端作者手写,后端改字段名照样全绿。扩契约文件覆盖高频 command,emit/listen 求差入冒烟
- 复杂度: 中
- 来源: 2026-08-18 全库勘察
- 标签: 核心
- 边界: 作为 R-284 事件契约的前置批次,不与其四批重复;词表定义归 R-284
- 验收: 契约覆盖高频 command;emit/listen 集合求差入冒烟;后端改事件名或字段名可被门禁捕获
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-299
- 停车: 排队:R-296 收口后恢复后续批次;恢复人:agent
- 对账: 2026-08-20 对账:p16 线(thread-line-1787020530803-1)提交已全部合入 dev(R-299 B1=7188ba76),停车点名的 ipc_contract.rs/ipc-contract.json/ipc-event-smoke.mjs/verify.ps1 均无未合并改动,停车解除;该 worktree 仅余 git.rs(+5)/ci.yml(+1) 未提交 WIP,处置归 R-306 B3;恢复动作=对账 B1 已入 dev 的证据后继续后续批次

## R-307 停车/依赖解锁机械化与依赖关系可视化:解除条件可判定、达成自动恢复、关键路径可见 [todo]
- refs: R-306 D-565 R-242 R-281 D-504 R-283
- 内容: 批1 停车/阻塞结构化解除条件:新增机器可判语法(例「解除条件: D-565 terminal」「解除条件: 用户」),tracker 每次调度扫描核验,条件达成自动清车并在对账字段留痕;存量自由文本停车不强迁,新写入走结构化。批2 取活裁决拓扑权重:反向依赖计数(unblocks N 条)进入 work next 排序,关键路径条目优先(实例:R-242 经 R-284/R-245 卡住 R-283/R-285/R-287/R-249 共 6 条);「依赖指向停车条目」给显式诊断而非静默等待。批3 前端依赖可视化:文档页依赖视图升级拓扑图,关键路径高亮、节点显示解锁数/被卡数、停车条目显示解除条件与达成状态,数据复用 scheduling.rs 反向依赖图(R-111)。批4 回归:三起现场案例成测试(条件达成自动恢复/终态依赖放行/全线 blocked 时点名可恢复项)
- 复杂度: 大
- 来源: 2026-08-20 用户反馈「依赖的关系不直观,依赖的解锁有问题经常导致阻塞」;同日实测三起解锁失灵现场:①D-486 收口后其排队停车链(D-504/R-242/R-296/R-299)无人恢复,work next 全线 blocked 空转;②R-306 停车等 D-565,D-565 修复归档后 R-306 仍停车未醒;③R-281 停车原因(让位 R-221)08-17 已消失,08-18 对账才发现。停车恢复完全依赖 agent 记得回头看,机制上是单向门
- 标签: 核心
- 边界: 不改单 WIP 纪律;自动恢复只清「排队/让位」类机器可判停车,「解除人:用户」永不自动清;不做自然语言解析,只认结构化语法,存量文本停车靠人工对账维持现状;不与 D-434(停车不被扫荡误清)冲突——自动清的前提是结构化条件显式达成
- 验收: ①结构化解除条件有解析与调度测试,达成即自动清车且留痕;②work next 全线 blocked 时输出「哪些停车解除条件已达成」清单,不再只报死锁;③取活依据可见拓扑权重(点名 unblocks 计数);④依赖视图实测截图:关键路径与解锁数可见;⑤三起现场案例回归通过
- 优先级: P1
- 停车: 主会话线执行中(批1/2 委派隔离工作树子代理实现,主会话验收合入);恢复人:主会话
- 执行者: 主会话(SOL)+委派子代理实现,循环勿取
- 批次: 2/4
- 进展: B1/B2 已交付并合入 dev(baf0bdd1/213ec07c,合并提交含 355→389 测试):①「解除条件:」结构化标记解析(全/半角冒号、多编号、字面量「用户」永不达成),停车/阻塞字段带标记且所列编号全部终态→调度动态视为可执行,不写回,取活依据点名「解除条件已达成」;②全线 blocked 时对存量自由文本停车提取 R-/D- 编号,全部终态即输出「前提可能已达成请复核」提醒;③同显式优先级槽位内按反向依赖 unblocks 计数加权取活,取活依据点名 unblocks=N。注意:运行态生效需发版重装(kzapp/kz 是安装位二进制)。剩余批3 前端依赖拓扑图(关键路径/解锁数徽章),批4 已由 B1/B2 的 10 个新测试覆盖三起现场案例,可并入批3 验收
- observed_head: 1a5753cc517ea18028d9d2fc034d5030c631da99
- observed_worktree_hash: fnv1a64:2401216920f5e68b
- recorded_at: 1787173976915
- 对账: 2026-08-20 发版 build-39cd402f 后 B1/B2 进入运行态;R-306/R-293 停车已改写「解除条件:」语法为首批真实消费者。批3 时顺带把「解除条件:」写入约定补进 conventions 停车纪律,让循环新写停车默认带机器可判条件

## R-309 门禁矩阵整合:按改动路径裁剪 verify、globals 免手工同步、脆性门禁加固 [doing]
- refs: D-510 D-555 D-539 D-540 D-458 R-300 D-642 D-643 D-644 D-645 D-646 D-647 D-648 D-649 D-650
- 内容: 批1 globals 免手工同步:eslint.config.js 加载时直接调 gen-ui-lint-globals 计算 globals,ui-lint-globals.json 降级缓存或删除——结构性消灭 D-458/D-484/D-523/D-547/D-560/D-562 一族(占门禁缺陷 18%)。批2 verify 按改动路径三档裁剪:无 Rust 改动跳 fmt/clippy/test(省 100.2s),无前端改动跳六冒烟(省 5.9s),verification.json 记录裁剪判据与被跳步骤,package.ps1 发版证据要求全量 verify 不受裁剪污染。批3 关闭门禁复用 verify 证据:frontend_smoke_passed 接受绑定当前 HEAD 且 ui_runtime/ui_lint/ui_i18n 全 pass 的 verification.json,去掉每批第 3 轮冒烟;同时补冒烟记录新鲜度校验(现状:三天前的 passed 记录可放行今天的关闭,coverage.rs frontend_smoke_passed 无时间/指纹比对)。批4 脆性加固:metrics 闸基线口径版本断言(口径不同拒绝出数而非出错数),metrics-regression-gate.ps1 里的 cargo build 移出 crate_sync 单独计时;parallel-lines-regression.mjs 改用 loadUiSources() 不再写死 8 个文件名;ipc-event-smoke/check-ps1-bom 补空集与下限断言(D-510 模式推广)
- 复杂度: 大
- 来源: 2026-08-20 门禁矩阵审计(115 条近期缺陷归因):门禁真假阳性比 1:4.7;34 条门禁类缺陷中 29% 是「忘了重跑生成器」类机械同步;verify 14 步 108.1s 中 Rust 三步占 92.7%,改前端的批次全额买单;fmt/clippy 每批次跑 4 轮、前端冒烟 3 轮。P0 两项(finalize 去重、删 ui_syntax)已由主会话落地
- 标签: 流程
- 边界: cargo test --workspace 的 90.3s 是真实成本不动;CI 全量保持;裁剪只作用本地 verify,发版通道必须全量
- 验收: ①改一行前端的批次 verify 墙钟 <15s(实测);②改 globals 清单的缺陷族在门禁上线后零新增;③关闭前端条目不再需要第三轮冒烟(用 verify 证据通过一次真实关闭);④metrics 口径漂移场景拒绝出数的定向测试;⑤裁剪过的 verification.json 被 package.ps1 拒绝的定向测试
- 优先级: P1
- status: doing
- 进展: B1 已提交：`a173eb6a`，globals 实时收集与缓存降级已验证。B2 已提交：`481f4463`，`scripts/verify-policy.mjs:5-91`、`scripts/verify.ps1:41-178`、`scripts/package.ps1:109-114` 完成路径裁剪和 full evidence 门禁；D-642/D-643/D-644 已关闭，归档收口提交 `1e28fe28`，真实 package 拒绝 cropped evidence 证据 T-1786922726629。B3 已提交：`b5d23c28`，`crates/kanzei-tools/src/test_record/coverage.rs` 优先消费当前 HEAD 的 `dist/verification.json`，要求 `all_pass=true`、`ui_runtime/ui_lint/ui_i18n` 三项 pass，并校验 24 小时新鲜度与工作区源码指纹；T-1786922726630：424 passed、0 failed、1 ignored。B4 已提交：`f2d10c44`，`crates/kanzei/src/cli/metrics.rs:401-437` 输出 v1；`scripts/metrics-regression-gate.ps1:27-70` 拒绝口径漂移且 cargo build 已移至 `scripts/verify.ps1:99-101` 独立 metrics_build；`scripts/parallel-lines-regression.mjs:4-23` 使用 loadUiSources 与源码标记；`scripts/ipc-event-smoke.mjs:46-53`、`scripts/check-ps1-bom.mjs:36-40` 增加下限/空集断言；`crates/kanzei-tools/src/git.rs:1929-2008`、`.github/workflows/ci.yml:32-33` 清单同步；`docs/design/metrics_baseline.md:4,11-60` 更新 v1 基线。D-645～D-650 已逐项 fixed，T-1786922726632：fmt、kanzei-tools 424 passed、kanzei 44+32 passed、JS/PS smoke 和真实 metrics gate 全通过。提交锚点补正：`8ad3bb28`。T-1786922726633：真实当前 HEAD targeted verify 3.34s，生成绑定 HEAD 的 verification.json，但 changed=1 且 rust/frontend 均 false，明确不能核销 frontend-only ①。T-1786922726634 与 T-1786922726635：cargo test --workspace 全量通过（各 crate 无 failed）。T-1786922726636：真实当前 HEAD full verify 通过，14/14 checks、skipped_steps 为空、rust/frontend 均执行，绑定 `b564f2134ae6c8d8828a33b42745df0f48a42b5a`，耗时 105.67s；该证据证明 full evidence，不冒充 frontend-only ①或真实前端关闭③。验收对账：①仍缺真实“只改一行前端”verify 墙钟 <15s；②仍缺门禁上线后 globals 缺陷族零新增证据；③仍缺一次真实前端条目关闭复用 verify 证据；④已满足，T-1786922726632 的 v999 metrics fixture 拒绝与真实 gate 通过；⑤已满足，T-1786922726629 证明 package 拒绝 cropped verification。R-309 保持 doing，批次 4/4；虽然 B4、targeted/full verify 与 workspace 测试已完成，不能把①-③的证据缺口降级或误标 done。
- observed_head: f2d10c44f9e47b2f199d346d13429e25d4a9f196
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787266377330
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-309(unblocks=0)
- 批次: 4/4
- 停车: 用户本轮明确要求优先从 defects.md 最上面可执行项开始；R-309 B1-B4 代码与门禁验证已完成，剩余①～③需真实前端事件证据，暂让出唯一 WIP 槽；恢复人:agent。

## R-312 Agent 减负:上下文供给账单、状态机字段瘦身与压缩协同(勘察+设计) [doing]
- refs: D-573 R-310 docs/design/context_compaction.md docs/design/weakness_register_20260820.md
- 内容: 本条只做测量+设计,实施条目由设计文档评审后另立(先计划后自举)。批1 测量:上下文账单按注入块出数(conventions 全量/memory-index/resolved-control-state/条目全文/工具输出),并统计真实会话里模型侧维护状态机自由文本字段(进展/对账/停车)的 token 占比与写入频次;批2 设计四个方向的方案与取舍:①机器可代填字段(测试记录号/提交号/批次等机械部分由引擎代写,模型只写判断性内容);②注入分层(当前 WIP 条目全文+依赖闭包,其余给索引行);③进展/对账历史段落按批次折叠沉档,req get 默认返回当前批次视图;④压缩与注入协同——可机械重取的注入块不进纪要预算(context_compaction.md L0 prune 思路从工具输出延伸到 harness 注入面),条目内 file:line 锚点腐烂的对策一并评估;批3 用户评审拍板后拆实施条目
- 复杂度: 中
- 来源: 2026-08-20 用户:「上下文压缩管理实际上有点问题,包括导航的问题;很多负载都维护到状态机里,应该思考怎么给 agent 减负」;外部评估同向——认知预算耗在操作 harness 而非解决问题
- 标签: 核心
- 设计文档: docs/design/weakness_register_20260820.md
- 边界: 本条不改任何代码;不推翻 conventions 全量注入决策(D-201)——除非账单证明占比失衡且经用户拍板;不动记忆注入口径(R-104);压缩引擎本体缺陷(如 D-573)走各自条目不并入
- 验收: ①注入块账单数据落档且覆盖不少于 5 个真实会话;②设计文档含字段瘦身/注入分层/沉档/压缩协同四方案及取舍与 token 收益估算;③有用户评审拍板记录;④实施条目登记完成并与本条互链
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-312(unblocks=0)
- 批次: 2/3
- 批次表: B1 测量：从不少于 5 个真实会话提取 conventions、memory-index、resolved-control-state、当前条目、工具输出及进展/对账/停车字段账单；B2 设计：形成机器字段、注入分层、历史沉档、压缩协同四方案，记录取舍与 token 收益估算；B3 评审收口：记录用户拍板，登记实施条目并与 R-312 互链。
- 进展: B0 已复核：R-312 无既有实现，边界保留 D-201/R-104/D-573。B1 已完成：读取真实 `.kanzei/state.db` 的 `episodes.context_json`，覆盖 11 个 distinct session 中 7 个最新账单非空 session；账单落档 `docs/design/context_supply_bill_20260821.md`，并确认 6 个现代 dev session 平均 66868.7 字符，tools/schema 48.6%、agent/system 20.7%、dev/conventions 17.4%、dev/memory 5.7%；当前 context_report 尚未拆出进展/对账/停车字段级 token 与写入频次，已明确记录缺口。B2 已完成：同一设计文档第 6-7 节形成四方向候选——机器事实信封、WIP/依赖注入分层、当前批次视图与完整历史分离、可重取注入与压缩协同；每项均写现有能力、取舍、收益假设和不应相加的估算，初步建议先方向一/三，再协同方向二/四，暂不改变 D-201/R-104。架构索引已登记并校验通过。下一步 B3：取得用户对四个拍板问题的明确选择，登记实施条目并与 R-312 互链；未获拍板前不写 accepted decision、不关闭本条。
- observed_head: f446bd018e2e03242a0d4756cdb77ccf4b76b56b
- observed_worktree_hash: fnv1a64:7580d1080253583e
- recorded_at: 1787299505789
- 阻塞: 等待用户评审并拍板四方向及实施顺序；解除人:用户

## R-319 事务边界感知的步数预算:收尾软延长避免 stage 后切轮 [doing]
- refs: R-307 R-311 D-335
- 内容: 让运行预算识别显式交付事务阶段；当剩余步数不足、且当前仅余 commit 与 tracker anchor 等确定性收尾动作时，授予小额 soft transaction extension 完成原子边界，再结束本轮；事件记录触发条件、扩展步数、实际动作与结果。
- 复杂度: 中
- 来源: 2026-08-21 用户提供运行复盘：多次在 steps 32 用尽时恰好切在 tests passed/files staged/commit pending，续轮必须重新 work next、status、log、diff 才恢复。可见日志只是尾段，结论限定为已观察到的事务中断形态。
- 标签: 流程
- 边界: 不增加通用自动轮数；只允许白名单收尾状态与有界额外步数；不得跳过 fmt/clippy/test/source_test_gate/权限/CAS；出现新代码编辑、测试失败、用户输入或未知状态立即取消扩展并正常切轮。
- 验收: ①tests passed+files staged+commit pending 且只剩 2 步时可完成 commit+tracker anchor 后结束；②测试失败、未暂存、存在审批或发生源码编辑时不延长；③扩展上限和原因进入 session event，可审计且重启恢复不重复提交；④对比至少 10 个真实长程条目，事务中点切轮和纯恢复工具调用显著下降且事故率不升。
- 优先级: P1
- 批次: 3/4
- 批次表: B1 复核现有 step budget/轮次结束与收尾事件链，锁定真实扩展入口；B2 实现白名单收尾状态、有界扩展与取消条件；B3 接入可审计 session event 与重启去重恢复；B4 真实长程条目对比、回归测试与验收收口。
- 取活依据: engine:唯一可执行 WIP 是 R-319，必须先恢复它
- 进展: B1 已完成：复核真实步数边界与轮末收口链。B2 已落地并提交 08992b47：events/mod.rs:256-435 以真实 test_record passed、git stage 成功、无源码编辑/审批/未知工具为白名单，最多授予一次 2 步 extension；core runner/drive.rs:243-256 在事件回调后读取有界信号，普通 max_steps 不变。B3 已提交并补强，提交 f62097cf：events/mod.rs:167-216 写 run.transaction_budget_extended，含 run_id、step、base_max_steps、extension_steps、reason、trigger、allowed_actions，并按 run_id 去重；persistence.rs:70-119 新增轮末 run.transaction_budget_result，按同一 run_id 的 run.trace 重放 extension 后实际 tool.completed 动作并写 completed/failed 结果。T-1786922726709 覆盖事务状态/taint/去重/结果事件；D-679 已由 2b27245f 修复失败 test_record/stage 永久 taint，回归 T-1786922726707。B4 基线已取：T-1786922726710 从真实 .kanzei/state.db 选取最近 10 条 steps>=28 episode，9 条 32-step、1 条 28-step，均无新 extension/result event；这是 rollout 前基线，不能宣称事务中点切轮或纯恢复调用已下降。完整 kzapp 回归 T-1786922726708 为既有两夹具失败（243 passed/2 failed），新增 5 个 R-319 测试均通过。下一步：积累并重放至少 10 条 f62097cf rollout 后真实长程条目；若无足够 post-rollout 样本，验收④保持未满足，不关闭 R-319。
- observed_head: f62097cfde97e559534c16f898a6c0f1fb5a3e23
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787306265598

## R-320 编辑后局部结构校验:在完整回归前捕获语法作用域与类型断裂 [todo]
- refs: R-310 D-615
- 内容: 为 edit/insert/write 的结构化代码改动增加低成本局部校验计划：按文件类型选择 parser/formatter check、目标模块 lint/type check 或已有最小 smoke；工具返回 changed region、校验命令、首个精确错误位置和可修复上下文，使 Agent 在扩大到 crate/workspace 回归前先收敛局部结构。
- 复杂度: 中
- 来源: 2026-08-21 用户提供运行复盘：B3 测试桩先后出现多余右花括号、VM sandbox 变量作用域错误；R-245 telemetry 插入先破坏 Rust 语法、修复后再暴露序列化类型问题。宏观架构判断正确，但微观 edit/insert 一次命中率偏低。
- 标签: 流程
- 边界: 不在每次编辑后自动跑 workspace 全量测试；不绕过 shell 权限与现有测试证据门禁；未知语言或无可靠校验器时明确 unsupported，只给建议命令；局部通过不等于功能验收或可提交。
- 验收: ①JS 多余括号在下一次业务 smoke 前由语法校验捕获；②VM 夹具未定义绑定由目标 lint/runtime probe 捕获；③Rust 插入破坏 AST 与局部类型/serde 不匹配由 fmt/check 或目标测试精确定位；④同文件连续修改可去抖合并校验，真实样本的一次业务测试前结构错误数相对基线下降；⑤所有局部结果可进入 test_record/telemetry，但不能冒充 crate 回归。
- 优先级: P1

## R-321 执行事故与产品缺陷分层:临时自致错误不污染正式 defect taxonomy [todo]
- refs: R-112 R-310 R-311 D-615
- 内容: 在既有领域/类型词表之上增加问题来源与生命周期分层：execution_incident、development_defect、product_defect、regression；预提交且未逃逸、可当轮修复的自致错误进入 append-only incident ledger/telemetry，不占 D-ID；达到复发阈值、跨轮阻塞、逃逸提交/发版或暴露真实产品契约时机械晋升 defect，并保留 incident→defect 链接。
- 复杂度: 小
- 来源: 2026-08-21 用户提供运行复盘：D-613 是真实 contract mismatch；D-614 是同步遗漏；D-615 是 Agent 在未提交工作树中自行插错位置并立即修复的 Rust 语法错误。三者同层登记使产品缺陷、回归与执行过程失手无法分别统计。
- 标签: 流程
- 边界: 分层不得用来少报真实缺陷；门禁发现的产品行为错误仍直接登记 defect；既有 D-ID 不批量重写，只给新条目默认分类与少量样本迁移；incident 也必须可审计、可聚合，不能只留在自然语言日志。
- 验收: ①D-615 等价的当轮预提交语法失手记 execution_incident 且不分配 D-ID；②D-613 等价 contract mismatch 直接为 product_defect，逃逸既有通过测试后标 regression；③相同 incident 指纹复发、跨轮阻塞或进入提交时自动建议/强制晋升并互链；④缺陷页与指标可分别查看各类数量、修复时长和逃逸率；⑤用历史样本回放证明分类一致且正式 defect 总量不再被瞬时失手污染。
- 优先级: P2

## R-322 门禁强度分档与模型停机权 [doing]
- 原始描述: 外部评估七点反馈中的 #1 Harness Tax、#4 模式区分不够明显、#7 双控制器问题。用户定调：控制权交给模型；结伴接近 Claude Code 的高自治，自主推进保留重门禁；决策点要呈现给用户
- 复杂度: 大
- 标签: 核心
- 验收: 结伴/自主两档门禁强度可判定且引擎行为不同；模型声明完成后引擎不再 Nudge；决策点在界面可见
- 先行调研: .kanzei/research/r322-prior-art/prior-art.md
- 优先级: P1
- 批次: 3/4
- 进展: B1(4f0f46a0)+B2(7523e6a4)+B3(a032aa49) 已落地并发版:build-32513251(2026-08-21),full verify 全绿,证据 dist/verification.json 绑定 32513251,main 已 ff 到 32513251 并推送。B3 按用户定调把结伴档 loop 的停止规则改成目标条件驱动(参照 Claude Code /goal):条件由用户写、达成与否由模型判(work handoff)、引擎只负责达成前不散场且不发明工作;挂目标后 backlog 与 NoAction 都不再停机,兜底=GOAL_IDLE_ROUND_LIMIT+D-583 零产出熔断。剩余 B4=真机端到端验收(目标条件 loop 需跑一次真实会话看回显与自动清除)
- observed_head: 32513251d54e6dd311f08c44fac6df2adfa8454b
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787283272605
- 停车: 本轮 WIP 超限，B1-B3 已完成；B4 需要真实端到端会话验收，先让位缺陷优先的 D-655，槽位释放后恢复。恢复人:agent

## R-323 工具编排抽象层：模型声明执行计划 [doing]
- 原始描述: 外部评估 #2：Harness 的保守规则可能成为模型能力的上限。用户定调：提供底层工具+一层抽象层，让模型去编排
- 复杂度: 大
- 标签: 核心
- 验收: 模型可声明工具调用的并行分组；引擎以 ToolConcurrency 冲突检测为安全网单调收紧；声明缺失时行为与现状逐字节一致
- 先行调研: .kanzei/research/r323-prior-art/prior-art.md
- refs: R-322 D-661
- 优先级: P2
- 批次: 1/2
- 进展: B1 已落地:工具并发契约审计。勘察发现 30+ 工具中 13 个未实现 concurrency() 走 Exclusive 默认,而该默认原意是「未审计前不自动并行」——是信息缺失的占位不是安全断言,#2 说的「保守规则成为模型能力上限」在这里字面成立。审计后提升:webfetch/frontend_locate/frontend_check/todowrite → Shared(生产路径只读或完全无状态,写全在 mod tests 之后);websearch 按入参分流(带 prior_art_topic 时读-改-写轮次预算,用 prior-art 专属锁键,不拖住代码树写入)。仍为 Exclusive 的 9 个(question/process/prior_art/research_*/tracker 四件套/work)真有共享可变状态,留 B2。关键结论:审计后剩余 Exclusive 大多真有状态,模型对其并行安全性并不比引擎知道得多,纯收紧方向在当前契约下也几乎无用武之地,故不预先造声明通道,等真实现场再做。3 条审计单测,workspace 15 个二进制全绿,clippy 干净
- observed_head: a032aa492ae157c2791c2cfa9cf768740f093517
- observed_worktree_hash: fnv1a64:dee5f1692c68b6ad
- recorded_at: 1787276362163
- 停车: 本轮 WIP 超限，B1 工具并发契约审计已完成；B2 尚未开始，先让位缺陷优先的 D-655，槽位释放后恢复。恢复人:agent
