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

## R-235 存量 28 条零证据 active 记忆逐条复核:保留(存量豁免)或降级 candidate,用户拍板 [todo]
- 优先级: P3
- 内容: 对 28 条零证据 active 记忆逐条复核:保留(存量豁免,接受不可计量)或降级 candidate(严格符合无来源不入 active,代价是不可检索注入)。复核结果与依据落到 memory 系统设计文档或本条目关闭证据。
- 复杂度: 小
- 来源: R-213 关闭时盘点发现(R-213 验收③处置的承接)
- 标签: 后端
- 背景: R-213 盘点:state.db 311 条 episode、memory_sources 0 行,project 域 28 条 active 记忆(M-001~M-063)全部零证据(global 域无条目)。这些是 provenance 门禁上线前由用户/交互会话/manager 产生的既有资产,source 字段均无机器可链接的 run_id,历史回填=变相伪造,不可行。R-213 的处置定为存量豁免+文档化,但控制平面「用数据判断记忆是否改善决策」对这些条目无法计量,保留还是逐条降级应由用户拍板。
- 验收: ①28 条清单逐条给出保留/降级结论与依据;②结论落地(设计文档或关闭证据);③如选择降级,操作后搜索不再命中 candidate 条目。
- 阻塞: 用户: 28 条零证据 active 记忆保留(存量豁免)或降级 candidate 需用户逐条拍板,解除权不在 agent。解除动作: 用户给出拍板结论(全部保留 / 逐条降级清单)后按结论落地并关闭。解除人: 用户。

## R-101 桌面端/前端 E2 测试 harness 与延期 E2 清单 [doing]
- 复杂度: 大
- 优先级: P2
- 归属: kanzei
- 背景: 多条缺陷按 conventions §1.2「可用即关闭」关闭,其验证增强项收拢至此,不再阻塞缺陷与需求推进;此前反复出现的阻塞原因是仓库无 package.json、无浏览器测试 harness,无法安全启动真实 Tauri UI。
- 验收: 建立可启动真实 Tauri UI 的 Windows 原生 E2 基座，保留失败非零退出、截图与断言；逐项覆盖权限弹窗、pending ask、切项目复位、手写内容保留、run_task 收尾、停止与长会话响应。CDP 端口和 connectOverCDP 不再是验收条件。
- 拆批(2026-08-08 用户定调「拆出能先做的部分」): **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。**留待 R-086**——依赖会话事件路由的三条:D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位。基座 + 四条 E2 交付即可关闭本条,剩余三条并入 R-086 验收。
- refs: R-086
- 阶段: 3

- 标签: 流程

- 拆批: 2026-08-08 用户定调「拆出能先做的部分」: **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。基座 + 四条 E2 交付即可关闭本条;R-086 已于本轮按 §1.2 可用即关闭关闭,原「并入 R-086 验收」的三条桌面 E2(D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位)留在本条目验收清单执行。

- 进展: 2026-08-17 用户确认 CDP 已不再使用。D-319/D-289 作为旧测试路线退役；R-101 保留延期桌面 E2 清单，但删除 WebView2 DevTools 端口/connectOverCDP 依赖，后续以 Windows 原生 UI Automation 和真实桌面路径交付。
- 状态纠正(2026-08-09): doing→todo。用户已挂起本条,实际不在推进中,却按旧 §1.1 口径占用 doing 名额,与 R-148 一起把 R-153 拒之门外(见 D-219)。恢复推进时再转 doing;挂起前提的小缺陷中 D-185/D-184 仍 open。

- 阻塞: 路线选型由 R-302 承接:等 R-302 选定浏览器工具通道或 Windows UI Automation 并跑通最小 E2 后,按新路线重写本条验收清单再复工。2026-08-18 对账:原阻塞点名的 D-428 已修复;背景中「仓库无 package.json」已过时(根目录已有);拆批点名的 D-060/D-086/D-064/D-066/D-051/D-055/D-056 已全部 fixed,原验收清单需按新路线重定义。解除人:agent(R-302 交付后)。

