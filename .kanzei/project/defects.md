# Defects

## D-204 SOP 用户易用性不佳:总结质量/查看展示/产生时机三处都不行 [fixing] (medium)
- refs: D-205 R-105 R-107
- 原始描述: SOP易用程度有问题，似乎总结的不太好
- 澄清(2026-08-09 用户逐项指认): 所指为**用户**查看/使用 SOP 时的易用性,不是 SOP 对模型的可消费性。三个维度都有问题:①**总结质量差**——条目内容泛化、丢关键步骤,看了不知道怎么照做;②**查看入口/展示**——界面上找到、打开、阅读 SOP 的路径不方便,展示形式不适合阅读;③**产生时机/数量**——该沉淀的没沉淀、不该沉淀的乱沉淀,产出节律不对。检索/命中用户未勾选,暂不在范围内。
- 复现: 桌面端 Memory 页(R-107)查看 sop 类条目;对照近期自举轮次的 SOP 产出(如 inbox 里的「候选 SOP:完成 D-155 的流程」类,只有工具顺序罗列,无判断依据与边界条件)。
- 影响: SOP 是 R-105 记忆蒸馏的主要产出形态之一,人读不动就只剩模型消费一条腿;产生时机不对还会稀释记忆库信噪比。
- 验收: ①总结质量:SOP 条目有可照做的结构(适用场景/步骤/每步判断依据/边界),不再是纯工具名罗列;②查看展示:Memory 页的 SOP 有适合阅读的排版,入口可发现;③产生时机:沉淀门槛可说明(什么样的流程值得成为 SOP),乱沉淀实例(纯机械序列)被拦;④用户复查确认三个维度都有改善。
- 备注: 本条登记过程本身暴露了快记的信息保真缺陷(伪复现「查看 SOP 时」+丢"用户"限定词),已单独登记为 D-205 并修了第一层。
- 优先级: P3
- 标签: 核心

- 批次: 2/2
- 进展: 两批交付+全量绿,逐条证据:①总结质量——harvest_sop 候选 detail 给 manager 可照做结构模板(1.适用场景 2.操作步骤:每步做什么+判断依据 3.边界与例外)(crates/kanzei-tools/src/memory/mod.rs harvest_sop),manager_agent() prompt 加 SOP 提炼规则(纯工具罗列不算 SOP、一次性流程 NOOP)(memory/manager.rs manager_agent),不再纯工具名罗列;②查看展示——Memory 页列表行 sop 加左边框+SOP 徽标(13-memory.js loadMemoryList + style.css .memory-row.sop/.memory-row-cat.sop,入口可发现),详情正文 renderMemoryBodyRead 识别「N. 标题」编号行渲染为结构化步骤块(.memory-sop-step 标题加粗+正文剥离冒号);③产生时机——harvest_sop 加工具序列门槛(tools<3 机械拦截,纯机械序列被拦,memory/mod.rs + 回归单测短流程不投)。④待用户复查确认三个维度。验证:T-1786451023(批1 定向 232 绿)+T-1786451128(批2 前端四冒烟)+T-1786451243(关闭前全量全绿)。2026-08-13 用户定调:要改但不是现在,需求比较边缘,优先级 P2→P3,阻塞保持成立。

- 阻塞: 验收④「用户复查确认三个维度都有改善」——工程面①②③已交付并全量绿,需用户实际查看 Memory 页 SOP 排版与新沉淀门槛后确认;解除人=用户(复查后确认改善即可关闭)。

- priority: 

