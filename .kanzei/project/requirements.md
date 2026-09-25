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
- 进展: 2026-08-17 用户确认：R-283 是二期系统升级总控条目，具体工作必须按不同任务展开为独立条目。现有子条目为 research(R-221/R-276/R-277)、记忆晋升与遥测(R-286)、统一事件契约(R-284)、金色神经流(R-285)、voice(R-287)；Wave 0 先修当前 dev 缺失的 D-428。R-283 保持 3/5，只在子条目形成真实证据后更新门禁，不再被取活为实现任务。；状态对账: 正文旧字段 `doing` 与权威标题状态 `doing` 重复;已移除正文副本。
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
- 进展: B4 已实现并提交 `605c6413`，按验收逐条对账：① 既有 B1 `crates/kanzei-core/src/experience_events.rs:20-41,97-119` 提供 snake_case JSON schema/validate，`T-1786922726586` 覆盖 schema 与 legacy 归一化；② memory/research 事实由 B2 `crates/kanzei-core/src/experience_events.rs:126-164` 幂等落库/回放，`crates/kanzei-app/ui/13-memory.js:18` 消费 `experience_facts` 恢复前端投影，`T-1786922726586` 覆盖 memory/research；验收降级：voice state 原文→当前仓库无真实 ASR/TTS/VAD 生产者，不能伪造 voice 证据，保留 R-287 缺口；③ B3 `crates/kanzei-app/ui/01-core.js:78-100,180-213` 按 session/topic/entity 归并并隔离后台 session，`T-1786922726593` 回归通过；④ B4 `01-core.js:102-150,197-200` 在一帧内合并 text delta、携带 delta_count，未知事件只诊断，`scripts/ui-runtime-smoke.mjs:1447-1481` 断言三段文本合为一次表现事件，`T-1786922726593` 通过；⑤ B4 `01-core.js:152-160` + `13-memory.js:18` 从持久 facts 恢复且 event_id 幂等，`scripts/ui-runtime-smoke.mjs:1483-1495` 断言首次恢复/重复恢复，`T-1786922726593` 通过。最终验证：`T-1786922726593` 六条前端冒烟全通过，`T-1786922726594` cargo test -p kanzei-app 231 passed。当前仍保持 doing：voice state 生产者缺口未满足，不能关闭 R-284。；状态对账: 正文旧字段 `todo` 与权威标题状态 `doing` 冲突;已移除正文副本。
- observed_head: 605c64135451bae1bd3128ef2a20666b98d57504
- observed_worktree_hash: fnv1a64:abf42289ad631ab3
- recorded_at: 1787246367339
- 批次表: B1 契约与事件包络：snake_case、持久事实/瞬时表现/high-frequency delta、归属字段和 schema；B2 后端生产者：memory/research/voice 事件接线与真实持久事实映射；B3 前端归并：按 session/topic/memory 归属入 store，再分发动画/音频/工作台；B4 压缩与恢复：delta 合并、未知事件诊断、重连回放和跨会话回归。
- 停车: B1-B4 已完成并通过验证;剩余验收②的 voice state 生产者属于 R-287 的真实 ASR/TTS/VAD 范围,本条让位 R-287,待其落地后回来补验收②;恢复人:agent;解除条件:R-287

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
- 进展: 2026-08-17 主会话完成批1+批2。新增 22-neural-flow.js:确定性神经拓扑、单 RAF 调度、活动/静息节流、ResizeObserver、DPR≤1.75、窗口隐藏暂停、reduced-motion 静态降级,实现呼吸/流动/结晶与失败阻塞四类表现;index.html 接主对话 Canvas 与记忆流舞台,style.css 使用 app 金色 token 且 Canvas pointer-events:none。真实接线:07-events.js 的 turn/text/reasoning/tool start/end/compacted/stopped/done/error;13-memory.js 的 snapshot/search/consolidate/candidate discard/cleanup,失败事件独立且不播放成功结晶。测试:T-1786922726035 前端全冒烟通过;T-1786922726036 真实 Chromium 1440/800 视觉验收通过。剩余批3=依赖 R-284 的原生 recall injected/candidate promoted/research/voice 事件;批4=设置/质量档/真实 WebView2 长会话性能与录屏。；状态对账: 正文旧字段 `doing` 与权威标题状态 `doing` 重复;已移除正文副本。

## R-287 voice 语音交互:流式 ASR/VAD、语音回复、定制声音与打断 [doing]
- 优先级: P2
- 复杂度: 大
- 标签: 后端 前端 语音 集成
- 来源: 2026-08-17 用户要求接入语音功能并优先支持定制语音；用户原话：「接入语音功能并优先支持定制语音」；技术调研结论为 chained voice:音频→ASR→现有文本 Agent→TTS。
- 依赖: 
- refs: R-285 docs/design/phase2_system_upgrade.md
- 内容: 按 phase2_system_upgrade.md §5.6 分五批。批1 Rust cpal/WASAPI 设备枚举、录音播放和 50 条中英/代码术语基准,sherpa-onnx 与 whisper.cpp 对照。批2 push-to-talk+partial/final 字幕回填输入框。批3 TTS provider adapter 与播放/暂停/停止。批4 经授权的 voice profile:CosyVoice 本地 sidecar、OpenAI Custom Voice、ElevenLabs 可替换。批5 VAD 自动收尾、barge-in、流式首包和 R-285 动画联动。
- 安全: 默认不保存原始录音;参考音频/consent 放应用数据目录且不进 Git/memory/普通日志;provider key 只在 Rust/后端;删除 voice 同时报告本地与云端结果;无明确授权不得克隆声音。
- 边界: 第一阶段不改为端到端 speech-to-speech,保留文本、工具、权限、审计和记忆链;模型下载/长时间 GPU 基准先由用户确认执行;任何 provider 激活必须有真实认证输出,配置或容器日志不算完成。
- 验收: ①真麦克风设备切换、录音和播放;②50 条基准报告 partial/final 延迟、CER/术语修正率、实时率与资源;③ASR 文本进入现有输入和权限链;④TTS 失败不阻断文本回复;⑤真实授权定制声音输出;⑥播放中插话能中断并恢复会话状态;⑦原始音频、consent、key 的存储与删除边界有测试。
- 批次: 0/5
- 进展: 已完成设计冻结与登记完整性补齐；尚未改源码。因 voice 属于高影响第三方集成，provider/模型/设备/授权声音与下载及 GPU 基准权限不能由 agent 猜测；已将以下待确认项保留在阻塞：ASR 选 whisper.cpp 或 sherpa-onnx，TTS/定制声音选 CosyVoice 本地 sidecar、OpenAI Custom Voice 或 ElevenLabs，Windows 真麦克风/播放设备，是否允许模型下载和长 GPU 基准，真实授权声音验收窗口。收到用户选择后从批1设备枚举、录放与50条基准开始。
- 前置: R-284(事件契约 B1-B4 已交付,voice 词表可直接消费;voice state 生产者由本条产出,R-284 验收②反向等待本条)
- 对账: 2026-09-05 对账:原「依赖: R-284」与 R-284 停车「让位 R-287、待 voice 生产者落地」构成互等环(R-284 等 R-287 产 voice 事件,R-287 等 R-284 关闭),两条永远互相阻塞;R-284 的契约已交付,本条所需的只是词表而非其关闭,故依赖降为前置断环。设备/模型基准批1 本就可先行
- 发现记录: {"Intent":"在保留文本 Agent、权限、工具、审计和记忆链的前提下接入可用语音交互","Explicit":"R-287五批：设备录放与ASR基准、PTT字幕、TTS、授权定制声音、VAD/barge-in/动画联动；验收含真设备、50条报告、输入权限链、TTS降级、真实授权输出、打断恢复与敏感数据边界","Assumptions":"首版采用音频→ASR→现有文本Agent→TTS；默认不保存原始录音；provider key仅后端；模型下载和长GPU基准由用户确认","Ambiguities":"真麦克风/音频设备、ASR/TTS provider凭据与模型、授权定制声音目标、真机与性能基准环境尚未指定；R-285动画联动接口需与现有事件契约核对","领域对象":"VoiceDevice、AudioCapture、ASR partial/final、VoiceProfile、TTS playback、barge-in、voice state event、consent与secret storage","最小成功闭环":"真实麦克风录音→ASR文本进入现有输入与权限链→文本回复可见；TTS可选且失败不阻断；退出后敏感音频/consent/key边界可验证","延后决策":"provider与模型组合、定制声音授权方式、VAD阈值、原始音频保留策略、50条基准硬指标与真设备/真机验收窗口由用户确认后冻结"}
- 阻塞: 用户：确认 R-287 的 ASR/TTS/provider 与模型组合、Windows 真麦克风/播放设备、模型下载/GPU 基准权限、授权定制声音及真实验收窗口；agent 不得在这些高影响决策未确认时实现第三方集成。解除条件:用户
- observed_head: 4a85596cbcb5f8a4fe056f11b741317a14216f75
- observed_worktree_hash: fnv1a64:30acc4843d86176e
- recorded_at: 1788659241649

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
- 停车: 排队:原排队对象 R-242/R-296 已 done、D-504 已恢复为 WIP;排在 R-249 之后恢复 B4 桌面 E2(UIA 基座 scripts/ui-desktop-uia.ps1 可冷启动自拉起 kzapp);恢复人:agent;解除条件:R-249

