# Requirements

## R-186 跨树越界检测与回滚:ManagedSnapshot 范围从托管文档扩到「不属于本线的 worktree」 [doing]
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(D-174 已交付同哲学的实现;本条是范围扩展)
- refs: D-267(本条是它的替代交付) D-173 D-174 R-183 R-184 R-177 D-258
- 来源: 2026-08-11 用户定调砍掉 bash 权限中间档后的替代方案。原话:「这些直接砍了,没啥用说真的,并行自举本来就是激进的玩法」。
- 为什么是这个形态(本条的全部理由): 并行下真正要防的**不是恶意,是串台**——A 线的命令跑进 B 线的树、把人家**未提交**的活覆盖了。而**命令语法闸门恰恰防不住这个**:`cd ../other && rm -rf` 里没有一个可疑 token,`cargo` 也是合法程序、其 `build.rs` 能干任何事。
  正解沿用本仓既有哲学。设计基线明写着 bash 对托管文档的保护「**在结果侧**(执行前后快照比对 + 隔离留证 + 整体回滚),**不在权限层**」——D-173/D-174 已经把这条路走通并有实现(`crates/kanzei-tools/src/bash.rs` 的 `ManagedSnapshot::capture` / `is_complete`)。它的关键优势是**不关心命令长什么样**,所以 `cd ../other` 与 `cargo run` 里 build.rs 干的坏事**一视同仁抓得到**——这正是闸门做不到的那一半。
- 内容: ①把 `ManagedSnapshot` 的保护范围从「主根 `.kanzei` 托管文档」扩到「**不属于本线的 worktree**」:执行前拍快照的集合 = 托管文档 ∪ 其它线的工作树;②越界写入的处置沿用 D-174 既有形态(隔离留证 + 整体回滚 + 归因到 owner run),**不新造机制**;③「本线的树」由 `ProcessHandle.worktree_path` 给出(前置 R-177),主树进程的"本线"= 主根;④性能:快照集合可能很大,按**只对其它线的树做 mtime 级粗筛、命中再细查**收敛,不得让每条 bash 都全量哈希(D-233 的教训:同步全量读+哈希会把主线程占死);⑤越界事件进轨迹,**同时作为 R-184 冲突带的数据源**——"谁写了不属于自己的文件"与"谁和谁改了同一个文件"是同一份数据,不要采两次。
- 边界: **不做事前拦截**(那是被砍掉的 D-267 的路子)。不保护未纳入任何线的目录(用户自己的其它项目不在范围内——本条只管本仓的树之间)。不做跨机器。`ManagedSnapshot` 对**托管文档**的既有行为**一个字不改**(它是 D-173/D-174 的交付,只加范围不改语义)。
- 验收: ①A 线执行 `cd <B线树> && <写操作>` 后:改动被检测、被隔离留证、被回滚,B 线的工作树**逐字节复原**,有实测轨迹(不是只断言函数返回);②归因正确:轨迹里指出是哪条线(owner run)越的界;③**`cargo run` 里 build.rs 写别人的树**同样被抓——这条是本条相对闸门的核心优势,必须有定向测试;④托管文档的既有保护行为无回归(D-174 既有测试全绿);⑤性能:单条 bash 的快照开销有实测数字,N 条线时不随 N 线性劣化到不可用(给出实测,不接受"看起来还行");⑥越界事件与 R-184 冲突带共用同一份数据,不存在两套采集(机械核验:grep 只有一处采集点)。
- 依赖: 
- 前置(不写进依赖,按 D-239 教训): **R-177**(要有 `worktree_path` 才知道"本线的树"是哪棵)。R-177 之前可以先做托管文档侧的重构与 mtime 粗筛。
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-186
- 进展: 自动运行已认领(doing)。2026-08-13 用户明确指示暂停本条、先交付 R-200(测试隔离夹具)并按其批次发版——本条 park,不占可执行槽位。未开工。
- 阻塞: 2026-08-14 复核:原阻塞的解除条件「R-200 及其后续完成后再开」已达成——R-200 已 done 并归档。剩的只是队列位置:本条 P0 但 R-202 占着唯一 WIP 槽,且缺陷队列现有 D-357/D-358/D-359 三条未阻塞条目按 defect-first 排在前面。用户原话是「先做 R-200 再发版,不是做这条」,发版已在本轮执行。解除动作: 用户说一声恢复推进,或 R-202 与缺陷队列清空后按队列自然取到本条。解除人: 用户(一句话即可,不需要额外信息)。
- observed_head: d124749aabe65ec0cde4f2280c9583dd4f33be40
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786609593506

## R-221 research 模式重定位:按 docs/design/research_mode.md 分批实施独立深度研究模式(文献+仓库调研,论文级产出) [doing]
- 优先级: P2
- 复杂度: 大
- 标签: 后端 前端 harness
- 来源: 2026-08-12 八维度审计维度8;设计文档 docs/design/research_mode.md(§2 八个定调点待用户逐项确认后动工)。
- 背景: research 模式骨架完整但形态错位(面向网络调研)且零使用(state.db 266 条 episodes 零调用 websearch/source/finding,.kanzei/research 全 git 历史只有空模板);真实勘察全在 dev 完成且结论无固定落点(勘察报告被 D-294 单行不变式折成单行塞进度字段);证据等级 E0-E4 被双重语义挪用;research/memory.md 是绕开记忆控制平面的第二套无校验记忆。
- 内容: 原按设计文档六批实施,2026-08-14 用户定调后重排:①档位收口(桌面注册 ReadonlyProfile、files/git 只读入列)——**bash 硬 deny 一项作废**,新定位要跑 LaTeX 编译与绘图,须改为白名单或专用工具通道;②topic 工件落点——**待重推**,论文形态需容纳 paper.tex/figures/refs.bib,不是单个 report.md;③证据等级 V 表进 conventions——**已定**,四档待按文献口径扩展;④回流通道(finding→req/defect 草稿)——**待重推**,research 独立后是否仍属本模式职责需重判;⑤记忆一元化——**待复核**,论文引用管理(refs.bib)与记忆晋升是两件事;⑥三形态收敛——**作废**,research 独立后不与 dev 侧 task 勘察/SCOUT_ROLES 收敛。
- 边界: research 不可提交 git、不动既有条目状态(add 草稿除外);不做报告 schema 校验。「不可写 docs/design」一条待重推(新定位下产出是论文而非设计文档,问法需重新表述)。**dev 侧「先计划后自举」的勘察工件落点问题不由本条承接**——那是独立课题,需另立条目。
- 验收: 以设计文档 §7 总则为准——一条真实 R- 条目的 勘察→报告→登记→dev 实施 完整链路有轨迹;每批验收见设计文档 §6。
- refs: D-276 R-201 D-304
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-221
- 进展: 2026-08-16 取活。勘察结论:R-221 的设计真源 docs/design/research_mode.md 状态为「设计基线草案(2026-08-12 八维度审计维度8 产出;定调点待用户逐项确认后转正)」——§2 的八个定调点(主形态/工件落点/证据等级 V 表/回流通道/记忆一元化/档位矩阵/可写 docs 边界/三形态收敛)全部标注「待用户确认」,括号内为本设计的默认建议。按 §1「需求边界不清楚时必须先提问确认,不允许在关键问题上自行假设后直接实现」,八个定调点未获用户拍板前实施会踩边界(如「research 不可写 docs/design」「证据等级单列 V 表」都是用户层面决策)。现状盘点(供解除阻塞后立即开工):批1 档位收口的 files/git 只读已在 R-218 完成(SubagentBase 6 件套),ReadonlyProfile 与 bash 硬 deny+替代指引是既有模式(profiles.rs:652-658 先例);批2-批6 的 topic 工件/证据口径/回流/记忆/三形态均未动。阻塞:等用户对 research_mode.md §2 八个定调点逐项拍板(解除人:用户;方案与默认建议已在 docs/design/research_mode.md §2)。 || 2026-08-14 用户过审:定调点1 被否——research 是**独立深度研究模式**(文献+仓库深度调研,产出论文/LaTeX/图表),不是「先计划后自举」载体,网络检索是主力不是辅助;连带定调点8(三形态收敛)作废;定调点3(V 表单列)已定,四档待按文献口径扩展;定调点2/4/6/7 待 §1 重写后重推,其中6 已知冲突(原案 bash 硬 deny 与跑 LaTeX 编译/绘图直接矛盾)。设计文档 §1/§2 已同步改写,§3 之后各节按旧命题写成尚未同步。
- 阻塞: 两条并存——①**排期**:用户定调实施在 dev 稳定之后,当前不开工不占可执行槽位;②**设计**:research_mode.md §3 以后各节仍按已作废的旧命题(代码库勘察载体)写成,须先随 §1 新定位重写并把 §2 待重推的四条(工件落点/回流通道/档位矩阵/可写 docs)重新过审,才具备实施条件。解除人:用户(两条都是)
- observed_head: b644f1657f2aadede85b26ef65050605740ceb04
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786633950047

