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

## D-364 托管文档并发写丢条目:kz req add 报 added 成功但条目被并发写者整体覆盖消失 [fixing] (high)
- refs: R-138 R-177 R-182 M-012 D-267
- 复杂度: 中
- 复现: 2026-08-15 04:00-04:10 实测,当场命中两次。环境:kzapp(pid 38688)内有自举轮正在写 .kanzei/project/(文件 mtime 实证:conventions.md 04:06:53、tests.md 04:09:02、requirements.md 04:09:13 相继被写),同时在主根用 kz req add 登记条目。第一次:add 输出 added R-254,紧接着的下一条 add 又被分配到 R-254,复核 requirements.md 发现前一条整体消失(标题、全部字段一并没了,不是截断);第二次同型:输出 added R-257 后,下一条 add 又拿到 R-257,前一条消失。改成 add 后立即 Select-String 复核 + 重试才落住(最终补登为 R-255 与 R-258)。
- 影响: 静默数据丢失,而且是最坏形态:工具明确回 added <id> 并给出编号,调用方(人或 agent)据此认为登记完成继续往下走,甚至在别处 refs 这个 id,而条目根本不在文件里。自举并发是本仓既定玩法(R-177/R-182 的前提),这个丢失面对每一次 桌面端自举轮 + 外部 agent 登记 都成立;同一 id 被二次分配还会撞上 M-012 的完整性门禁(活动与归档同 id 会拒绝所有 tracker 写)。本条不是理论风险,是本轮登记过程中真实发生的两次。
- 来源: self-found(2026-08-15 登记第二轮巨石拆解条目时当场命中)
- 标签: 核心
- 根因假设(未定位,待读码): docstore 的 读全文-改-整体回写 不是跨进程原子的,或 R-138 FileLock 的加锁范围没覆盖 桌面端进程 与 kz CLI 进程 这两个写者(锁只在单进程内生效,或只锁单个文档路径而 id 分配读的是另一份快照)。需确认:①FileLock 实际加锁位置与持有时长;②next_id 计算与写盘是否在同一临界区;③桌面端写托管文档走的是不是同一条 docstore 路径。
- 验收: ①并发场景有确定性回归测试(两个进程同时 add),后写者不得覆盖先写者;②失败时工具必须报错,禁止回 added——宁可失败也不能假成功;③id 分配与写入在同一临界区完成,不出现同 id 二次分配;④桌面端自举轮在跑时,外部 kz req/defect add 能稳定落住(实测,不是只跑单测)。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-364
- 批次: 2/2
- 进展: B2 完成:端到端回归 4 条(crates/kanzei/tests/d364_concurrent_doc_add.rs)——①围栏持锁窗口内 CLI add 等待后落住编号唯一(验收④机械形态);②窗口超 CLI 3s 锁预算时 CLI 明确报错退出、绝不回 added(验收②);③双 CLI 进程真并发 add 编号互异条目齐全(验收①后写不覆盖);④真 BashTool 围栏窗口内并发 CLI add 不被误回滚(走真实 acquire_managed_locks 管线)。反证已做:禁用围栏持锁后④精确复现 D-364 丢失([managed-files] BLOCKED AND ROLLED BACK, requirements.md 被回滚)——测试咬得住回归。kanzei-tools 250 绿 + d364 e2e 4/4 绿(T-1786743149)。待办:全量 + 逐条验收对照 + 关闭。
- observed_head: 6b8799e1cf8345383800f4b1c48df4f1c8b09687
- observed_worktree_hash: fnv1a64:7d352a5c997bbee0
- recorded_at: 1786743164964

## D-365 R-207 worktree 下沉停在中间态:processes.rs 仍留 19 处 wt:: 转发壳,两层抽象长期并存 [open] (medium)
- refs: R-207 R-254 R-177
- 备注: 修复动作可并入 R-254 的内容②,本条独立登记是为了让 R-207 的收尾缺口在缺陷队列里可见,不被"R-207 已 done"掩盖。
- 复杂度: 小
- 复现: 2026-08-15 dev@f09242c 实测:Select-String -Path crates/kanzei-app/src/processes.rs -Pattern wt:: 命中 19 处。worktree_target/worktree_status/branch_exists/rev_parse/git_worktrees/validate_worktree_path 等函数体只是转调 kanzei_tools::worktree 的同名实现,代码注释自述"实现已下沉 kanzei-tools::worktree(R-207)"。R-207 在归档里状态是 done。
- 影响: 下沉的收益(桌面与 CLI 共用一份工作树实现)只兑现了一半:实现虽在一处,调用侧仍隔着一层桌面私有壳,改工作树行为要先判断改哪层;新代码不知道该调壳还是调下沉实现,两条路都能编译;processes.rs 的 1628 行生产码里这一层是纯噪声,推高了 R-254 拆解的读码成本。
- 来源: self-found(2026-08-15 第二轮巨石扫描读码时发现)
- 标签: 核心
- 验收: ①processes.rs 中 wt:: 转发壳数量为 0(机械核验 grep),调用点直接用 kanzei_tools::worktree;②worktree_tests.rs 全绿 + kanzei-app 全量绿;③若某个壳确有存在理由(如桌面侧要做额外的路径规范化),在删壳批里写明理由并保留,不允许"看着像转发就删"。
- 优先级: P2

