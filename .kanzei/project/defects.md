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

- 阻塞: 2026-08-14 前置已满足:新版 build-9a06e05 已发布,工程面①②③早已交付并全量绿。剩验收④一条:装新版后打开 Memory 页,看 SOP 的总结质量/查看展示/产生时机三处是否确有改善(新排版 + 新沉淀门槛)。解除动作: 用户复查后确认改善即可关闭。解除人: 用户。

- priority: 

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
- 阻塞: 2026-08-14 前置已满足:新版 build-9a06e05 已发布并含 cbe768a 及后续。剩验收②后半一条实测动作:在桌面端跑一轮任务,中途点「停止」,立刻发一条新任务,看模型能不能复述被打断那轮做过的事(能复述=被打断轮的对话历史没整轮丢)。解除动作: 用户实测并反馈结果后补关验收②。解除人: 用户。
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
- 阻塞: 等待 R-244 Tool Pipeline 结果阶段稳定并由 R-245 实施。R-244 已于 2026-08-14 由用户定调列入主任务、主线串行做,依赖链有确定落点,不再是「等用户决定」。当前仍作为事实丢失缺陷登记(high),不单独修——在 R-244/R-245 的 Result Policy 与 spill 落点上一并解决。解除人: 依赖自然解除。
- 验收: ①超过阈值的 bash/git/test_record/web 类结果完整原文进入 durable artifact，事件只存 preview+artifact_id+bytes+sha256+retrieval_hint；②重启后按引用取回内容与工具原始字节 sha256 一致；③artifact 写失败时不得提交成功引用事件，事件写失败时无引用 artifact 可由整理入口识别；④UI/模型明确显示结果已外置而非已丢弃；⑤read 的原文件 offset/limit 回读不重复复制；⑥现有工具权限与错误码不变。
- 优先级: P1