## R-216 记忆写入侧质量三闸:近似去重下沉 store.add 双 scope、[fp:] 指纹一致性校验、tracker 交付状态内容拒收 [doing]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 记忆
- 来源: 2026-08-12 八维度审计(§5)。M-055/M-056 于近似去重上线当天英文复述 M-044 并携带编造指纹——「假指纹立即污染注入」经反证驳回(FingerprintIndex 只收 active 且不扫标题),但穿透与伪造本身实证成立;另有 6 条交付状态类内容落进记忆与 tracker 重复。
- 内容: ①classify_novelty 的 FTS 语义探测下沉进 store.add 作为硬闸(Uncertain 即拒并返回候选),查重范围扩到双 scope;②新条目携带的 [fp:] 必须与来源 note 中引擎生成的指纹逐字一致,拒绝自造;③标题/subject 命中「R-/D- 编号+已交付/勿重复/验收边界」形态时拒绝并指路 tracker(或强制挂 refs 并随条目关闭自动 deprecate)。
- 验收: ①复刻「英文改写 M-044」场景被拦并指路 memory_update(单测);②伪造指纹的 add 被拒;③存量 6 条交付状态记忆逐条处置;④各拦截路径有单测。
- refs: R-194 R-195 R-196 D-299 D-282
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-216
- 进展: 2026-08-16 收口此前引擎取活留下的半成品实现(工作树未提交,6 测试红)。已完成:①三闸实现确认完整(store.rs add 内:交付状态拒收 has_tracker_id、指纹一致性 fp_markers、语义探测下沉 classify_novelty 双 scope);②修复 6 个失败 fixture(5 个自造指纹被新指纹闸拦——merge_gate/find_by_marker/merge_conservative/merge_自动搬运 注入来源 note 或 force,1 个 novelty_gate 语义断言适配 R-216 口径);③新增 3 个验收单测:自造指纹的add被拒_来源note指纹放行、交付状态内容被拒并指路tracker、英文改写被add硬闸拦截返回候选。验证:memory 95 passed + kanzei-tools 346 passed + clippy/fmt 全过。验收对照:①英文改写被拦并指路 memory_update——英文改写被add硬闸拦截返回候选 测试(Uncertain 返回候选);②伪造指纹的 add 被拒——自造指纹的add被拒 测试;③存量 6 条交付状态记忆逐条处置——数据工作待做(见剩余);④各拦截路径有单测——三测试覆盖指纹/交付状态/语义三闸。剩余:验收③存量 6 条交付状态记忆逐条处置(数据工作)。
- 阻塞: 2026-08-14 复核:本条不存在外部阻塞——原阻塞写的「解除人:本 agent 后续轮」是自己阻塞自己,那不是阻塞,是没做完。真实状态:三闸实现已完整交付且全绿(memory 95 + kanzei-tools 346 passed),验收①②④齐,只剩验收③的数据工作(逐条查 memory 库定位 6 条交付状态记忆并归档/改写)。当前挂 doing 却被阻塞字段整条 park,是因为 R-202 占着唯一 WIP 槽——清掉阻塞会让 work next 判 wip_violation 禁止全线取活(本轮在 R-183 上实测过)。用户 2026-08-14 明确本条数据工作不交给本会话代做。解除动作: R-202 关闭腾出槽位后直接续做验收③并关闭本条,不需要用户拍板。解除人: agent。
- observed_head: a104ba12af981e0e591aff0c9a5057385ce2f854
- observed_worktree_hash: fnv1a64:025c9fc9adc6d9d2
- recorded_at: 1786637389551

## R-195 candidate 记忆的晋升与清退闭环:存量 5 条无人验收,最后一次晋升停在 2026-08-10 [doing]
- 内容: 给 candidate 定一条会被执行的闸门,形态不在本条强行拍板:或按复发计数自动晋升(计数已可用),或轮末/每 N 轮让 manager 逐条判定晋升与清退,超期未处置的自动 deprecate 归档。同时把存量 5 条走一遍该流程。
- 复杂度: 中
- 来源: 2026-08-12 记忆库存清理:22 条 candidate 里 15 条是重复或空正文条目(已置 deprecated 归档,见提交 2bc5899/216120e),剩 5 条 M-034/M-035/M-037/M-038/M-040 自 2026-08-10 起无人处置;最后一次成功晋升是 M-032(2026-08-10 21:31)。
- 标签: 核心
- 现状: ①promote 有 provenance 硬约束(store.rs:promote 要求至少一条 episode 证据),但没有任何常规动作会产出这种证据,于是没人晋升得了;②三段晋升(第 2 次建 candidate、第 3 次+带修复证据晋升)依赖跨轮复发计数,而计数此前因指纹里含命令载荷永远停在 1——该病灶已在 f104890 修掉(mask_volatile_payload + normalize_fp_marker),计数从此能涨;③即便计数能涨,晋升仍需一个会真正被执行的判定动作。
- 边界: 不改「未验证不注入」的取舍(R-165):本条不是要让 candidate 参与召回,而是不让它永远躺着。已在 f104890 落地的部分(candidate 对去重与复发检测可见)不重做。
- 验收: ①存量 5 条 candidate 全部有归宿(晋升 active 或 deprecated 归档),逐条给出依据;②有机制测试:满足条件的 candidate 能被自动处置,不满足的不动;③candidate 存量不再单调增长——用 index.db 与文件数给出前后对照。
- 优先级: P2
- 批次: 2/2
- 进展: 用户于本轮明确选择暂存本条，暂不推进；保留原验收与 0/2 计划，待 R-236 释放 WIP 槽后再恢复。来源：用户本轮选择 1。
- observed_head: 79d3c4e383a13032ff26c4cd0a13bcd74128c2f2
- observed_worktree_hash: fnv1a64:fe871977f10a5179
- recorded_at: 1786648602102
- 取活依据: engine:唯一可执行 WIP 是 R-195，必须先恢复它
- 用户挂起: 是；用户明确选择暂存 R-195，待 R-236 完成后恢复。
- 阻塞: 2026-08-14 复核:原阻塞「待 R-236 完成后决定恢复本条」的前置已达成——R-236(上下文压缩重设计)已 done 并归档。所以这条现在是纯决策点,不是等待:candidate 记忆的晋升与清退闭环要不要现在恢复推进。相关:R-235(28 条零证据 active 记忆逐条拍板)是同一批记忆治理工作,两条宜一起定。解除动作: 用户说恢复即按 P2 入队。解除人: 用户。

## R-235 存量 28 条零证据 active 记忆逐条复核:保留(存量豁免)或降级 candidate,用户拍板 [todo]
- 优先级: P3
- 内容: 对 28 条零证据 active 记忆逐条复核:保留(存量豁免,接受不可计量)或降级 candidate(严格符合无来源不入 active,代价是不可检索注入)。复核结果与依据落到 memory 系统设计文档或本条目关闭证据。
- 复杂度: 小
- 来源: R-213 关闭时盘点发现(R-213 验收③处置的承接)
- 标签: 后端
- 背景: R-213 盘点:state.db 311 条 episode、memory_sources 0 行,project 域 28 条 active 记忆(M-001~M-063)全部零证据(global 域无条目)。这些是 provenance 门禁上线前由用户/交互会话/manager 产生的既有资产,source 字段均无机器可链接的 run_id,历史回填=变相伪造,不可行。R-213 的处置定为存量豁免+文档化,但控制平面「用数据判断记忆是否改善决策」对这些条目无法计量,保留还是逐条降级应由用户拍板。
- 验收: ①28 条清单逐条给出保留/降级结论与依据;②结论落地(设计文档或关闭证据);③如选择降级,操作后搜索不再命中 candidate 条目。
- 阻塞: 用户: 28 条零证据 active 记忆保留(存量豁免)或降级 candidate 需用户逐条拍板,解除权不在 agent。解除动作: 用户给出拍板结论(全部保留 / 逐条降级清单)后按结论落地并关闭。解除人: 用户。