## D-366 MemoryStore 与 MemoryIndex 检索边界未切净:排序实现在 store,index 反过来调 store.search 取 BM25 [open] (medium)
- refs: R-255 R-150 docs/design/memory_control_plane.md
- 备注: 修复由 R-255 第三刀承载,本条独立登记是为了把"边界在哪"这个判断先固定下来,避免 R-255 执行时临时决定。
- 复杂度: 中
- 复现: 2026-08-15 dev@f09242c 读码:crates/kanzei-memory/src/memory/store.rs L960 的 MemoryStore::search 里实现了 BM25 + 状态加权 + 采纳率决策加权 + active 排序 + 命中追踪 + snippet;而 crates/kanzei-memory/src/memory/index.rs(1204 总/661 生产)L222-227 的 Tier1 又反过来调 MemoryStore::project(root).search(...),其文件头 L14 与 L222 的注释都写明"store.search 已做 bm25 + 采纳率决策加权 + active 排序"。也就是 Index 是检索门面,真正的排序住在 Store 里。
- 影响: ①排序调权要改 store,但读代码的人会先去 index 找,认知落点与实现落点错位;②index 想换检索后端(向量/混合)时被 store 的 SQL 实现绑死;③这是 R-255 里最难迁出的一块——store.rs 2073 行生产码中检索是唯一有下游依赖的部分,边界不先定清楚,第三刀会卡住;④记忆研究要做召回实验时,policy(怎么排)与 storage(怎么存)改在同一个文件里,无法独立归因。
- 来源: self-found(2026-08-15 第二轮巨石扫描读码时发现)
- 标签: 核心
- 验收: ①BM25 与状态/采纳率加权的实现只出现在检索侧一处(机械核验 grep),store 不再持有 ranking;②index 与 store 的依赖方向单一,不存在 index 调 store 再由 store 做排序的回环;③同一组 query 在改动前后 top-k 命中集合一致(给出对照);④memory crate 全量绿。
- 优先级: P2

## D-367 主根与工作树根的硬不变式只靠文件头注释站岗:类型上都是 PathBuf,传反了编译器不报错 [open] (medium)
- refs: R-254 R-177 R-182 D-176 D-267
- 备注: 修复由 R-254 的内容③承载;本条独立登记是因为它是一条独立成立的结构性风险,不随 R-254 是否拆解而消失。
- 复杂度: 中
- 复现: crates/kanzei-app/src/processes.rs 文件头 L3-18 用一整段 //! 注释锁定不变式:ProcessHandle.project_dir 与 origin_project 恒为主根,执行工作树只由 worktree_path 承担;注释自己逐条列出违反后果——p{n} 进程编号按 project_dir 分桶,存成 worktree 后每棵树各自从 p1 开始立刻撞车;process_update/process_close 用 project_dir 反推 root 开 state.db,存成 worktree 会把库落进工作树,线一关连库一起没;state.rs 的 process_info 用 project_dir 算 session_id,存成 worktree 等于给同一条线换身份串,会话历史集体失联(D-176 红线)。但类型上二者都是普通字符串/PathBuf,传反了 rustc 一声不吭。
- 影响: 后果全是运行时才暴露的重症(编号撞车、state.db 落错位置随工作树一起删、会话身份断裂),而防线是"改本文件前先读这一段注释"。同族现场已经发生过:D-267/R-182 里发现式取根命中了 worktree 中被 checkout 出来的 .kanzei 分支副本,两棵树相隔 10 秒各跑 kz defect add,各自在自己的副本上算 next_id,都拿到 D-267。注释挡不住这类错误,类型可以。R-254 会大幅搬动这个文件,搬动期间正是最容易传反的时候。
- 来源: self-found(2026-08-15 第二轮巨石扫描读码时发现)
- 标签: 核心
- 验收: ①主根与工作树根是两个不同类型(newtype),互相传反编译不过——给出实证(被注释掉的反例 + 编译错误原文,或等价断言),不接受"改完看着对";②processes.rs 文件头那段注释从"改前必读的纪律"降级为"设计说明",即注释没了也不会写错;③进程编号、state.db 落点、session_id 推导三条行为零回归(worktree_tests.rs 全绿 + 实跑一次建线到关线闭环)。
- 优先级: P2
