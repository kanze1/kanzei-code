# Defects

## D-283 会话状态按轮次投影导致运行中显示空闲、停止按钮消失、鞭挞与活动记录串线 [done] (high)
- 优先级: P0
- 复杂度: 大
- 标签: 核心 后端 前端 并行 自举
- 来源: 2026-08-12 用户连续截图与复现；全局扫描确认不是单一 CSS 问题，而是后端会话态、前端事件态、轮询快照、线路设置和 trace 落库粒度共同漂移。
- 复现: 
  1. 任务实际收到进度事件，左侧线路仍显示「空闲」，顶部 stop 不出现。
  2. 一轮结束后鞭挞仍会等待/续跑，但底部通过 `setStatus(..., false)` 显示普通空闲。
  3. 主线开启 `dev-auto`/鞭挞后切到未配置并行线，新线继承旧 profile、checkbox 或旧 timer。
  4. 运行中切线或重载，右侧活动记录要等轮末 trace 落库，轮内轨迹暂时消失。
- 根因: 
- `kz: done` 是轮末事件，旧投影把它当会话终态；`kz:idle` 才是运行循环结束。
  - 运行态既由后端 `runtime.running`、前端 `sessionStates`、`process_list`、`collaboration_snapshot` 投影，又被多个 handler 直接写状态，缺少单一投影出口。
  - 并行线路页依赖 3.5/8 秒轮询，实时事件与轮询没有明确优先级。
  - `run.trace` 主要在轮末收尾写入，活动回放天然是轮粒度。
  - profile/auto UI 有全局 localStorage fallback，切线时先同步新 session、后应用目标设置，且目标 profile 为空时不清理旧值。
- 统一修复: 归并到 R-197，按其 10 批次执行；设计基线见 `docs/design/session_state_and_line_runtime.md`。
- 验收: 以 R-197 八条验收为准，额外保留两条反证：①`kz:done` 后模拟第二轮/排队输入仍显示运行；②主线鞭挞开启后切未配置并行线不会产生 `auto=true` 的目标 session 请求。
- 证据等级: E1(用户复现 + 代码调用链核实，修复后需提升为 E2/E3)
- 进展: 2026-08-12 已完成并发布。运行态以 session_id 实时事件为先、轮询仅校准；`kz:done` 与 `kz:idle` 分离；鞭挞等待保持可停止的独立状态；profile/auto/timer 按线路隔离；run.trace 增量落库并保留轮级汇总；历史继续挂在线路下。最终审计补上旧轮询快照覆盖实时事件/启动意图的竞态，并把 `kz:error` 分为非终态告警与终态失败。相关测试与安装核验见本次交付记录，WebView2 E2 受当前探针环境限制未计入通过证据。
- 阻塞: 

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