## R-245 Tool Result Spill 与显式空间整理：完整 artifact、可恢复引用、无自动过期 [doing]
- refs: D-209 R-180 D-297 D-298 R-242 docs/design/deepseek_harness_upgrade.md
- 依赖: R-242
- 内容: 统一工具结果为 Inline 或 Spilled{preview,artifact_id,bytes,sha256,retrieval_hint}；read 优先指向原文件 offset/limit，bash、git、test_record、web 等完整原文进入与 state.db 同生命周期的 Git 忽略运行目录。提供存储与整理入口，按类别、会话、日期、大小预览占用，支持清理无引用 artifact；经风险确认后，用可恢复失败的删除计划物理删除已选会话的事件、投影和引用 artifact；并支持 SQLite checkpoint、VACUUM 与迁移备份管理。默认不自动过期。
- 复杂度: 大
- 批次: 8/9
- 来源: DeepSeek Harness spill policy、本地 state.db 输出分布统计，以及用户确认“不自动过期但需要显式整理入口”。
- 标签: 核心
- 边界: 任何事件仍引用的 artifact 不得被静默清理；整理前显示预计释放空间和不可恢复范围，执行后给清单与实际释放量。32 KiB 先做 shadow telemetry。普通会话删除保证产品不可检索且重启不复生；安全整理才处理 SQLite freelist、WAL 和含旧正文备份。当前库为 WAL、secure_delete=OFF、auto_vacuum=NONE，不能把 DELETE 行等同磁盘字节已擦除。弹窗必须区分仅删除与删除并安全整理，取消零写入；显式整理不是定时任务。
- 迁移与回滚: artifact 原子写入后再提交引用事件；任一步失败不得留下有效事件指向缺失文件。删除使用引用图和事务清单，失败可重试；schema 迁移前备份。关闭 Spill 可回到 Inline，但已有引用仍必须可读。
- 阻塞: 
- 验收: ①32 KiB shadow telemetry 不改变模型输入并产出按工具分布；②Spill 原文 sha256 与工具原输出一致，重启后可取回；③事件提交与 artifact 写入故障注入无悬空引用；④明确无自动过期任务；⑤整理入口列出总占用、数据库、WAL、freelist、artifact、无引用文件和迁移备份并支持 dry-run；⑥清理引用中 artifact 被拒，清理无引用 artifact 成功且释放量可核对；⑦删除弹窗列出会话事件、轨迹、草稿与 artifact，仅删除和删除并安全整理差异明确，取消零写入；⑧确认删除后事件、投影和引用 artifact 产品层不可检索且重启不复生，删除计划任一点失败可恢复重试；⑨安全整理仅在运行静止时执行，成功后 checkpoint、VACUUM 与备份处置可核对，busy 或失败不静默；⑩权限、路径逃逸、不可预测文件名和磁盘配额有测试。
- 优先级: P1
- 进展: B8 已提交 eab92725：`crates/kanzei-core/src/runner/tool_exec.rs` 增加 2 GiB 全目录配额计量、共享独占锁保护 spill/telemetry 写入、超限时 Inline 截断且不创建 artifact；保留现有 1 MiB spill 阈值。配额专项 19 项通过(T-1786922727007)，crate 全量 292 项通过(T-1786922727010)，workspace fmt 通过(T-1786922727011)。B9 仅剩验收⑦真实桌面删除弹窗 E2；当前 kzapp 是既有用户会话，D-746 禁止接管，需用户确认窗口空闲并允许执行；届时验证弹窗内容、两种删除选项与取消零写入。
- observed_head: eab92725608f19a803cf39f2cc74dfa25acf39f9
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1790338268439
- 停车: 排队:B8 配额的性能与截断问题见 D-760(已在发版分支修复,待核验);⑦真实桌面删除弹窗 E2 按 A-002 转入 R-101 延期清单后即可关闭(自举循环运行在安装位 kzapp 中,无法自测该弹窗);解除条件:D-748
- 对账: 2026-09-25 用户拍板验收⑩磁盘配额:上限 2 GiB,超限时降级为 Inline 截断并注明(原话「2 GiB,超了退回截断。先这样」)。配额统计 .kanzei/artifacts/tool-results 总占用,与 R-376(外置阈值降到 32 KiB)同口径;停车前提已达成,剩余⑦真实桌面 E2 由 agent 在 kzapp 空闲时自行执行。

## R-249 工具结果可返回图片:ToolOutput 承载 image part,打通图片读取与 UI 截图 [doing]
- refs: R-014 R-101 R-244 R-245
- 依赖: 
- 内容: 现状 `ToolOutput.content` 只有 String(kanzei-harness/src/tool.rs:178),任何工具都无法把图片交给模型;`Part::Image` 的三协议映射早在 R-014 交付,但入口只有桌面端用户附件(kanzei-app/src/state.rs:29)。本条把 ToolOutput 扩成可携带 image part,并打通两个消费点:①`read` 读图片文件(PNG/JPEG/WebP/GIF)按 media_type 编码返回;②UI 自检补截图通道——现有 `ui_probe` 窗口通道加 `screenshot`,让 ui_dom/ui_style 的结构读数配上真实渲染画面。
- 复杂度: 大
- 批次: 0/4
- 来源: 2026-08-14 三系统工具面对照(DeepSeek harness / Claude Code / kanzei):read_image 是唯一的能力硬缺口。桌面端 ui_dom/ui_console/ui_style 能读结构与数值但看不见渲染结果,对齐、遮挡、观感一类问题无法自查。
- 标签: 核心
- 边界: ToolOutput 是 harness 核心契约,R-244 明确要冻结「ToolOutput 公共契约」、R-245 要把它改成 Inline/Spilled 二态——本条**不得抢在 R-244 之前改这个结构**,否则必然返工。图片体积走 R-245 的 spill 口径,不在 ToolOutput 内联大 base64。不实现 UI 点击/输入/滚动(那是 R-101 的 E2 harness 范围),本条只做「看得见」不做「动得了」。deepseek_responses 协议当前丢弃 Image part,本条不负责补齐该 provider,但要在 provider 不支持时给出显式降级提示,不静默丢弃。
- 进展: 2026-08-14 批1 交付(1831239)。勘察修正了原条目的一处前提:`Part::Image` 的三协议映射早在 R-014 就通了,缺的只是**工具侧出口**,协议层零改动即可打通——不必等 R-244。实现:①ToolOutput 增 images 载荷(空 vec 与既有行为逐字节一致,53 处 `ToolOutput {` 里只有 4 个真构造点,其余是解构模式);②read 按 magic bytes 而非扩展名识图(PNG/JPEG/WebP/GIF),扩展名撒谎会让 media_type 与真实字节不符、provider 400 且报错指向请求体;③图片 Part 只能追加在所有 ToolResult 之后——Anthropic 要求 tool_result 块在 user 消息最前,而 results[i]↔calls[i] 由 note_step 的 debug_assert 锁着,中间也不能插;④provider 不支持时**在进 messages 前**降级为显式文本说明,判据收敛为 Route::supports_images() 与 client.rs 硬拒绝共用一处。新增 10 条测试。 || 2026-08-14 批2 交付:新增 ui_screenshot 工具(kanzei-app/src/screenshot.rs)。实窗验证三轮才对,两次假绿都值得记——①未声明 DPI 感知时 GetWindowRect 返回虚拟化坐标(2582px 的窗口报成 1295px),抓到的是横跨多个窗口的错误区域;②改用正确矩形后,屏幕 DC 抓取拿到的是压在上面那个应用的界面(kzapp 被完全遮挡),内容丰富所以 looks_blank 一路放行。两次都是「测试通过但抓的不是那个窗口」。最终改用 PrintWindow+PW_RENDERFULLCONTENT 离屏渲染,免疫遮挡,在完全被盖住的状态下抓到 kzapp 完整界面并经人眼与用户实拍逐项比对一致;屏幕 DC 仅在 PrintWindow 失效且本窗口为前台时作回退,不是前台宁可报错——返回别人的界面比返回错误坏得多。测试记录 T-1786705800。 || 2026-08-16 复核:批1 已解除;批3 的依赖 R-244 已 done 并归档(Tool Pipeline 契约已冻结),只余 R-245 确定图片类 artifact 的 spill 落点,而 R-245 自身仍等 R-242。当前 park 的唯一原因是 WIP 槽由 R-195 持有(用户 2026-08-16 指定)。解除动作: R-195 关闭后清本字段直接续做批2。解除人: agent(批2)/ 依赖自然解除(批3 等 R-245)。 || 2026-08-16 让位:本轮按队列顺序取 R-186(P0 队首),本条 doing→todo 让位,待 R-186 交付后按队列轮转;批1/批2(ui_screenshot/read 识图)已交付,剩余批3 等 R-245(R-242 完成后才解)。；状态对账: 正文旧字段 `todo` 与权威标题状态 `doing` 冲突;已移除正文副本。
- 阻塞: 
- 验收: ①read 读 PNG/JPEG/WebP/GIF 各有定向测试,media_type 正确,非图片文件走原文本路径无回归;②ui_probe screenshot 返回的图片能被模型消费,桌面端实测有轨迹;③provider 不支持图片时有显式降级诊断;④图片 artifact 走 R-245 spill,ToolOutput 不内联超阈值 base64;⑤R-014 既有附件路径逐条无回归;⑥ToolOutput 结构变更后既有全部工具返回路径编译通过且行为不变(机械核验)。
- 优先级: P1
- observed_head: 98d7a586f38a09f5b449b75b7a3c93c62d01852f
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786835811870
- 停车: 排队:R-244 已 done、R-245 spill 落点已交付,依赖改为前置;排在 R-299 之后恢复批3;恢复人:agent;解除条件:R-299
- 前置: R-245(spill 落点 B1-B7 已交付,仅余桌面 E2/配额验证缺口,不再作为阻塞依赖)
- 对账: 2026-09-05 对账:批1(1831239)/批2(ui_screenshot)已交付;批3 所等的 R-245 spill 落点(Inline/Spilled 二态与 artifact 目录)已随 R-245 B1-B7 落地,R-245 仅余真实桌面 E2 与配额语义验证缺口,与本条批3 无关,故 依赖 R-245 改为 前置 R-245,不再等其关闭

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
- 批次: 1/3
- 进展: 批1代码已完成并通过：crates/kanzei-core/src/runner/event.rs 增加 TaskTrace.text；runner/subagent.rs 上抛 AssistantMessageCommitted 完整 Text parts 并有 assistant_message_text_keeps_full_text_parts 测试；crates/kanzei-app/src/run/events/mod.rs 转发 text/usage；phase_pipeline.rs 两处终态文本改为完整原文、不再使用 lines().next()；ui/06-agent-panel.js 使用 renderMarkdown 渲染正文且工具/正文默认折叠。T-1786922726071、T-1786922726072、T-1786922726077 均通过，六条前端冒烟与 workspace 全量覆盖已完成。当前 39 文件 staged，最近实际 hash 6eb4f03c4de88cb4；已按该 hash 执行 R-281 B1 提交，仍被结构化 git 的旧 source_test_gate 拒绝：门禁仍选 R-285 Playwright 记录，未读取当前源码指纹记录。源码侧 crates/kanzei-tools/src/git.rs:746 已有 last_passed_for_fingerprint 修复且对应测试通过，但当前 git 工具运行态尚未加载。批1 已提交(ed305ae8)。被拦的真实原因是旧 kzapp(2026-08-09 安装版)把 13 位毫秒测试 id 当秒比较，恒选无收尾的 R-285 Playwright 记录，详见 D-349 进展。下一步做批2 transcript Tauri 读取通道。
- observed_head: 148386f3d467b701f334932b2bfc85bbcfcea475
- observed_worktree_hash: fnv1a64:6aa6fbd939a238f6
- recorded_at: 1786933041284
- 停车: 排队:前置 R-221 已 done;排在 R-340 之后恢复批2 transcript Tauri 读取通道;恢复人:agent;解除条件:R-340

