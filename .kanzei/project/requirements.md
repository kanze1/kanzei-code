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

## R-284 运行体验事件契约:统一 session/tool/memory/research/voice 的事实投影和瞬时表现事件 [todo]
- 优先级: P1
- 复杂度: 大
- 标签: 核心 后端 前端
- 来源: 2026-08-17 二期升级依赖分析;主对话已有 kz:* 事件,但 memory/research/voice 无共同事件词表,动画若直接从 DOM 推断会形成影子状态。
- 依赖: R-242
- refs: R-277 R-285 R-286 R-287 docs/design/phase2_system_upgrade.md docs/design/session_state_and_line_runtime.md
- 内容: 按 phase2_system_upgrade.md §5.4 分四批。批1 定义 snake_case 事件包络、持久事实/瞬时表现/高频 delta 三类边界与词表。批2 memory/research/voice 后端生产者接线,统一 project/session/run/topic/entity 归属。批3 前端按归属先归并 store 再分发 animation/audio/工作台,重放幂等。批4 高频 delta 合并、未知事件诊断、重连恢复和跨会话回归。
- 边界: 不改变 R-242 的 session 真源;瞬时表现事件允许丢帧且不写长期数据库;动画和音频不得反向决定业务状态;未知事件不崩 UI;事件字段不混用 camelCase,第三方/旧事件在适配层转换。
- 验收: ①词表和 JSON 契约有 schema/单测;②同一 memory promote/research verify/voice state 事实可重放且不重复副作用;③后台会话事件不驱动当前会话动画;④text delta 压缩后长回复无事件风暴;⑤重连从持久事实恢复状态,不依赖错过的表现事件。
- 批次: 0/4
- 状态: todo

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

## R-245 Tool Result Spill 与显式空间整理：完整 artifact、可恢复引用、无自动过期 [todo]
- refs: D-209 R-180 D-297 D-298 R-242 docs/design/deepseek_harness_upgrade.md
- 依赖: R-242
- 内容: 统一工具结果为 Inline 或 Spilled{preview,artifact_id,bytes,sha256,retrieval_hint}；read 优先指向原文件 offset/limit，bash、git、test_record、web 等完整原文进入与 state.db 同生命周期的 Git 忽略运行目录。提供存储与整理入口，按类别、会话、日期、大小预览占用，支持清理无引用 artifact；经风险确认后，用可恢复失败的删除计划物理删除已选会话的事件、投影和引用 artifact；并支持 SQLite checkpoint、VACUUM 与迁移备份管理。默认不自动过期。
- 复杂度: 大
- 批次: 0/5
- 来源: DeepSeek Harness spill policy、本地 state.db 输出分布统计，以及用户确认“不自动过期但需要显式整理入口”。
- 标签: 核心
- 边界: 任何事件仍引用的 artifact 不得被静默清理；整理前显示预计释放空间和不可恢复范围，执行后给清单与实际释放量。32 KiB 先做 shadow telemetry。普通会话删除保证产品不可检索且重启不复生；安全整理才处理 SQLite freelist、WAL 和含旧正文备份。当前库为 WAL、secure_delete=OFF、auto_vacuum=NONE，不能把 DELETE 行等同磁盘字节已擦除。弹窗必须区分仅删除与删除并安全整理，取消零写入；显式整理不是定时任务。
- 迁移与回滚: artifact 原子写入后再提交引用事件；任一步失败不得留下有效事件指向缺失文件。删除使用引用图和事务清单，失败可重试；schema 迁移前备份。关闭 Spill 可回到 Inline，但已有引用仍必须可读。
- 阻塞: 
- 验收: ①32 KiB shadow telemetry 不改变模型输入并产出按工具分布；②Spill 原文 sha256 与工具原输出一致，重启后可取回；③事件提交与 artifact 写入故障注入无悬空引用；④明确无自动过期任务；⑤整理入口列出总占用、数据库、WAL、freelist、artifact、无引用文件和迁移备份并支持 dry-run；⑥清理引用中 artifact 被拒，清理无引用 artifact 成功且释放量可核对；⑦删除弹窗列出会话事件、轨迹、草稿与 artifact，仅删除和删除并安全整理差异明确，取消零写入；⑧确认删除后事件、投影和引用 artifact 产品层不可检索且重启不复生，删除计划任一点失败可恢复重试；⑨安全整理仅在运行静止时执行，成功后 checkpoint、VACUUM 与备份处置可核对，busy 或失败不静默；⑩权限、路径逃逸、不可预测文件名和磁盘配额有测试。
- 优先级: P1