## R-193 plan勾选响应延迟优化需求 [doing]
- 复杂度: 中
- 标签: 前端
- 验收: plan勾选项点击后实现即时视觉反馈和状态更新
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-193
- 批次: 0/1
- 进展: 2026-08-16 认领并勘察。现状:todo/当前计划面板由 tool-end 事件的 display.kind==='todo' 渲染(07-events.js:223-224 renderTodoPanel),条目纯展示状态文本(○/●/✓/×,07-events.js:52-63),无任何点击/勾选交互绑定。R-193 验收『plan勾选项点击后即时视觉反馈和状态更新』隐含存在可勾选的 plan 列表,但当前实现无勾选交互、无内容/来源字段、未指明勾选动作应写入哪个真源(前端本地态?后端 todo 状态?)。按 §1.1 需求边界不清不得自行假设实现;需用户澄清:①plan 指当前计划(todo)面板还是其它列表;②勾选后状态写哪里(仅前端视觉 / 调用某命令持久化);③当前是否有已感知的『响应延迟』具体场景(点击无反馈?状态更新慢?)。本条保持 doing,待澄清后实现。批次:0/1。
- observed_head: 273c4a3cc6138331a4c07469127773835af001ef
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786656932641
- 阻塞: 用户: R-193 缺内容/来源/交互定义,验收仅一句『plan勾选项点击后即时视觉反馈和状态更新』;需用户澄清:①plan 指哪个面板(当前计划 todo 面板还是其它);②勾选动作状态写哪里(前端视觉 / 后端命令持久化);③当前『响应延迟』的具体场景。解除动作:用户给出澄清后实现。解除人: 用户。

## R-101 桌面端/前端 E2 测试 harness 与延期 E2 清单 [doing]
- 复杂度: 大
- 优先级: P0
- 归属: kanzei
- 背景: 多条缺陷按 conventions §1.2「可用即关闭」关闭,其验证增强项收拢至此,不再阻塞缺陷与需求推进;此前反复出现的阻塞原因是仓库无 package.json、无浏览器测试 harness,无法安全启动真实 Tauri UI。
- 验收: 建立可在测试基座安全启动真实 Tauri UI(或等价 WebView 驱动)的 E2 harness;逐项补齐延期 E2:D-051 桌面权限弹窗真实 UI E2;D-055 切回进程补发 pending ask 前端 E2;D-056 运行中切项目→终态复位 E2;D-060 update/close/reorder 手写内容保留与并发写入回归;D-064 注入故障的 run_task 收尾 E2;D-066 真实 Tauri Window/provider 停止 E2;D-086 runner 级 task→subagent read 拦截执行回归;R-139 bash 硬门禁桌面端真实模型工具调用 E2(2026-08-08 R-139 关闭时转入,验收条款外残余验证);D-202 真机 Event Timing/长任务量化(几百轮会话下 Event Timing 数据 + 侧栏点击 <200ms,2026-08-10 D-241 处置时转入)与 D-202 DOM 节点数上界(对话渲染上限策略,窗口化/折叠历史/分页任一)。
- 拆批(2026-08-08 用户定调「拆出能先做的部分」): **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。**留待 R-086**——依赖会话事件路由的三条:D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位。基座 + 四条 E2 交付即可关闭本条,剩余三条并入 R-086 验收。
- refs: R-086
- 阶段: 3

- 标签: 流程

- 拆批: 2026-08-08 用户定调「拆出能先做的部分」: **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。基座 + 四条 E2 交付即可关闭本条;R-086 已于本轮按 §1.2 可用即关闭关闭,原「并入 R-086 验收」的三条桌面 E2(D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位)留在本条目验收清单执行。

- 进展: 2026-08-16 复核:阻塞解除条件①已满足(node --version/node --check 实测放行,cargo build -p kanzei-app 已启动未拦),当场清空阻塞,恢复推进 B1 基座验证。当前 target/debug/kzapp.exe 构建于 08-12 04:30,早于 CDP 注入提交 695305d(08-12 10:07),需重建后跑 node scripts/e2e-smoke.mjs。
- 状态纠正(2026-08-09): doing→todo。用户已挂起本条,实际不在推进中,却按旧 §1.1 口径占用 doing 名额,与 R-148 一起把 R-153 拒之门外(见 D-219)。恢复推进时再转 doing;挂起前提的小缺陷中 D-185/D-184 仍 open。

- 阻塞: 2026-08-14 分成两半看:①「重建 kzapp」这一半已达成——build-9a06e05 已发布,含 CDP 注入提交 695305d 之后的全部代码,不再是 08-12 的旧构建;②真正卡住的是 D-319——WebView2 Runtime 151 在本机 DevTools 端口不绑定(9 轮实验证据链),e2e-smoke connectOverCDP 20 秒超时,换新构建也绕不过环境问题。另有用户 2026-08-13 的 park 定调(专注 D-318,D-318 现已 fixed 归档)。解除动作: 先解决 D-319(重装/更新 WebView2 Runtime,或拍板改 WebDriver/tauri-driver 路线),再用新版跑 node scripts/e2e-smoke.mjs 验证 B1 基座。解除人: 用户(D-319 那条)。

- 批次: 0/8

## R-059 子代理独立升级与移动端通知交互支持 [todo]
- 复杂度: 大
- 优先级: P3
- 原始描述: 手机端可实现子代理和主要代理的交互和通知展示,同时子代理升级为管理项目的容器,可独立于项目存在
- 验收: ①可配置主/子代理间的消息双向通信 ②实时显示来自主要及次级代理的通知推送 ③支持子代理独立升级为管理项目容器(不依赖具体项目结构)
- 已完成: SQLite v2 持久化 agent_notifications 与 delivery_cursors 并有跨重建回放测试(kanzei-core/src/store.rs:496-513/173-256/641-656);运行开始/成功/失败真实写入通知;本机认证 HTTP 桥接已接线(kanzei-app/src/main.rs:1785-1942,回环监听 + bearer 鉴权,提供 health/notifications/messages),设置页有启停按钮;设计文档 docs/design/r059_mobile_agent_communication.md 对边界诚实。
- 退回原因: 2026-08-07 验收核查发现验收三条一条都未实质达成(验收原文要求"在移动端完成")。①双向通信未实现:InMemoryBroker 只被测试使用,生产代码零调用;POST /v1/messages 只把 payload 写成 mobile.message 事件(main.rs:1881),全仓库无任何消费方,消息进库即死信;且该端点因 Content-Length 解析缺陷恒返回 400(见 D-063),从未真正工作过。②移动端实时显示未实现:不存在任何移动端工程,只有本机轮询端点无推送;通知 agent_id 硬编码 "primary"(2532),次级代理从不产生通知。③"子代理升级为项目容器"是空壳:agent_container_*(1944-2013)只往 manifest.json 写字符串,无任何运行时读取,与 SubagentRuntime 零关联,前端"升级到 2"硬编码版本号。
- 下一步: 已完成的属"阶段 B 桌面桥接",应作为独立子需求单独验收;本需求保留移动端三条验收,待用户排期。
- 遗留质量问题: HTTP 桥接与 agent_container 三命令零测试;通知端点要求 thread_id 但无任何端点可枚举 thread,客户端无法自举。
- refs: D-063
- 阶段: 5
- 证据等级: E4
- 设计定位: 功能需求(2026-08-08 用户定调:R-093 的"质量先行"阶段门槛作废,按普通优先级参与取活)

- 标签: 后端

- 进展: 2026-08-08 复核:验收三条原文要求「在移动端完成」,本仓库不存在移动端工程;2026-08-07 退回原因明确本需求保留移动端三条验收、待用户排期。桌面桥接(阶段 B)属既有能力,按退回意见应拆为独立子需求,不在本条验收范围内。
- 阻塞: 用户: 需对移动端三条验收(双向通信/通知推送/子代理升级容器)排期并确认交付载体(真实手机端工程或 web 模拟端)。解除动作:用户拍板移动端交付形态与排期,再按新载体拆子需求动工。