## R-288 Android 真机 E3 验收:移动端 PWA 通知与双向消息真实链路 [todo]
- refs: R-059 R-270 R-271 D-389
- 内容: 在同一 LAN 下使用 Android 真机打开 PWA，完成 bearer 配对，验证主/次代理通知展示、SSE 更新与消息发送；只补真实设备证据，不重做桥接或 PWA。
- 复杂度: 小
- 批次: 0/1
- 来源: R-059 拆分；R-270/R-271 已完成服务端与 PWA 实现，剩余仅是真实 Android 设备验收。
- 标签: 流程
- 阻塞: 
- 验收: ①Android 真机可访问并完成鉴权；②收到真实运行成功/失败通知；③从手机发送消息后服务端产生可追溯事件；④保存截图、端口/设备与 session 证据；⑤失败时明确网络、权限或设备边界。
- 优先级: P3
- 停车: 原停车前提 R-333 已 done;本条验收全部需要同一 LAN 下的 Android 真机(打开 PWA、bearer 配对、收通知、发消息),agent 无设备可用;需用户提供真机并预留一次配对窗口;解除人:用户;解除条件:用户

## R-299 IPC 与事件契约机械比对扩面 [doing]
- refs: R-284
- 内容: scripts/ipc-contract.json 仅锁 docs_snapshot 一个顶层键(1/104 command),而该机制自述正是 30+ 命令手搓 JSON 两侧各写一遍字符串(crates/kanzei-app/src/ipc_contract.rs:1-19);后端 emit 事件集合(kz:compacted/kz:meta/kz:reasoning/kz:step 等)与前端 on() 订阅集合无任何机械求差;ui-runtime 冒烟的多会话/记忆页 fixture 是前端作者手写,后端改字段名照样全绿。扩契约文件覆盖高频 command,emit/listen 求差入冒烟
- 复杂度: 中
- 来源: 2026-08-18 全库勘察
- 标签: 核心
- 边界: 作为 R-284 事件契约的前置批次,不与其四批重复;词表定义归 R-284
- 验收: 契约覆盖高频 command;emit/listen 集合求差入冒烟;后端改事件名或字段名可被门禁捕获
- 优先级: P2
- 停车: 排队:前置 R-296 已 done,B1 已入 dev(7188ba76);排在 R-323 之后恢复后续批次;恢复人:agent;解除条件:R-323
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
- 停车: B1/B2 已合入 dev 并随 build-39cd402f 进入运行态;剩余 B3 前端依赖拓扑图与 B4 验收由主会话执行(执行者字段:循环勿取),需用户开一次主会话安排,或明确放行给自举循环;解除人:用户;解除条件:用户
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
- 进展: B1 已提交：`a173eb6a`，globals 实时收集与缓存降级已验证。B2 已提交：`481f4463`，`scripts/verify-policy.mjs:5-91`、`scripts/verify.ps1:41-178`、`scripts/package.ps1:109-114` 完成路径裁剪和 full evidence 门禁；D-642/D-643/D-644 已关闭，归档收口提交 `1e28fe28`，真实 package 拒绝 cropped evidence 证据 T-1786922726629。B3 已提交：`b5d23c28`，`crates/kanzei-tools/src/test_record/coverage.rs` 优先消费当前 HEAD 的 `dist/verification.json`，要求 `all_pass=true`、`ui_runtime/ui_lint/ui_i18n` 三项 pass，并校验 24 小时新鲜度与工作区源码指纹；T-1786922726630：424 passed、0 failed、1 ignored。B4 已提交：`f2d10c44`，`crates/kanzei/src/cli/metrics.rs:401-437` 输出 v1；`scripts/metrics-regression-gate.ps1:27-70` 拒绝口径漂移且 cargo build 已移至 `scripts/verify.ps1:99-101` 独立 metrics_build；`scripts/parallel-lines-regression.mjs:4-23` 使用 loadUiSources 与源码标记；`scripts/ipc-event-smoke.mjs:46-53`、`scripts/check-ps1-bom.mjs:36-40` 增加下限/空集断言；`crates/kanzei-tools/src/git.rs:1929-2008`、`.github/workflows/ci.yml:32-33` 清单同步；`docs/design/metrics_baseline.md:4,11-60` 更新 v1 基线。D-645～D-650 已逐项 fixed，T-1786922726632：fmt、kanzei-tools 424 passed、kanzei 44+32 passed、JS/PS smoke 和真实 metrics gate 全通过。提交锚点补正：`8ad3bb28`。T-1786922726633：真实当前 HEAD targeted verify 3.34s，生成绑定 HEAD 的 verification.json，但 changed=1 且 rust/frontend 均 false，明确不能核销 frontend-only ①。T-1786922726634 与 T-1786922726635：cargo test --workspace 全量通过（各 crate 无 failed）。T-1786922726636：真实当前 HEAD full verify 通过，14/14 checks、skipped_steps 为空、rust/frontend 均执行，绑定 `b564f2134ae6c8d8828a33b42745df0f48a42b5a`，耗时 105.67s；该证据证明 full evidence，不冒充 frontend-only ①或真实前端关闭③。验收对账：①仍缺真实“只改一行前端”verify 墙钟 <15s；②仍缺门禁上线后 globals 缺陷族零新增证据；③仍缺一次真实前端条目关闭复用 verify 证据；④已满足，T-1786922726632 的 v999 metrics fixture 拒绝与真实 gate 通过；⑤已满足，T-1786922726629 证明 package 拒绝 cropped verification。R-309 保持 doing，批次 4/4；虽然 B4、targeted/full verify 与 workspace 测试已完成，不能把①-③的证据缺口降级或误标 done。；状态对账: 正文旧字段 `doing` 与权威标题状态 `doing` 重复;已移除正文副本。
- observed_head: f2d10c44f9e47b2f199d346d13429e25d4a9f196
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787266377330
- 批次: 4/4
- 停车: 排队:原停车前提(用户当轮指示从缺陷队首开始)已过期;排在 R-319 之后恢复①～③真实证据:①只改前端的 targeted verify 墙钟实测;②globals 补偿机制已由 R-331 整体删除,该缺陷族无法再新增;③一次真实前端条目关闭复用 verify 证据;恢复人:agent;解除条件:R-319

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
- 批次: 2/3
- 批次表: B1 测量：从不少于 5 个真实会话提取 conventions、memory-index、resolved-control-state、当前条目、工具输出及进展/对账/停车字段账单；B2 设计：形成机器字段、注入分层、历史沉档、压缩协同四方案，记录取舍与 token 收益估算；B3 评审收口：记录用户拍板，登记实施条目并与 R-312 互链。
- 进展: B0 已复核：R-312 无既有实现，边界保留 D-201/R-104/D-573。B1 已完成：读取真实 `.kanzei/state.db` 的 `episodes.context_json`，覆盖 11 个 distinct session 中 7 个最新账单非空 session；账单落档 `docs/design/context_supply_bill_20260821.md`，并确认 6 个现代 dev session 平均 66868.7 字符，tools/schema 48.6%、agent/system 20.7%、dev/conventions 17.4%、dev/memory 5.7%；当前 context_report 尚未拆出进展/对账/停车字段级 token 与写入频次，已明确记录缺口。B2 已完成：同一设计文档第 6-7 节形成四方向候选——机器事实信封、WIP/依赖注入分层、当前批次视图与完整历史分离、可重取注入与压缩协同；每项均写现有能力、取舍、收益假设和不应相加的估算，初步建议先方向一/三，再协同方向二/四，暂不改变 D-201/R-104。架构索引已登记并校验通过。下一步 B3：取得用户对四个拍板问题的明确选择，登记实施条目并与 R-312 互链；未获拍板前不写 accepted decision、不关闭本条。
- observed_head: f446bd018e2e03242a0d4756cdb77ccf4b76b56b
- observed_worktree_hash: fnv1a64:7580d1080253583e
- recorded_at: 1787299505789
- 阻塞: 
- 停车: 原停车前提 R-333 已 done;B1/B2 已完成,B3 需用户对 docs/design/context_supply_bill_20260821.md §7 的四个拍板问题作出选择(注入层级默认、当前批次视图+可回放历史、方向一先行、conventions 按需化实验),拍板后 agent 登记实施条目并关闭本条;解除人:用户;解除条件:用户

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
- 进展: B1 已完成：复核真实步数边界与轮末收口链。B2 已落地并提交 08992b47：events/mod.rs:256-435 以真实 test_record passed、git stage 成功、无源码编辑/审批/未知工具为白名单，最多授予一次 2 步 extension；core runner/drive.rs:243-256 在事件回调后读取有界信号，普通 max_steps 不变。B3 已提交并补强，提交 f62097cf：events/mod.rs:167-216 写 run.transaction_budget_extended，含 run_id、step、base_max_steps、extension_steps、reason、trigger、allowed_actions，并按 run_id 去重；persistence.rs:70-119 新增轮末 run.transaction_budget_result，按同一 run_id 的 run.trace 重放 extension 后实际 tool.completed 动作并写 completed/failed 结果。T-1786922726709 覆盖事务状态/taint/去重/结果事件；D-679 已由 2b27245f 修复失败 test_record/stage 永久 taint，回归 T-1786922726707。B4 基线已取：T-1786922726710 从真实 .kanzei/state.db 选取最近 10 条 steps>=28 episode，9 条 32-step、1 条 28-step，均无新 extension/result event；这是 rollout 前基线，不能宣称事务中点切轮或纯恢复调用已下降。完整 kzapp 回归 T-1786922726708 为既有两夹具失败（243 passed/2 failed），新增 5 个 R-319 测试均通过。下一步：积累并重放至少 10 条 f62097cf rollout 后真实长程条目；若无足够 post-rollout 样本，验收④保持未满足，不关闭 R-319。
- observed_head: f62097cfde97e559534c16f898a6c0f1fb5a3e23
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787306265598
- 停车: 排队:排在 R-281 之后恢复 B4(重放 f62097cf rollout 后不少于 10 条真实长程 episode 对比基线 T-1786922726710);恢复人:agent;解除条件:R-281

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
- 停车: 排队:原停车前提 D-568 改排缺陷队列;排在 R-101 之后恢复 B4 真机端到端验收(目标条件 loop 跑一次真实会话看回显与自动清除);恢复人:agent;解除条件:R-101

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
- 停车: 排队:原停车前提 R-353 未提交改动已不存在;排在 R-309 之后恢复 B2;恢复人:agent;解除条件:R-309

