# Defects

## D-332 治理控制面三硬伤:tracker lifecycle fail-open、存量污染无 normalize 修复通道、验证 ceremony 循环(两份运行评估合并) [fixing] (high)
- refs: R-233 R-210 R-212 R-209 R-200 D-267 D-330 D-331 R-208 D-209
- 复杂度: 大
- 复现: ①**tracker lifecycle fail-open**:requirements.md 存量 `[open]`(R-208 等)不被 docstore.rs:1084 剥离(不在合法枚举)→ lifecycle_status 解析为空字符串;work.rs:482-497 候选筛选只查 `!terminal.contains(status)`,空状态不在终态枚举 → 被当作「非终态、未阻塞、可执行」,已关闭/污染的条目可能被 work next 重新取活。②**存量污染无合法 repair 通道**:R-233/R-234 重复 `优先级` 字段,D-330 只防新增;direct tracker write 被拒、raw_delete 只删游离行 → 合法状态不可达(Raw write denied + Official API incapable = 合法状态不可达)。③**验证 ceremony 循环**:test → stage → commit → fmt 拦 → cargo fmt → source hash 变 → restage → 测试证据过期 → retest → test_record → tests-archive 变 → restage → commit(R-233 B2 实测完整发生一遍;每步单独合理,组合无拓扑排序)。④**test evidence 靠 mtime**:fmt 后纯 non-semantic 变换也强制重测,Harness 无法自行判定是否需要重测。⑤**work next 裁决后无 decision_locked**:Agent 反复讨论已冻结的决策(R-183 场景),无控制面事实变化也重复推理。
- 影响: 治理系统对自己最关键的控制状态采用 permissive parsing——已完成需求可能被重新执行(重复工作/错误工作);修复动作不存在导致脏数据永久积压;验证 pipeline 组合摩擦让每条提交多 5-6 次机械 tool call;Agent 把 token 花在反刍已冻结决策上。两份评估一致认定:fail-open 是当前最值得优先修的治理缺陷(P0),其余是 P1 摩擦。
- 来源: 用户消息(2026-08-13 两份运行评估合并:16:52 首评 + 16:54 补评「两个结合起来」),第一份结尾明确指令「把这个分析登记成最新的缺陷,然后把这个缺陷排序成当前的第一个任务,解决并发版」
- 标签: 核心
- 进展: B4 完成(2026-08-13):test evidence 绑定 source hash——①git.rs 新增 staged_source_fingerprint(同步):只对暂存源码路径的 diff 段求 fnv hash,tests.md/tracker 等托管文档不改变源码指纹;②test_record 收尾(status!=running)写入「源码指纹」字段,execute 自动取暂存指纹;③source_test_gate 优先比指纹:记录指纹与当前暂存源码指纹一致 = 背书成立(不再被 test_record 自己写 tests.md 的 mtime 误伤),不一致拦截并点名;指纹缺失(旧记录/非 git)退回 mtime;④既修:coverage_intersects 测试 write_record 改用实时时钟消除跨秒竞态(既有隐患)。测试:source_test_gate_prefers_fingerprint_over_mtime(指纹一致放行/改源码后不一致拦截)、staged_source_fingerprint_ignores_non_source_paths(非源码暂存指纹为空)新增;git 18 passed 连续 6 轮全绿,kanzei-tools 307 passed,clippy -D warnings 干净。B4 提交后 B5:test_record 不抖 staged set + decision_locked + work lease/turn 解耦。
- 验收: ①未知/畸形 lifecycle(requirement 出现 `[open]`/`[fixed]` 等)解析后进入 INVALID,`work next` 永不选中,integrity 错误明示条目与非法值(有测试:构造 `[open]` 污染需求,断言 work next 不选它且报 integrity 错误);②存在统一 repair surface(`tracker normalize` 或等价):能机械、幂等、dry-run-first 修复 invalid lifecycle、duplicate fields、title/status mismatch、multi-marker、archive/active mismatch;存量 R-208/R-233 污染用工具收敛(有测试);③验证 pipeline 重排:fmt 在 test 之前执行,commit gate 不再在提交时第一次暴露 fmt 问题(有测试断言 commit 前已 fmt);④test evidence 绑定 source/staged hash,不再纯靠 mtime;fmt 后若仅 non-semantic 变换,Harness 判定可复用或自动重测(有测试);⑤test_record 写入不再让 staged set 抖动(Harness 自动纳入 expected set 或独立 ledger)(有测试);⑥work next 裁决后给 decision_locked 信号,无新 control-plane fact 时不再被重新讨论(结构化证据);⑦work lease 与 turn granularity 解耦:强约束 = 同时一个 mutation WIP,做完释放后可继续下一项(调度语义明确,有测试);⑧全量 workspace 测试绿,发版。
- 优先级: P0
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-332
- 批次: 4/6
- observed_head: 7f3822b37f847661673732dc8df1154d421aa1f8
- observed_worktree_hash: fnv1a64:d6598f3a58c54d6e
- recorded_at: 1786613068558

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