## R-242 会话投影真源切换与分段清空恢复 [todo]
- refs: D-209 D-342 R-236 docs/design/deepseek_harness_upgrade.md
- 依赖: R-241
- 内容: 在 shadow gate 通过后，将 conversation_get/list、runner prior、子代理 transcript 和 UI 历史恢复逐项切到事件投影；进程内 Vec<Message> 仅作缓存。清空对话追加 conversation.reset 并开启新 segment，新 segment 的模型 prior 为空，旧 segment 仍可审计。验证期保留 legacy snapshot 只读对照，五条读路径全部稳定后停止新增 conversation.updated。
- 复杂度: 大
- 批次: 2/4
- 来源: 2026-08-14 DeepSeek Harness 升级方案；用户确认清空保留、删除确定性物理清除并弹窗提示风险。
- 标签: 核心
- 边界: 本需求只负责事件投影真源切换与 segment reset，不实现会话物理删除、Spill artifact 联动删除、WAL/VACUUM 或迁移备份安全整理；这些统一由 R-245 的删除计划与显式整理入口承担。第一批不改事件 format_version 与 SessionFact 公共词表；任一读路径可通过 feature gate 独立回退 legacy snapshot。
- 迁移与回滚: 不新增表、列或索引时不创建空 migration。切换按五条读路径分别启用 feature gate，legacy snapshot 在观察期只读保留；任一路径出现未知差异即回退该路径。全部 gate 稳定后才停止新增 conversation.updated，既有 snapshot 不删除。
- 阻塞: 2026-08-14 前置已满足一半:含 R-241 的安装版本已发布(build-9a06e05,R-241 typed event 真源已 done 并归档;此前装机版停在 08-09,根本产不出 shadow 样本)。剩的是攒样本,不是等决策:装新版后正常使用,直到达到门槛——至少 30 个真实 turn;typed_write_errors 为 0;正常可比较 turn 全部 equal=true;停止、权限拒绝、工具错误、多工具部分完成及受控 draft/tool 重启路径均有可解释证据。解除动作: 用户装新版后正常用几天,样本达标即开工五条读路径真源切换(R-243 随后串行)。解除人: 使用量自然积累(不需要拍板)。
- 验收: ①五条读路径从同一事件日志恢复一致消息；②user/assistant/tool 各安全边界强杀后重启无已发生事实丢失；③孤立 tool call 投影为 interrupted 且不自动重放；④conversation.reset 后新 segment prior 为空但旧 segment 可审计，重复 reset 幂等；⑤至少30个真实 shadow turn 达标，typed_write_errors=0、正常可比较 turn 全部 equal=true、未知差异为0；⑥五条 feature gate 可独立回滚，回滚后 legacy 行为与切换前一致；⑦对照稳定后停止新增 conversation.updated，既有 snapshot 仍可只读回放。
- 优先级: P1

## R-243 Surface Compaction 追加事务：原始事件不变、上下文由 surface 投影 [todo]
- refs: R-236 D-209 docs/design/context_compaction.md docs/design/deepseek_harness_upgrade.md
- 依赖: R-242
- 内容: 将现有 compact_with_digest 的存储语义改为 compaction_started→compaction_summary→surface_replaced→compaction_ended 追加事务；模型上下文只消费 surface projection，原始 Session 事件不修改不删除；连续压缩走已交付滚动合并。
- 复杂度: 中
- 批次: 0/3
- 来源: DeepSeek Harness compaction 事件事务；复用已交付 R-236 的纪要模型、模板和质量闸。
- 标签: 核心
- 边界: 不重写 R-236 纪要算法、压缩模型配置和质量闸；不把 Memory 作为对话恢复源。Compaction 只在 R-242 正式 surface projection 上追加事务，失败保留原 surface，未完成事务在恢复时显式失效；不修改 format_version=1 的既有消息事实。
- 阻塞: 等待 R-242 完成五条读路径真源切换并冻结正式 SessionProjection/segment 语义；R-243 与 R-242 由同一主线串行实施，不交给并行自举线。
- 验收: ①压缩前后 raw event hash 不变；②边界上的 tool call/result 必须完整配对，否则拒绝压缩；③不完整 compaction transaction 重启后不生效且有可见诊断；④连续两次压缩 replay 一致，首段关键实体仍保留；⑤模型 surface 变短但 transcript/audit 仍能回看原文；⑥R-236 全部压缩回归保持通过。
- 优先级: P1

## R-245 Tool Result Spill 与显式空间整理：完整 artifact、可恢复引用、无自动过期 [todo]
- refs: D-209 R-180 D-297 D-298 R-242 docs/design/deepseek_harness_upgrade.md
- 依赖: R-242 R-244
- 内容: 统一工具结果为 Inline 或 Spilled{preview,artifact_id,bytes,sha256,retrieval_hint}；read 优先指向原文件 offset/limit，bash、git、test_record、web 等完整原文进入与 state.db 同生命周期的 Git 忽略运行目录。提供存储与整理入口，按类别、会话、日期、大小预览占用，支持清理无引用 artifact；经风险确认后，用可恢复失败的删除计划物理删除已选会话的事件、投影和引用 artifact；并支持 SQLite checkpoint、VACUUM 与迁移备份管理。默认不自动过期。
- 复杂度: 大
- 批次: 0/5
- 来源: DeepSeek Harness spill policy、本地 state.db 输出分布统计，以及用户确认“不自动过期但需要显式整理入口”。
- 标签: 核心
- 边界: 任何事件仍引用的 artifact 不得被静默清理；整理前显示预计释放空间和不可恢复范围，执行后给清单与实际释放量。32 KiB 先做 shadow telemetry。普通会话删除保证产品不可检索且重启不复生；安全整理才处理 SQLite freelist、WAL 和含旧正文备份。当前库为 WAL、secure_delete=OFF、auto_vacuum=NONE，不能把 DELETE 行等同磁盘字节已擦除。弹窗必须区分仅删除与删除并安全整理，取消零写入；显式整理不是定时任务。
- 迁移与回滚: artifact 原子写入后再提交引用事件；任一步失败不得留下有效事件指向缺失文件。删除使用引用图和事务清单，失败可重试；schema 迁移前备份。关闭 Spill 可回到 Inline，但已有引用仍必须可读。
- 阻塞: 未完成依赖 R-242、R-244(依赖式阻塞,非人工 park)。原文里「R-244 是否列入主任务待用户决定」这一条已在 2026-08-14 由用户拍板:R-244 列入主任务、主线串行做。故本条剩纯依赖:等 R-242 固定 segment/会话投影边界、等 R-244 冻结 Result Policy 与 ToolOutput 公共契约;契约冻结后 telemetry、artifact 适配和整理 UI 可拆给自举线,物理删除与安全整理事务仍由主线审查。解除人: 依赖自然解除(R-242 + R-244 完成即解)。
- 验收: ①32 KiB shadow telemetry 不改变模型输入并产出按工具分布；②Spill 原文 sha256 与工具原输出一致，重启后可取回；③事件提交与 artifact 写入故障注入无悬空引用；④明确无自动过期任务；⑤整理入口列出总占用、数据库、WAL、freelist、artifact、无引用文件和迁移备份并支持 dry-run；⑥清理引用中 artifact 被拒，清理无引用 artifact 成功且释放量可核对；⑦删除弹窗列出会话事件、轨迹、草稿与 artifact，仅删除和删除并安全整理差异明确，取消零写入；⑧确认删除后事件、投影和引用 artifact 产品层不可检索且重启不复生，删除计划任一点失败可恢复重试；⑨安全整理仅在运行静止时执行，成功后 checkpoint、VACUUM 与备份处置可核对，busy 或失败不静默；⑩权限、路径逃逸、不可预测文件名和磁盘配额有测试。
- 优先级: P1

## R-246 LineRuntime 统一资源 owner：幂等 dispose 与持久服务显式移交 [todo]
- refs: R-174 R-175 R-180 D-275 docs/design/session_state_and_line_runtime.md docs/design/deepseek_harness_upgrade.md
- 内容: 建立 LineRuntime，统一持有 cancellation token、active run、child agents、transcript projection、background results、notifications、background processes、writer/read leases、worktree binding 和 temporary artifacts。dispose 幂等且并发调用共享同一完成 future；persistent 服务必须通过 adoption 事件显式移交 ProjectRuntime。
- 前置: R-241 R-244
- 复杂度: 大
- 批次: 0/5
- 来源: DeepSeek Harness Scope 生命周期约束；Kanzei 已有 cancellation、子代理、transcript、notification、background process 多注册表。
- 标签: 核心
- 边界: 不重做 R-180 已交付的长驻服务注册表和日志；以适配/收口方式接入。普通资源生命周期不超过 LineRuntime；persistent 只能显式 adopt，不接受布尔值或 drop 泄漏式脱离 owner。
- 阻塞: 依赖 R-244 的 Tool Pipeline 契约冻结(R-244 已于 2026-08-14 由用户定为主线串行的主任务,不再是「是否列入」的悬案)。本条自己那半——是否交给自举线实施——仍待用户定,但不必现在定:R-244 落地前本条无论如何开不了工。解除人: 依赖自然解除后由用户决定实施线路。
- 验收: ①并发两次 dispose 共享完成结果且只收尾一次；②取消子代理并等待退出，三种终态均释放读槽；③非 persistent 后台进程、通知订阅、临时 artifact 和租约全部回收；④dispose 返回前工具 wrapper 已静止且生命周期终态落库；⑤persistent 服务显式 adopt 后跨 run 存活并有 adoption 事件，未 adopt 的全部收回；⑥强杀重启后无幽灵 owner，能确定恢复或标失败；⑦R-174/R-180 现有测试保持通过。
- 优先级: P2