## D-357 占位符门禁扫描删除行:archive_fill 回填后的清理提交被自己的门禁拒绝 [open] (medium)
- refs: R-227
- 复杂度: 小
- 复现: 2026-08-14 实测:R-227 存量 8 处占位符经 archive_fill 回填后,工作树 diff 里出现 8 行以 - 开头的旧占位符文本;此时若用结构化 git 工具提交 .kanzei/project/*.md,placeholder_id_gate 直接拒绝,理由是「tracker 文件 diff 出现 8 处占位符测试 ID」——而这些占位符正是本次提交要删掉的。本轮只能改用 shell 侧 git 绕过门禁才把清理提交出去(commit f8302f5)。
- 影响: 门禁把自己配套的清理通道(archive_fill)堵死:自举 agent 只能走结构化 git 工具,于是「按门禁要求回填占位符」这件事在 agent 手里永远提交不了,只有人在 shell 里绕过才行。R-227 已按验收关闭,但该矛盾会在下一次占位符清理时原样复发。
- 标签: 流程
- 根因: git.rs:504 `for line in diff.lines()` 对 staged diff 逐行扫描,不区分 +/- 前缀。删除一行占位符与新增一行占位符在门禁眼里完全一样。
- 验收: ①只含删除行的占位符 diff 放行(单测:diff 仅 `-` 行带占位符 → Ok);②新增行占位符仍被拒(既有断言保持绿);③diff 文件头 `--- a/xxx` `+++ b/xxx` 不参与判定;④同一 diff 里既删旧占位符又加新占位符时仍拒。
- 优先级: P2

## D-358 normalize apply 少报修复数且 dry-run 文案否认自身能力 [open] (low)
- refs: D-333 D-332
- 复杂度: 小
- 复现: 2026-08-14 实测:`kz req normalize --apply` 对 6 条归档重复「进展」字段真实执行了 dedupe_archived_fields 并写盘(apply 后再跑 dry-run = 0 finding,clean;git numstat 显示归档 12 删 6 增),但 apply 那次的输出仍是「6 finding(s), 0 fix(es)」,且没有「已修复」段。另一半:dry-run 对同样的条目打印「duplicate field 进展 — 需手动整理归档」,而 apply 明明能自动修。
- 影响: 工具少报自己的工作,并且用文案主动否认自己的能力。实际代价已发生:上一轮据「需手动整理归档」判定 D-333 验收③不可修,挂上「解除人=用户」的阻塞;本轮一条 normalize --apply 就修完了。
- 标签: 核心
- 根因: actions.rs:967 的 content 在归档 dedupe 循环(982-1004)之前就拼好了,循环里 push 进 fixed 的条目不再进输出;findings 的「需手动整理归档」文案是 apply 具备归档去重能力之前写下的,能力补上后没跟着改。
- 验收: ①apply 输出的 fix 计数与「已修复」段包含归档 dedupe 结果(单测:构造归档重复字段 → apply 输出 fix(es) >= 1 且列出条目 id);②findings 文案改为指向 apply 可修,不再说「需手动整理归档」;③dry-run 仍不写盘(既有断言保持);④非进展字段的 dedupe 只保首条这一取舍在文案里写明(D-180 两条内容不同的「验证」字段会因此丢一条)。
- 优先级: P3

## D-359 kz reopen CLI 不解析 --reason:强制必填的 reason 在命令行侧无法传,合法退路不可用 [open] (medium)
- refs: D-329 R-183
- 复杂度: 小
- 复现: 2026-08-14 实测:`kz req reopen R-183 --reason "..."` 报 "`reason` is required for reopen"。main.rs:1208 的 reopen 分支只从 positional 取 id,--reason 及其取值被 parse_tracker_flags 当成普通 positional 丢在后面,input["reason"] 从未被填。fix_terminal 分支(main.rs:1223)专门写了 --reason 解析,reopen/void_id 没跟上。
- 影响: reopen 是「fixing/doing 推不动时的合法退路」,强制 reason 是它的设计前提,而 CLI 侧永远给不出 reason = 这条退路在命令行完全不可用。实测后果:R-183 是 engine 自动认领却从未开工的僵尸 doing,清掉阻塞后立刻与 R-202 构成 2 个可执行 WIP,work next 判 wip_violation 禁止全线取活;想退回 todo 却退不了(update 拒绝 doing→todo 逆向迁移),只能把阻塞原样挂回去。
- 标签: 流程
- 根因: D-329 给 reopen/archive/void_id 等补了 positional id,但没补它们各自的必填参数;reason 的解析只在 fix_terminal 分支里单独实现,没有下沉成公共 flag。
- 验收: ①`kz req reopen <id> --reason "..."` 能落 reason 并把状态退回初始态(集成测试或 CLI 单测);②缺 --reason 时仍报错拒绝(不许空理由绕过);③--reason 解析下沉为公共 flag,fix_terminal 与 reopen 共用一处,void_id 等同族动作的必填参数一并核对补齐;④R-183 用修好的通道退回 todo,阻塞字段清空。
- 优先级: P2

## D-360 「被取得」标记退回推断:所有 doing 无条件标记,取得线不存在时代号渲染成问号 [open] (medium)
- refs: R-247 D-329
- 复杂度: 小
- 复现: 2026-08-14 用户截图:文档页分组视图「核心·13」里 5 条显示「● ? 被取得」(R-202/R-186/R-183/R-195/R-249),8 条 todo 条目无标记——被标的恰好是全部 doing 条目。而此刻 kzapp 引擎已于 20:18 退出(state.db-wal 已 checkpoint 清除、Get-Process kzapp 为空),没有任何线在持有任何条目;代号位还是个光秃秃的问号。
- 影响: 这个徽标存在的全部意义就是回答「被哪条线取得」,答不出「谁」时它是纯噪音;而现在更糟——它在引擎根本没运行时宣称 5 条需求有人在做。用户按它判断「哪些在推进」会得到完全错误的图像。现有反证测试 ui-runtime-smoke.mjs:1294 只构造了「排在队首但无人 claim」的非 doing 条目,正好绕开这条推断路径,所以一路绿着。
- 标签: 前端
- 根因: 两处叠加。①11-docs-list.js:205 `const defaultOwned = !explicitOwner && ["doing","fixing"].includes(entry.status)`——没有 claimed_by 就按状态推断「默认线持有」。这正是 parallel_lines_ui §1.2「被取得是事实,不是推断」明令删掉的东西(R-247 交付、D-329 复核过「全仓 grep isAgentNext 零命中」),推断值换了个名字回来了:isAgentNext 没了,defaultOwned 顶上。②11-docs-list.js:213 `code: line ? (codes.get(line.process_id) ?? "?") : "?"`——找不到对应线时仍然渲染徽标,只是把代号打成问号。
- 验收: ①无 claimed_by 的 doing/fixing 条目不显示「被取得」标记(断言补在 ui-runtime-smoke.mjs:1294 现有反证旁,构造 doing 且无 claim 的条目);②有 claimed_by 但该线不在 collaborationLines 里时,不渲染徽标(或明确渲染「取得线已离线」),不得出现代号为 "?" 的徽标;③真实 claim 仍正常渲染「● 代号 被取得」(ui-runtime-smoke.mjs:5529 既有断言保持绿);④全仓 grep 确认无第二处按状态推断持有的代码。
- 优先级: P2

## D-361 task 被算作非进展工具:整轮派子代理干活被判空转,连两轮鞭挞自停 [open] (high)
- refs: R-169 R-174 R-076
- 复杂度: 中
- 复现: 2026-08-14 用户报告「鞭挞会被子代理终结」。代码核实成立:kanzei-harness/src/auto_run.rs:28 的 NON_PROGRESS_TOOLS 常量里含 "task";has_progress_tools(同文件 32-36)的判据是「本轮至少有一个不在该表里的工具」才算有进展。于是主代理把活整轮派给 task 子代理时,画像里只有 task 一项 → has_progress_tools=false → decide() 的 `no_action = ctx.steps <= 1 || !has_progress_tools(ctx.tools)` 为真 → 第一次返回 Nudge(rounds+1),紧接着第二次就 stop_with(AutoStopReason::NoAction)。连着两轮派子代理,鞭挞自停。
- 影响: 与「用子代理分担」这条正路直接对冲:模型越守规矩地委派,鞭挞越快自杀,而且停止原因报的是 NoAction(空转)——对用户是误导,那轮实际干了活。子代理越好用,这条越疼。
- 标签: 核心
- 根因: 主轮的工具画像只统计主 conversation 的本轮消息切片(run.rs:1653 `summarize_tools(&summary.messages[prior.len()..])`),子代理内部调用的 read/grep/edit 全在子代理自己的消息列表里,不进主轮画像——主轮能看见的只有一次 task 调用与它的返回。而 task 又被登记为非进展工具,于是「把活派出去」在鞭挞眼里等价于「什么都没干」。task 当初进这张表大概是防「反复派子代理查东西却不落地」的空转,但代价是把正常委派也一起打死了。
- 验收: ①整轮只调 task、且子代理确有实质工具调用时,decide 不判 NoAction(单测:构造 tools=["task"] + 子代理画像有 edit,断言不返回 Stop(NoAction));②子代理的工具画像上卷进主轮画像(或等价机制,如 task 结果携带子代理 tools 摘要),口径写进 auto_run 的模块注释;③真正空转的轮仍判 NoAction——task 派出去但子代理自己也没动作时,反证测试断言照旧 Nudge/Stop;④NON_PROGRESS_TOOLS 其余成员语义不变,既有鞭挞测试(harness auto_run)全绿。
- 优先级: P1

## D-362 文档页列表行内元素流式排列:可选徽标把优先级/复杂度/标题三列推得行行错位 [open] (low)
- refs: D-360
- 复杂度: 小
- 复现: 2026-08-14 用户截图(文档页分组视图「核心·13」):同一列表 13 行,优先级徽标出现在 7 个不同横坐标,标题起点 13 行几乎各不相同——R-238 的 P2 在 x≈101,R-195 的 P2 在 x≈330,相差两百多像素。肉眼扫不出「哪些是 P0」,得逐行读。
- 影响: 列表的价值是横向扫读(一眼看出优先级分布、哪些被阻塞),列错位之后只能逐行读,等于把列表退化成一堆句子。条目越多越明显。
- 标签: 前端
- 根因: 11-docs-list.js 的 doc-row 是纯流式行:勾选框 → 被取得徽标(可选)→ 批次进度格(可选,格数还随批次总数 1~12 变宽)→ 阻塞徽标(可选)→ 待澄清徽标(可选)→ 优先级 → 复杂度 → 标题,全部 appendChild 顺次排列。可选元素的有无与宽窄直接把后面所有列往右推,行与行之间没有任何列对齐机制。侧栏窄、行少时看不出来,文档页宽列表 13 行摊开就全散了。
- 验收: ①文档页列表的优先级/复杂度/标题三列在同组内左对齐(grid 固定列宽或等价方案),可选徽标的有无不影响后续列起点;②侧栏窄列表形态不回归(它本来就该紧凑,不强求同一套列宽);③批次格宽度随格数变化时不破坏对齐;④ui-runtime-smoke 与 ui-a11y 冒烟保持绿。
- 优先级: P3