## D-209 对话落库粒度太粗(用户反馈,具体维度待澄清) [open] (medium)
- refs: D-208 D-185
- 原始描述: 用户 2026-08-09 原话"落库对话粒度太粗"(与活动栏回放问题同时反馈)。
- 机制现状(供收敛方向): ①对话持久化是 `conversation.updated` 事件整份 messages 快照替换,轮内不落盘,恢复只能回到轮边界;②工具轨迹 run.trace 只在收尾 flush 一次(D-179 补了停止路径,但仍是整轮一包);③episodes 是轮级摘要。三层都是"轮"粒度,轮内的中间态(改到一半、流式输出中断点)不可恢复、不可检索。
- 待澄清: 用户所指的具体痛点——候选:a) 历史恢复丢轮内进度;b) 回放时一整轮的工具轨迹糊成一批看不出先后;c) 检索/引用历史时只能按整轮拿、拿不到单条消息;d) 其他。按 D-205 教训不代用户猜死,取活前先确认。
- 验收: 待澄清后按维度改写;暂置:对话落库粒度支持轮内增量(或用户确认的等价目标),恢复/回放/检索三条消费路径至少一条受益并有实测。
- 证据等级: E2(用户反馈,机制已核实)
- 优先级: P2
- 标签: 后端

- 进展: 2026-08-10 取活时仍待澄清:候选 a)恢复丢轮内进度 b)工具轨迹糊成一批 c)只能按整轮拿消息 d)其他——机制现状已核实(三层都是轮粒度),按 D-205 教训不代用户猜死;本轮跳过,待用户确认维度后改写验收再取活。

- 阻塞: 用户: 2026-08-09 用户原话「落库对话粒度太粗」的具体痛点维度待指认(候选 a 恢复丢轮内进度 / b 工具轨迹糊成一批 / c 只能按整轮拿消息 / d 其他)——按 D-205 教训不代用户猜死。解除动作: 用户在 a/b/c/d 中指认具体痛点(或给出等价目标),再改写验收取活。解除人: 用户。

## D-333 存量 tracker 污染收敛:活动区双优先级字段、归档区双终态标记、重复进展字段(D-330/D-331 修复前残留) [open] (low)
- refs: D-332 D-331 D-330
- 复杂度: 小
- 复现: normalize dry-run 全仓扫描实测检出(2026-08-13,CLI kz req normalize):活动区 R-234/R-235 各带重复「优先级」字段(D-330 修复前的存量);归档区 R-201/R-198/R-199/R-213 标题为 [open][done] 双终态标记(D-331 修复前的存量,parser 只剥最后一个 done,[open] 残留标题);归档区 R-225/R-226 重复「进展」字段。当前会话引擎跑旧编译,工具通道(req update)对存量双字段无法去重(update 只覆盖首个匹配),CLI normalize apply 写盘被托管围栏拦截。
- 影响: 重复字段让 UI 显示歧义(哪个优先级生效未知);归档双终态标记污染统计与审计;这些是 D-330/D-331 修复前的存量,合法修复面 normalize/fix_terminal 已存在但需引擎重启后执行。
- 来源: self-found(D-332 B3 存量收敛时 normalize 扫描检出)
- 标签: 核心
- 验收: ①R-234/R-235 各只剩一个「优先级」字段,值与首个一致(有测试或工具输出证据);②归档 R-201/R-198/R-199/R-213 标题只剩单一终态标记,残留的 open 标记被剥离(有测试或工具输出证据);③R-225/R-226 归档重复「进展」字段收敛;④全程走专用工具(normalize apply / fix_terminal / req update),无手改 markdown。
- 优先级: P1