## R-248 先行调研内建:新方向开工前默认产出「已有方案对照」,不靠用户开口 [todo]
- refs: R-221 docs/design/research_mode.md
- 依赖: R-221
- 内容: 把「先查已有方案再动手」从用户每次口头要求变成 harness 的默认动作。①触发判据机械可判、不交模型自由裁量:项目根首次初始化 `.kanzei/`、req add 时 refs 为空且标签为核心、用户显式发起,三者之一成立即触发;②产物落 `.kanzei/research/<topic>/prior-art.md`,每条结论含「方案名 + 出处(URL 或 file:line) + 与本课题的差异 + 采用或不采用的理由」,**外部已有实现**(开源方案、协议、公开设计)与**仓内既有设计**(docs/design/**、requirements/defects 现存与 archive)两侧都必须覆盖;③新方向判定成立而无对照工件时,req add 要求 refs 指向该工件,或由用户显式豁免并留痕。
- 复杂度: 中
- 批次: 0/3
- 来源: 2026-08-14 用户观察——开新项目应先深度调研已有方案与设计,不适合从零开始;这是当前 coding agent 的通病(非得用户主动请求才去调研),直接影响自举质量。
- 标签: 核心
- 边界: 不是每条需求都调研,只在触发判据成立时启动;判据必须机械可判,不接受模型自行裁量「这算不算新方向」。websearch 轮次设上限,不做无限扩散爬取。本条只产出对照工件与开工门禁,不改 req/defect 状态机,也不自动把调研结论写成条目——那是 R-221 定调点4 的回流通道。
- 阻塞: 2026-08-14 用户定调已解决定位问题——research 是独立深度研究模式,主形态即「文献 + 仓库的深度调研」,网络检索是主力;本条的先行调研正是该形态的一种应用,归属明确,原「网络检索被降级为辅助」的矛盾消失。用户同时定调**本条不拆批**(仓内既有设计对照那半也不提前落 dev),整条跟随 R-221 排在 dev 稳定之后。故阻塞= R-221(排期 + §1 重写后重推定调点),解除人:用户。
- 验收: ①三种触发判据各有定向测试,未触发的普通条目不受影响;②prior-art.md 每条结论都带出处,无出处结论被机械拒绝(复用 V0 标注同一套校验);③外部与仓内两侧覆盖各有独立断言,只查一侧不算通过;④新方向下 req add 缺 refs 被拒,豁免路径留痕可审计;⑤websearch 轮次上限有实测,超限给明确诊断而非静默截断;⑥既有 req add 路径无回归。
- 优先级: P1

## R-249 工具结果可返回图片:ToolOutput 承载 image part,打通图片读取与 UI 截图 [doing]
- refs: R-014 R-101 R-244 R-245
- 依赖: R-244
- 内容: 现状 `ToolOutput.content` 只有 String(kanzei-harness/src/tool.rs:178),任何工具都无法把图片交给模型;`Part::Image` 的三协议映射早在 R-014 交付,但入口只有桌面端用户附件(kanzei-app/src/state.rs:29)。本条把 ToolOutput 扩成可携带 image part,并打通两个消费点:①`read` 读图片文件(PNG/JPEG/WebP/GIF)按 media_type 编码返回;②UI 自检补截图通道——现有 `ui_probe` 窗口通道加 `screenshot`,让 ui_dom/ui_style 的结构读数配上真实渲染画面。
- 复杂度: 大
- 批次: 0/4
- 来源: 2026-08-14 三系统工具面对照(DeepSeek harness / Claude Code / kanzei):read_image 是唯一的能力硬缺口。桌面端 ui_dom/ui_console/ui_style 能读结构与数值但看不见渲染结果,对齐、遮挡、观感一类问题无法自查。
- 标签: 核心
- 边界: ToolOutput 是 harness 核心契约,R-244 明确要冻结「ToolOutput 公共契约」、R-245 要把它改成 Inline/Spilled 二态——本条**不得抢在 R-244 之前改这个结构**,否则必然返工。图片体积走 R-245 的 spill 口径,不在 ToolOutput 内联大 base64。不实现 UI 点击/输入/滚动(那是 R-101 的 E2 harness 范围),本条只做「看得见」不做「动得了」。deepseek_responses 协议当前丢弃 Image part,本条不负责补齐该 provider,但要在 provider 不支持时给出显式降级提示,不静默丢弃。
- 进展: 2026-08-14 批1 交付(1831239)。勘察修正了原条目的一处前提:`Part::Image` 的三协议映射早在 R-014 就通了,缺的只是**工具侧出口**,协议层零改动即可打通——不必等 R-244。实现:①ToolOutput 增 images 载荷(空 vec 与既有行为逐字节一致,53 处 `ToolOutput {` 里只有 4 个真构造点,其余是解构模式);②read 按 magic bytes 而非扩展名识图(PNG/JPEG/WebP/GIF),扩展名撒谎会让 media_type 与真实字节不符、provider 400 且报错指向请求体;③图片 Part 只能追加在所有 ToolResult 之后——Anthropic 要求 tool_result 块在 user 消息最前,而 results[i]↔calls[i] 由 note_step 的 debug_assert 锁着,中间也不能插;④provider 不支持时**在进 messages 前**降级为显式文本说明,判据收敛为 Route::supports_images() 与 client.rs 硬拒绝共用一处。新增 10 条测试。**剩余批次**:批3 图片 artifact 走 R-245 spill(仍等 R-244/R-245);批4 deepseek 协议补齐(不在本条范围,只保证降级不静默)。 || 2026-08-14 批2 交付:新增 ui_screenshot 工具(kanzei-app/src/screenshot.rs)。实窗验证三轮才对,两次假绿都值得记——①未声明 DPI 感知时 GetWindowRect 返回虚拟化坐标(2582px 的窗口报成 1295px),抓到的是横跨多个窗口的错误区域;②改用正确矩形后,屏幕 DC 抓取拿到的是压在上面那个应用的界面(kzapp 被完全遮挡),内容丰富所以 looks_blank 一路放行。两次都是「测试通过但抓的不是那个窗口」。最终改用 PrintWindow+PW_RENDERFULLCONTENT 离屏渲染,免疫遮挡,在完全被盖住的状态下抓到 kzapp 完整界面并经人眼与用户实拍逐项比对一致;屏幕 DC 仅在 PrintWindow 失效且本窗口为前台时作回退,不是前台宁可报错——返回别人的界面比返回错误坏得多。测试记录 T-1786705800。
- 阻塞: 批1 已解除(协议层无需改动,不依赖 R-244)。批2 无阻塞、随时可开工——本条目前挂 doing 且被阻塞字段整条 park,调度看不到批2;R-202 关闭腾出 WIP 槽后应直接续做批2(不需要用户拍板)。批3 仍等 R-244 冻结 ToolOutput 公共契约与 Result Policy、R-245 确定图片类 artifact 的 spill 落点——原文「R-244 是否列入主任务待用户决定」已于 2026-08-14 拍板:列入主任务、主线串行做,故批3 是纯依赖等待。解除人: agent(批2)/ 依赖自然解除(批3)。
- 验收: ①read 读 PNG/JPEG/WebP/GIF 各有定向测试,media_type 正确,非图片文件走原文本路径无回归;②ui_probe screenshot 返回的图片能被模型消费,桌面端实测有轨迹;③provider 不支持图片时有显式降级诊断;④图片 artifact 走 R-245 spill,ToolOutput 不内联超阈值 base64;⑤R-014 既有附件路径逐条无回归;⑥ToolOutput 结构变更后既有全部工具返回路径编译通过且行为不变(机械核验)。
- 优先级: P1

## R-255 MemoryStore 收缩回仓储:准入/生命周期/合并/检索/效果画像/收件箱/迁移七域迁出(2073 行生产码) [doing]
- refs: R-216 R-195 R-235 R-155 D-366 docs/design/memory_control_plane.md docs/design/monolith_decomposition_round2.md(批次地图:B 节)
- 为什么是这个形态: 它比 run.rs 更迷惑,因为"都和 memory 有关"看起来内聚——但语义相关不等于同一个抽象。真正的危害在可迭代性:准入(什么有资格成为记忆)、压缩、合并、episodic 到 semantic 固化,是记忆研究里变更最频繁的一层;把它锁在 Repository 私有逻辑里等于让 research policy 与 storage mechanics 强耦合,每做一次记忆实验都要动仓储。
- 内容: 分三刀,每刀独立可提交可回滚。第一刀(零行为变更,最容易):inbox 一族、migrate_legacy、hit_profile/hits_map 三块迁出成 memory/inbox.rs、memory/migration.rs、memory/telemetry.rs。第二刀:准入策略从 add 提成 MemoryAdmission(枚举校验/description 必填/近似标题判重/refs 契约/subject 不变式/指纹与新颖度),生命周期从 promote/deprecate 提成 MemoryLifecycle(candidate 老化、晋升、清退、provenance 门禁),Store 只接 save/load。第三刀:检索与排序迁进 retrieval 子目录并与 memory/index.rs 收口(见配套缺陷:index 反过来调 store.search 取 BM25,排序有两个落点)。最终 MemoryStore 只剩 load/save/archive/事务原子性。
- 复杂度: 大
- 来源: 2026-08-15 第二轮巨石扫描 R2。
- 标签: 核心
- 现状(2026-08-15 实测): crates/kanzei-memory/src/memory/store.rs 总 4085 行,其中同文件测试 2012 行、生产码 2073 行(生产行数全仓第三)。名字叫 Store,实际至少七种变更理由同居:①文件持久化与归档(L1-212);②add(L232 起)不是 CRUD 而是 校验→准入策略→指纹/新颖度→同名判重→subject 冲突→落盘→派生索引刷新 一条链;③promote(L533 起)带 provenance 硬门禁(episode 必须真实存在、证据先落库才转 active);④candidate→active→deprecated 生命周期状态机;⑤检索与排序(search L960:BM25 + 状态加权 + 采纳率决策加权 + 命中追踪 + snippet);⑥ID ledger/void/重复合并/完整性审计;⑦效果画像 hit_profile(L1529)/hits_map(L1551);⑧收件箱 read_inbox/clear_inbox/append_note/pending_notes(L1569-1727);⑨migrate_legacy(L1728)迁旧版 memory.md。
- 边界: 不改记忆的对外语义与 CLI/桌面/工具契约(kz memory、Memory 页、memory 工具一字不动);不动 SQLite schema 与派生索引结构;不趁机改召回排序权重(那是 R-150 系的独立话题,混进来无法归因);不删零调用 pub 方法。
- 验收: ①store.rs 生产行数 ≤ 600;②准入策略有独立可测入口(不经 add 也能构造场景测),生命周期同理;③检索/排序实现只有一处(机械核验:BM25 与状态加权代码只出现在 retrieval 侧);④memory crate 全量 + workspace 全量绿;⑤召回质量无回归:同一组 query 在拆解前后 top-k 命中集合一致(给出对照,不接受"应该没变");⑥迁出后做一次真实记忆实验(如调准入门槛)只需改 admission 一处,给出 diff 为证。
- 优先级: P0
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-255
- 批次: 2/3
- 现状(2026-08-16 实测复核): store.rs 现 3836 行(生产码 1506、测试 1824 起)。七域同居确认:add(252-461)/promote(557-634)/reconcile_candidates(635-710)/search_candidates(993-1066)/merge(1343-1446)/inbox(1593-1750 已迁)/migrate_legacy(1754-1804 已迁)/hit_profile(1553-1591 已迁)。D-366 已修:检索排序在 index.rs,store 只产 SearchCandidate(见 SearchCandidate 注释)。
- 进展: 批2 完成已 push(680baf8):第二刀——准入策略从 store.add 提纯为 memory/admission.rs(MemoryAdmission:validate_basic/subject 不变式/交付状态拒收/指纹一致性/精确+近似判重+机械辅助 has_tracker_id/normalize_title/title_containment 等),生命周期从 promote/reconcile_candidates 提纯为 memory/lifecycle.rs(MemoryLifecycle:promote_guard provenance 硬门禁/should_promote/should_deprecate);store 的 add/promote/reconcile_candidates 变薄壳(只查条目+分流+落盘),行为零变更;admission/lifecycle 各带独立可测入口(8 个新测试不经 store 直接构造场景,验收②证据);store.rs 生产码 1506→1346;kanzei-memory 138 passed(T-1786791658),clippy 零警告。批3 下一步:第三刀——检索/排序迁进 retrieval 子目录并与 memory/index.rs 收口(D-366 已修,store 只产 SearchCandidate,机械核验 BM25/状态加权只在 retrieval 侧,验收③)+ novelty 域(classify_novelty/record_novelty/bump_recurrence/recurrence_count)与 ID ledger 域(merge/voided/integrity)迁出,store.rs 生产码 ≤600(验收①);批3 收尾:memory crate 全量+workspace 全量(验收④)+ 召回质量对照 top-k 一致(验收⑤)+ 真实记忆实验 diff(验收⑥)+ close。
- observed_head: 680baf83830e6ae1b0003fc5f84c3138ce1bd4cd
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786791718350

## R-256 Desktop 与 CLI 共用 RunService:kz main.rs 的第二套 application layer 收敛,两端只剩 EventSink/AskRouter/RuntimePolicy 之差 [todo]
- refs: R-253 R-254 R-255 R-183 docs/design/monolith_decomposition.md
- 为什么是这个形态: 两端各写一遍编排的直接代价是 每加一个运行期能力就要改两处,而且只有一处会被真正验证(桌面端有人用、CLI 靠自举跑);R-183 的非交互放行、R-186 的越界回滚、记忆召回这些都落在编排层,双份实现会持续漂移。把 main.rs 切成 8 个文件解决不了这个,共用 service 才能。
- 内容: ①抽出 RunService(或等价的单一编排入口),桌面 run_prompt 与 CLI run 都只调它;②两端差异收敛成三个注入点——事件汇(UI EventSink vs 终端 EventSink)、询问路由(交互 AskRouter vs 非交互 AskRouter)、运行策略(桌面 RuntimePolicy vs CLI RuntimePolicy);③CLI 侧剩下的 replay eval / memory manager / tracker / work / config / lock / worktree 各自成模块,main.rs 收敛为命令分发 + 装配;④先做只读对照:把两边的装配步骤逐项列表比对,差异逐条判定是 有意的 还是 漂移的,漂移的先对齐再合并——不要在合并动作里顺手改行为。
- 复杂度: 大
- 来源: 2026-08-15 第二轮巨石扫描 R4;收益最大、风险也最大,排在前三条把边界稳定下来之后。
- 标签: 核心
- 现状(2026-08-15 实测): crates/kanzei/src/main.rs 总 2216 行、生产码 1590 行。问题不是 CLI 子命令多,是它自己又实现了一遍 harness 装配、agent 选择、模型解析、LLM route、RunnerConfig、ToolCtx、记忆召回、typed events、run_once、Ctrl-C 处理、落库——与桌面端 run.rs 的 assemble_run 概念重复。此外还叠着 replay eval、memory manager、tracker CLI、work CLI、config、lock、worktree 等适配层,形态是 CLI presentation + application orchestration + domain adapters 三合一。
- 边界: 排在 R-253/R-254/R-255 之后,前三条未稳定前不动(边界还在漂就合不出正确的公共面);不改 CLI 命令面与桌面 IPC 面;不引入新的运行期能力;合并过程中发现的行为差异一律登记为独立缺陷,不在本条顺手改。
- 验收: ①harness 装配/agent 选择/模型解析/RunnerConfig/ToolCtx/记忆召回/typed events/run_once 只有一处实现(机械核验:grep 只剩一个装配点);②桌面端与 CLI 各跑一次真实闭环(改代码→跑测试→提交)无回归;③kz main.rs 生产行数 ≤ 500;④第③步的漂移对照表落进设计文档,逐条给出 有意/漂移 判定;⑤workspace 全量绿 + 前端冒烟绿。
- 优先级: P1

## R-257 第二梯队模块化:drive.rs(1826)/docstore.rs(1417)/git.rs(1257)/harness config.rs(1218) 按域切分 [todo]
- refs: R-155 R-202 R-204 R-253 R-257 docs/design/monolith_decomposition.md
- 为什么优先级低于前四条: ②③④ 的职责虽多,但都围绕单一 bounded context(结构化文档存储 / git 交付 / 配置),是 large cohesive module 而非 God Module,不改动就不痛;真正需要盯的是 ①drive.rs 与 ③git.rs 的 finalize——前者是运行核心,后者已经跨出适配器语义。
- 内容: ①drive.rs:先做只读复查,判定 R-202 之后剩下的 1826 行里哪些是 模型循环本体(应留)、哪些是 可迁出的子域(工具执行/重试/流恢复/指标),再决定切法——本条不预设结论,复查结论回填本条后再排批次;②docstore.rs 切 model/parse/render/repository/archive/validation;③git.rs 切 tool/commands/diff/finalize,finalize 明确按 workflow 对待而不是一个 git action;④config.rs 按配置域分组,测试随域下沉。
- 复杂度: 中
- 来源: 2026-08-15 第二轮巨石扫描 R5,外加机器复核补上的 drive.rs。
- 标签: 后端
- 现状(2026-08-15 实测生产行数): ①crates/kanzei-core/src/runner/drive.rs 2058 总/1826 生产/7 处 too_many_arguments——R-155 B8 只做整体搬迁、R-202 拆了 run_task 与 run_once_with_parts 内部,但它仍是模型循环的核心且是全仓生产行数第四,这轮用户榜单漏了它,列本条队首;②crates/kanzei-memory/src/docstore.rs 2471/1417——requirements/defects/sources/findings 四类的统一结构化 markdown engine,parsing/status 语义/锁/原子改写/归档/ID/raw-line 保真/模板同居;③crates/kanzei-tools/src/git.rs 2318/1257——GitTool 从 status/diff/log/stage/commit 长到 finalize 交付工作流(跑门禁、记 test_record、提交、返回 complete),已从 git 命令适配器扩张成 delivery workflow;④crates/kanzei-harness/src/config.rs 2937/1218——测试占 1719 行,生产码偏大但不失控。
- 边界: 零行为变更、零外部 API 面变更(沿用 R-155 的顶层再导出纪律);不与 R-253/R-254/R-255 并发执行(大搬迁互相冲突,见 monolith_decomposition.md 执行纪律 3);drive.rs 一项若复查结论是"当前形态合理",允许只写结论不动代码,但结论必须落进本条。
- 验收: ①四个文件各自给出拆解前后生产行数对照(按 R-257 的口径,不用 wc -l);②外部 API 面零变更断言(下游 crate cargo check 通过);③各 crate 定向测试 + workspace 全量绿;④drive.rs 的复查结论(拆或不拆、理由)明确落在本条进展里;⑤git.rs finalize 迁出后,git 只读命令与交付工作流的调用方各跑一次真实验证。
- 优先级: P2

## R-258 巨石度量口径:生产行数/测试行数/最大函数行数/参数train,禁止拿 wc -l 当门禁 [todo]
- refs: R-253 R-254 R-255 R-256 R-257 R-191 docs/design/monolith_decomposition.md
- 为什么是这个形态: 本仓大量 Rust 文件把测试放在同文件 cfg(test) mod tests 里,raw 行数与巨石程度不相关。若把文件行数直接做成 harness 门禁,自举 agent 会去"优化"测试最密集的文件——把测试搬走或删掉就能过线,正好惩罚了测试写得最足的模块。口径错了,门禁就是负向激励。
- 内容: ①出一个度量入口(kz 子命令或 harness 指标皆可),按文件产出:总行数、生产行数(扣 cfg(test) 块,按大括号配平算,不能只找第一个 cfg(test) 就一刀切——processes.rs 第一处 cfg(test) 在 L468,那是外挂测试文件声明,一刀切会把 1628 行生产码误报成 467)、测试行数、函数数、最大函数行数、too_many_arguments 处数;②给出全仓 Top-N 榜单与阈值建议;③conventions 写明阈值与超阈值动作——超了必须登记条目,不自动拒绝提交(自用工具,威胁模型里没有敌对模型,防线放可见性不放闸门);④榜单落一次快照,作为 R-253/R-254/R-255 的验收对照基线。
- 复杂度: 中
- 来源: 2026-08-15 第二轮巨石扫描的方法论副产物:用户按 GitHub 页面行数排的榜单里 tracker.rs 进了第二梯队,机器复核后发现它 3253 行中 2593 行是同文件测试,生产码只有 660 行,纯误诊;反向也成立——drive.rs 页面上 2058 行看着不起眼,生产码 1826 行、7 处 too_many_arguments,是那轮榜单漏掉的第四块真巨石。
- 标签: 流程
- 边界: 不做提交闸门、不做 CI 硬失败;不引入外部复杂度分析工具进依赖树;扇入扇出若实现成本高可降级为仅统计 use 行数并在条目里说明,不为指标齐全拖成大工程。
- 验收: ①对 crates/kanzei-tools/src/tracker.rs 报生产 660 行而非 3253;②对 crates/kanzei-app/src/run.rs 报生产 2885 行;③对 crates/kanzei-app/src/processes.rs 报生产 1628 行(验证 cfg(test) 块识别正确,不被 L468 的外挂测试模块声明骗过);④输出全仓 Top-20 榜单,drive.rs 出现在前五;⑤conventions 文本落地且 grep 单一来源;⑥基线快照可被后续拆解条目引用做前后对照。
- 优先级: P1

## R-259 pipeline Wrap 阶段收敛:timeout/cancellation/progress 只在 wrapper 实现一处(R-244 残余) [todo]
- refs: R-244
- 优先级: P3
- 内容: R-244 收口后残余(验收④未完全收敛):把 timeout/cancellation/progress 从 runner 层(drive.rs 串行 progress 旁路 + tool_exec.rs 并行 progress 通道 + bash_body 内 timeout)抽象进 harness tool_pipeline 的 Wrap 阶段,使「timeout/cancellation/progress 只在 wrapper 实现一处」字面成立。骨架已立(ToolPhase::Wrap 预留),本条目做收敛 + 契约测试(同一工具串行/并行路径走同一 wrapper,行为逐字节一致)。
- 前置: R-244
- 复杂度: 中
- 来源: R-244 批5 收口时验收④评估:progress 现实现于 runner 层两处(串行旁路/并行通道),timeout 在 bash body——功能统一但未字面收敛进 pipeline Wrap 阶段。
- 标签: 核心
- 验收: ①timeout/cancellation/progress 三能力都只在一处 wrapper 实现,工具 body 不再含三者的实现代码;②bash 超时/进度行为与 R-244 前逐字节一致(既有测试全绿);③串行与并行执行路径共用同一 wrapper 实现;④cargo test --workspace 全绿。

## R-263 设置面板暴露子代理并行上限(max_tasks_per_turn) [todo]
- 内容: 子代理并行上限 max_tasks_per_turn 目前只在 kanzei.toml [limits] 手写配置(默认 16),设置面板无入口。用户想「把派子代理的并行强度提高一点」——需要可视化入口:设置页新增「子代理并行上限」输入(数字,1~N),保存写入 kanzei.toml [limits] max_tasks_per_turn(向后兼容 serde default),并透传生效。
- 复杂度: 中
- 来源: 2026-08-15 用户反馈「考虑把派子代理的并行强度提高一点」,调研确认上限可配但无 UI 入口,用户拍板方向(问题2-C)
- 标签: 前端
- 验收: ①设置页出现「子代理并行上限」输入(带说明:同轮并行 task 数上限,默认 16);②保存后写入 kanzei.toml [limits] max_tasks_per_turn,重读配置生效;③已存在的其它 limits 字段不被覆盖(向后兼容);④前端冒烟 + kanzei-app 定向测试全绿。
- 优先级: P2

## R-265 symbols 加「符号名 → 定义位置」反查,穿透跨 crate re-export [todo]
- refs: R-234
- 优先级: P2
- 复杂度: 中
- 标签: harness 流程
- 内容: symbols 现在只能「给定文件 → 列符号」与「给定符号 → 列引用点(callers)」,缺第三问「这个符号定义在哪」。新增 define 参数:输入裸名或限定路径(crate::atomic_file::try_lock_exclusive),全树按名精确命中后再解释模块路径为何对不上。**跨 crate re-export 是核心情形不是边角**,要吃下三型:①模块整体跨 crate 再导出(kanzei-tools/src/lib.rs:6 pub use kanzei_base::atomic_file);②带 as 改名(lib.rs:43/:45 pub use background::kill_process as kill_background_processes_for_process——搜新名时定义名根本不叫这个);③跨行花括号列表(tracker.rs:25-29)。配套需要 crate ident → 源码目录映射(读 workspace members 的 [package].name,`-`→`_`),toml 依赖已有不新增。
- 为什么是这个形态: 不新建工具。symbols 已注册进主代理 BaseComponent 与勘察子代理 SubagentBase,并有三处「只读快照 6 件套」硬断言(subagent.rs:170、parallel_scouting_under_serial_writer.rs:169、state_tests.rs:141);新建工具要动这些加 explore_agent 的工具清单,纯收税且与列表模式共享 90% 解析代码,拆开后两份解析器迟早漂移。callers(R-234 B2)已确立「symbols 是符号查询入口」,define 是同族第三问。
- 边界: 不引入 syn 等语法解析依赖(与 R-154 轻量哲学一致,行级扫描够用);不做 IDE 级跳转;模块路径只参与输出解释、不参与命中判定——事故成因正是「按路径字面解析→扑空」。
- 来源: 2026-08-15 自举勘察实测事故:agent 在 managed.rs:299 看到 `crate::atomic_file::try_lock_exclusive`,按字面去 crates/kanzei-tools/src/atomic_file.rs 找,扑空;真实定义在 crates/kanzei-base/src/atomic_file.rs:256,kanzei-tools 只是 lib.rs:6 再导出。同轮另修两个前置缺陷(关键字词边界假符号、表头无条件入队把命中埋掉,见提交 a26df63)——那两个不修,反查会直接给出错误答案。
- 验收: ①`symbols` 传 define=try_lock_exclusive 能定位到 crates/kanzei-base/src/atomic_file.rs 并给出经 kanzei-tools/src/lib.rs:6 的再导出链;②对 as 改名(kill_background_processes_for_process)能回落原名找到定义;③对跨行花括号再导出(tracker.rs:25-29)不漏;④define 与 callers 同时给出时显式报错而非静默取其一;⑤输出带上限与「已截断」提示,与 grep 的 DEFAULT_LIMIT 口径一致(现 callers 无上限,`callers: "self"` 能灌上万行);⑥description 的 Params 补齐 define 与 callers(callers 自 B2 起就没进描述)。

## R-264 前端迁移原生 ESM(勘察已完成,方案见 docs/design/ui_esm_migration.md) [todo]
- refs: docs/design/ui_esm_migration.md R-142 R-154
- 优先级: P3
- 复杂度: 大
- 标签: 前端 流程
- 内容: ui/*.js 现为 21 个经典 script 共享全局作用域,靠 gen-ui-lint-globals.mjs 生成的白名单补 no-undef。迁到原生 ESM(<script type="module">,仍零构建步骤)可得真模块作用域并删掉整套补偿机制。**动工前必须先重建测试 harness**,顺序不可颠倒——详见设计文档 §二/§四。
- 前置: 无(但内部三道前置 B1/B2/B3 必须先于任何前端文件改动完成)
- 为什么是这个形态: 不上打包器。打包器的收益(minify/tree-shake 业务代码)在本仓不成立,而代价是 devDependencies 从 3 个变几百个、cargo build 经 beforeBuildCommand 依赖 npm 构建、六个冒烟脚本的加载模型全部重做。原生 ESM 拿到全部核心收益且仍无构建步骤。
- 边界: 不删 vendor/monaco/basic-languages(独立决策,与本条无关);不引入打包器/TypeScript;不借机重构业务逻辑,迁移期间只改模块边界。
- 来源: 2026-08-15 用户提出「前端改成打包呢」。勘察(21 文件逐文件审计 + index.html 专项 + 外部依赖专项)结论:前端本身不是障碍(587 个真顶层符号、零重名冲突、零内联事件处理器),阻塞全在测试 harness——ui-runtime-smoke.mjs 的 6799 行断言建立在 vm.runInContext 逐文件跑经典脚本之上,ESM 下整体作废;且 ui-sources.mjs 修好正则后会出现「三个冒烟静默变绿」的失效模式。同轮用户问「做了对自举有收益吗」,结论是没有:ESM 不影响 cargo 任何耗时,前端六个冒烟合计约 4 秒;唯一收益(模型读代码时 import 自带溯源)已被 20467db 修好白名单后大体覆盖。故降为 P3 留档。
- 验收: ①B1 ui-sources.mjs 改为遍历 ui/*.js 目录并带文件数下限断言,不再解析 HTML 取清单;②B2 ui-runtime-smoke.mjs 换用可跑 ESM 的执行模型,且保住「逐文件执行以复刻浏览器多 script TDZ 语义」这一能力(设计文档 §二 B2 说明为何不能丢);③B3 __kzTest 钩子改为 08-compose.js 显式 export,冒烟改 import 取用;④以上三条完成且 6799 行断言全绿之后,才开始逐文件迁移,每迁一个文件跑一次全套六个冒烟;⑤迁移完成后删除 gen-ui-lint-globals.mjs、ui-lint-globals.json 及 ui-lint-smoke.mjs 的清单同步校验,eslint.config.js 改 sourceType: "module";⑥设计文档 §三 表格里 10 处顶层跨文件读与 6 处 typeof 守卫逐条改为显式 import 并在验收中点名。

## R-266 workspace crate 清单与 README 项目结构表机械同步 [todo]
- refs: R-258
- 为什么是这个形态: 只校验清单一致性,不生成 README。生成会把人写的职责描述冲掉,而实际漂移的一向是「新 crate 忘了写进表」而非「描述过期」——本次漏的正是 kanzei-base 与 kanzei-memory 两个新成员,表里六行描述本身都还准。校验是集合比对,零新依赖、零耗时,属于本仓一贯的做法:能确定性执行的事不靠人记。
- 内容: Cargo.toml [workspace] members 现有 8 个 crate,README ## 项目结构 表只列 6 个——缺 kanzei-base 与 kanzei-memory。①先把这两行补进表并写清职责;②加一道机械校验:从 Cargo.toml members 取 crate 名,与 README 表格第一列反引号里的 crate 名做集合比对,不一致即失败并点名缺/多的那个;③校验同时挂进 scripts/verify.ps1 与 .github/workflows/ci.yml,两处口径机械一致(CI 配置里本就要求 checklist 与 verify.ps1 同步)。
- 复杂度: 小
- 来源: 2026-08-15 第三方对 dev 分支的仓库评审指出 README 结构表落后于 Cargo.toml,实测属实(members 8 个,表里 6 个)。同轮机器核对该评审提出的另四条建议,结论是均不新增条目:①拆 git.rs 已在 R-257 ③,且在册版本定性更准(真问题是 finalize 从 git 适配器长成交付工作流,不是行数);②前端迁 ESM 已在 R-264,附完整勘察与设计文档并已明确降 P3;③coverage 阈值正踩 R-258 记的负向激励陷阱(测试与生产码同文件,搬走测试即可过线);④settings.rs 实测 857 生产行/671 测试行,够不上 R-257 第二梯队门槛(1218)。该评审的热点排名整体建立在 GitHub 页面行数上,即 R-258 明令禁用的口径——其点名的 permission.rs 1147 行里 698 行是测试,生产码仅 449;真正的生产码前二 drive.rs 1851 与 main.rs 1640 反而没被它看见。故只本条落地。
- 标签: 流程
- 边界: 不校验职责描述的内容是否准确;不扩到 docs/design 下的其它清单;不引入任何文档生成器或模板引擎。
- 验收: ①README ## 项目结构 表含全部 8 个 crate,kanzei-base 与 kanzei-memory 各有职责描述;②校验脚本存在且真能拦:临时给 Cargo.toml 加一个假 member(或从 README 删一行)后校验必须失败并点名该 crate,给出实测输出,不接受「应该会失败」;③verify.ps1 与 ci.yml 两处都跑到该校验;④对表格行顺序差异、crate 名大小写、多余空格不误报(各给一个反证用例)。
- 优先级: P2
