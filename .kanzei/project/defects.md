# Defects

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

## D-207 取活顺序所见非所得:视图排序与优先级徽章都不参与取活,界面零提示 [open] (medium)
- refs: R-054 R-111
- 复现: 2026-08-09 用户反馈"取需求和缺陷的顺序看不懂了,因为侧边栏可以调整顺序"。机制现状:①取活真序 = md 文件物理顺序从上到下(dev prompt "Scan from top to bottom",schedule_entries 只后置阻塞项、不改文件);②侧栏拖拽(manual 排序+无筛选时)经 docs_update reorder **写回文件**,真的改变取活顺序;③侧栏另有 id/状态/复杂度/优先级四种视图排序(main.js filterRequirements),**只改显示**;④优先级徽章 P0~P3 完全不参与取活(prompt 明言 "Priority labels are background info, not the ordering")。
- 影响: 选了任何视图排序后,用户看到的顺序与 agent 取活顺序完全无关,界面没有任何提示;优先级徽章满屏,人天然以为按 P0→P3 取活,实际一票不投——近期把 5 条需求升 P0(576d725)在取活上零效果,用户的调度意图静默落空。三种顺序语义(文件序=取活序/视图序/优先级暗示序)混在同一个列表上,只在"manual+无筛选"时才重合。
- 根因: R-054 定了"文件顺序即开发顺序"的单一真源,后续视图排序与优先级徽章叠上去时,没有同步交代它们与真源的关系;取活规则只写在 prompt 里,UI 侧无任何投影。
- 验收: ①非 manual 排序视图下,侧栏显式提示"当前显示顺序≠取活顺序"(或等价视觉语言);②有一处能看到真序:取活预览(下一条会被拿的条目有标记,阻塞项显示跳过原因)或一键切回文件序;③优先级二选一——要么参与取活(prompt 与 schedule 同步改,并写清与文件序的优先关系),要么在 UI 上明示"仅参考,不影响取活";④用户复查确认能看懂"agent 下一个会拿哪条、为什么"。
- 证据等级: E1(代码四处机制实证 + 用户反馈)
- 优先级: P1
- 标签: 前端

- 进展: 2026-08-09 部分交付(①取活焦点可视化、②拖拽禁用提示);本轮补验收③:优先级二选一选 B(UI 明示)——侧栏筛选 title 与行内徽章 title 均注明「仅参考,不影响取活」(index.html/main.js renderDocList badge.title),i18n 两条词条,冒烟断言全绿。2026-08-10 用户反馈验收④未过:blocked doing 被渲染成「运行中」——computeAgentFocus 修复①:active 排除 entry.blocked(12-docs-pages.js),四条冒烟全绿(c0864b0)。2026-08-10 再反馈:active 集合无意义,退化为单条——computeAgentFocus 改为取活序第一个可执行的 doing/fixing 单条 id,11-docs-list.js has()→===,冒烟新增多条 doing 只标取活序第一条断言,ui-runtime 243 invoke 全绿。①②③已交付,验收④(用户重建 kzapp 后复查确认能看懂「下一个会拿哪条、为什么」)待用户侧动作。

- 阻塞: 验收④用户复查:ui 资源打包进 exe,需用户跑 release.ps1 重建 kzapp 后实际查看侧栏取活焦点并确认能看懂;解除人=用户(重建+复查后确认即关闭)。

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