## R-340 运行画像任务主视图与 session/round 下钻 [doing]
- 内容: 在 R-338 task projection 契约评审并可消费后，重做运行画像前端主展示：以已关闭 task 为趋势/主列表，进行中 task 独立展示；点击 task 下钻到 session 分段、input 与 round/episode 细节，保留 provider/model、工具、上下文账单和错误；保留旧 rounds/legacy 提示与空态，不在前端自行聚合。
- 发现记录: {"Intent":"让用户先看到可比较的任务完成结果，再查看长 session 内的执行细节","Explicit":"task close 是主粒度，session 仅下钻，未关闭任务单列进行中","Assumptions":"后端 projection 提供 completed_tasks、in_progress_tasks、trend 和下钻所需 rounds/sessions","Ambiguities":"task 标题、关闭 outcome 标签、下钻交互和跨 session 展示待评审","领域对象":"运行画像页面、task trend、completed task、in-progress task、session drilldown、round、legacy","最小成功闭环":"真实 metrics 入口加载 task projection，完成/进行中分区正确，点击 task 能看到 session→round 下钻，未关闭不进入趋势","延后决策":"分页/排序、筛选字段、移动端布局和旧 rounds 的下线时间"}
- 复杂度: 中
- 来源: 用户消息：「运行画像的部分显示的统计量非常少，会话的粒度也有问题，我们现在的鞭挞模式其实一个会话非常长，这个不应该按照会话来分，按照执行任务关闭的粒度来显示我觉得比较合理。」
- 标签: 前端
- 边界: 只覆盖运行画像 UI 真实消费者和 task/session/round 展示，不在前端复制 task 状态机，不修改后端事实生产和迁移。
- 验收: ①真实 metrics 入口消费后端 task projection 而非 display-only fixture；②已关闭 task 趋势、进行中单列、legacy 空态均有运行时断言；③task→session→round 下钻显示真实数据与错误/空态；④前端保持 snake_case API 适配边界，不擅自决定未确认语义
- refs: R-337 R-338
- 优先级: P1
- 依赖: R-338
- 进展: 用户已解除语义评审阻塞；R-340 仍依赖 R-338 的真实 task projection/API。待后端契约可消费后，再修改 13-memory.js 的真实 metrics 消费者，不在前端自行聚合或猜测字段。
- 阻塞: 
- observed_head: 25485bdf9c636006b964c5653fe4c9d1bcd85e22
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787898129566
- 确认记录: 用户确认（本轮）：“按建议全部确认”：前端沿用 task 主趋势、进行中/legacy 分区、session/round 下钻和仅显式 attach 的后端语义。
- 停车: 排队:依赖 R-338 已 done,原停车前提 D-568 改排缺陷队列;排在 R-358 之后恢复实现;恢复人:agent;解除条件:R-358

## R-341 运行画像任务级真实链路收口与回归矩阵 [doing]
- 内容: 作为 R-338/R-339/R-340 的链路收口，验证真实入口从 task 事实生产、SQLite projection/API 查询到运行画像 UI 的端到端闭环：创建并关闭多个任务、同一长 session 多轮、未关闭任务、session 下钻、legacy 历史和失败路径均可复核；不以单测、viewport 模拟或替身服务冒充真实链路。
- 发现记录: {"Intent":"防止后端 task 事实、历史兼容和前端展示各自通过但真实运行画像仍断链","Explicit":"任务关闭主粒度必须贯通真实入口到 UI 效果","Assumptions":"R-338/R-339/R-340 各自交付后可在真实桌面端重放","Ambiguities":"真实验收环境、任务创建/关闭入口和跨 session 场景等待前置设计评审","领域对象":"真实 Tauri 入口、session_events、SQLite projection、run_metrics API、运行画像 UI、task/session/round","最小成功闭环":"真实入口产生 task start/close，UI 显示关闭 task 趋势并能下钻 session/round，未关闭与 legacy 边界正确，失败路径可见","延后决策":"桌面验收账号/项目、跨 session 是否纳入首版和发布门禁范围"}
- 复杂度: 中
- 来源: 用户消息：「运行画像的部分显示的统计量非常少，会话的粒度也有问题，我们现在的鞭挞模式其实一个会话非常长，这个不应该按照会话来分，按照执行任务关闭的粒度来显示我觉得比较合理。」
- 标签: 流程
- 边界: 只验证 R-338/R-339/R-340 组成的真实用户链路与回归矩阵，不实现 task 事实、迁移或 UI 组件，不用替身或静态副本核销。
- 验收: ①真实任务开始/关闭入口到 SQLite projection/API/UI 有可重放命令和真实目标；②长 session 多任务、未关闭、legacy、失败路径逐项有结果；③task→session→round 下钻与主趋势的用户可见效果可核验；④单测、viewport 模拟、替身服务仅作辅助，不能作为链路关闭证据
- refs: R-337 R-338 R-339 R-340
- 优先级: P1
- 依赖: R-338 R-339 R-340
- 进展: 用户已解除语义评审阻塞；R-341 仍依赖 R-338/R-339/R-340 完成。待三项实现与自动化证据具备后，执行真实桌面入口到 SQLite projection/API/UI 的回归矩阵。
- 阻塞: 
- observed_head: 25485bdf9c636006b964c5653fe4c9d1bcd85e22
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787898137370
- 确认记录: 用户确认（本轮）：“按建议全部确认”：真实收口覆盖显式 task start/close、长 session 多任务、未关闭、legacy、失败和 task→session→round 下钻；不以替身或单测替代真实链路。
- 关联缺陷: D-746：运行画像历史输入审计相关全表扫描阻塞桌面窗口；2026-09-08源码修复与10项定向测试已完成，真实数据库只读task_metrics查询267-281ms；桌面安装及真实窗口点击验收待执行，本需求状态不变。

## R-344 Experiment Runner:本机与 SSH 执行、@@kanzei 回调解析与 run 事实落盘 [doing]
- 内容: 按设计 §3/§5 实现实验运行器:local 起进程与 ssh 复用系统客户端两种执行;逐行解析带 @@kanzei 前缀的单行 JSON(stage/metric/progress/artifact/checkpoint/message/heartbeat/result),其余输出原样保留为终端日志;运行器另记 run_started/run_finished/run_failed/run_cancelled/environment_captured;结果事实进 state.db 并回写探索文件的实验结果表,产物落 explorations/<E-id>/<result-id>/;记录 params_text、code_ref、policy/lease_id/max_duration/cleanup、callback_stats。
- 发现记录: {"Intent":"让实验真的能在本机和显卡服务器上跑起来并回传结构化实时事件","Explicit":"第一版只做 local 与 ssh;协议是 @@kanzei 前缀单行 JSON;不依赖第三方实验平台","Assumptions":"远端只需能跑命令并把带前缀的行打到 stdout;系统 ssh 客户端可用","Ambiguities":"远端工作目录与代码同步方式、并发 run 上限,本条按人工准备 workdir 与串行一次一个处理","领域对象":"Run、execution(local/ssh)、callback 事件、产物目录、state.db 运行事实、终端日志","最小成功闭环":"一条真实 SSH 实验跑完:阶段/指标/进度/产物事件被解析,run 事实与产物可回读","延后决策":"作业调度器提交、并发队列、数据集同步、语言级便利封装库"}
- 复杂度: 大
- 来源: 用户选定第一版执行方式为「本机 + SSH 远程服务器」;协议方向见 A-014 用户确认「确认采用」。
- 标签: 后端
- 边界: 运行器是专用工具通道,不给 research 档开 bash(research_mode.md 定调点 6 的硬 deny 不变);不自动同步代码与数据集(远端第一次人工准备,准备步骤记进环境登记项后复用);不做实验队列与并行调度;不做 Slurm/K8s 提交、容器编排、跨机分布式训练编排;不做 W&B/TensorBoard 对接;不把回调写进 Markdown。
- 验收: ①本机与 SSH 两条路径都能跑完一次真实实验并留下结果事实、产物与回写的结果表行;②坏 JSON、超长行、未知事件不终止运行且计入 callback_stats;③取消与远端强杀都能收敛到终态,heartbeat 超时判定卡死;④断线重连后视图从持久事实恢复,不显示假运行中;⑤research 档 bash 仍为硬 deny,运行器不成为绕过通道;⑥新环境缺准备步骤时运行器询问并记录,不自行同步文件。
- refs: R-343 R-221
- 优先级: P1
- 批次: 4/4
- 进展: 批次 4/4 已完成并提交 `00ae196f`：`crates/kanzei-tools/src/research_runner.rs:102-112,157-206` 新增 cancel 独立并发槽、pid 读取与 process tree kill，`209-490` 收尾优先保留 cancelled、heartbeat_timeout→stuck、run_finished/run_failed/run_cancelled 事实；`492-535` 回写探索 Markdown 的实验结果表；`583-645` 继续复用 core callback parser 并更新 callback_stats；`profiles/research.rs:38-41,133-138,162-166` 注册专用 runner、允许 run/cancel/get 且 bash 硬 deny。证据 `T-1786922726860`（tools 518 passed/1 ignored、app 250 passed）与 `T-1786922726861`（runner 5 passed）：①本机结果事实/产物/结果表已验证；SSH 真实服务器链路因缺用户提供的目标/凭据/人工准备目录，验收降级并登记外部阻塞；②坏 JSON/超长行/未知事件沿用 core parser 与 B3 测试，callback_stats 写入 `583-645`；③cancel/进程树 kill `157-206`、heartbeat stuck `343-410` 已验证；④get 从 state.db 回读 `137-155`，真实断线重连恢复由 R-347 承接，当前验收降级；⑤research bash 硬 deny `profiles/research.rs:133-138`，runner 专用权限 `162-166` 已回归；⑥环境快照 `229-303` 已记录，但缺环境准备步骤询问/记录由 R-345 承接，当前验收降级。后续依赖 R-345/R-347 完成后恢复本条收口。
- observed_head: 00ae196ffdb6aa47a46f67ba7464d637a771b505
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1788271993948
- 状态: doing
- 依赖: R-345 R-347
- 阻塞: 依赖 R-345/R-347 已 done 归档,依赖侧已解除;剩余验收①SSH 真实服务器端到端需用户提供可连接的 SSH 目标、账号/凭据与人工准备目录(本机跑通部分已验证),或接受①SSH 侧降级后由 agent 收口;解除人:用户;解除条件:用户