## D-239 取活口径漂移复现追踪:伪阻塞/伪可执行/挂起无载体 [open] (medium)
- 复现: 2026-08-10 复盘取活时发现三处阻塞/挂起口径漂移:①R-151/R-162~R-167 把非阻塞内部依赖(R-150/R-161 等,解除权在 agent)写进「依赖」字段,list 据未完成依赖判 blocked,调度器整批跳过,需求队列后半截系统性锁死;②R-157 实质卡在 D-235(conventions.md 无专用写入通道,edit 被 ruleset 拒绝),却无阻塞字段,以 doing 形态占可执行 WIP 名额、实际推不动;③R-101 用户 08-09 挂起只写在进展里,状态 todo 无阻塞字段,取活器会误取。
- 根因假设: §1.1 阻塞口径只在「触碰条目时」顺带复核,无周期机械核对;2026-08-09 WIP 口径修订后历史条目未回扫(R-151 的阻塞恰在口径修订期写入)。
- 进展: 2026-08-10 已修当前三条:R-101 补挂起阻塞字段(解除人=用户);R-157 补合法阻塞(⑤依赖 D-235);R-151/R-162~R-167 清空伪阻塞依赖字段。| 2026-08-13 验收②复核:发现并清理一处伪阻塞漂移——R-176 阻塞字段写「未完成依赖: R-175」已清空,依赖保留在依赖字段;其余条目阻塞字段均为合法外部阻塞,无伪可执行 doing(三个 doing 均带具名解除人),无挂起无载体;未升级 §1.1/取活器(单条误写,§1.1 已覆盖)。| 2026-08-13 第二轮复核(autonomous 会话):R-157 阻塞更新(conventions 工具已就位但 patch 需用户批准);R-164 阻塞保持(无 cargo 权限跑不了全量);其余用户/环境阻塞均仍成立;未升级 §1.1/取活器。| 2026-08-14 第三轮复核(defect-first 取活前):defects 侧 D-209/D-207/D-204/D-259/D-278/D-280 阻塞全合法(用户验收/指认/重启引擎);requirements 侧 R-174/R-059/R-135 阻塞合法;R-176 阻塞字段文件为空、list 的 blocked 为依赖字段推导(R-175 未完成)非伪阻塞;R-101 doing 无阻塞=真实活动项;本轮无同类漂移复现,未升级。| 2026-08-16 第四轮复核(D-260/D-261/D-286 收口后):defects 侧 D-209/D-207/D-204/D-278/D-280 阻塞仍全合法(解除人=用户);顺带修复 tracker 完整性门禁——archive 旧 D-283 与新 D-283 语义不同,repair_reused_id 迁为 D-285(见 D-283 进展);requirements 侧 R-174/R-059/R-135 阻塞合法,R-176 阻塞字段仍空、list blocked 为依赖推导(R-175 未完成),R-101 doing 无阻塞=真实活动项;本轮无同类漂移复现,复核累计 4 轮,距 10 轮尚余 6 轮。| 2026-08-16 第五轮复核(R-101 转阻塞后取活前):本轮变化——R-101 由 doing 无阻塞(第四轮确认为真实活动项)转为 doing+用户阻塞,系用户 2026-08-13 指示「把R101转入阻塞,取其他的去」,解除人=用户(加白名单或切交互轮),合法外部阻塞非漂移。全量核对:defects 侧 D-209/D-207/D-204/D-278/D-280 用户阻塞仍合法(指认痛点/重建 kzapp 复查),D-239 自身为复核追踪 open、不占 doing 名额,其余 open 缺陷(D-266/D-268/D-270/D-275/D-276/D-279/D-281/D-282/D-289)无阻塞字段但属会话级档位限制(autonomous 档无 edit crates/cargo/node 权限,解除权在用户切交互轮或加白名单),非伪可执行(均为 open 未进 fixing);requirements 侧 R-174/R-059/R-135 用户阻塞合法,R-176 依赖推导 blocked 合法(口径与第三/四轮一致),R-144~R-196 todo 无阻塞=未开工真可执行但受会话档位限制;无伪可执行 doing(仅 R-174/R-101 两个 doing 均带合法用户阻塞),无挂起无载体。本轮无同类漂移复现,未升级 §1.1/取活器。复核累计 5 轮,距验收③连续 10 轮尚余 5 轮。| 2026-08-16 第六轮复核(requirement-first 取活前):本轮变化——R-200 由无阻塞待取活转为带阻塞(缺权限/环境,§1.1 类②):autonomous 档位下 edit(仅 style.css 白名单)/git stage/commit/cargo 全被权限拦截(本轮实测 edit .gitignore 与 git stage 均报 permission requires user approval,kanzei.toml 权限段无 action="git" 与 cargo 规则),其两路只读勘察已完成(消费点清单见 R-200 进展),解除人=用户(加白名单或切交互轮),合法外部阻塞非漂移;R-198/R-199 仍无阻塞但同受档位限制(需 edit crates+cargo),未进 doing 非伪可执行。全量核对:requirements 侧 R-174/R-059/R-101/R-135 用户阻塞合法(发版实测/移动端排期/加白名单/补条目),R-176 依赖推导 blocked 合法(口径与第三/四/五轮一致),R-144~R-196 todo 受档位限制非伪可执行;defects 侧 D-209/D-207/D-204/D-278/D-280 用户阻塞合法(指认痛点/重建 kzapp 复查),D-293/D-266/D-268/D-270/D-275/D-276/D-279/D-281/D-282/D-289 open 无阻塞受档位限制非伪可执行;无伪可执行 doing(仅 R-174/R-101 两个 doing 均带合法用户阻塞),无挂起无载体。本轮无同类漂移复现,未升级 §1.1/取活器。复核累计 6 轮,距验收③连续 10 轮尚余 4 轮。| 2026-08-16 第七轮复核(defect-first 取活前,D-293 关闭后):本轮变化——D-293 已关闭(fixed,20 轮全量零失败,commit 59aa868),从 open 列表移除;其余阻塞字段复核:defects 侧 D-209/D-207/D-204/D-278/D-280 用户阻塞仍合法(指认痛点/重建 kzapp 复查),D-239 自身 open 不占 doing;requirements 侧 R-174/R-059/R-101/R-135 用户阻塞合法,R-176 依赖推导 blocked 合法(口径与第三~六轮一致),R-200 权限阻塞合法(解除人=用户加白名单),R-144~R-196 todo 无阻塞未开工;doing 仅 R-174/R-101 均带具名解除人,无伪可执行 doing,无挂起无载体。本轮无同类漂移复现,未升级 §1.1/取活器。复核累计 7 轮,距验收③连续 10 轮尚余 3 轮。| 2026-08-16 第八轮复核(defect-first 取活前,D-266 关闭后):本轮变化——D-266 已关闭(fixed,install-setup.ps1 装后校验,commit 817b477),从 open 列表移除;其余阻塞字段复核与第七轮一致:defects 侧 D-209/D-207/D-204/D-278/D-280 用户阻塞仍合法(指认痛点/重建 kzapp 复查),D-239 自身 open 不占 doing;requirements 侧 R-174/R-059/R-101/R-135 用户阻塞合法,R-176 依赖推导 blocked 合法,R-200 权限阻塞合法(解除人=用户),R-144~R-196 todo 无阻塞未开工;doing 仅 R-174/R-101 均带具名解除人,无伪可执行 doing,无挂起无载体。本轮无同类漂移复现,未升级 §1.1/取活器。复核累计 8 轮,距验收③连续 10 轮尚余 2 轮。| 2026-08-16 第九轮复核(defect-first 取活前,D-268 关闭后):本轮变化——D-268 已关闭(fixed,围栏测试跨进程文件锁,commit 21e36c9),从 open 列表移除;其余阻塞字段复核与第八轮一致:defects 侧 D-209/D-207/D-204/D-278/D-280 用户阻塞仍合法(指认痛点/重建 kzapp 复查),D-239 自身 open 不占 doing;requirements 侧 R-174/R-059/R-101/R-135 用户阻塞合法,R-176 依赖推导 blocked 合法,R-200 权限阻塞合法(解除人=用户),R-144~R-196 todo 无阻塞未开工;doing 仅 R-174/R-101 均带具名解除人,无伪可执行 doing,无挂起无载体。本轮无同类漂移复现,未升级 §1.1/取活器。复核累计 9 轮,距验收③连续 10 轮尚余 1 轮。| 2026-08-16 第十轮复核(defect-first 取活前,D-270 关闭后):本轮变化——D-270 已关闭(fixed,HOME 守卫四处缺口,commit 15cc5f6),从 open 列表移除;其余阻塞字段复核与第九轮一致:defects 侧 D-209/D-207/D-204/D-278/D-280 用户阻塞仍合法(指认痛点/重建 kzapp 复查),D-239 自身 open 不占 doing;requirements 侧 R-174/R-059/R-101/R-135 用户阻塞合法,R-176 依赖推导 blocked 合法,R-200 权限阻塞合法(解除人=用户),R-144~R-196 todo 无阻塞未开工;doing 仅 R-174/R-101 均带具名解除人,无伪可执行 doing,无挂起无载体。本轮无同类漂移复现,未升级 §1.1/取活器。**复核累计 10 轮,验收③「连续 10 轮无同类复现」达成,按验收原文需用户确认后关闭本条**。
- 验收: ①当前三条已修,req get 各条目可见清理后口径(证据:R-101/R-157 有合法阻塞字段,R-151/R-162~R-167 依赖字段为空、进展注明解锁条件);②此后每轮取活前复核阻塞/依赖字段口径,若再次出现同类漂移(伪阻塞、伪可执行 doing、挂起无载体)→ 确认为规则缺陷,升级修 §1.1/取活器并记根因;③连续 10 轮无同类复现 → 用户确认后关闭本条。复核已累计 3 轮(2026-08-13 ×2、2026-08-14 ×1),无同类复现。
- refs: R-101 R-157 R-151 R-162 R-163 R-164 R-165 R-166 R-167