## D-239 取活口径漂移复现追踪:伪阻塞/伪可执行/挂起无载体 [fixing] (medium)
- 复现: 2026-08-10 复盘取活时发现三处阻塞/挂起口径漂移:①R-151/R-162~R-167 把非阻塞内部依赖(R-150/R-161 等,解除权在 agent)写进「依赖」字段,list 据未完成依赖判 blocked,调度器整批跳过,需求队列后半截系统性锁死;②R-157 实质卡在 D-235(conventions.md 无专用写入通道,edit 被 ruleset 拒绝),却无阻塞字段,以 doing 形态占可执行 WIP 名额、实际推不动;③R-101 用户 08-09 挂起只写在进展里,状态 todo 无阻塞字段,取活器会误取。
- 根因假设: §1.1 阻塞口径只在「触碰条目时」顺带复核,无周期机械核对;2026-08-09 WIP 口径修订后历史条目未回扫(R-151 的阻塞恰在口径修订期写入)。
- 进展: | 2026-08-16 第十一轮复核(D-283 关闭后、R-213 关闭+R-235 新增后取活前):本轮变化——D-283 由 open 转 fixed(经 R-197 交付补关闭书据,commit 6f2aed0),R-213 关闭(done,provenance 校验+引擎代填,commit 65ac9cd),R-235 新增(todo 无阻塞=未开工真可执行),D-330(他会话登记)open 无阻塞未进 fixing;全量核对:requirements 侧 R-174/R-059/R-101/R-135 用户阻塞仍合法,R-176 依赖推导 blocked 合法(R-173/R-175 未完成),R-200 权限阻塞合法(解除人=用户),R-235 无阻塞未开工;defects 侧 D-209/D-207/D-204/D-278/D-280 用户阻塞仍合法,D-330 open 无阻塞未进 fixing,D-239 自身 open 不占 doing;doing 仅 R-174/R-101 均带具名解除人,无伪可执行 doing,无挂起无载体。本轮无同类漂移复现。验收③已达成待用户确认,本条按验收原文转阻塞(解除人=用户)。
- 验收: ①当前三条已修,req get 各条目可见清理后口径(证据:R-101/R-157 有合法阻塞字段,R-151/R-162~R-167 依赖字段为空、进展注明解锁条件);②此后每轮取活前复核阻塞/依赖字段口径,若再次出现同类漂移(伪阻塞、伪可执行 doing、挂起无载体)→ 确认为规则缺陷,升级修 §1.1/取活器并记根因;③连续 10 轮无同类复现 → 用户确认后关闭本条。复核已累计 3 轮(2026-08-13 ×2、2026-08-14 ×1),无同类复现。
- refs: R-101 R-157 R-151 R-162 R-163 R-164 R-165 R-166 R-167
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-239
- 阻塞: 验收③已达成:连续 10 轮无同类复现(第 10 轮 2026-08-16),验收原文要求「用户确认后关闭本条」。解除动作: 用户确认验收③达成后关闭本条。解除人: 用户。
- observed_head: 45fd276e9ac4ac6a23c0027b801f95d6c6c3fe4f
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786598176958

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

## D-278 子代理面板打开后无就绪状态:侧边栏小窗口看不到「子代理可用」文案(设置页有,面板没有) [fixing] (medium)
- content: 侧边栏 ◉ 按钮打开的子代理面板(#agent-panel)只有「运行中/已完成/已关闭」三个分区,没有任何就绪/可用状态信息。设置页 fast 行已正确显示「✓ 子代理就绪(qwen3.5:4b)」(fast_model_status 返回 ready=true),但面板打开后用户看不到子代理是否可用——缺环时(Ollama 未装/服务未起/模型未拉)也无法从面板感知。
- label: 前端
- priority: P2
- severity: medium
- 修复: 面板头部加状态行:打开面板时 invoke fast_model_status 并按 managed/ready 显示与设置页同源的文案(就绪/未安装/服务未运行/模型未拉取/外部 provider)。文案计算抽成共享函数 fastStatusText(s) 供设置页与面板同源,避免两处漂移。
- 复现: 1) 打开设置页确认 fast 行显示就绪(或缺环文案);2) 点侧边栏 ◉ 打开子代理面板;3) 面板内只有空的三分区,无任何就绪/可用文案。
- 根因: R-174 子代理面板只消费 RunEvent 渲染运行记录,未接入 fast_model_status 就绪数据源;就绪状态只在设置页(refreshFastStatus)渲染过一次,面板打开时无独立查询与展示。
- 进展: 修复完成:①index.html 面板头部加 #agent-panel-status 状态行(role=status);②06-agent-panel.js 新增共享函数 fastStatusText(s)(就绪/未安装/服务未运行/模型未拉取/外部 provider 文案分支)与 refreshAgentPanelStatus(打开面板时 invoke fast_model_status),面板打开即刷新,并监听 kz:fast-setup 事件保持同步;③16-settings.js refreshFastStatus 改为复用 fastStatusText,设置页与面板文案同源不再漂移;④style.css 加 .agent-panel-status 样式(warn-text 复用)。验证:node --check ×2、frontend_check(花括号完整)、ui-runtime-smoke 21 项通过、cargo test -p kanzei-app 122 passed(T-1786476071)。残余:ui 资源打包进 exe,需用户重建 kzapp 后目视确认面板打开显示就绪文案。
- status: fixing
- 阻塞: 外部阻塞(验收确认):ui 资源打包进 exe,当前运行中的 kzapp 是旧构建,面板就绪状态行无法目视。解除动作:用户跑 release.ps1 重建 kzapp 后,打开侧边栏子代理面板确认显示「✓ 子代理就绪(qwen3.5:4b)」(或缺环文案),确认后关闭。解除人:用户。