## R-248 先行调研内建:新方向开工前默认产出「已有方案对照」,不靠用户开口 [doing]
- refs: R-221 docs/design/research_mode.md
- 依赖: R-221
- 内容: 把「先查已有方案再动手」从用户每次口头要求变成 harness 的默认动作。①触发判据机械可判、不交模型自由裁量:项目根首次初始化 `.kanzei/`、req add 时 refs 为空且标签为核心、用户显式发起,三者之一成立即触发;②产物落 `.kanzei/research/<topic>/prior-art.md`,每条结论含「方案名 + 出处(URL 或 file:line) + 与本课题的差异 + 采用或不采用的理由」,**外部已有实现**(开源方案、协议、公开设计)与**仓内既有设计**(docs/design/**、requirements/defects 现存与 archive)两侧都必须覆盖;③新方向判定成立而无对照工件时,req add 要求 refs 指向该工件,或由用户显式豁免并留痕。
- 复杂度: 中
- 批次: 0/3
- 来源: 2026-08-14 用户观察——开新项目应先深度调研已有方案与设计,不适合从零开始;这是当前 coding agent 的通病(非得用户主动请求才去调研),直接影响自举质量。
- 标签: 核心
- 边界: 不是每条需求都调研,只在触发判据成立时启动;判据必须机械可判,不接受模型自行裁量「这算不算新方向」。websearch 轮次设上限,不做无限扩散爬取。本条只产出对照工件与开工门禁,不改 req/defect 状态机,也不自动把调研结论写成条目——那是 R-221 定调点4 的回流通道。
- 阻塞: 
- 验收: ①三种触发判据各有定向测试,未触发的普通条目不受影响;②prior-art.md 每条结论都带出处,无出处结论被机械拒绝(复用 V0 标注同一套校验);③外部与仓内两侧覆盖各有独立断言,只查一侧不算通过;④新方向下 req add 缺 refs 被拒,豁免路径留痕可审计;⑤websearch 轮次上限有实测,超限给明确诊断而非静默截断;⑥既有 req add 路径无回归。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-248
- 停车: 排队:方案已于 2026-08-20 拍板(独立 prior_art 字段+R-304 勘察工件落点约定,见对账字段),原等待条件消失;待队列轮到时恢复批1;恢复人:agent
- 进展: 已读 docs/design/research_mode_prior_art.md、docs/design/research_mode.md、crates/kanzei-tools/src/tracker.rs:241-270/789-843、tracker/actions.rs:290-360、websearch.rs:14-110、kanzei-app/src/projects.rs:43-127。确认当前没有 prior-art 生成/校验/轮次预算实现；现有 req add 仅做通用 refs 校验，项目初始化只创建 `.kanzei/`，无法凭现有接口生成无 topic 的 prior-art.md。未修改代码。
- observed_head: 3950c0348331956fda32a18d0789ce52d3d30eee
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786960050685
- 对账: 2026-08-20 用户拍板:API 形态=新增独立 prior_art 字段(不动通用 refs 契约,refs 仍只收 R-/D-/T- 编号);三触发的 topic 来源沿用 R-304 固化的 dev 勘察工件落点约定。停车/阻塞解除,按队列恢复批1

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

## R-296 Tauri command 与 run 链路测试基座 [doing]
- 内容: kanzei-app 无 tests/ 目录(全仓唯一集成层在 crates/kanzei/tests/integration),104 个 #[tauri::command] 零测试;装配→执行→落库主链 commands/run.rs(604行)、processes/lifecycle.rs(593)、processes/workspace.rs(548)、run/persistence.rs(489)、run/coordinator.rs(424)、run/execution.rs(313)、harness_ext.rs(284) 全部 0 个 #[test];数据面 memory.rs(13 command)/docs.rs(16 command) 同样近零。建立可测基座(状态抽离/伪 AppHandle/集成层)并优先覆盖 run 主链
- 复杂度: 大
- 来源: 2026-08-18 全库勘察
- 标签: 后端
- 边界: 不追求覆盖率数字,优先真实断言关键路径;不重构业务逻辑
- 验收: run 主链关键路径有自动化断言;新增 command 有明确测试落点范式;cargo test 全绿并入 verify
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-296
- 停车: 排队:R-306 收编后恢复——收编会改变全量测试面,先收编再跑 workspace 全量避免重复;恢复人:agent
- 进展: 已落地并提交 1e076db6：commands/run.rs 新增真实 episode/复杂度来源的 command 测试，run/mod.rs 新增真实 SessionStore 通知回放测试；cargo test -p kanzei-app 213 passed（T-1786922726379）。发布脚本 cargo test --workspace 在 kanzei-tools background 越界终止测试处 343 passed、1 failed（T-1786922726380），已登记 D-529；下一步修复并重跑全量。
- observed_head: 9304ec92c35670db6a002feeddef0d31c6dc1bea
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787059362211
- 对账: 2026-08-20 对账:停车前提 D-529 已 fixed 归档,停车解除;恢复动作=重跑 cargo test --workspace 全绿并入 verify,补齐验收后关闭

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

## R-308 记忆冗余治理与晋升门槛机械化:同指纹聚类合并、candidate 单轨化、复发阈值硬执行 [todo]
- refs: D-567 D-568 R-293 R-235
- 内容: 批1 同指纹聚类合并:按 [fp:...] 指纹与标题相似度机械聚类,重复簇合并为单条(保最完整正文,合并复发计数),归档被并条目;批2 晋升门槛机械化:复发阈值(如第 2 次才建 candidate、第 N 次+修复证据才 active)由写入方硬执行而非提示词约定,低于阈值的 note 只进 inbox 不落盘;批3 candidate 单轨化:candidate 要么进 INDEX 带标记要么不进检索,消除「索引看不见、检索跑得出」;批4 global 域处置:74 条 candidate 走一次批量复核(晋升/合并/清退),global 域接入 recall 遥测
- 复杂度: 中
- 来源: 2026-08-20 记忆系统全面勘察:61 条顶层条目实质仅约 31 个主题(重复簇 8 个共 39 条,49% 冗余);M-205 与 M-207 标题逐字相同;C6 簇三条(M-248/250/253)共用同一指纹一天内产生;M-245 正文自述「本轮第 1 次复发→暂不建」却仍落盘——晋升门槛写在文里没有被执行;24 条 project candidate 不进 INDEX 却被 FTS 检索(双轨);global 域 74 条全 candidate 零 active 零遥测,晋升管道在全局域没跑通
- 标签: 后端
- 边界: 不动 R-293 的 F(m) 漏斗与效应量框架;不动 R-235 已拍板的 28 条存量豁免;合并动作走 M-059 SOP 归档不裸删
- 验收: ①顶层条目数≈实质主题数(勘察口径复查冗余率<15%);②同指纹重复写入被机械拒绝并有定向测试;③candidate 可见性单轨有断言;④global 域 74 条处置留痕且 recall 遥测非零;⑤INDEX 行与源文件 description 一致性核对通过(与 D-568 对齐)
- 优先级: P2

## R-309 门禁矩阵整合:按改动路径裁剪 verify、globals 免手工同步、脆性门禁加固 [todo]
- refs: D-510 D-555 D-539 D-540 D-458 R-300
- 内容: 批1 globals 免手工同步:eslint.config.js 加载时直接调 gen-ui-lint-globals 计算 globals,ui-lint-globals.json 降级缓存或删除——结构性消灭 D-458/D-484/D-523/D-547/D-560/D-562 一族(占门禁缺陷 18%)。批2 verify 按改动路径三档裁剪:无 Rust 改动跳 fmt/clippy/test(省 100.2s),无前端改动跳六冒烟(省 5.9s),verification.json 记录裁剪判据与被跳步骤,package.ps1 发版证据要求全量 verify 不受裁剪污染。批3 关闭门禁复用 verify 证据:frontend_smoke_passed 接受绑定当前 HEAD 且 ui_runtime/ui_lint/ui_i18n 全 pass 的 verification.json,去掉每批第 3 轮冒烟;同时补冒烟记录新鲜度校验(现状:三天前的 passed 记录可放行今天的关闭,coverage.rs frontend_smoke_passed 无时间/指纹比对)。批4 脆性加固:metrics 闸基线口径版本断言(口径不同拒绝出数而非出错数),metrics-regression-gate.ps1 里的 cargo build 移出 crate_sync 单独计时;parallel-lines-regression.mjs 改用 loadUiSources() 不再写死 8 个文件名;ipc-event-smoke/check-ps1-bom 补空集与下限断言(D-510 模式推广)
- 复杂度: 大
- 来源: 2026-08-20 门禁矩阵审计(115 条近期缺陷归因):门禁真假阳性比 1:4.7;34 条门禁类缺陷中 29% 是「忘了重跑生成器」类机械同步;verify 14 步 108.1s 中 Rust 三步占 92.7%,改前端的批次全额买单;fmt/clippy 每批次跑 4 轮、前端冒烟 3 轮。P0 两项(finalize 去重、删 ui_syntax)已由主会话落地
- 标签: 流程
- 边界: cargo test --workspace 的 90.3s 是真实成本不动;CI 全量保持;裁剪只作用本地 verify,发版通道必须全量
- 验收: ①改一行前端的批次 verify 墙钟 <15s(实测);②改 globals 清单的缺陷族在门禁上线后零新增;③关闭前端条目不再需要第三轮冒烟(用 verify 证据通过一次真实关闭);④metrics 口径漂移场景拒绝出数的定向测试;⑤裁剪过的 verification.json 被 package.ps1 拒绝的定向测试
- 优先级: P1

## R-310 仓库导航效率:失手遥测、工具自愈报错与代码地图,把认知预算还给问题本身 [todo]
- refs: D-575 D-568 R-308 docs/design/weakness_register_20260820.md
- 内容: 批1 失手遥测:工具调用失败机械分类(不存在路径/越界范围/漏参数/空搜索/权限拒绝),按 run 落 telemetry,产出失手率基线;批2 报错自愈:落地 D-575 验收(最近邻候选/合法范围/必填参数点名);批3 代码地图:crate→模块→公共符号的机器生成索引,symbols 扩仓级查询或注入轻量 repo map——批3 动工前先出小设计对比 token 成本再定形态;批4 复测:同类任务失手率对比基线,弱模型(自举档)实测
- 复杂度: 大
- 来源: 2026-08-20 外部工程评估对照 Claude Code/Codex:仓库导航效率(7.8)为最大差距,一轮真实轨迹七次导航失手;定性为 tool proprioception 差距而非模型智力差距——认知预算耗在操作 harness 而非解决软件问题
- 标签: 核心
- 设计文档: docs/design/weakness_register_20260820.md
- 边界: 不做 embedding/语义代码检索;repo map 若走注入必须过 token 成本核算,超预算宁可工具化按需查;不改 grep/glob 既有语义;记忆召回不相关问题归 D-568/R-308 不在本条
- 验收: ①失手遥测有分类与按 run 聚合,基线数字落档;②D-575 五条验收全部通过;③代码地图机器生成、随提交可增量更新,查询路径有定向测试;④失手率相对基线下降有真实运行数据支撑;⑤repo map 的 token 成本核算落档
- 优先级: P1

## R-311 收尾闭环硬化:设计冻结不变式可执行化与收尾链完成度遥测 [todo]
- refs: R-309 R-310 docs/design/weakness_register_20260820.md
- 内容: 批1 不变式可执行化:设计冻结字段支持登记机器可跑断言(grep 模式/测试名/脚本),finalize 与条目关闭时自动执行,失败拒关并点名失败断言;批2 收尾链遥测:条目关闭时机械核对收尾链各环节(编译/定向测试/回归/验收对照/提交)证据是否在档,缺环计数落 telemetry;批3 长程统计:按条目/批次聚合导航失手率(数据来自 R-310)、门禁拒绝、返工次数、收尾链完整度,滚动报表进 metrics——这是外部评估点名缺失的「连续几十个 requirement 的统计证据」载体
- 复杂度: 中
- 来源: 2026-08-20 外部工程评估:execution tail reliability(实现→定向测试→回归→不变式复查→验收对照→提交的最后一公里)是与 Codex 的主要剩余差距;kanzei 已有 13 步 verify 与关闭门禁,缺的是不变式机械检查与按条目的收尾链证据统计
- 标签: 流程
- 设计文档: docs/design/weakness_register_20260820.md
- 边界: 不重复 R-309 的门禁裁剪与成本治理;不变式登记是新增可选能力,不给存量条目回填;报表只出数不自动拒绝任何操作
- 验收: ①冻结不变式断言在 finalize/close 自动执行且失败拒关,有定向测试;②收尾链缺环可观测并落 telemetry;③滚动报表真实出数且覆盖不少于 10 个已关闭条目
- 优先级: P2
- 对账: 2026-08-20 需求发现实测补充真实案例:文章获取器项目 D-001 在后置条件未复核下归档 fixed(进展字段自写「复核应确认 raw_lines 为空」即验证后置),本会话复查游离行仍在(D-577)——「终态迁移无后置条件核验」正是批1 不变式可执行化要防的形态,复测场景纳入批4 回归

## R-312 Agent 减负:上下文供给账单、状态机字段瘦身与压缩协同(勘察+设计) [todo]
- refs: D-573 R-310 docs/design/context_compaction.md docs/design/weakness_register_20260820.md
- 内容: 本条只做测量+设计,实施条目由设计文档评审后另立(先计划后自举)。批1 测量:上下文账单按注入块出数(conventions 全量/memory-index/resolved-control-state/条目全文/工具输出),并统计真实会话里模型侧维护状态机自由文本字段(进展/对账/停车)的 token 占比与写入频次;批2 设计四个方向的方案与取舍:①机器可代填字段(测试记录号/提交号/批次等机械部分由引擎代写,模型只写判断性内容);②注入分层(当前 WIP 条目全文+依赖闭包,其余给索引行);③进展/对账历史段落按批次折叠沉档,req get 默认返回当前批次视图;④压缩与注入协同——可机械重取的注入块不进纪要预算(context_compaction.md L0 prune 思路从工具输出延伸到 harness 注入面),条目内 file:line 锚点腐烂的对策一并评估;批3 用户评审拍板后拆实施条目
- 复杂度: 中
- 来源: 2026-08-20 用户:「上下文压缩管理实际上有点问题,包括导航的问题;很多负载都维护到状态机里,应该思考怎么给 agent 减负」;外部评估同向——认知预算耗在操作 harness 而非解决问题
- 标签: 核心
- 设计文档: docs/design/weakness_register_20260820.md
- 边界: 本条不改任何代码;不推翻 conventions 全量注入决策(D-201)——除非账单证明占比失衡且经用户拍板;不动记忆注入口径(R-104);压缩引擎本体缺陷(如 D-573)走各自条目不并入
- 验收: ①注入块账单数据落档且覆盖不少于 5 个真实会话;②设计文档含字段瘦身/注入分层/沉档/压缩协同四方案及取舍与 token 收益估算;③有用户评审拍板记录;④实施条目登记完成并与本条互链
- 优先级: P1

## R-313 需求发现分层:Discovery Record、待确认字段生命周期与歧义落点,让发现阶段先于交付冲动 [todo]
- refs: R-248 R-311 D-577 docs/design/weakness_register_20260820.md
- 内容: 批1 Discovery Record:中/大需求 req add 前产出轻量发现记录(Intent 用户真正要什么/Explicit 用户原话/Assumptions 推断/Ambiguities 歧义/领域对象/最小成功闭环/延后决策),来源字段必须含用户原话引用而非只写「用户消息」;批2 待确认生命周期:核心语义类待确认未决时,设计冻结与进入 doing 前要求先走 question 工具或用户显式豁免留痕——把「检测到的歧义」从散文变成有 teeth 的状态;批3 新增限定词一致性检查:需求文本出现用户原话中没有的关键限定词(如用户说「收藏」需求写「浏览器书签」)时机械提示「未确认解释:确认/标 assumption/移除限定」;批4 复测:文章获取器场景回归,同样输入下歧义在冻结前被逼出
- 复杂度: 大
- 来源: 2026-08-20 用户需求发现实测(文章获取器项目)+外部评估。实测对评估的关键修正:模型其实检测到了歧义并写入原 R-003 待确认字段(逐字:「收藏」默认解释为浏览器书签/收藏夹;需要确认是否还要适配特定网站的站内收藏),但 question 工具全程零调用、默认解释选错边(上下文「帖子喜好」指向站内收藏)、设计冻结把「浏览器书签 API/导入文件」写进权威数据源——待确认是死字段,没有任何门禁消费它,歧义靠用户事后「先知乎就行」纠正
- 标签: 核心
- 设计文档: docs/design/weakness_register_20260820.md
- 边界: 不加重 hard gate,Discovery Record 是轻结构不是审批流;「图 ontology 是否 user-centric」这类高级语义判断不做规则化(评估共识:靠模型或产品 persona,规则 gate 判不了);小需求不强制;不改 R-248 prior-art 门,两者在 req add 处组合(prior-art 管「查已有方案」,本条管「问题究竟是什么」)
- 验收: ①中/大需求缺 Discovery Record 或来源无用户原话引用被拒,有定向测试;②含未决核心语义待确认的条目在冻结/doing 前被拦并指向 question,豁免路径留痕可审计;③限定词一致性检查有真实触发与放行案例各一;④文章获取器场景复测:「收藏」歧义在登记前被逼出而非用户事后纠正;⑤既有小需求登记路径无回归
- 优先级: P1

## R-314 单线程运行时协作者工具自动隐去 [todo]
- 复杂度: 小
- 标签: 前端
- 验收: 检测到仅有 1 条运行线时，前端隐藏"协作者工具"组件
- 优先级: P1