## D-267 bash 授权缺一个安全的中间档:只有「逐条逐字节精确」与「整体全放行」两端,无人值守只能靠 yolo [dropped] (high)
- **2026-08-11 关闭为 `dropped`(用户定调): 不做中间档,bash 非交互直接放行,防线整体挪到结果侧。**
  以下五条是关闭理由,按份量排序。**本条的现象描述与实测清单全部依然属实**——变的是处置,不是事实。
  1. **它挡不住有意的。** §0 定案 1 已经承认并接受:段级闸门是纯 shell 语法过滤器,对「被允许的**程序**本身是什么」一无所知。`cargo` 按设计编译并运行工作树里的代码(本仓 `build.rs` 与两个可运行 bin 都在),而 agent 持有 `edit` 权限。**任何黑名单都关不掉,危险性在程序语义里不在 shell 语法里。**
  2. **既然挡不住有意的,它挡的只剩无意的——而无意的错误有更便宜且更管用的防法。** 见下方「替代方案」。
  3. **实证:两轮对抗复核各绕过一次。** 第一轮:`scan()` 不处理反斜杠,`\'` 被当成引号态开关而 bash 里它是字面引号(方向恰好相反),一对 `\'` 之间的内容 bash 照常执行、词法器整段吞掉,**模块的每一类拦截同时失效**(真 bash 5.2.37 已复现)。第二轮:`cd`/`pushd`/`PATH=` 不在任何表里——`git -C` 被否决而等价的 `cd ../other && git ...` 一路放行;`PATH=../evil:$PATH; cargo build` 让那个叫 cargo 的 token 解析到别的二进制,**连「程序是操作员写下的那一个」都不成立**(假 cargo 实测跑通)。**1045 行换来一个被绕过两次的过滤器。**
  4. **威胁模型里根本没有「模型是敌人」这一条。** kanzei 是用户自用的激进工具,不是给不受信任用户的通用 harness。任务源与模型都由用户自己掌握。为一个不存在的威胁模型付账,还付成了一个可绕过的过滤器。
  5. **它是无人值守的唯一硬卡点。** 保留它就等于保留「并行线跑不到底」。