## D-280 「回到最新」按钮悬浮位置错误:相对 #main 硬编码 bottom:92px,被输入区遮挡 [open] (medium)
- content: 「回到最新」按钮(#jump-latest)悬浮位置错误:它用 position:absolute 相对 #main 定位,bottom:92px 是硬编码,而 #composer 实际高度约 120px+(padding 24 + textarea 3 行 + composer-bar),按钮被压在输入区里;附件条/继续文案面板展开时被遮挡更严重。
- label: 前端
- priority: P2
- severity: low
- 修复: 把按钮移进 #messages 内部并给 #messages 加 position:relative,按钮改为 right:22px;bottom:14px 相对消息区右下角悬浮,composer 高度变化不再影响;删除已失效的 #messages + #jump-latest 兄弟选择器规则。
- 复现: 1) 长对话向上滚动,出现「回到最新」按钮;2) 按钮落在输入框区域内/紧贴输入框,而不是悬浮在消息列表右下角。
- 根因: #jump-latest 是 #messages 的兄弟节点,包含块是 #main(position:relative),bottom:92px 相对整个主视图底部,与 composer 真实高度不耦合。
- 进展: 修复完成:①index.html 把 #jump-latest 从 #messages 兄弟位移进 #messages 内部(empty-state 之后);②style.css #messages 加 position:relative,#jump-latest 改为 right:22px;bottom:14px(相对消息区右下角悬浮,与 messages padding 对齐),删除已失效的 #messages + #jump-latest 兄弟选择器。验证:frontend_check 花括号完整、ui-runtime-smoke 21 项通过 0 错误(T-1786476379)。影响范围:仅对话视图「回到最新」按钮定位,JS 引用(getElementById)不受父子结构影响。残余:ui 打包进 exe,需重建 kzapp 后目视确认按钮悬浮在消息列表右下角、输入框上方。 ‖ 2026-08-12 05:20 上一版修复把界面搞崩了(用户装 build-92b0bf1 后实测):把 #jump-latest 移进 #messages 内部,而 renderRecoveredMessages(15-views-misc.js:198)与 clearChat(:366)都做 messages.innerHTML = 空串——一清就把按钮删掉,之后任何滚动或渲染触发 updateLatestButton(05-chat-render.js:12)就抛 Cannot read properties of null (reading classList),表现为「历史消息恢复失败」「创建并行线路失败」两条红错 + 一条裸 TypeError。真正的修法是给消息区套一层不滚动的定位容器:新增 #chat-area(flex 列 + position:relative + min-height:0),#messages 在里面负责滚动,#jump-latest 是它的**兄弟**而不是孩子——既不被 innerHTML 清掉,也不跟着内容滚走(放在滚动容器内部会随内容漂移,这是上一版没暴露的第二个问题)。另把 updateLatestButton 改成拿不到按钮就跳过,后续再犯只是按钮不更新,不会拖崩整条链路。回归护栏:ui-runtime-smoke 加结构断言(直接查 index.html 源文本,#messages 开闭标签之间不许出现 jump-latest)+ 两条清空路径不得抛异常;已用临时回退实测该断言确实会红。
- status: fixing
- 阻塞: 外部阻塞(验收确认):ui 资源打包进 exe,当前运行中的 kzapp 是旧构建,按钮新位置无法目视。解除动作:用户跑 release.ps1 重建 kzapp 后,长对话向上滚动,确认「回到最新」按钮悬浮在消息列表右下角、输入框上方(不再被遮挡),确认后关闭。解除人:用户。

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