## D-322 记忆更新/整合环节跨主题覆写存量未清:M-016/U-005 缝合、M-044 英文化,D-282 校验只防增量 [fixing] (medium)
- 复现: M-016 原 docs 整理正文被删光换成三主题缝合;U-005 title 讲 R-163 而 description 讲 edit 指纹且与 M-032 重复;archive M-044 被英文化改写(文件名含 s0p 错字);INDEX candidate 计数改了条目行没加
- 影响: 记忆可信度受损,检索命中错误主题;D-282 主题一致性校验上线前的存量脏数据无人回收
- 来源: 2026-08-13 会话复盘(缝合体已归档留证:archive/M-016、全局 archive/U-005)
- 标签: 后端
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-322
- 进展: 勘察完成(2026-08-16):三处损坏条目定位并确认恢复源——①M-016(项目 archive,缝合体:文件名 docs-目录整理、内容换成权限拒绝三主题):原文完整可恢复,git show 32cc02f:.kanzei/memory/M-016-docs-目录整理-2026-08-08-design-统一-snake-cas.md(docs 整理六条结论);②M-044(项目 archive,英文化改写+文件名 s0p 错字):中文原文完整可恢复,git show d4a4f08:.kanzei/memory/M-044-defect-update-字段键名与多字段处理-sop-防英文-key-追加与.md;③U-005(全局 archive,title= R-163、description= edit 指纹缝合):原文不可恢复(全局仓 2026-08-13 建仓时即以缝合体归档留证,commit ced6352),且正文与 M-032 重复,处置=标记 deprecated+注记指向 M-032;④INDEX candidate 计数与文件一致(4=4,83a71b2 已对齐)。修复被写通道门禁阻断:记忆库 policy-managed,主 agent 对 .kanzei/memory/** 的 write/edit 被 ruleset 硬拒(唯一合法写通道 memory_note),memory-manager 工具只能处理活动条目、够不到 archive/ 里已归档条目,git 历史恢复需要文件写权限——三处手术均无法在本会话执行。
- 阻塞: 修复被写通道门禁阻断(§1.1 类②):记忆库 policy-managed,主 agent 无 .kanzei/memory 写权限(唯一通道 memory_note),manager 工具够不到已归档条目,git 历史恢复需文件写权限。恢复源已备好(M-016@32cc02f、M-044@d4a4f08 原文可 git show;U-005 处置=deprecated+指向 M-032)。解除动作: 用户在交互会话执行恢复(写回 M-016/M-044 原文、U-005 标 deprecated),或给 memory-manager 增加对归档条目的直接写能力(新需求)。解除人: 用户。
- observed_head: 1b09a249d57dac40ac07a3d94fcd7ef641596888
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786599000900

## D-319 WebView2 当前环境 DevTools 端口不监听:e2e-smoke connectOverCDP 20 秒超时(参数已传入但不绑定) [fixing] (medium)
- 复杂度: 中
- 复现: 2026-08-16 D-289 实测验证中发现:无论 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 环境变量还是 KANZEI_E2E_CDP 注入路径,WebView2 进程命令行均带上 --remote-debugging-port=<port> --remote-allow-origins=*(进程命令行实证),但端口 20 秒不监听、user-data-dir 无 DevToolsActivePort 文件、进程树完整(renderer/gpu/network 均在)、会话与用户 kzapp 同为 Session 1、无策略禁用(注册表 HKCU/HKLM EdgeWebView/Edge 均空)。对照:同参数字符串起 Edge --headless,1 秒即监听。结论:WebView2 在当前机器/环境不启动 DevTools 端口,与参数注入路径无关。
- 影响: R-101 e2e-smoke 基座在自举环境无法实测 connectOverCDP(端口不监听→20 秒超时→FAIL)。这独立于 D-289 的 origin 白名单修复——即使端口能监听,D-289 也是必需的(M111+ 拒非白名单客户端);但端口不监听会让 e2e-smoke 永远失败。
- 来源: self-found(D-289 实测验证中发现)
- 标签: 流程
- 优先级: P2
- 进展: 2026-08-16 取活诊断(9 轮实验,证据链完整)。已排除:①注入通道——环境变量 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 与 KANZEI_E2E_CDP additional_browser_args 两条路都传参成功(msedgewebview2.exe 命令行实证含 --remote-debugging-port/--remote-allow-origins/--remote-debugging-address);②参数格式——同参数字符串起 Edge(含非 headless)1 秒监听;③策略——HKCU/HKLM EdgeWebView/Edge DeveloperToolsAvailability 全空;④AppContainer/容器——whoami 无 AppContainerSid,Session 1 正常;⑤日志——--enable-logging=stderr --v=1 无 devtools 相关输出;⑥版本——WEBVIEW2_FIXED_VERSION 指定 151.0.4129.59 未生效(Tauri 仍用 78),151.0.4129.78 签名有效、安装完整;⑦进程树——renderer/gpu/network 完整,webview 已创建但 browser 进程不绑定端口、无 DevToolsActivePort 文件。结论:WebView2 Runtime 151 在当前机器不启动 DevTools HTTP 服务,与参数注入/代码路径/系统策略均无关。排除后剩余变量是 WebView2 Runtime 本身或其与系统环境的交互(需重装/更新 runtime 或换环境验证,或改用 WebDriver/tauri-driver 路线)。阻塞:解除人=用户。
- 阻塞: WebView2 Runtime 151 在当前机器 DevTools 端口不绑定(9 轮实验证据链,见进展)。解除动作:①用户重装/更新 Microsoft Edge WebView2 Runtime 后重跑 e2e-smoke;或②用户提供 WebView2 DevTools 正常的环境验证;或③用户拍板改 WebDriver/tauri-driver 路线(不在本条范围内)。解除人:用户。

## D-289 R-101 harness CDP 注入缺 --remote-allow-origins:可能致 e2e-smoke connectOverCDP 握手失败 [fixing] (medium)
- severity: medium
- 优先级: P1
- 复现: R-101 harness 基座静态审查:crates/kanzei-app/src/main.rs:110-116 的 KANZEI_E2E_CDP 注入只加 --remote-debugging-port=<port>,未加 --remote-allow-origins=*;同轮实验脚本 output/e2e-exp/env-var-exp.mjs:16 加了 --remote-allow-origins=*(且 .playwright-cli 08-11 快照证明 CLI 曾连上)。WebView2 基于 Chromium,自 M111 起 CDP 要求显式 origin 白名单,否则非 DevTools 客户端(playwright-core connectOverCDP)握手被拒。
- 影响: scripts/e2e-smoke.mjs 可能 connectOverCDP 失败,harness 基座验证被卡;若 e2e-smoke 实际能连上则本条为误报,实测后关闭。
- 来源: self-found(2026-08-13 R-101 静态审查)
- 标签: 流程

- 复杂度: 小
- 进展: 2026-08-16 修复落地(commit 待定):main.rs KANZEI_E2E_CDP 注入的 additional_browser_args 追加 --remote-allow-origins=*(仅 E2 注入路径,生产不带)。验证:①编译+kanzei-app 137 测试通过;②WebView2 进程命令行实证参数已传入(--remote-debugging-port=<port> --remote-allow-origins=* 均出现在 msedgewebview2.exe 命令行);③Edge 对照:同参数字符串起 Edge --headless 1 秒监听,证明参数格式/网络层无误。实测:e2e-smoke 20 秒超时 FAIL——根因已定位为 WebView2 在当前环境 DevTools 端口根本不监听(端口不绑定、无 DevToolsActivePort、无策略禁用、进程树完整),与 D-289 的 origin 白名单修复正交(端口能监听时 connectOverCDP 仍需白名单,D-289 是必要修复);环境问题已登记 D-319。按 §1.25 保持 fixing,验收(connectOverCDP 实测成功)待 D-319 解除后补齐。

- 阻塞: D-319(WebView2 当前环境 DevTools 端口不监听)未解决前,e2e-smoke connectOverCDP 20 秒超时无法实测。解除人:解决 D-319 或确认 WebView2 环境可起 DevTools 后重跑 e2e-smoke。

## D-275 托管路径 OS 层写隔离(残余):后台进程与专用工具同窗口同前缀时仍可蒙混 [fixing] (medium)
- 优先级: P2
- 复杂度: 大
- 来源: 2026-08-11 D-258 关闭时转出(验收①的 OS 层条款未做,成本收益倒挂且与验收②互斥)
- 标签: 核心
- 缺陷: D-275
- 证据等级: E1
- 进展: B1 代价评估(2026-08-16,取活即评估):路线比对——①纯 ACL(icacls 锁两颗托管树):不可行。引擎与子进程共享用户 SID,ACL 按 user/group 判定,锁掉后台子进程同时锁掉引擎自身(引擎进程内工具 test_record/memory_note/tracker 都要写托管路径)。②受限令牌(子进程 CreateRestrictedToken+deny SID):技术上可行但集成面=全仓散点 spawn(kanzei-app processes.rs/bash 工具/docs.rs/background 任务等,std::process::Command 无令牌支持需改 CreateProcessW),且 POSIX 无等效(flock 是 advisory 不区分进程),跨平台价值存疑。③低完整性(low-integrity 子进程+target//node_modules/ 显式授予):唯一可区分进程的 OS 机制,但仍是 Windows-only、需在 spawn 面铺完整性级别管线+为 target/ 建低完整性 ACE+测试,与验收①「不破坏 target/ 写」可相容;POSIX 降级=显式告警无等效。风险收益:残余窗口是毫秒级(需恶意/故障子进程恰在专用工具执行时写同一托管前缀),D-258 快照吸收已覆盖常见路径,原评估定调成本收益倒挂。结论:OS 层隔离存在技术上可行路线(低完整性)但成本高、仅 Windows 有效,残余风险极低——是否值得投入由用户拍板:接受残余边界(文档化,关闭或维持 open)或另立范围化实现条目。
- 验收: ①存在一条后台进程在操作系统层面写托管路径失败的机制(受限令牌/低完整性/ACL),有实测证据,且不得破坏后台任务写 target/、node_modules/(与 D-258 验收②同口径);②该机制与托管写入窗口(managed_fence)组合后,窗口内后台进程与专用工具写同一批路径也不能蒙混——即吸收/回滚不再依赖镜像快照区分;③跨平台降级路径有明确说明(Windows 独占句柄 vs POSIX advisory lock),降级时不静默放行而是显式告警。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-275
- 批次: 1/1
- 阻塞: B1 代价评估已完成(见进展),结论=OS 层隔离技术可行但成本高/仅 Windows/残余风险毫秒级,是否投入由用户定夺。解除动作: 用户拍板——接受残余边界(文档化后关闭/维持 open)或另立范围化实现条目(低完整性路线)。解除人: 用户。
- observed_head: 45fd276e9ac4ac6a23c0027b801f95d6c6c3fe4f
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786598491964

## D-209 对话轮内事实与中断 assistant 草稿无法增量恢复 [open] (high)
- refs: D-208 D-185 D-342 R-236 docs/design/deepseek_harness_upgrade.md
- 原始描述: 用户 2026-08-09 原话"落库对话粒度太粗"(与活动栏回放问题同时反馈)。
- 机制现状(供收敛方向): ①对话持久化是 `conversation.updated` 事件整份 messages 快照替换,轮内不落盘,恢复只能回到轮边界;②工具轨迹 run.trace 只在收尾 flush 一次(D-179 补了停止路径,但仍是整轮一包);③episodes 是轮级摘要。三层都是"轮"粒度,轮内的中间态(改到一半、流式输出中断点)不可恢复、不可检索。
- 待澄清: 已澄清(2026-08-14):用户确认三项都属于真实痛点——恢复会丢轮内进度、工具轨迹缺少可回放顺序、历史只能按整轮获取；同时要求保留生成到一半的可见 assistant 内容，以复盘中断原因。
- 验收: ①user message、assistant 可见文本、tool call/result 和 turn 终态按原子 sequence 增量持久化，重启投影顺序确定；②强杀发生在流式生成中时，重启可看到已持久化的未完成草稿、明确 interrupted 标识和最后检查点，不能显示为完整回答；③多工具只完成一部分时，已完成结果保留，未完成调用闭合为 interrupted，禁止自动重放有副作用工具；④conversation_get、模型 prior、活动/审计投影对同一事件序列给出一致事实；⑤D-342 正常停止路径回归保持通过。
- 证据等级: E3(用户再次确认三项痛点和部分生成恢复需求；现有轮级机制已读码核实)
- 优先级: P1
- 标签: 核心

- 进展: 2026-08-10 取活时仍待澄清:候选 a)恢复丢轮内进度 b)工具轨迹糊成一批 c)只能按整轮拿消息 d)其他——机制现状已核实(三层都是轮粒度),按 D-205 教训不代用户猜死;本轮跳过,待用户确认维度后改写验收再取活。

- 阻塞: 无
- 修复方向: 以 SQLite typed session events 为运行时会话真源；user/assistant/tool/终态按发生顺序增量落库。可见 assistant 流式内容按有界批次追加 draft chunk，最终追加 committed 或 interrupted 终态；中断草稿可在 UI/审计中回放，但不伪装成完整 assistant message。
- 影响: 崩溃、停止或异常中断后，已发生的轮内事实和部分生成内容不能被完整恢复；下一轮模型、用户历史与审计视图看到的事实可能不一致。D-342 已修正常停止的整轮写回，但不能替代逐事件持久化和异常中断恢复。

## D-333 存量 tracker 污染收敛:活动区双优先级字段、归档区双终态标记、重复进展字段(D-330/D-331 修复前残留) [fixing] (low)
- refs: D-332 D-331 D-330
- 复杂度: 小
- 复现: normalize dry-run 全仓扫描实测检出(2026-08-13,CLI kz req normalize):活动区 R-234/R-235 各带重复「优先级」字段(D-330 修复前的存量);归档区 R-201/R-198/R-199/R-213 标题为 [open][done] 双终态标记(D-331 修复前的存量,parser 只剥最后一个 done,[open] 残留标题);归档区 R-225/R-226 重复「进展」字段。当前会话引擎跑旧编译,工具通道(req update)对存量双字段无法去重(update 只覆盖首个匹配),CLI normalize apply 写盘被托管围栏拦截。
- 影响: 重复字段让 UI 显示歧义(哪个优先级生效未知);归档双终态标记污染统计与审计;这些是 D-330/D-331 修复前的存量,合法修复面 normalize/fix_terminal 已存在但需引擎重启后执行。
- 来源: self-found(D-332 B3 存量收敛时 normalize 扫描检出)
- 标签: 核心
- 验收: ①R-234/R-235 各只剩一个「优先级」字段,值与首个一致(有测试或工具输出证据);②归档 R-201/R-198/R-199/R-213 标题只剩单一终态标记,残留的 open 标记被剥离(有测试或工具输出证据);③R-225/R-226 归档重复「进展」字段收敛;④全程走专用工具(normalize apply / fix_terminal / req update),无手改 markdown。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-333
- 进展: B1 完成(2026-08-13):验收②达成——归档区 R-201/R-198/R-199/R-213 的 [open][done] 双终态标记已用 fix_terminal 收敛为单一 [done](status 保持 done、标题残留 open 剥离、进展留 [terminal-fix] 审计,commit f3b7dcd)。剩余:验收①R-234/R-235 双「优先级」字段、验收③R-225/R-226 双「进展」字段——均需 normalize apply 去重,当前会话引擎旧编译无 normalize 动作(实测 req normalize 报 unknown action),CLI 写被围栏拦;引擎重启后执行 normalize apply 收敛。验收④全程走专用工具(fix_terminal/normalize/req update),无手改 markdown——B1 已满足,剩余部分同样只走工具。
- 阻塞: 用户: 当前会话引擎(kzapp pid 13704)仍跑旧编译,D-332 交付的 normalize 动作在工具面不可用(实测 req normalize 报 unknown action);CLI 写盘被托管围栏拦截。验收①③(R-234/R-235 双优先级、R-225/R-226 双进展)必须走 normalize apply,需引擎重启后执行。解除动作: 用户重启 kzapp(关闭后重开),新工具面加载 normalize 后执行 `kz req normalize --apply` 或工具面 normalize 收敛剩余重复字段。解除人: 用户。
- observed_head: bd629cdd4ec0ac641c11fd4177e57cfa2aaa9c49
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786613794715

## D-342 停止运行 = handle.abort() 硬杀,被打断轮的对话历史整轮丢失 [fixing] (high)
- refs: R-236 docs/design/context_compaction.md
- 复现: 自动推进中点「停止」再发新任务:stop_runtime_and_finalize(kanzei-app/src/state.rs:534)直接 handle.abort() 杀掉 run_task 的 future;而对话写回只在轮末(run.rs:1032 内存表、run.rs:1089 conversation.updated 事件),abort 永远到不了那两行 → 被打断轮的全部消息(可能几十步工具调用/改动/结论)从对话投影消失,下一轮 prior 停在上一轮轮末。模型于是称"之前没做过 X"(用户 2026-08-14 实测报告)。
- 影响: 打断+插临时任务是自动推进的高频交互,每次都让模型对被打断轮完全失忆;episode/run.trace 有留档但那是回放用的,模型看不到。runner 侧没有优雅停止:halted_by_user=true 唯一产生路径是权限弹窗被拒(kanzei-core/src/runner/drive.rs:1059),步循环里没有任何 halt 检查点。
- 来源: 用户报告(2026-08-14 自动推进打断丢上下文)+ 读码定位
- 标签: 核心
- 进展: 2026-08-16 复核轮:实现(commit cbe768a)与既有测试逐条核对完毕。验收①③④⑤证据齐备:①cooperative_halt.rs 测试①(halted 轮 messages 完整交还:prior+本轮用户消息)+ run.rs:1114-1117 写内存表、run.rs:1228-1244 conversation.updated 事件落库 + conversation.rs:158 recover_messages_raw 从事件恢复 + conversation_tests.rs:94-107 写回→恢复链路测试;③drive.rs 步首 222/流内 select 422/工具间 790/步末 991 四类检查点 + cooperative_halt.rs 测试②(执行中停止挂起子代理被打断、取消占位配对、filter 后逐字节无孤儿);④state.rs:610 stale_run_needs_abort 纯函数 + process_tests.rs:129 兜底硬杀只认停止时那一代测试 + process_tests.rs:73 正常停止置令牌不 abort;⑤process_tests.rs:114 排队输入即刻取消(cancelled==1)+ finalize_interrupt 在 state.rs:592/603/633 全部路径保留。验收②机制部分(新一轮 prior 含被打断轮内容)由 cooperative_halt 测试①(prior 原样在 messages)与 run.rs:699 conversation_prior 取内存表证明;但「模型可复述被打断轮做过的事」需真实模型桌面端场景——R-236 联测(requirements-archive R-236 验收⑤)明确未覆盖 D-342 停止路径(CLI halt 通道为 None,桌面端停止场景待用户新版实际使用验证),本复核轮无真实 provider 桌面端联测条件,该项缺口保持。
- 验收: ①自动推进中途停止后,conversation_get 能看到被打断轮已完成步骤的消息(实测轨迹,不是只断言函数返回);②停止后立刻发新任务,新一轮 prior 含被打断轮内容,模型可复述被打断轮做过的事;③停止响应有上界:当前工具执行结束即停,不等整轮跑完;④abort 兜底路径保留(防挂死)且有测试,正常停止不走它;⑤停止仍取消排队输入并释放写租约(现有 finalize_interrupt 语义无回归)。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-342
- 阻塞: 用户: 需在桌面端新版(含 cbe768a 及后续)实际点「停止」后立刻发新任务,验证模型能复述被打断轮做过的事(验收②后半)。解除动作:用户实测并反馈结果后,再补关验收②。
- observed_head: dd5e5fd66bfe1387331ccac3f449f51924d7a103
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786652156479

## D-349 工具大输出在事实入库前不可逆截断，trace 仅留 preview 且无完整原文回读 [open] (high)
- refs: D-209 R-180 R-245 docs/design/deepseek_harness_upgrade.md
- 复杂度: 中
- 复现: 执行输出超过上限的 bash/git/webfetch 或后台任务：bash/git 在工具层截断，run.trace 再仅记录 preview；当前会话没有 artifact_id 或回读指引。进程退出或上下文压缩后，用户和模型均无法从会话恢复完整原文。
- 影响: 工具结果的事实在写入事件日志前已经丢失；审计、故障复盘、后续精确引用和压缩后回读只能看到片段，可能隐藏真正报错或把截断结果误认为完整结果。
- 来源: 2026-08-14 DeepSeek Harness Spill 对照审计与现行代码核查。
- 标签: 核心
- 根因: 各工具各自实现容量上限和截断文案，ToolOutput 没有 Inline/Spilled 统一结果类型，也没有“完整 artifact 写成功后再提交引用事件”的原子契约。
- 证据等级: E2(静态读码确认截断点与 preview 入库路径；本地输出分布已量化)
- 阻塞: 等待 R-244 Tool Pipeline 结果阶段稳定并由 R-245 实施；当前先作为事实丢失缺陷登记。
- 验收: ①超过阈值的 bash/git/test_record/web 类结果完整原文进入 durable artifact，事件只存 preview+artifact_id+bytes+sha256+retrieval_hint；②重启后按引用取回内容与工具原始字节 sha256 一致；③artifact 写失败时不得提交成功引用事件，事件写失败时无引用 artifact 可由整理入口识别；④UI/模型明确显示结果已外置而非已丢弃；⑤read 的原文件 offset/limit 回读不重复复制；⑥现有工具权限与错误码不变。
- 优先级: P1

## D-351 亮色主题更新后仍不可读：D-348 发布验收失败 [fixing] (high)
- refs: D-348
- 复杂度: 小
- 复现: 用户安装应用内更新后再次打开亮色主题；底部运行状态栏仍是浅黄底白字，主对话历史工具记录接近白色，正文、代码与工具日志字号偏小，实际 WebView 无法清晰阅读。截图底栏版本为 v0.1.0 (660309d)，未包含 D-348 修复提交 dd28f9b。
- 影响: 亮色主题核心对话和运行状态不可读；首轮仅靠静态 token 断言关闭，且发布包未包含修复提交，造成源码状态与用户实际版本验收脱节。
- 来源: 用户更新后验收失败与截图（2026-08-14）
- 标签: 前端
- 验收: ①亮色实际 Chromium/WebView 下 assistant 正文、内联/块代码、实时及历史工具记录清晰可读；②运行中状态栏使用深色前景，自动放行、版本、模式均达到可读对比；③正文默认约 15px、工具/代码/日志不低于 13px，历史记录不再整块 opacity 淡化；④暗色主题无回归；⑤真实浏览器亮/暗截图与 computed style 证据通过；⑥交付版本号所示提交必须包含本修复，不能再以源码静态测试代替发布包验收。
- 优先级: P1
- 取活依据: override:用户明确报告更新后 D-348 验收失败，先于 R-241 修复实际可读性与发布边界。
- 进展: 源码与发布链路完成（2026-08-14）：提交 ddc3ae4 完成 statusbar-fg/statusbar-run-fg 语义前景、正文 15px、工具/代码/日志至少 13px、历史工具透明度修复；Chromium 亮暗主题截图与 computed style 已通过。pwsh 7.6.3 执行 scripts/verify.ps1 全绿，verification.json 绑定完整提交 ddc3ae464e6ba27e62049b002fdc1781f764e683；scripts/package.ps1 -Ack 3 -Publish 成功，正式 Release build-ddc3ae4 已发布。远端 tag、main、dev 均指向同一完整提交；安装包 kanzei-setup-ddc3ae4.exe 大小 12525045 字节，本地与 GitHub asset SHA-256 均为 628bab9d2d15a8f46b77da4d455193527de754da5a4799d8158c6396e2aa4995。待用户通过应用内更新或安装包升级后做亮色实机复验，再关闭缺陷。
- 阻塞: 用户：正式版本 build-ddc3ae4 已发布，源码与发布物完整性已验收，尚待用户安装后确认真实 WebView 亮色可读性。解除动作：安装 build-ddc3ae4，确认底栏显示 ddc3ae4，并复验正文、工具历史与运行状态栏。解除人：用户。
- observed_head: ddc3ae464e6ba27e62049b002fdc1781f764e683
- observed_worktree_hash: fnv1a64:bf9a4e6d2fa0000a
- recorded_at: 1786666434589