- **替代方案(不是"什么都不做")**: 并行下真正要防的**不是恶意,是串台**——A 线的命令跑进 B 线的树、把人家未提交的活覆盖了。而闸门恰恰防不住这个(`cd ../other && rm -rf` 里没有一个可疑 token)。正解沿用本仓既有哲学:**不事前拦,检测 + 回滚**。D-173/D-174 已经把这条路走通了——`ManagedSnapshot` 执行前后快照比对、越界写入隔离留证、整体回滚,**它不关心命令长什么样**,所以 `cd ../other` 与 `cargo run` 里 build.rs 干的坏事一视同仁抓得到。本条的替代交付登记为 **R-186**(把 `ManagedSnapshot` 的范围从「托管文档」扩到「不属于本线的 worktree」)。
- **保留不动的**: ①硬 deny(`.kanzei/project/*` 等)——它是结果侧围栏,不受本条影响;②审计轨迹(谁在什么时候跑了什么)归 R-183;③**D-269 仍要修**(硬 deny 也走同一条匹配路径,且只 1/21 条规则受影响,是 5 行的事)。
- **作废的产出**: `par/f2` 分支上的 `crates/kanzei-harness/src/cmdline.rs`(1045 行)整个丢弃,不合入。`docs/design/tier1_implementation_plan.md` 的 F2/F5 两批随之作废,F8 里 ACE 告警那部分并入 R-183。
- **反悔条件(写清楚免得将来靠猜)**: 若将来要把**不受信任的任务源**(如外部 issue、他人提交的需求)交给自举循环,本条的诉求会重新成立——但那时要做的是 §0 定案 1 里写的「只放行真正的叶子命令」那一档,**而不是重做中间档**。那一档 Rust 开发跑不了,是不同的产品形态。
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 证据等级: E1(读码自证 + 2026-08-11 三条线实测各自 3~6 秒内被拒停机)
- refs: R-183 R-182 D-051 D-004
- 复现: 2026-08-11 三条 `kz run` 独立线跑任务级并行,先给每个 worktree 的 `.kanzei/kanzei.toml` 追加了按需放行规则(`action="bash"`,`resource="cargo *"` / `"node *"` 等)。三条线**全部在 3~6 秒内**以 `EXIT=3` / `stopped: permission declined` 停机,第一条 bash 就被拒。被拒的资源形态是 `{"command":"git branch --show-current; git status --short","workdir":"c:/users/kanzei/documents/kz-par-b"}`。对照:同一批任务换用外部 agent(权限规则可写成 `Bash(cargo:*)` 这种按需形态)则正常运行——所以问题不在 agent 侧。
- **本条不是「代码写错了」——先读这段再动手**: 造成上述结果的三层判定**全部是有意设计**,且各自有测试或缺陷编号背书。任何修法若让下面三条性质失效,就是把 D-051 重新放回去:
  ①`crates/kanzei-harness/src/permission.rs:198-216` `resource_match_for_action`:bash 的实际 value 永远是含 `command`+`workdir` 的结构化 JSON;pattern 非结构化时直接 `return false`。**意图**:workdir 是授权身份的一部分——同一条命令在不同目录里后果不同。测试 `bash_resources_keep_shell_text_opaque_during_matching`(:476)钉死了「换 workdir 即变 Ask」;测试 `legacy_bash_rules_do_not_authorize_structured_resources`(:494)钉死了「旧的纯字符串规则不得授权结构化请求」——因为旧规则是在 workdir 还不算身份时记的,让它们生效等于凭空授权了用户从未批准过的目录。
  ②`permission.rs:234-236` `command_chaining_escapes`:任何 `pattern != "*"` 且含 `*` 的 bash 规则被降级成 Ask。**意图**(函数头注释点名 D-051):通配规则表达不了命令内部的 shell 语义——`"git *"` 会匹配上 `git status; rm -rf ~`。测试 :472 同处对照:**精确**规则可以放行含 `rm -rf ~` 的整串,因为用户批准的正是那一整串。
  ③`resource_match_for_action:200` 对 `pattern == "*"` 前置直通,且 ② 明确排除 `pattern != "*"`。**意图**:整体放行是用户显式选择的 yolo,不应被降级(`permission.rs:466` 有同义注释),且 `config.rs:434` 会为此发告警。