## D-319 WebView2 当前环境 DevTools 端口不监听:e2e-smoke connectOverCDP 20 秒超时(参数已传入但不绑定) [fixing] (medium)
- 复杂度: 中
- 复现: 2026-08-16 D-289 实测验证中发现:无论 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 环境变量还是 KANZEI_E2E_CDP 注入路径,WebView2 进程命令行均带上 --remote-debugging-port=<port> --remote-allow-origins=*(进程命令行实证),但端口 20 秒不监听、user-data-dir 无 DevToolsActivePort 文件、进程树完整(renderer/gpu/network 均在)、会话与用户 kzapp 同为 Session 1、无策略禁用(注册表 HKCU/HKLM EdgeWebView/Edge 均空)。对照:同参数字符串起 Edge --headless,1 秒即监听。结论:WebView2 在当前机器/环境不启动 DevTools 端口,与参数注入路径无关。
- 影响: R-101 e2e-smoke 基座在自举环境无法实测 connectOverCDP(端口不监听→20 秒超时→FAIL)。这独立于 D-289 的 origin 白名单修复——即使端口能监听,D-289 也是必需的(M111+ 拒非白名单客户端);但端口不监听会让 e2e-smoke 永远失败。
- 来源: self-found(D-289 实测验证中发现)
- 标签: 流程
- 优先级: P2
- 进展: 2026-08-16 取活诊断(9 轮实验,证据链完整)。已排除:①注入通道——环境变量 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 与 KANZEI_E2E_CDP additional_browser_args 两条路都传参成功(msedgewebview2.exe 命令行实证含 --remote-debugging-port/--remote-allow-origins/--remote-debugging-address);②参数格式——同参数字符串起 Edge(含非 headless)1 秒监听;③策略——HKCU/HKLM EdgeWebView/Edge DeveloperToolsAvailability 全空;④AppContainer/容器——whoami 无 AppContainerSid,Session 1 正常;⑤日志——--enable-logging=stderr --v=1 无 devtools 相关输出;⑥版本——WEBVIEW2_FIXED_VERSION 指定 151.0.4129.59 未生效(Tauri 仍用 78),151.0.4129.78 签名有效、安装完整;⑦进程树——renderer/gpu/network 完整,webview 已创建但 browser 进程不绑定端口、无 DevToolsActivePort 文件。结论:WebView2 Runtime 151 在当前机器不启动 DevTools HTTP 服务,与参数注入/代码路径/系统策略均无关。排除后剩余变量是 WebView2 Runtime 本身或其与系统环境的交互(需重装/更新 runtime 或换环境验证,或改用 WebDriver/tauri-driver 路线)。阻塞:解除人=用户。
- 阻塞: WebView2 Runtime 151 在当前机器 DevTools 端口不绑定(9 轮实验证据链,见进展)。解除动作:①用户重装/更新 Microsoft Edge WebView2 Runtime 后重跑 e2e-smoke;或②用户提供 WebView2 DevTools 正常的环境验证;或③用户拍板改 WebDriver/tauri-driver 路线(不在本条范围内)。解除人:用户。

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