- 批次: 0/6
- 技术路线: CDP/connectOverCDP 路线于 2026-08-17 退役；桌面 E2 改为 Windows 原生 UI Automation/真实 WebView2 用户路径，必要时再评估受支持的 WebDriver，而不是依赖 DevTools 端口。
- observed_head: 148386f3d467b701f334932b2bfc85bbcfcea475
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786925391968

## R-242 会话投影真源切换与分段清空恢复 [doing]
- refs: D-209 D-342 D-417 R-236 R-279 docs/design/deepseek_harness_upgrade.md
- 依赖: 
- 内容: 在 shadow gate 通过后，将 conversation_get/list、runner prior、子代理 transcript 和 UI 历史恢复逐项切到事件投影；进程内 Vec<Message> 仅作缓存。清空对话追加 conversation.reset 并开启新 segment，新 segment 的模型 prior 为空，旧 segment 仍可审计。验证期保留 legacy snapshot 只读对照，五条读路径全部稳定后停止新增 conversation.updated。
- 复杂度: 大
- 批次: 8/8
- 来源: 2026-08-14 DeepSeek Harness 升级方案；用户确认清空保留、删除确定性物理清除并弹窗提示风险。
- 标签: 核心
- 边界: 本需求只负责事件投影真源切换与 segment reset，不实现会话物理删除、Spill artifact 联动删除、WAL/VACUUM 或迁移备份安全整理；这些统一由 R-245 的删除计划与显式整理入口承担。第一批不改事件 format_version 与 SessionFact 公共词表；任一读路径可通过 feature gate 独立回退 legacy snapshot。
- 迁移与回滚: 不新增表、列或索引时不创建空 migration。切换按五条读路径分别启用 feature gate，legacy snapshot 在观察期只读保留；任一路径出现未知差异即回退该路径。全部 gate 稳定后才停止新增 conversation.updated，既有 snapshot 不删除。
- 阻塞: 
- 验收: ①五条读路径从同一事件日志恢复一致消息；②user/assistant/tool 各安全边界强杀后重启无已发生事实丢失；③孤立 tool call 投影为 interrupted 且不自动重放；④conversation.reset 后新 segment prior 为空但旧 segment 可审计，重复 reset 幂等；⑤至少30个真实 shadow turn 达标，typed_write_errors=0、正常可比较 turn 全部 equal=true、未知差异为0；⑥五条 feature gate 可独立回滚，回滚后 legacy 行为与切换前一致；⑦对照稳定后停止新增 conversation.updated，既有 snapshot 仍可只读回放。
- 优先级: P1
- 进展: 本批 reset segment 修复已提交 `7cbc06cb`（R-242 B8），并完成验收⑤真实窗口：①`crates/kanzei-core/src/store/typed.rs:609-628` 的 `list_latest_segment_facts` 按最新 `conversation.reset` sequence 过滤当前 segment，旧事实仍由 `list_session_facts` 保留可审计；typed shadow writer 在 `typed.rs:1152-1169` 使用该入口；②桌面 shadow `crates/kanzei-app/src/conversation.rs:130-137` 与 UI history harvest `crates/kanzei-app/src/processes/workspace.rs:509-517` 同步使用当前 segment；③T-1786922726244：`cargo test -p kanzei-core` 225 passed，覆盖旧事实8条仍可读、新 segment仅4条、重复 reset 后空段；④T-1786922726245：`cargo test -p kanzei-app` 202 passed；⑤T-1786922726246 先验证真实 CLI 2 turn，随后 T-1786922726248 在同一隔离项目继续执行28次真实 `run --new`，`kz shadow --mismatches` 输出共30 turn、equal=30、expected=0、unknown=0、typed_write_errors=0，验收⑤真实窗口达标；⑥现有五条 feature gate 的独立回滚能力与 legacy fallback 保持既有实现和测试证据；⑦仍依赖 R-243 compaction 事务，停止新增 conversation.updated 与最终 snapshot 对照尚未收口。R-242 不关闭，下一步等待/复核 R-243 后补验收⑦。
- observed_head: 7cbc06cbc9d1c58f3fb3be60e322f3c4a1eda740
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787000896795
- 取活依据: engine:唯一可执行 WIP 是 R-242，必须先恢复它
- 对账: 2026-08-18 对账:与 R-243 互锁(本条验收⑦「停止新增 conversation.updated」依赖 R-243 compaction 事务,R-243 又依赖本条)。收口顺序:本条以验收①-⑥(含⑤真实 30 turn 窗口)为关闭门禁,⑦的收口动作移交 R-243 验收承接
- 停车: 让位 R-243 承接 compaction 追加事务：R-242 的 projection/reset 与验收①-⑥已完成，验收⑦明确需要 R-243 的 compaction 事务来停止新增 conversation.updated；R-243 收口后恢复本条复核最终 snapshot 只读回放并关闭。