- 影响(可用性缺口,不是安全漏洞): 三层叠加的净效果是 bash 授权**只剩两端**:逐条逐字节精确的单命令,或整体 `*`。中间那一档——「某类命令、可复用、可手写」——在代码里不存在。直接后果:①用户配置里已累积 **12 条巨长的结构化 JSON 规则**,每条只覆盖一个具体命令、复用率为零,只会无限累积;②启动时长期存在「检测到 N 条旧 bash 权限规则；将逐次询问」的告警且无法自愈;③**`kz` 无法无人值守运行**——R-183 的直接卡点,也是本次实测停摆的原因;④用户被结构性地推向 yolo,权限系统在实际使用中被架空——**这是最严重的一条,也是定 P0/high 的理由**:偏严到只剩全放行,结果比适度放宽更不安全。
- 边界: **判定失败方向是偏严**(该允许的没允许),不是越权。因此本条**不得以「放宽匹配」作为修法**——不能简单让纯字符串 pattern 去匹配结构化 value(那会同时废掉 ①的两条测试)。要交付的是**新增一档可安全表达的规则形态**,不是削弱现有两档。
- 修复方向(待设计,勿直接照做): 大致形状是——把命令**真正解析**成子命令序列(按 `;`/`&&`/`||`/`|`/换行切分,并对命令替换 `$(...)`/反引号、重定向到规则外路径等无法静态判定的构造保持 Ask),要求**每一个**子命令都命中允许规则才放行;workdir 维度改为**可显式表达**(规则能写「任意 workdir」,但必须是用户显式写出来的,不是旧规则被默认提权)。这一条与 R-183 内容②「worktree 应继承主根规则」是同一诉求的两半:继承必须是可见的、写出来的,不是隐式的。
- 验收: ①存在一种**可手写、可复用**的规则形态,能表达「任意 workdir 下的 cargo 命令」,有单测。②命令**确实没有**链接符/替换构造时不再被无条件降级;**确实有**时仍降级——两个方向各有单测,且含 `git status; rm -rf ~` 这类反例。③**D-051 三条性质的反证测试全部保留且仍绿**:换 workdir 即变 Ask(:476)、旧纯字符串规则不授权结构化请求(:494)、精确规则可放行整串(:472)。把这三条测试改红或删掉即视为验收不通过。④既有 12 条结构化 JSON 规则不因本次修改而失效(向后兼容单测)。⑤修复后 `kz run` 能在 worktree 里靠一组**可手写的**规则完成一次「改代码 → cargo test → 提交」闭环(与 R-183 验收①同一条轨迹)。⑥启动告警「N 条旧 bash 权限规则」在规则可正常匹配后消失,或给出可执行的收敛路径。⑦**实测被拒命令清单**作为规则模板的输入:本次并行实测里被拒/需要放行的命令要归档,R-183 内容④的模板据此收敛,不靠拍脑袋。
- **实测被拒命令清单(验收⑦的输入,2026-08-11 三条并行线实采)**: 五条,全部来自**外部 agent 的权限层**(该层已经在做本条想要的按子命令匹配),形态高度一致——
  ①`node <脚本> && echo "EXIT=$?"` —— 拦截理由明确点名 `echo "EXIT=$?"` 这一段需批准,**拆掉尾部 echo 后同一条命令直接放行**;
  ②`ls <路径> | head -0; ls <路径>`;
  ③`awk '<程序>' <文件>`(单命令,只是 `awk` 不在允许集);
  ④PowerShell `cargo test ... | Select-Object -Last 40`;
  ⑤bash `... | head -30; echo ...`。
  **归纳**:①②④⑤ 全是**复合命令**(`&&` / `;` / `|`),③ 是**未列入允许集的单个可执行**。两类都在改成单条纯命令后放行。对本条的三点含义:(a) 修复方向里「解析成子命令序列、要求每个都命中」的形状**已有活的参照实现**,不必再论证可行性;(b) 拦截必须**点名具体是哪一段**不被允许,否则无法自我修正——这是可用性的关键,不是锦上添花;(c) R-183 内容④的基础规则模板至少要覆盖 agent 实际会用的这批 shell 动词:`echo`/`head`/`tail`/`awk`/`grep`/`ls`,以及 PowerShell 的 `Select-Object`——它们几乎只出现在管道尾部做截断,危险面低但出现频率极高,是「不放行就寸步难行、放行也没什么风险」的典型。