## R-353 改动面账本与交付态推导:让条目、改动、证据成为可机械关联的事实 [doing]
- 内容: 按 docs/design/tracker_evidence_ledger.md §3.1/§3.2 建立地基:在 .kanzei/artifacts/work-log.jsonl 新增 deliver 事件(条目 id、commit、paths、test_record_ids、时间戳、来源),写入点设在 git finalize 提交通道——该处已在机器写 passed 测试记录,同一时刻引擎已知 WIP 持有者、暂存文件集与提交 sha。据此推导交付态 unstarted/uncommitted/committed/verified,作为纯派生量供门禁与调度消费,不落 Markdown、不新增条目字段。
- 发现记录: {"Intent":"让门禁与调度能问出本条目的改动面,而不是拿全局代理量猜条目与改动的关系","Explicit":"账本由引擎在提交通道写;交付态是纯派生量不落 Markdown;意图态仍归模型;存量走遗留模式","Assumptions":"git finalize 处已知 WIP 持有者与暂存集;work-log.jsonl 已是引擎写的 append-only 事实流","Ambiguities":"主树手工 commit 如何补账,本条按遗留模式只提示不拦处理,不引入模型可写的补记通道","领域对象":"deliver 事件、改动面 paths、交付态、意图态、work-log、提交通道","最小成功闭环":"一条需求从取活到提交,账本自动留痕,交付态推导正确,遗留条目不被误拦","延后决策":"归档面对账、跨线路账本合并、账本的压缩与轮转"}
- 复杂度: 大
- 来源: 用户就「交付态由谁产」定调选择「引擎在提交通道自动记账本」,并接受「所有提交须走 kz 提交通道、主树手工 commit 缺账落遗留模式」的代价;根因见用户原话「Tracker 已无法可靠表达当前真实状态」。
- 标签: 核心
- 边界: 不改意图态(todo/doing/done)的语义与写法,模型继续声明意图;交付态引擎独产,模型不得写;不回改存量条目,账本零行者走遗留模式仅提示不拦;不为防敌对模型设计,账本不可伪造仅因模型不经手提交通道;主树手工 commit 缺账属已接受代价。
- 验收: ①一次经 kz 提交通道的真实交付在 work-log 留下 deliver 行,paths 与该次提交暂存集一致;②交付态可由账本推导且四种取值各有真实实例;③账本零行的存量条目落入遗留模式,判定只作提示不产生拒绝;④交付态不出现在任何 Markdown 条目字段里;⑤模型侧工具无法写入 deliver 事件。
- refs: R-349 D-736
- 优先级: P0
- 进展: 批次: 1/2；B1 已提交 `2337474b`。①已完成：`git finalize` 在真实提交后写入 deliver（条目 id、commit、commit 实际 paths、test_record_ids、source），`work/log.rs` 已能按同 run/line 找到最近 claim；测试与门禁证据为 T-1786922726932、`cargo check --workspace --all-targets`、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`。既有能力（本次未新增）：`work/reconcile.rs` 已有四类分类框架，但当前仍主要读 tracker observed_head/test 证据，尚未消费 deliver 账本。B2 下一步：让 reconciliation 按 deliver paths/commit/test_record_ids 推导四态，补账本零行遗留提示不拦、Markdown 无交付态字段和模型不能写 deliver 的回归。
- observed_head: 2337474bb3467c028d6929fd3ef373c163b5dc05
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1788387719951
- 停车: 排队:原停车前提 D-737 已 fixed 归档;按 defect-first 排在 D-662 之后恢复 B2(reconciliation 消费 deliver 账本推导四态);恢复人:agent;解除条件:D-662

## R-357 活动面按裁决可达性划分:停车超期移出、tests.md 废止、缺口由 work gaps 回答 [doing]
- 内容: 按设计 §3.7 重划活动面与归档面:停车超过 14 天且解除条件未变的条目移入 parked 面,裁决不再遍历但 work reconcile 与前端仍可见;废止 tests.md 作为 Markdown 面(它只有 12 字节、零记录,唯一可能的居民 running 是瞬态,不该占治理真源),running 迁 state.db;新增 kz work gaps 回答「还需要跑什么」,输入为账本改动面加该条目 passed 记录的指纹覆盖与关闭门禁判据,每条缺口一行并附一条可直接复制的命令,给不出可执行命令的缺口不出行。
- 发现记录: {"Intent":"让活动面重新回答得了「还剩什么没做、还需要跑什么」这两个问题","Explicit":"按裁决可达性划面而非按是否终态;tests.md 废止;缺口由命令回答","Assumptions":"parked 面可被前端与 reconcile 消费;running 迁 state.db 不影响回放","Ambiguities":"14 天阈值与解除条件未变的判定方式,本条按字段文本未变处理","领域对象":"活动面、parked 面、tests.md、running 记录、work gaps、缺口","最小成功闭环":"裁决只遍历真正可达的条目,work gaps 对一条待关闭条目给出可复制命令","延后决策":"parked 面的复活策略、归档面对账、阈值可配置化"}
- 复杂度: 中
- 来源: 用户原话「tests.md 只有一个 #,全部 1600+ 测试记录进入 archive,当前活动测试面无法直接表达还需要跑什么」;实测活动面 29 条里 23 条处于停车,有效不足 6 条却每轮全遍历。
- 标签: 流程
- 边界: 不回改归档条目;不改 Markdown 作为治理真源的地位;停车条目只移面不改内容;work gaps 只读不写。
- 验收: ①停车超期条目不再进入取活遍历,但在 work reconcile 与前端可见;②tests.md 不再作为治理面存在且 running 记录可从 state.db 读回;③work gaps 对每条活动条目给出缺口与可执行命令;④缺口行为零时该条目确实可关闭。
- refs: R-353 R-355
- 优先级: P2
- 批次: 0/4
- 进展: 批次: 0/4；已确认现状：调度/取活入口在 `crates/kanzei-tools/src/work.rs` 与 `tracker/scheduling.rs`，显式停车目前仍作为 `parked_items` 输出但没有超期面判定；test_record 的 running/终态写入与 coverage 查询仍以 `tests.md`/`tests-archive.md` 为真源；CLI 只有 `work next/claim/...`，没有 gaps。B1 下一步：增加停车超期判定并确保 reconcile/前端仍读取完整可见条目。
- observed_head: f321ee34118a9cbfbd95dfa90a02a1862cc718b6
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1788393813821
- 停车: 排队:原停车前提 R-359 已 done 归档并提交(0ac5755c);排在 R-353 之后恢复 B1;恢复人:agent;解除条件:R-353

## R-358 批次与 Work Unit 收敛为一套:分子停手写,验收条款绑结构化证据 [doing]
- 内容: 按设计 §3.8 废除 Work Unit 的独立机制、保留批次,并把 Work Unit 唯一不可替代的能力搬上来:①批次分子由 git 推导,add/update 接受 批次: /N 形式,存量 k/N 继续解析但 k 被忽略,删掉那道分子对账门(引擎已能推导且推导值已优先,只是对模型不可见,导致模型必然在 close 那一刻才知道对不对得上);②把 acceptance 与 evidence 的机械绑定下沉到条目级,从「进展文本包含条款号」升级为「每条验收条款一行结构化 evidence(file:line 或 T- 记录 id)」;③删除 执行模型: work_units_v1 开关。
- 发现记录: {"Intent":"把两套并行的子任务机制并成一套,并消除模型手写分子这个必然漂移源","Explicit":"废 Work Unit 独立机制、保留批次、分子由 git 推导、验收条款绑结构化证据、删开关","Assumptions":"git_batches 的推导已可用且推导值已优先;条目级已有半成品的条款覆盖检查","Ambiguities":"结构化 evidence 的最小格式,本条按 file:line 或 T- 记录 id 两种处理","领域对象":"批次、Work Unit、批次分子、验收条款、evidence、work_units_v1 开关","最小成功闭环":"一条多批次需求在不手写分子的情况下走完并关闭,未覆盖条款被逐条点名","延后决策":"历史 work_events 的处置、跨批次证据继承、分母的自动建议"}
- 复杂度: 中
- 来源: 勘察发现两套并行账本 close 时两道门同时跑而中间零一致性校验(设计文档明写这是有意的);Work Unit 全库只用过一条而批次遍地,且「Git 是批次真源」对 work_units_v1 因命名空间不认 W 而静默失效。
- 标签: 流程
- 边界: 分母(总批数)git 推不出来,继续手写,不属冗余;不回改存量条目的 k/N 写法;不迁移 state.db 里既有 work_events 记录;删开关不等于删历史数据。
- 验收: ①批次分子不再由模型手写,且 R-283 R-101 R-312 R-249 这四条分子与实际不符的存量条目不再触发对账拒绝;②每条验收条款可绑定结构化 evidence,关闭时逐条点名未覆盖项;③work_units_v1 开关移除后不再存在两道互不校验的并行门;④批次推导认得 W 命名空间,不再对 work_units_v1 静默失效。
- refs: R-353
- 优先级: P2
- 批次: 0/4
- 进展: 批次: 0/4；已确认设计 §3.8 的目标是保留批次但让 git 推导分子、条目级 evidence 逐条绑定、移除 `执行模型: work_units_v1` 分流；现有 `git_batches` 已可推导 B/S/批次，但需核对 W 命名空间，`actions` close 仍检查批次和验收文本，work_units_v1 仍由 `work.rs`/`work` 工具消费。B1 下一步：先收敛批次分子写入/校验与 git marker 解析，不改存量条目。
- observed_head: f321ee34118a9cbfbd95dfa90a02a1862cc718b6
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1788393904818
- 停车: 排队:原停车前提 R-359 已 done 归档并提交(0ac5755c);排在 R-357 之后恢复 B1;恢复人:agent;解除条件:R-357

## R-361 记忆收益闭环与任务信息呈现改进 [doing]
- 内容: 分三批：观测真源与证据判据；决策点召回与固定任务收益对照；有界任务上下文与结果呈现。
- 发现记录: {"Intent":"让记忆改善任务表现并可理解呈现","Explicit":"统一召回真源，厘清注入读取采用验证，收紧晋升与效果判据，改善当前任务呈现","Assumptions":"复用 SQLite 与现有记忆生命周期","Ambiguities":"无阻碍第一批实施的未决项","领域对象":"recall_event、memory_entry、episode、work_unit","最小成功闭环":"同一运行召回读取与恢复证据可对应且不夸大收益","延后决策":"真实 provider 对照结果与长期调度策略"}
- 复杂度: 大
- 批次: 3/3
- 来源: 2026-09-06 用户原话「同意，开工吧」，确认上一轮审计建议
- 标签: 前端
- 进展: B3 已提交：8057f2cc R-361 B3 完善有界任务上下文与结果呈现；需求证据更新已提交 6e9969ea。工具层 `structured_control_output` 在 crates/kanzei-tools/src/work/output.rs:45-108 派生有界 `context_capsule`，phase scout/review 在 crates/kanzei-app/src/phase_pipeline.rs:606-670 消费 checkpoint/决策/来源/验证，docs snapshot 在 crates/kanzei-app/src/docs.rs:404-418 提供 Work Projection，UI 在 crates/kanzei-app/ui/11-docs-list.js:729-800 展示实质进展、决策记录、记忆来源、验证结果；D-747 已 fixed（02-i18n.js:92-98）。T-1786922726986 六项前端冒烟与卡片 DOM 断言通过；T-1786922726988 为 rustfmt 后当前源码的 kanzei-tools 545、kanzei-app 261 定向回归通过。验收①～⑤已有 B1/B2/B3 代码、测试与当前提交证据；验收⑥「实际收益独立对照留证」仍开放，不能用单测/replay 底座冒充真实 provider 结果。当前阻塞为真实 provider、固定任务与执行窗口，解除后下一步运行 Current/NoMemory 配对并写入成功率、重试、重复探索、用户纠正、耗时证据。
- 验收: 新旧库迁移可回归；读取按运行隔离；正常结束不冒充收益；无匹配恢复证据不得自动晋升；前端区分注入读取与未知；实际收益独立对照留证。
- 优先级: P1
- observed_head: 8057f2cc91fa44016bfc38b17ff813e8f60a9845
- observed_worktree_hash: fnv1a64:d1d568a4ca377cc2
- recorded_at: 1788803004691
- 阻塞: 用户：提供一个可重复的真实 provider 运行入口（已配置模型/凭据或运行中的 provider）、固定任务与允许执行窗口；解除条件:用户

## R-364 工具延迟加载:常驻层约 20 个工具,其余经 tool_search 按需加载 [doing]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.1/§5.2;实施地图(行号、批次、陷阱、裁决)见 docs/design/cc_codex_alignment_impl_maps.md §1。B1 逐工具 schema 字符账单并定稿常驻名单;B2 tool_search 与通用追加路径、预算门禁拆成常驻面与延迟目录两个数、denial_hint 指向 tool_search 与提示词改写(与 B4 同版发布);B3 Anthropic/Responses 原生延迟加载(先研究后编码);B4 同源测试与硬拒覆盖扩到延迟层。
- 发现记录: {"Intent":"降低每步工具 schema 注入成本","Explicit":"A 档全收;常驻约 20 个,其余按需加载","Assumptions":"低频工具靠名称目录与 denial_hint 仍可被发现","Ambiguities":"原生 defer_loading 的字段名与载体待探针核对","领域对象":"常驻层、延迟目录、tool_search、已加载集","最小成功闭环":"首个请求只含常驻层,select 加载后下一步可调用","延后决策":"研究档是否分层;已加载集跨重启持久化"}
- 复杂度: 大
- 批次: 1/4
- 来源: 2026-09-25 用户审阅 CC/Codex 三方对照,A 档回答「同意」并要求「直接登记就行」;R-312 B1 实测工具 schema 占每步系统注入 48.6%
- 标签: 核心
- 边界: 不做 MCP/skills/hooks;不改 Part::ToolResult 与 LlmRequest 结构;research/readonly/子代理不分层;已加载集不跨重启持久化
- 验收: ①dev 档主代理首个请求的 tools 只含常驻层(CLI 20、桌面 21),延迟工具以「名称 — 一句话」进 system,账单有 tools/catalog;②tool_search 支持 select 精确加载与关键词检索,加载后下一步起可调用,压缩与溢出恢复后仍可用;③直接调用未加载的延迟工具时自动加载并执行;④预算门禁拆为常驻面与延迟目录两个数,research/readonly/子代理工具面不变;⑤对延迟托管工具的硬拒提示指向 tool_search select;⑥Anthropic/Responses 原生延迟加载经探针核对后启用且加载不改变 tools 前缀,其余协议走追加路径并在账单可见
- refs: D-662 R-312 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P1
- 进展: B1 已提交 811497b8（批次 1/4），账单/测试见前述进展。B2 只读复核 §5.2 与实施地图：tool_search 未实现，现有 DEV_TOOL_BUDGET=30 只有单一工具数；地图裁决包括直接调用延迟工具自动加载、deny tool_search 回退全常驻、已加载集合从 prior 播种。地图同时指出 resident/catalog 双预算计入目录文本、搜索结果 schema、task_spec 与 tool_search 的公式未定。此处为核心门禁语义，已用 question 询问用户 A/B/自定义，当前 pending；没有改 B2 代码，收到答复后再设计冻结。
- observed_head: 811497b8ac6db8734d9ba9060e79af178af353f0
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1790356949944
- 确认记录: 2026-09-25 B2 双预算计费语义已用 question 工具询问，当前 pending。问题：resident budget 是否统计本次请求全部完整 ToolSpec（初始常驻含 tool_search/task_spec + 已加载延迟项），catalog budget 是否只统计未加载延迟项的名称+一句话并在加载后移除；tool_search 返回 schema 是否仅走通用上下文预算。选项 A 为按实际集合计费并加载后移出目录；B 为始终保留全部目录项、加载后双计；另可自定义。未收到答复前不实现此门禁。
- 阻塞: 
- 对账: 2026-09-26 波次审计答复 B2 计费口径(无需用户确认,实施地图已规定):按 docs/design/cc_codex_alignment_impl_maps.md §1 B2 第 9 条与设计 §5.1——预算门禁数的是工具个数不是字符:常驻面预算 = resident_tools 名称数 + 手工补上的 task(dev 20 / 桌面 21,tool_search 计入常驻);延迟目录预算 = deferred_tools 个数(dev 12 / 桌面 19)。已加载工具的 schema 字符与 catalog 文本只进上下文账单(tools/catalog、tools/loaded:<name>),不计入预算门禁;core 私有的 task_spec 在账单表注明「未计入」。据此继续 B2。

## R-365 网页搜索与抓取升级:模型自带搜索、webfetch 按问题提取与翻页查找、websearch 批量与过滤 [todo]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.5;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §2。B1 订阅通道探针(codex 托管 web_search、codex /alpha/search、claude 服务端搜索、回放与边界行为);B2a 协议层托管搜索的声明、解析、原样回放与计费(Part::Hosted 带通道标识);B2b runner、[web] 配置与桌面渲染(pause_turn 续跑、编号来源列表);B3 webfetch 重做(url|ref、带行号 markdown、落盘、15 分钟缓存、from_line/find/links、跨域重定向不跟、PDF 落盘、web_extract 按问题提取);B4 websearch 批量化与 recency/domains、codex 搜索后端失效自动退 DuckDuckGo、research 档 add_finding 存证门禁。
- 发现记录: {"Intent":"提高联网检索质量与抓取的上下文效率","Explicit":"结合 CC 的按问题提取与域名过滤、Codex 的批量查询/翻页/查找/引用编号;搜索源用模型自带搜索,探针失败退 Codex 搜索接口,最后 DuckDuckGo 兜底","Assumptions":"订阅后端对托管搜索的接受度以探针为准","Ambiguities":"codex 订阅后端是否接受 gpt-5.6 的托管 web_search","领域对象":"托管搜索条目、引用来源、web ref、web artifact、提取模型","最小成功闭环":"codex 通道主对话可搜索并带来源,webfetch 可按问题提取","延后决策":"L0 清理对托管条目的裁剪;设置页 [web] 表单"}
- 复杂度: 大
- 批次: 0/5
- 来源: 用户 2026-09-25 原话「网页搜索这个很重要,我建议结合两者优点,我的体感是会严重影响工作质量,所以这个我很看重」;搜索源选「模型自带搜索」,探针失败退路选「复用 Codex 搜索接口」
- 标签: 核心
- 边界: 不做 web.run 的 click/screenshot/finance 等命令;不声明 Anthropic web_fetch 服务端工具;不给 DeepSeek/本地模型原生搜索;不改 html_to_text 与 fetch_bytes 共享函数
- 验收: ①B1 的探针结论与脱敏样例写入设计文档,通道开关可追溯到探针证据;②探针通过的订阅通道上主对话可用模型自带搜索,条目持久化后下一轮原样回放不报错,异通道条目不进请求体;③webfetch 支持 url|ref、行号翻页、页内查找、链接清单与按问题提取(失败回落原文分页),同 URL 15 分钟内不重复联网,预览路径格式不变;④websearch 支持 1-4 个查询与时间/域名过滤,结果带 ref,codex 后端失效时自动退回 DuckDuckGo 并注明;⑤research 档默认不开原生搜索,引用未经 webfetch 存证的来源在 add_finding 被拒;⑥桌面显示搜索工具块与编号来源列表,重开会话后一致
- refs: R-023 R-217 R-248 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P1

## R-366 回退:每条用户消息一个检查点,可选对话+代码/只回退对话/只回退代码 [doing]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.9;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §3。B1 检查点存储(编辑前像按内容寻址存 .kanzei/artifacts/checkpoints,索引 file_checkpoints 表);B2 conversation.rewind 事件与历史重建(桌面、CLI、shadow 同口径);B3 代码还原、外部改动默认跳过、覆盖前留证、预览不会还原的内容;B4 桌面 UI(悬停回退按钮、三选项、预览确认、回填输入框、历史列表可查看已回退段)。
- 发现记录: {"Intent":"像 CC 一样可回退到任一用户消息","Explicit":"默认对话+代码,可选只对话或只代码,外部改动默认跳过","Assumptions":"只还原编辑工具改过的文件,与 CC 行为契约一致","Ambiguities":"无阻塞项","领域对象":"文件检查点、前像 blob、隐藏区间、回退点","最小成功闭环":"一轮 edit 后回退,文件与对话同时回到该消息之前","延后决策":"检查点 blob 清理策略;冲突文件 diff 展示"}
- 复杂度: 大
- 批次: 1/4
- 来源: 用户 2026-09-25 原话「回退我很常用很重要」,回退范围选「对话+代码」
- 标签: 核心
- 边界: 不做模型回退工具与 CLI 回退命令;不还原 bash、git、追踪文档、记忆与附件;检查点 blob 的清理随 R-245 配额机制处理
- 验收: ①edit/write/insert 首次触碰文件时保存前像,新建文件记为原本不存在,无 run_id 的调用无副作用;②回退对话后桌面、下一轮 prior 与 kz run 都看不到被回退段,历史列表可只读打开回退前原貌;③回退代码恢复前像并删除检查点后新建的文件,外部改动过的文件默认跳过并列出,确认后才覆盖且覆盖前留证;④预览列出将恢复与删除的文件,以及不会还原的 bash、提交、追踪文档与记忆写入;⑤运行中或有排队输入时拒绝回退;⑥不新增模型工具
- refs: R-242 R-245 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P1
- 进展: B1 已提交于 ef114f4c（atomic_file.rs、store/{mod.rs,schema.rs,file_checkpoints.rs}、tools/{edit.rs,write.rs}）：原子字节写、schema v24/file_checkpoints 表、每 run/path 首次前像、SHA256 blob、new-file 不存在哨兵、后像哈希；Write/Edit/Insert 接线。无 run_id/空 project_root 跳过检查点和 write-log副作用；带身份日志 path/run/process 保留。D-757 已 fixed。证据：T-1786922727053 tools fmt+573 passed/1 ignored+Clippy，T-1786922727054 base 23 passed，T-1786922727055 core 298 passed，T-1786922727056 verify.ps1 当前提交全绿并绑定 ef114f4c；全项在 commit ef114f4c。B2 冻结方案已向用户确认，等待批准/调整，未写 B2 代码。
- observed_head: ef114f4c2aa733d6ef5454ef240c5b9c6cd17513
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1790354370498
- 确认记录: 2026-09-26 波次审计更正:用户未对单个批次作答;09-25 23:17 B1 冻结方案发出后只收到自动续跑提示「继续推进,规则按系统提示执行。」,不构成授权,也不存在「仅授权 B1」的限定(见 D-764)。R-366 整体方案经用户 2026-09-25「直接登记就行」批准,B2-B4 按 docs/design/cc_codex_alignment_impl_maps.md §3 实施,无需逐批授权;09-26 00:38 的 question(seq 12791)作废。
- 阻塞: 
- 停车: 排队:R-245 收尾后按实施地图 §3 继续 B2;B1 记录口径偏离见 D-762(已在发版分支修复,待核验);解除条件:R-245

## R-367 先读后写:edit/write/insert 对未读或读后被改的文件返回纠错码 [todo]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.6;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §4。B1 ReadLedger 与 ToolCtx 接线(含与 content_hash 格式一致的流式 hash);B2 read 记账与三个写工具的门禁;B3 桌面与 CLI 生产接线,子代理使用独立账本。
- 发现记录: {"Intent":"防止按过期内容改文件","Explicit":"复刻 CC 的先读后写契约","Assumptions":"hash 一致加锚点精确匹配足以判定新鲜","Ambiguities":"无阻塞项","领域对象":"读取账本、内容 hash","最小成功闭环":"未读先改被拦,读后放行,外部改动后再改被拦","延后决策":"账本跨重启持久化"}
- 复杂度: 中
- 批次: 0/3
- 来源: 用户 2026-09-25 对 A 档回答「同意」;并行线、codex 共用主工作树与自举并发下,读与写之间文件被改是真实风险
- 标签: 后端
- 边界: 不改 bash;账本不持久化;不改模糊回退;不新增工具
- 验收: ①带账本时对已存在文件未 read 就写返回 READ_BEFORE_WRITE 且文件不变;②部分读取也算读过;③读后被外部改动时 edit/insert 返回 FILE_CHANGED_SINCE_READ 并附目标附近实际内容,write 必须重新 read;④新建文件不受限,写后账本刷新,连续 edit 无需重读;⑤子代理读取不计入主代理账本,新对话清空账本;⑥无账本的 ctx 行为与现状逐字节一致
- refs: D-395 R-268 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P1

## R-368 文档引用标记、引用历史与引用图:统一抽取、写入校验、git 推导历史、前端侧栏与邻域图 [todo]
- 内容: 设计见 docs/design/doc_reference_graph.md。B1 refgraph 抽取内核与统一分词(节点模型、强/弱引用、关系类型前缀、区间展开、脏 token 上报、设计文档身份与改名别名);B2 tracker refs 写入校验(只校验本次传入)与 update 删除显式回显、kz refs check/show、req/defect get 附被引用、architecture 工具 refs 动作、引用约定写入 conventions 与设计模板;B3 git 推导引用历史(归档搬运折叠、改号、改名、未提交标记,缓存于 .kanzei/artifacts/refgraph);B4 前端统一跳转(A/M/T 与设计文档路径、Markdown 编号链接)、引用侧栏与历史时间线;B5 邻域关系图与体检面板(与 R-307 B3 共用渲染器)。
- 发现记录: {"Intent":"让文档之间的引用可标记、可查询、可追溯、可视化管理","Explicit":"引用标记约定、引用历史、前端展示与管理","Assumptions":"Markdown 为唯一真源,引用历史由 git 推导","Ambiguities":"关系类型词表(依据/实现/取代/来源)待用户审阅","领域对象":"文档节点、引用边、关系类型、引用事件、体检问题","最小成功闭环":"打开一份设计文档能看到被哪些条目引用、何时建立,并能跳转","延后决策":"新增违规是否接入 verify;项目根外研究工作区是否纳入"}
- 复杂度: 大
- 批次: 0/5
- 来源: 用户 2026-09-25 原话「我还需要你登记一个功能就是关于kanzei内部注册的文档的引用历史,因为我们不是有设计文档,需求会参考对吧,我想弄一套引用的标记和管理,然后前端做出来显示方便我们管理文档之间的引用关系」
- 标签: 核心
- 边界: 不另建链接库作为真源;不新增工具;不引入图库;不覆盖项目根之外的研究工作区;存量违规只报告不阻断
- 验收: ①抽取覆盖 refs、依赖、设计文档关联头、测试关联、记忆 refs/supersedes 与行内弱引用,每条边带锚点行号;②tracker 写入 refs 时不存在的 ID 或路径返回纠错并列出 token,存量不阻断;③kz refs check 报告悬空引用、设计与决策单向、缺关联头、改名残留路径、从未存在的 T 与脏 token;④任一文档可查看出入边与引用历史,事件区分建立、删除、搬运、改号并带提交号;⑤前端可从任意编号或设计文档路径跳转,邻域图与依赖图共用渲染器;⑥不新增 agent 工具,索引与历史缓存可删可重建
- refs: R-307 R-353 R-358 docs/design/doc_reference_graph.md
- 优先级: P2

## R-369 子代理补齐:自定义 agent 进 task、后台运行与完成通知、worktree 可写、续聊 [todo]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.3;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §5。B1 自定义 agent(description)进 task 名册,task 支持 background:true,完成时以 task-notification 注入主对话;B2 isolation:"worktree" 可写子代理(引擎快照提交,主 agent 经 git merge_task squash 合并并走常规 commit 门禁);B3 resume 续聊(按 task_id 恢复 transcript,worktree 任务在原树续)。
- 发现记录: {"Intent":"让委派更灵活并支持隔离写入","Explicit":"后台、worktree 可写、自定义 agent、续聊四项","Assumptions":"隔离 worktree 是结果侧防线,合并是决策点","Ambiguities":"运行中任务的 resume 在 v1 返回稳定 code,待运行中插话落地后再改","领域对象":"task 名册、后台通知、任务 worktree、transcript、task_id","最小成功闭环":"派出后台可写 task,完成通知到达后主 agent 合并其分支","延后决策":"运行中续聊;任务 worktree 保留策略可配置"}
- 复杂度: 大
- 批次: 0/3
- 来源: 用户 2026-09-25 在筛选问卷中勾选「后台运行,worktree 可写,自定义 agent,续聊」
- 标签: 核心
- 边界: 不新增工具(只在 task 与 git 上加参数和动作);不做运行中子代理插话;不给编排角色开后台或可写;重启后不自动恢复后台子代理执行
- 验收: ①.kanzei/agents/*.md 中 mode: subagent 的 agent 出现在 task 枚举,人格缺省模型生效;②background:true 只后台化该次调用,完成通知进入主对话(运行中排队、空闲开新一轮),可单条停止;③isolation:"worktree" 时主树零改动,子代理内 git 写动作与 .kanzei 托管写入被硬拒,合并经 merge_task 与常规 commit 门禁;④resume 可续已结束的任务且跨重启可用,未知或运行中的 id 返回稳定 code;⑤子代理仍不能嵌套,CLI 与 readonly 档的 schema 不出现 background/isolation
- refs: R-175 R-176 R-281 R-327 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P2

## R-370 定时任务:一个概念一个面板,触发器到固定流程到可选回写,触发位置可选 app/system/server [todo]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.10;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §6。B1 定义格式(.kanzei/schedules/*.md,人话频率)、应用内调度器、独立会话执行;B2 回写通道(notify、memory_inbox、file、idea);B3 任务面板与表单;B4 错过补跑、历史、连续失败自动停用;B5 host=system(kz schedule run 与 Windows 任务计划程序);B6 host=server(登记服务器上的 cron、SSH 拉回、本地补做回写)。
- 发现记录: {"Intent":"用简单直观的定时任务承接触发器与固定流程","Explicit":"一个面板、人话频率、回写可选、三种触发位置","Assumptions":"server 档指托管到已登记的自有服务器","Ambiguities":"server 档的理解待用户审阅设计文档时确认","领域对象":"定时任务定义、运行记录、回写通道、触发位置","最小成功闭环":"每 5 分钟的任务在应用内到点运行并回写文件与通知","延后决策":"事件触发;全局任务归属;远端完整对话拉回"}
- 复杂度: 大
- 批次: 0/6
- 来源: 用户 2026-09-25 原话「比起MCP我更想定义触发器和固定pipline的回写」「倾向于定时任务那样子,但是我认为CC的定时任务偏复杂不好用,并且前端呈现不直观」「这个做成可选比较合适吧」「提供几个选项,一直开着APP的时候触发,关闭触发,服务器自定义托管触发」
- 标签: 核心
- 边界: 不给模型定时任务工具;不做事件触发(留 v2);v1 只做项目级任务;不引入 YAML/cron crate
- 验收: ①§5.10 示例定义能被识别,解析失败带行号显示在面板;②每次运行是独立会话,主对话零污染,需要询问的动作被拒并列出,运行继续到结束;③四种回写可多选且逐项记录成败;④面板用频率选择器,不出现 cron,开发与研究空间都可见;⑤catch_up 行为正确,新建或刚启用的任务不补跑;⑥host=system 在应用关闭时照常运行,host=server 在远端运行并拉回,回写只做一次
- refs: A-016 A-018 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P2

## R-371 运行中插话:steer 在工具边界注入当前 run,界面标记已送达 [todo]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.8;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §7。B1 core 注入点、SteerCommitted 事件与 SteerMessageCommitted typed 事实;B2 桌面 inbox 取件、落库、kz:steer-delivered 事件;B3 界面已送达标记。RunnerConfig 新字段与手动压缩、缓存测量两条共享,最先落地的条目一次性加齐三个字段。
- 发现记录: {"Intent":"插话不必等整个 run 结束","Explicit":"steer 在工具边界注入,UI 标记已送达","Assumptions":"插话作为独立 typed 事实持久化","Ambiguities":"无阻塞项","领域对象":"steer 输入、插话事实、送达事件","最小成功闭环":"运行中插一句,下一步请求里模型就看到","延后决策":"手机端插话;子代理插话"}
- 复杂度: 大
- 批次: 0/3
- 来源: 用户 2026-09-25 对 A 档回答「同意」;用户「好用」五条标准中的「快和流畅」「少打断」
- 标签: 核心
- 边界: 不改 queue 语义与 run 边界循环;不做手机端插话;不给子代理转发插话
- 验收: ①run 进行中发送的 steer 在下一次 provider 请求前以一条 user 消息进入上下文,同一 run 内模型可见;②queue 仍在 run 边界处理,最后一步才到达的 steer 留到 run 边界开新 run;③插话被持久化,重启后对话与下一轮 prior 中仍在;④送达后输入状态为 completed,之后停止不改写;⑤界面气泡显示已送达,排队条同步消失;⑥子代理与 CLI 不消费 steer
- refs: R-241 R-242 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P2

## R-372 手动压缩:桌面 /compact [焦点] 与 kz compact,纪要不再依赖子代理运行时 [todo]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.8;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §8。B1 core 的 DigestModel 与子代理解耦并支持焦点(焦点写进 system 段);B2 桌面与 CLI 装配 DigestModel,轮末压缩改用它;B3 入口:桌面输入框 /compact 与 kz compact 子命令。
- 发现记录: {"Intent":"可手动压缩并指定保留重点","Explicit":"桌面 /compact 与 kz compact,纪要不依赖子代理","Assumptions":"沿用现有 compaction 事务与投影","Ambiguities":"无阻塞项","领域对象":"DigestModel、焦点、compaction 事务","最小成功闭环":"输入 /compact 某主题后下一轮 prior 为纪要加近期原文","延后决策":"CLI 压缩并行线会话"}
- 复杂度: 中
- 批次: 0/3
- 来源: 用户 2026-09-25 对 A 档回答「同意」;CC 与 Codex 都提供手动压缩并可指定焦点
- 标签: 后端
- 边界: 不动 summarize_chat;不改 L0 prune 与应急压缩;不改自动压缩触发线
- 验收: ①桌面输入 /compact [焦点] 与 kz compact [--focus 文本] 都能手动触发压缩,焦点进入 L1 纪要指令;②压缩结果以 compaction 事务持久化,下一轮 prior 与重启后都是压缩后的 surface;③关闭子代理时轮内、轮末与手动压缩都能产出纪要;④运行中手动压缩被拒,压缩进行中的新发送被拒;⑤无可压缩内容时给出说明且不写事务
- refs: R-236 D-181 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P2

## R-373 缓存测量:按步落盘 cache_read/cache_write,Responses 带 prompt_cache_key,看数据决定是否挪动每步刷新段 [todo]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.8;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §9。B1a 每步用量进 RunSummary 并落 episodes.metrics_json(不加列、不升 schema);B1b Responses 请求带 prompt_cache_key=会话 id,codex 路由 session_id 头稳定化并真跑验证;B2(有条件)仅当数据显示刷新段变化明显拉低命中率时,把每步刷新段从 system 末尾挪到临时尾消息并同步调整 Anthropic 第二断点。
- 发现记录: {"Intent":"量化并改善提示词缓存命中","Explicit":"先测量,再决定是否挪动每步刷新段","Assumptions":"metrics_json 为自由 JSON,加键零迁移","Ambiguities":"codex 订阅后端是否接受 prompt_cache_key 待真跑","领域对象":"step_usage、cache_read、cache_write、prompt_cache_key","最小成功闭环":"真跑一轮后 episodes 可查到逐步缓存数据","延后决策":"挪动注入位置;命中率看板"}
- 复杂度: 中
- 批次: 0/3
- 来源: 用户 2026-09-25 对 A 档回答「同意」(A7 先测量再动注入位置)
- 标签: 模型
- 边界: B1 不改 system 拼装与缓存断点;不改 Chat Completions/DeepSeek 请求;不做 UI 看板
- 验收: ①主代理每个 run 的 episodes.metrics_json 含 cache.read/cache.write 总量与逐步 step_usage,数组长度等于步数;②每步记录 refreshable_changed,可分组比较命中率,口径统一为 cache_read/(input+cache_read+cache_write);③Responses 请求带 prompt_cache_key,其他协议请求体不变;④有一份基于真实数据的结论决定 B2 做或不做,写回设计文档
- refs: R-106 R-184 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P2

## R-374 question 批量化:一次问 1-4 个问题,单弹窗展示后一次提交 [todo]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.4;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §10(以其中「裁决」的单弹窗形态为准)。B1 后端:questions 数组与旧单问归一、多题 AskRequest 与 Answers 响应、answer_ask 支持答案数组、CLI 逐题 stdin、非交互登记全部问题;B2 桌面单弹窗纵向展示全部问题(各题选项与「其他」输入)、工具块摘要与 QUESTION_PENDING 回放支持多题。
- 发现记录: {"Intent":"一次把问题问完,减少打断","Explicit":"一次 1-4 个问题,选项带说明","Assumptions":"单弹窗一次提交比逐题弹窗更少打断","Ambiguities":"无阻塞项","领域对象":"多题提问、答案数组、延迟提问","最小成功闭环":"模型一次调用问两题,用户在一个弹窗里答完","延后决策":"手机端多题作答"}
- 复杂度: 中
- 批次: 0/2
- 来源: 用户 2026-09-25 选择「question 一次问多个」;用户「好用」五条标准中的「少打断」
- 标签: 前端
- 边界: 不修手机端 question 只能批准/拒绝的缺口(另登缺陷);运行时不强制选项数量;不改 question 的权限规则
- 验收: ①旧形态 {question, options, default, multiple} 不报错,等价于一个元素的 questions,单题结果逐字保持「User answer: …」;②交互运行中 1-4 个问题在一个弹窗内展示并一次提交,结果按顺序列出每题问题与答案,「其他」原样返回;③questions 为空、超过 4 个或某题为空时返回纠错且不打扰用户;④非交互运行返回 QUESTION_PENDING,display 含各题且不带 default;⑤中途取消返回错误并附已答题;⑥模型可见 schema 只推新形态且不含 $ref
- refs: D-337 D-745 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P2

## R-375 项目指令文件:注入仓库根到 cwd 各层的 AGENTS.md(缺省取 CLAUDE.md) [todo]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.8;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §11。单批:新建 project_instructions 纯函数模块,在 BaseComponent 注册 stable source「project/instructions」,dev/research/readonly 三档注入,task 子代理不注入;32 KiB 预算,超出显式注明;段首说明指令里点名的工具在 kanzei 不存在时按意图用等价工具。
- 复杂度: 小
- 批次: 0/1
- 来源: 用户 2026-09-25 对 A 档回答「同意」;kanzei 在其他项目中运行时会漏掉该项目已有的 AGENTS.md/CLAUDE.md 约定
- 标签: 后端
- 边界: 不读全局 AGENTS.md;不支持 AGENTS.override.md、.claude/CLAUDE.md 与 @import;不改 dev/conventions 注入
- 验收: ①从仓库根到 cwd 逐层,每层优先 AGENTS.md,不存在才取 CLAUDE.md,根到叶顺序拼接并标出相对路径;②worktree 线读 worktree 里的文件;③总预算 32 KiB,超出时显式注明原大小与截掉字节数,截断落在字符边界;④没有指令文件时不产生段落与账单项;⑤task 子代理不注入,且该段不随每步刷新
- refs: D-201 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P2

## R-376 工具结果外置阈值降到 32 KiB:头尾预览与绝对回取路径,read 与 task 结果豁免 [todo]
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.7;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §12。单批:外置阈值 1 MiB 改为 32 KiB,预览为头 8 KiB 加尾 4 KiB(按字符边界切),模型文本给出可直接 read 的绝对路径;read 与 task 结果豁免;外置时保留 terminal display;删除每次调用写文件的影子遥测。磁盘配额(用户 2026-09-25 拍板:上限 2 GiB,超限降级为 Inline 截断)的实现归 R-245。
- 复杂度: 小
- 批次: 0/1
- 来源: 用户 2026-09-25 对 A 档回答「同意」;CC 约 3 万字符、Codex 约 1 万 tokens 即外置或截断
- 标签: 后端
- 边界: 不改 bash 1 MiB 捕获;不改 read 自身分页上限;不做自动过期;配额实现留在 R-245
- 验收: ①超过 32 KiB 的非 read、非 task 工具结果外置,模型看到头尾预览、原始字节数与绝对回取路径;②不超过 32 KiB 的结果与现状逐字节一致;③多字节内容预览不 panic 且为合法 UTF-8;④活动面板对外置的 bash/git 长输出仍显示终端块;⑤artifact 引用图与清理计划照常识别被引用文件
- refs: R-245 D-349 D-237 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P2

## R-377 会话分叉:从某条用户消息分叉出新线,原线不动 [todo]
- 依赖: R-366
- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.11;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §13。单批:复用回退条目 B2 的 validate_rewind_target 与 project_visible_surface_before,以 fork: 伪 id 的 LegacySeeded 事实为新线种子历史,回退菜单加「从这里分叉」。
- 发现记录: {"Intent":"从某条消息另起一条线尝试别的方向","Explicit":"分叉可以要,优先级低于回退","Assumptions":"复用回退条目的可见历史投影","Ambiguities":"无阻塞项","领域对象":"分叉种子、新线","最小成功闭环":"从中间消息分叉后新线能引用分叉前的上下文","延后决策":"分支树视图;CLI 分叉"}
- 复杂度: 中
- 批次: 0/1
- 来源: 用户 2026-09-25 原话「分叉可以要,但是用的情况相对少,因为有良好的上下文管理其实我不怎么用到分叉」
- 标签: 前端
- 边界: 只分叉对话历史;不复制 worktree、run.trace、episodes 与子代理 transcript;不做分支树视图与 CLI 入口
- 验收: ①分叉新建一条线,历史截止到被点中消息之前,原文回填新线输入框,原线事件不变;②分叉不带代码状态,从 worktree 线分叉时明确提示;③新线首轮 prior 即种子历史,conversation_list 中显示为一段;④种子只写一条 LegacySeeded 事实
- refs: R-242 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P3

## R-378 文件检查点 blob 的保留、统计与清理:进入存储报告与整理入口,按回退可达范围回收 [todo]
- 依赖: R-366
- 内容: R-366 B1 起每个 run 首触文件的前像都写入 .kanzei/artifacts/checkpoints 与 file_checkpoints 表,但没有任何统计、配额或清理路径;R-366 边界称「随 R-245 配额机制处理」,而 R-245 配额只统计 tool-results,交接悬空。本条:存储报告单列 checkpoint_files/checkpoint_bytes;以 file_checkpoints.pre_blob 与回退留证 blob 为引用集构造清理计划(不能复用 session_events 引用图,否则全部 blob 被判无引用);会话删除时删除对应行;保留策略决定回退可达范围,需用户拍板。
- 发现记录: {"Intent":"让检查点存储有上限且可整理","Explicit":"统计、清理与保留策略","Assumptions":"检查点不计入 tool-results 的 2 GiB 配额","Ambiguities":"保留天数或总量上限待用户拍板","领域对象":"检查点 blob、file_checkpoints 行、存储报告","最小成功闭环":"存储报告能看到检查点占用并能安全清理无引用 blob","延后决策":"保留策略的具体数值"}
- 复杂度: 中
- 来源: 波次质量审计 2026-09-26 发现 R-366 B1 的 blob 已在生产落盘而无任何清理归属;用户原话「回退我很常用很重要」决定了保留策略需要用户拍板
- 标签: 后端
- 验收: ①存储报告与 kz artifacts stats 单列检查点占用;②清理计划只回收无引用 blob 并有 dry-run;③会话删除同步删除检查点行;④保留策略经用户拍板后实施并有测试
- refs: R-366 R-245 docs/design/bootstrap_quality_audit.md
- 优先级: P2