## R-243 Surface Compaction 追加事务：原始事件不变、上下文由 surface 投影 [todo]
- refs: R-242 R-236 D-209 docs/design/context_compaction.md docs/design/deepseek_harness_upgrade.md
- 依赖: 
- 内容: 将现有 compact_with_digest 的存储语义改为 compaction_started→compaction_summary→surface_replaced→compaction_ended 追加事务；模型上下文只消费 surface projection，原始 Session 事件不修改不删除；连续压缩走已交付滚动合并。
- 复杂度: 中
- 批次: 0/3
- 来源: DeepSeek Harness compaction 事件事务；复用已交付 R-236 的纪要模型、模板和质量闸。
- 标签: 核心
- 边界: 不重写 R-236 纪要算法、压缩模型配置和质量闸；不把 Memory 作为对话恢复源。Compaction 只在 R-242 正式 surface projection 上追加事务，失败保留原 surface，未完成事务在恢复时显式失效；不修改 format_version=1 的既有消息事实。
- 阻塞: 
- 验收: ①压缩前后 raw event hash 不变；②边界上的 tool call/result 必须完整配对，否则拒绝压缩；③不完整 compaction transaction 重启后不生效且有可见诊断；④连续两次压缩 replay 一致，首段关键实体仍保留；⑤模型 surface 变短但 transcript/audit 仍能回看原文；⑥R-236 全部压缩回归保持通过。
- 优先级: P1
- 对账: 2026-08-18 对账更新：R-242 批次8/8 的 surface projection 已交付，当前依赖字段不再要求 R-242 关闭；本条正式承接 R-242 验收⑦，负责 compaction_started→compaction_summary→surface_replaced→compaction_ended 事务、停止新增 conversation.updated 以及失败恢复后的可见诊断。下一步先完成批1设计冻结与事件事务入口，再接全部 compaction 写者和回归。

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
- 阻塞: API 形态待拍板:验收要求 req refs 指向 .kanzei/research/<topic>/prior-art.md,现行通用 refs 契约只收 R-/D-/T- 编号;且首次 .kanzei/ 初始化无明确 topic 来源。解除动作:用户拍板扩展 refs 命名空间或新增独立 prior_art 字段,并确定三触发的 topic 来源(可参考 R-304 勘察工件落点约定)。解除人:用户。
- 验收: ①三种触发判据各有定向测试,未触发的普通条目不受影响;②prior-art.md 每条结论都带出处,无出处结论被机械拒绝(复用 V0 标注同一套校验);③外部与仓内两侧覆盖各有独立断言,只查一侧不算通过;④新方向下 req add 缺 refs 被拒,豁免路径留痕可审计;⑤websearch 轮次上限有实测,超限给明确诊断而非静默截断;⑥既有 req add 路径无回归。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-248
- 停车: R-248 暂停等待方案确认：验收要求 req refs 指向 `.kanzei/research/<topic>/prior-art.md`，现行通用 refs 契约只允许 R-/D-/T- 追踪编号；且首次 `.kanzei/` 初始化没有明确 topic/触发产物命名。恢复时需先确定兼容 API（扩展 refs 命名空间或新增独立 prior_art 字段/工具）及三触发的 topic 来源，不能凭猜测改数据模型。
- 进展: 已读 docs/design/research_mode_prior_art.md、docs/design/research_mode.md、crates/kanzei-tools/src/tracker.rs:241-270/789-843、tracker/actions.rs:290-360、websearch.rs:14-110、kanzei-app/src/projects.rs:43-127。确认当前没有 prior-art 生成/校验/轮次预算实现；现有 req add 仅做通用 refs 校验，项目初始化只创建 `.kanzei/`，无法凭现有接口生成无 topic 的 prior-art.md。未修改代码。
- observed_head: 3950c0348331956fda32a18d0789ce52d3d30eee
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786960050685

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