## D-271 主对话切线程时消息短暂消失、侧栏只显示单条并行任务、子代理无关闭/删除生命周期 [closed] (medium)
- 优先级: P0
- 复杂度: 中
- 标签: 前端 并行 子代理
- 证据等级: E1(用户复现 + 运行时冒烟回归)
- refs: R-174 R-184 D-263
- 复现: 三条并行线运行时切换主对话线程，旧实现先清空消息再等待 IPC；迟到的旧线程历史还可能覆盖新线程。侧栏只显示单个“当前在做”焦点，未按 `process_list` 投影 N 条线路；子代理面板只有运行中/已完成，没有关闭与删除语义。
- 影响: 切线期间对话区出现空白或串线，用户无法判断三条线各自是否运行/处于哪个阶段；已结束子代理只能长期堆积，无法收起或清理。
- 修复: `conversation_get`/`conversation_trace_get` 锁定项目、进程和切换代次，目标历史完整恢复后再原子替换消息；侧栏按每个进程显示主代理/并行线、运行态、阶段并支持点击切换；子代理生命周期明确为 `running → finished → closed → deleted`，关闭/删除仅作用于当前 UI 条目，保留后端 transcript 与审计，停止仍调用真实 `stop_task`；主代理写入、比对、合并、发版边界同步写入系统提示与 task_spec，子代理工具白名单保持 `read/glob/grep`。
- 验收: `node scripts/ui-runtime-smoke.mjs` 覆盖三线状态、切线不清空、关闭/重开/删除；`ui-i18n-smoke`、`ui-a11y-smoke`、`ui-markdown-smoke`、`parallel-lines-regression` 全绿；`cargo test -p kanzei-app` 112 passed、`cargo test -p kanzei-core` 130 passed。2026-08-11 随本次桌面端发版交付，待用户安装后进行最终桌面实测。