## D-323 R-199 第 4 处前端私有否决残留:暂停恢复路径档位不匹配时静默不调度续跑,引擎不知情 [open] (medium)
- 复现: 08-compose.js 约 643 行 auto-pause 恢复分支:!autoPaused 且勾选 auto-continue 时仍要 autoContinueAllowed() 才 scheduleAutoContinue,档位不是 dev-auto 就静默不调度,引擎计数与状态不知情;D-320 只修了 syncAutoContinueWithProfile 那处
- 影响: R-199 验收①「前端不再持有任何引擎不知道的续跑否决条件」在暂停→恢复路径上仍未兑现
- 来源: 2026-08-13 自举复盘(探查代理逐处核对 autoContinueAllowed 残留)
- 标签: 前端
- 优先级: P2

## D-330 tracker add/repair_missing_id 时 priority 参数与 fields 里「优先级」键双写重复字段 [open] (medium)
- 复杂度: 小
- 复现: tracker add/repair_missing_id 分支(tracker.rs:484-489 与 :363-368)在 priority 参数有值时无条件 fields.push(("优先级", priority)),不去查 input.fields 里是否已有「优先级」键。调用方若同时传 priority 参数 + fields 里「优先级」键,Vec 里得到两条「优先级」字段。本轮 R-233/R-234 即踩中:值相同时两条相同字段冗余;值不同时(P1/P2)两条矛盾字段,下游读取语义不定。
- 影响: add 静默写两条同名字段:值相同仅冗余,值不同则优先级字段语义歧义;update 分支(:614-621)已有正确合并去重逻辑,add/repair 分支未复用,是不一致缺陷。
- 来源: 2026-08-16 本轮自举:R-233/R-234 add 时 priority 参数与 fields 里优先级键双写,get 显示两条「优先级: P1」,raw_lines 另有游离空行(已清)。
- 标签: 后端
- 优先级: P2

## D-331 归档终态无法安全修正且非法状态会污染缺陷标题，reopen 对归档 ID 误报 unknown id [open] (high)
- refs: D-267 D-241 D-284 D-329
- 复现: D-267 在 defects.md 中带缺陷状态机不支持的 [dropped]；close/archive 后工具未拒绝该标记，而是在标题继续追加 [fixed]，归档结果成为 [dropped] [fixed]。随后 defect reopen D-267：缺 reason 时先报参数错误，补 reason 后只查活动文档并报 unknown id，无法通过专用工具改为 wontfix。
- 影响: 同一缺陷可同时呈现互相矛盾的终态，调度、统计和人工审计失真；agent 收到 unknown id 后无法区分真正不存在与已经归档，容易绕过专用工具手改托管文档，破坏原子写入、格式保护和审计链。
- 期望: 缺陷写入口拒绝 dropped/done 等跨文档状态标记；活动操作遇到归档 ID 时返回“已归档”及允许动作；提供仅限终态到终态、强制 reason、不重新入队的归档纠错动作，并用它把 D-267 收敛为单一 [wontfix]。
- 标签: 核心
- 根因: DocStore 对标题中形似状态但不属于当前 DocKind 的标记缺少写入校验；close 渲染时把解析不到的 [dropped] 保留为标题正文并追加合法终态。TrackerTool get 可回落读取归档，但 update/reopen 只查活动 entries；reopen 的语义仅为 fixing→open，当前没有归档终态纠错动作。
- 验收: ①缺陷 add/update/close 对标题或状态位置中的跨 DocKind 状态标记给出明确错误，测试覆盖 dropped 不得混入标题；②reopen/update 命中归档 ID 时不再报 unknown id，而是明确 archived 且 reopen 不适用；③新增受限归档终态纠错动作，只允许 fixed↔wontfix、必须 reason、保持条目在归档、原子写入并追加审计进展；④D-267 修复为单一 [wontfix]，原有关闭理由与自由文本逐字保留；⑤回归覆盖真实不存在 ID、活动 fixing→open、归档内容保真、并发锁与完整性门禁。
- 优先级: P0