## R-291 verify 聚合报告与步骤按耗时重排 [todo]
- refs: D-510
- 内容: scripts/verify.ps1:20-28 Step-With-Timing 包 try/catch,失败累计继续跑,末尾统一报告全部失败;仅全绿才落盘证据(verify.ps1:77-83);按 dist/verification.json 实测耗时重排步骤,亚秒级 node 冒烟先跑、73.6s 的 cargo test 后置,先暴露廉价失败;12 步互不依赖续跑安全,预计改动约 15 行
- 复杂度: 小
- 来源: 2026-08-18 全库勘察;用户长期痛点一次只报一个失败(见记忆 verify-before-release)
- 标签: 流程
- 边界: 不改 12 步清单本身;守护测试(crates/kanzei-tools/src/git.rs:1896-1910 解析键集合)不受函数体重写影响;git.rs 侧聚合归 D-510
- 验收: 一次运行报出全部失败步骤;失败时不写 verification.json;步骤顺序按耗时优化;守护测试通过
- 优先级: P1

## R-292 mobile-pwa 入门禁并对齐桌面 UI 纪律 [todo]
- 内容: crates/kanzei-app/mobile-pwa/app.js(325行)/sw.js(65行) 现无任何门禁:ui_syntax 只 glob ui/*.js(verify.ps1:45),ESLint 只盖 ui/*.js 与 scripts/*.mjs(eslint.config.js:14,74);app.js:260,268,269 用 alert()(桌面端已为此做 confirmDialog/inputDialog 且清零原生弹窗),全部文案硬编码中文零 i18n(app.js:16,55,84,146,161-162,182,216-256)
- 复杂度: 小
- 来源: 2026-08-18 全库勘察
- 标签: 前端
- 边界: 不重做 PWA 功能;代码级与桌面端重复度低(仅 escapeHtml 约 7 行),重点是设计纪律传导与门禁覆盖
- 验收: mobile-pwa 进 node --check 与 ESLint;无原生弹窗;文案接 i18n 通道;verify 通过
- 优先级: P2

## R-293 记忆价值闭环点亮:反事实评估与 outcome 漏斗产出真实数据 [todo]
- refs: D-507 docs/design/memory_control_plane.md
- 内容: 生产实况:memory_eval_agg 0 行(写入方 upsert_memory_effect 与消费链 kanzei-core/src/store/eval.rs:54-90、控制面 UI 全通但无人触发)、arm=outcome_improved 0 行无任何写入方(telemetry.rs:174-183 恒 N/A)、deprecate_candidates 依赖 effect_mean<=0 永远返回空集(eval.rs:76,338)、memory_eval 1670 行里 1640 行是在线 action_changed 对账挤占离线回放语义。接通 outcome 写入方与聚合调度,让 F(m) 漏斗四段真实产数
- 复杂度: 大
- 来源: 2026-08-18 全库勘察;memory_control_plane.md 立身之本「用数据判断记忆是否改善决策」当前无数据
- 标签: 后端
- 边界: 不改回放台六臂框架;先点亮既有链路再谈扩展
- 验收: 生产漏斗 RETRIEVED/INJECTED/ACTION_CHANGED/OUTCOME_IMPROVED 四段有真实数据;控制面 F(m) 栏显示非空;deprecate 判定可被真实数据触发;回归测试
- 优先级: P1

## R-294 记忆检索路线拍板:启用 embeddings 走三臂门禁或正式降级 hybrid 承诺 [todo]
- refs: D-500 docs/design/memory_control_plane.md
- 内容: .kanzei/kanzei.toml 无 [embeddings]、index.db 无 memory_vectors 表、search_hybrid 恒走无 embedder 退化 lexical 分支(crates/kanzei-memory/src/memory/index.rs:428-431);replay_eval 六臂各仅 5 case 且 Candidate 臂自身退化(replay_eval.rs:99-163);RecallAction 七态只落地四种(memory/mod.rs:606-610)。按设计 §5 启用门禁跑三臂对比给量化结论,或在 memory_control_plane.md 正式降级 hybrid/PlanInject/StateAudit 承诺并收缩词表
- 复杂度: 中
- 来源: 2026-08-18 全库勘察
- 标签: 后端
- 边界: 启用与否由数据说话;D-500(embed runtime 缺陷)是启用路线的前置
- 验收: 量化对比结论落文档;启用则配置+向量索引真实生效,放弃则文档与词表同步收缩;RecallAction 词表与实现一致
- 优先级: P2

## R-295 candidate 清退提速:出口策略与生产速率匹配 [todo]
- refs: D-492 D-494
- 内容: reconcile_candidates 现仅两条出口:recurrence>=3+指纹+当轮 episode 晋升(crates/kanzei-memory/src/memory/mod.rs:972)或 age>=14 天清退(store.rs:552-601);96 条 08-17 候选要等 08-31 才清,期间持续挤占 FTS top-24 检索窗口(与 D-492 叠加)。新增清退策略:语义近似合并、低价值提前退、单日产出上限或等效手段
- 复杂度: 中
- 来源: 2026-08-18 全库勘察;2026-08-17 单日 96 条 candidate 实证
- 标签: 后端
- 边界: 不动晋升的 provenance 门禁;清退遵循归档不裸删 SOP
- 验收: 候选存量收敛至健康水位并可持续;策略有测试;现存 96 条按新策略处置;检索窗口占用可量化改善
- 优先级: P1

## R-296 Tauri command 与 run 链路测试基座 [todo]
- 内容: kanzei-app 无 tests/ 目录(全仓唯一集成层在 crates/kanzei/tests/integration),104 个 #[tauri::command] 零测试;装配→执行→落库主链 commands/run.rs(604行)、processes/lifecycle.rs(593)、processes/workspace.rs(548)、run/persistence.rs(489)、run/coordinator.rs(424)、run/execution.rs(313)、harness_ext.rs(284) 全部 0 个 #[test];数据面 memory.rs(13 command)/docs.rs(16 command) 同样近零。建立可测基座(状态抽离/伪 AppHandle/集成层)并优先覆盖 run 主链
- 复杂度: 大
- 来源: 2026-08-18 全库勘察
- 标签: 后端
- 边界: 不追求覆盖率数字,优先真实断言关键路径;不重构业务逻辑
- 验收: run 主链关键路径有自动化断言;新增 command 有明确测试落点范式;cargo test 全绿并入 verify
- 优先级: P1

## R-297 kanzei-llm auth token 刷新路径测试 [todo]
- 内容: crates/kanzei-llm/src/auth/codex.rs(124行,含 token 过期判定+RFC3339 解析+刷新写回)0 测试,auth/mod.rs、event.rs 同为 0;全 crate 52 test/4043 行且几乎全是 SSE/JSON 形状断言
- 复杂度: 小
- 影响: token 刷新写错表现为莫名其妙掉线,难定位
- 来源: 2026-08-18 全库勘察
- 标签: 模型
- 验收: 过期判定/刷新/写回路径有单测;伪造时钟覆盖边界;cargo test -p kanzei-llm 通过
- 优先级: P2

## R-298 发布链装后验证与证据补全 [todo]
- refs: docs/design/ci_release_evidence_chain.md
- 内容: 打包链止于拷进 dist(scripts/package.ps1:130-136),setup.exe 从未被自动安装验证;install-setup.ps1 全仓零调用方(仅 release.ps1:67 报错文案提及);版本双源冻结 0.1.0 无比对(Cargo.toml:15 与 crates/kanzei-app/tauri.conf.json:4);release notes 无安装器 SHA256(package.ps1:148,设计列为后续可选 ci_release_evidence_chain.md:189);dist 堆 6 个无人引用 setup.exe 约 85MB 无保留策略;release.ps1 开发通道仅 cargo test(release.ps1:15-19),能把过不了 12 步中 10 步的二进制装进系统;install-setup.ps1:41-59 装前不备份装坏不还原
- 复杂度: 中
- 来源: 2026-08-18 全库勘察
- 标签: 发布
- 边界: NSIS 路线不重做;安装验证需考虑 LOCALAPPDATA 容器重定向问题(见记忆 localappdata-container-redirect)
- 验收: 打包后自动静默装+装后自校验入链;SHA256 入 notes;版本一致性检查;dist 保留策略;开发通道最低门禁明确并留档
- 优先级: P2

## R-299 IPC 与事件契约机械比对扩面 [todo]
- refs: R-284
- 内容: scripts/ipc-contract.json 仅锁 docs_snapshot 一个顶层键(1/104 command),而该机制自述正是 30+ 命令手搓 JSON 两侧各写一遍字符串(crates/kanzei-app/src/ipc_contract.rs:1-19);后端 emit 事件集合(kz:compacted/kz:meta/kz:reasoning/kz:step 等)与前端 on() 订阅集合无任何机械求差;ui-runtime 冒烟的多会话/记忆页 fixture 是前端作者手写,后端改字段名照样全绿。扩契约文件覆盖高频 command,emit/listen 求差入冒烟
- 复杂度: 中
- 来源: 2026-08-18 全库勘察
- 标签: 核心
- 边界: 作为 R-284 事件契约的前置批次,不与其四批重复;词表定义归 R-284
- 验收: 契约覆盖高频 command;emit/listen 集合求差入冒烟;后端改事件名或字段名可被门禁捕获
- 优先级: P2

## R-300 大文件拆解第三轮与回涨闸门 [todo]
- refs: docs/design/metrics_baseline.md docs/design/monolith_decomposition_round2.md
- 内容: Rust 侧:background.rs 2019 生产行居全仓 Top-1 却不在 R-257/R-202/R-204 任何拆解条目范围;drive.rs:930 execute_tool_calls 582 行及同文件 307/256/249 行函数群;profiles.rs:68/:605 同一 trait 方法两份实现(537+242 行);tracker/actions.rs 已回涨至 1356 行;kanzei/src/cli/run.rs:27 单函数 651 行且文件零测试。前端侧:08-compose.js 1535 行(拆解预算 941,超 60%),09-sessions/16-settings/11-docs-list/20-lines/07-events/06-activity 五文件回涨至 900+;06-agent-panel.js 是 06-activity.js 的逐行分叉复制(八对函数一一对应共用 bg-* CSS);index.html 1150 行 8 视图未拆。拆解后行数上限回涨闸门进 verify;完成后重跑 kz metrics 前后对照
- 复杂度: 大
- 来源: 2026-08-18 全库勘察;metrics_baseline.md 仅 08-16 单份基线从未重跑对照
- 标签: 核心
- 边界: 拆解不改行为;每批全冒烟+cargo test;闸门阈值宽松起步防误伤
- 验收: Top 目标拆解落地;06-agent-panel 与 06-activity 合流;回涨闸门在 verify 生效;metrics 对照落 metrics_baseline.md
- 优先级: P2

## R-301 泳道三级卡住判据 [todo]
- refs: docs/design/parallel_lines_ui.md
- 内容: 按 docs/design/parallel_lines_ui.md:217,250 交付泳道状态机三态:跑着/疑似卡住/失败,禁止只按多久没动判死;判据结合事件流真实进展而非纯时间阈值
- 复杂度: 中
- 来源: 2026-08-18 全库勘察;parallel_lines_ui.md P3 设计验收文案已写好,承接条目 R-184 已归档无人认领
- 标签: 前端
- 验收: 按设计文档既有验收文案交付;三态转换有测试;真实长任务不被误判死
- 优先级: P2

## R-302 桌面 E2 路线立项:浏览器工具通道 vs Windows UI Automation 选型 [todo]
- refs: R-101 D-511
- 内容: R-269 结论:浏览器工具通道可行,落地依赖 kzapp 暴露 URL 入口(requirements-archive.md:3445);R-101 技术路线:Windows 原生 UI Automation(requirements.md)。二选一给出对比依据与最小可行验证(真实桌面跑通一条 E2),选定后 R-101 的延期 E2 清单挂到该路线并重写其过期验收
- 复杂度: 中
- 来源: 2026-08-18 全库勘察;R-269 关闭说明⑦与 R-101 技术路线两条候选互不引用均无实施条目
- 标签: 流程
- 边界: 本条只做选型+最小验证,不交付全部 E2 清单(那是 R-101);CDP 不再是候选
- 验收: 选型结论有对比依据落档;最小 E2 真实桌面跑通;R-101 验收清单按新路线更新
- 优先级: P2

## R-303 文档一致性批次订正 [todo]
- refs: R-283 R-264
- 内容: ①README.md:65,107,149 仍把已退役的 goals 线列为事实源(R-252 B5 已删 goals.md 改指 IDEAS),:57-68 漏报 research 工作台/LaTeX 绘图/移动端 PWA+LAN 桥/浏览器工具/ui_screenshot/按线设置(R-269~R-277/R-290)等已交付能力;②docs/使用手册.md:38-45 漏「想法」收件箱整块能力,:67-74 快捷键表缺 Ctrl/Cmd+P 命令面板(ui/21-palette.js 已实现);③memory 三份设计文档落后实现约 8 个需求:memory_control_plane.md 需求清单停在 R-167(R-194/R-195/R-213/R-216/R-233/R-255/R-286 与 D-366/D-368 均未进文档),memory_system.md 状态枚举仍 active|stale(实际五态)且工具集缺 memory_promote 等,memory_decision_sufficiency.md 实施边界仍指旧路径(R-203 已迁);④docs/design/ui_esm_migration.md:3 状态过期(B1/B2 已完成,规模已从 21 文件/12389 行涨到 24 文件/15528 行,typeof 守卫 6 处涨到 44 处);⑤docs/design/phase2_system_upgrade.md:301-322 Wave 0/1 门禁记录引用的 R-221/R-277/R-286/D-428 状态全部过期,R-283 验收②依赖该记录
- 复杂度: 小
- 来源: 2026-08-18 全库勘察
- 标签: 流程
- 验收: 五组文档与现实对齐;R-283 验收②的 Wave 记录恢复可用;订正一次批次收口
- 优先级: P3

## R-304 dev 勘察工件固定落点 [todo]
- refs: R-248 docs/design/research_mode.md
- 内容: dev 侧勘察产物(调研笔记/证据/对照)目前无固定目录与生命周期约定;定义落点(如 .kanzei/research/ 或专用目录)、命名、与条目 refs 的关联方式及清理策略
- 复杂度: 小
- 来源: 2026-08-18 全库勘察;research_mode.md:27 与 R-221 关闭说明(requirements-archive.md:3652)两处明写需另立条目承接,全库无对应条目
- 标签: 流程
- 边界: 与 R-248 prior-art 门禁互补不重复:R-248 管开工前对照,本条管勘察产物落盘可回溯
- 验收: 落点约定落档并有工具/文档支持;勘察产物可按条目回溯;R-248 恢复时可直接复用该约定
- 优先级: P3

## R-305 subagent 策略层与 Agent 目录 [todo]
- refs: R-281 D-513 docs/design/subagent_management.md
- 内容: 按 docs/design/subagent_management.md:36-84 交付三块:①Agent 目录(可用 agent 类型的注册与描述);②策略面板(每轮 task 上限/并发/超时/重试/预算可配置并生效);③运行审计摘要(每轮 subagent 派发与结果的可读汇总)。与 R-281 子代理阅读器互补:R-281 管看单个子代理说话,本条管策略与全局审计
- 复杂度: 大
- 来源: 2026-08-18 全库勘察;2026-08-18 用户拍板需要;docs/design/subagent_management.md P2 三块未实施,唯一演进条目 R-117 已 dropped
- 标签: 核心
- 边界: 不重做 R-281 的对话读取器;策略默认值保持现行为,配置生效有真实验证
- 验收: 按设计文档三块交付;策略修改真实生效有测试;审计摘要在 UI 可见;roster_cap 类静默截断(D-513)在策略面板可见化
- 优先级: P2