## D-272 并行线/自举 ASK 串到用户弹窗并中断自动推进 [closed] (high)
- 来源: 2026-08-11 用户复现——代理线调用 ASK 时弹窗出现在主用户界面，自举运行被迫等待或停止。
- 根因: 所有 `AskRequest` 默认复用桌面端用户询问闭包；运行模式没有把“可等待用户”与“后台自动推进”区分开，前端也没有按 ASK 来源做最后一道隔离。
- 修复: `RunnerConfig.ask_policy` 明确区分 `Interactive` 与 `NonInteractive`。主线手动运行保持交互；并行进程与自举续跑使用非交互策略：权限 ASK 转成可回喂模型的错误并继续，`question` 转成明确的不可询问工具错误，不创建 `PendingAsk`、不发用户弹窗。ASK 事件附带 `source`，前端对旧运行/异常事件再做并行、自举来源拦截。子代理继续保持只读与硬拒绝 ASK。
- 边界: 当前交付解决“不会串到用户”的安全行为；真正的代理间问答需独立的带 source/target 的内部消息通道，后续另立需求，不复用用户 ASK。
- 验收: `cargo check --workspace` 通过；`kanzei-core` 的 ASK 策略单测通过；UI 事件回归确认后台来源不进入用户 ASK 队列；桌面安装后需实际启动三线并开启自举，确认无弹窗且线路继续推进。
- 证据等级: E1(读码 + 编译/定向测试)，桌面最终验收待用户安装后实测。
- refs: D-271 R-169 R-174

## D-275 托管路径 OS 层写隔离(残余):后台进程与专用工具同窗口同前缀时仍可蒙混 [open] (medium)
- 优先级: P2
- 复杂度: 大
- 来源: 2026-08-11 D-258 关闭时转出(验收①的 OS 层条款未做,成本收益倒挂且与验收②互斥)
- 标签: 核心
- 缺陷: D-275
- 证据等级: E1
- 进展: D-258 已交付等价拦截(守卫不整树推进 + 双快照精确吸收),残余边界为:后台进程在专用工具窗口内、写窗口前缀内的路径(与专用工具同窗口)仍会被吸收进基线。OS 层隔离(受限令牌/ACL/低完整性)是彻底消除该残余的唯一方向,但 D-258 评估为成本收益倒挂且与「后台能写 target/」互斥——本条目仅在有明确收益方案(如仅对 .kanzei/project 与 .kanzei/memory 两颗托管树设 ACL,不影响 target/)时取活,取活前先评估代价。
- 验收: ①存在一条后台进程在操作系统层面写托管路径失败的机制(受限令牌/低完整性/ACL),有实测证据,且不得破坏后台任务写 target/、node_modules/(与 D-258 验收②同口径);②该机制与托管写入窗口(managed_fence)组合后,窗口内后台进程与专用工具写同一批路径也不能蒙混——即吸收/回滚不再依赖镜像快照区分;③跨平台降级路径有明确说明(Windows 独占句柄 vs POSIX advisory lock),降级时不静默放行而是显式告警。

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

## D-316 引擎归档动作产生重复条目与孤儿字段:archive 中 D-309 两份、open 的 D-289 字段被误切入且无工具清理通道 [fixing] (medium)
- 复现: 上一轮关闭一批缺陷后,引擎自动归档把 fixed 条目移入 defects-archive.md 但未提交(工作树遗留)。实测归档产物两处脏数据:①D-309 在 archive 重复两份(3238/3252 行,内容完全相同);②open 的 D-289 字段行(复现/影响/来源/标签/阻塞/优先级)被误切进 archive 尾部,活动文件 D-289 字段随之下线。
- 影响: archive 出现重复条目与孤儿字段行;活动文件 open 条目字段被误移(已用 defect update 手工补回 D-289,但 archive 尾部残留 6 行孤儿字段)。归档是引擎管理文件,edit 被 ruleset 拒绝、defect 工具不认归档条目,当前无合法清理通道——同类问题与 D-294 的「游离段落无删除通道」一致。
- 标签: 流程
- 根因: 引擎归档动作的切割/复制逻辑疑似把 D-312 之后的 D-289 字段行一并划入归档,并对 D-309 重复落盘;具体在 harness 归档实现,待定位。
- 优先级: P2

- 复杂度: 中
- 批次: 2/2
- 进展: 2026-08-16 取活。现状核实:①archive_terminal(docstore.rs)的 archived.extend(terminal) 只对模板去重、Entry 列表未按 id 去重——重复归档会二次追加(D-309 两份 3238/3252 实证);②D-289 的 6 行孤儿字段已污染进 archive 的 D-312 条目(复现/影响/来源/标签/阻塞/优先级 重复 key + 空阻塞)。B1 完成(commit 44c10cf):①archive_terminal 写回前调 normalize_archive 净化整个归档(按 id 去重保留先归档、每条目同 key 字段去重保留第一个非空、删空字段),净化有变化时即使无新终态条目也强制写回(archived 动作=清理通道);②extend 前 Entry 列表按 id 去重(与模板去重一致);③新测试 archive_terminal_净化重复条目与孤儿字段 构造 D-309 两份+D-312 污染,断言收敛;docstore 19 passed,fmt/clippy 全过(T-1786564595)。真实环境注意:当前 agent 会话的 defect 工具跑的是旧引擎,archive 实测返回 nothing to archive(旧代码无净化)——真实文件脏数据(D-309 重复/D-312 污染)会在引擎更新后的首次归档动作被自动收敛,净化逻辑已有单元测试背书。| 2026-08-16 关闭:全量 cargo test --workspace 全绿(T-1786563xxx,tools 263)。逐条对照:①D-309 重复两份——根因 archive_terminal extend 未按 id 去重,已修(Entry 列表去重 + normalize_archive 整体净化),测试断言重复收敛为一份;②D-289 孤儿字段污染 D-312——normalize_archive 同 key 字段去重(保留第一个非空)+ 删空字段(如 `- 阻塞: `),测试断言复现保留原条目值、空阻塞被删;③无工具清理通道——已建立:任何归档动作(archived=清理通道)自动净化整个归档,无需新工具;净化有变化即强制写回。残余:当前工作树 defects-archive.md 的真实脏数据由含本修复的新引擎在首次归档动作自动收敛(代码已提交,引擎重启后生效),进展已记录。关闭。

## D-319 WebView2 当前环境 DevTools 端口不监听:e2e-smoke connectOverCDP 20 秒超时(参数已传入但不绑定) [open] (medium)
- 复杂度: 中
- 复现: 2026-08-16 D-289 实测验证中发现:无论 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 环境变量还是 KANZEI_E2E_CDP 注入路径,WebView2 进程命令行均带上 --remote-debugging-port=<port> --remote-allow-origins=*(进程命令行实证),但端口 20 秒不监听、user-data-dir 无 DevToolsActivePort 文件、进程树完整(renderer/gpu/network 均在)、会话与用户 kzapp 同为 Session 1、无策略禁用(注册表 HKCU/HKLM EdgeWebView/Edge 均空)。对照:同参数字符串起 Edge --headless,1 秒即监听。结论:WebView2 在当前机器/环境不启动 DevTools 端口,与参数注入路径无关。
- 影响: R-101 e2e-smoke 基座在自举环境无法实测 connectOverCDP(端口不监听→20 秒超时→FAIL)。这独立于 D-289 的 origin 白名单修复——即使端口能监听,D-289 也是必需的(M111+ 拒非白名单客户端);但端口不监听会让 e2e-smoke 永远失败。
- 来源: self-found(D-289 实测验证中发现)
- 标签: 流程
- 优先级: P2
