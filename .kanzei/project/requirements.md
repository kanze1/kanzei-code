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
- 阻塞: 2026-08-16 复核:实质前置**全部达成**——R-200 已 done 并归档、R-202 已 done(不再占 WIP 槽)、原文点名的缺陷队列 D-357/D-358/D-359 已全部 fixed 并归档、发版也已多轮执行。剩下唯一原因是队列位置:唯一 WIP 槽由 R-195 持有(用户 2026-08-16 指定)。解除动作: R-195 关闭腾出槽后按队列自然取到本条(P0,不需要额外信息);或用户改指定本条接管。解除人: 依赖自然解除 / 用户。
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
- 阻塞: 2026-08-16 复核:不存在外部阻塞,实现已完整交付且全绿,只剩验收③的数据工作(逐条查 memory 库定位 6 条交付状态记忆并归档/改写)。原阻塞成因"R-202 占着唯一 WIP 槽"已消失(R-202 done),但槽现由 R-195 持有(用户 2026-08-16 指定),故仍 park——清掉阻塞会让可执行 WIP 达 2 条,work next 直接判 wip_violation 禁止全线取活。解除动作: R-195 关闭后清本字段直接做验收③并关闭本条,不需要用户拍板。解除人: agent。
- observed_head: a104ba12af981e0e591aff0c9a5057385ce2f854
- observed_worktree_hash: fnv1a64:025c9fc9adc6d9d2
- recorded_at: 1786637389551

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
- refs: D-063 R-269 R-270 R-271
- 阶段: 5
- 证据等级: E4
- 设计定位: 功能需求(2026-08-08 用户定调:R-093 的"质量先行"阶段门槛作废,按普通优先级参与取活)

- 标签: 后端

- 进展: 2026-08-16 交付形态已拍板:PWA+现成通知桥(手机为 Android),原生壳不做(息屏通知由 LAN 推送桥零开发补齐);双向通信与通知推送两条验收的实施载体为 R-270(服务端)+R-271(PWA 界面),本条在其交付后按新载体核销;第三条『子代理升级为管理项目容器』与移动端无关,待用户重估是否保留。 || 2026-08-08 复核:验收三条原文要求「在移动端完成」,本仓库不存在移动端工程;2026-08-07 退回原因明确本需求保留移动端三条验收、待用户排期。桌面桥接(阶段 B)属既有能力,按退回意见应拆为独立子需求,不在本条验收范围内。
- 阻塞: 等 R-270/R-271 交付后核销双向通信与通知推送两条验收;第三条『子代理升级为项目容器』需用户重估是否仍要。解除动作: R-270 R-271 关闭后核销并收口本条。解除人: 依赖自然解除(R-270 R-271)/用户(第三条)。
- observed_head: 2fffa0829d54c008df04af3941bf7c3e31d6612d
- observed_worktree_hash: fnv1a64:1bbe2535e877fb93
- recorded_at: 1786819036617

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
- 阻塞: 2026-08-16 实测复核:原阻塞写的"剩的是攒样本,不是等决策"**已不准确**。样本侧三条里两条已达标:shadow_compared 样本 45 条(门槛 ≥30 个真实 turn,库内 turn_started 4168)、typed_write_errors 合计 0;但"正常可比较 turn 全部 equal=true"**未达成**——45 条里 7 条 equal=false,且**全部 interrupted_assistants=0**,即不落在停止/中断/权限拒绝这些被门槛排除的异常路径上。7 条形态一致:projected 远大于 legacy(607→2163、42→1206、87→608、227→574、146→238、873→1076、0→521),疑似上下文压缩把 legacy 快照换成纪要、而 typed 流仍留全量,属 R-243(Surface Compaction)要处理的语义。解除动作: 先解释这 7 条差异是"投影正确、legacy 被压缩"还是"投影有 bug",再决定门槛是否按压缩语义重写。解除人: 依赖自然解除 / 用户改口径。
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
- 阻塞: 2026-08-16 复核收窄:两个依赖里 **R-244 已 done 并归档**(Result Policy 与 ToolOutput 公共契约已冻结),故本条只剩等 R-242 固定 segment/会话投影边界;而 R-242 自身卡在 7 条 shadow equal=false(见该条阻塞)。契约冻结后 telemetry、artifact 适配和整理 UI 可拆给自举线,物理删除与安全整理事务仍由主线审查。解除人: 依赖自然解除(R-242 完成即解)。
- 验收: ①32 KiB shadow telemetry 不改变模型输入并产出按工具分布；②Spill 原文 sha256 与工具原输出一致，重启后可取回；③事件提交与 artifact 写入故障注入无悬空引用；④明确无自动过期任务；⑤整理入口列出总占用、数据库、WAL、freelist、artifact、无引用文件和迁移备份并支持 dry-run；⑥清理引用中 artifact 被拒，清理无引用 artifact 成功且释放量可核对；⑦删除弹窗列出会话事件、轨迹、草稿与 artifact，仅删除和删除并安全整理差异明确，取消零写入；⑧确认删除后事件、投影和引用 artifact 产品层不可检索且重启不复生，删除计划任一点失败可恢复重试；⑨安全整理仅在运行静止时执行，成功后 checkpoint、VACUUM 与备份处置可核对，busy 或失败不静默；⑩权限、路径逃逸、不可预测文件名和磁盘配额有测试。
- 优先级: P1

## R-246 LineRuntime 统一资源 owner：幂等 dispose 与持久服务显式移交 [doing]
- refs: R-174 R-175 R-180 D-275 docs/design/session_state_and_line_runtime.md docs/design/deepseek_harness_upgrade.md
- 内容: 建立 LineRuntime，统一持有 cancellation token、active run、child agents、transcript projection、background results、notifications、background processes、writer/read leases、worktree binding 和 temporary artifacts。dispose 幂等且并发调用共享同一完成 future；persistent 服务必须通过 adoption 事件显式移交 ProjectRuntime。
- 前置: R-241 R-244
- 复杂度: 大
- 批次: 2/5
- 来源: DeepSeek Harness Scope 生命周期约束；Kanzei 已有 cancellation、子代理、transcript、notification、background process 多注册表。
- 标签: 核心
- 边界: 不重做 R-180 已交付的长驻服务注册表和日志；以适配/收口方式接入。普通资源生命周期不超过 LineRuntime；persistent 只能显式 adopt，不接受布尔值或 drop 泄漏式脱离 owner。
- 验收: ①并发两次 dispose 共享完成结果且只收尾一次；②取消子代理并等待退出，三种终态均释放读槽；③非 persistent 后台进程、通知订阅、临时 artifact 和租约全部回收；④dispose 返回前工具 wrapper 已静止且生命周期终态落库；⑤persistent 服务显式 adopt 后跨 run 存活并有 adoption 事件，未 adopt 的全部收回；⑥强杀重启后无幽灵 owner，能确定恢复或标失败；⑦R-174/R-180 现有测试保持通过。
- 优先级: P2
- 进展: 批1+批2 已完成(此前轮次落地,本批复核确认代码在库):line_runtime.rs 骨架——Inner 持有 cancellation/child_agents(TaskCancellations)/child_agent_joins/background_processes,dispose 幂等(AtomicBool CAS 首调 + Mutex<Option<Shared future>> 复用,并发共享同一完成 future,performed 只归赢家),dispose_once 顺序:cancel token → cancel_all 子代理 → drain+await 全部 join(三种终态在 run_subagent 返回时释放读槽)→ 清空后台进程 id;4 单测全绿(并发幂等/取消令牌触发/new 默认不取消/等待子代理退出)。drive.rs 后台 spawn 接线(track_child_agent 调用点)并入批3 与后台进程收口一起做。批3:非 persistent 资源回收(后台进程真实 kill + 通知订阅 + artifact + 租约)+ drive.rs spawn 接线;批4:终态落库+wrapper 静止;批5:persistent adopt+幽灵 owner 恢复+全量。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-246
- observed_head: 2fffa0829d54c008df04af3941bf7c3e31d6612d
- observed_worktree_hash: fnv1a64:f942ffb698473c93
- recorded_at: 1786818826147

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
- 阻塞: 2026-08-16 复核:批1 已解除;批2 无阻塞、随时可开工且**不需要用户拍板**(原文即如此写);批3 的依赖 R-244 已 done 并归档(Tool Pipeline 契约已冻结),只余 R-245 确定图片类 artifact 的 spill 落点,而 R-245 自身仍等 R-242。当前 park 的唯一原因是 WIP 槽由 R-195 持有(用户 2026-08-16 指定)。解除动作: R-195 关闭后清本字段直接续做批2。解除人: agent(批2)/ 依赖自然解除(批3 等 R-245)。
- 验收: ①read 读 PNG/JPEG/WebP/GIF 各有定向测试,media_type 正确,非图片文件走原文本路径无回归;②ui_probe screenshot 返回的图片能被模型消费,桌面端实测有轨迹;③provider 不支持图片时有显式降级诊断;④图片 artifact 走 R-245 spill,ToolOutput 不内联超阈值 base64;⑤R-014 既有附件路径逐条无回归;⑥ToolOutput 结构变更后既有全部工具返回路径编译通过且行为不变(机械核验)。
- 优先级: P1

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

## R-268 写者与 bash 围栏窗口解耦:托管文档写入不再等全局 bash 静默,不变式从「窗口内没有写者」换成「窗口内的变化可归因」 [todo]
- 关联: D-382(围栏共享档,已修)、D-383(注册表毒化,残余机械缺陷)、D-364/D-368(围栏归因不变式)、D-258(absorb_paths 按路径吸收)
- 复杂度: 大
- 方向: 专用工具写入走写日志(路径+写后内容指纹,必要时含内容):围栏窗口收口时对 diff 逐路径对账,终态与日志一致的吸收进基线(同 D-258 absorb_paths 的按路径吸收口径),不一致的按越界回滚到最后一次合法日志内容(不是窗口开点快照)。写者从此不取跨窗口互斥,只保留毫秒级文件锁。远期与「tracker 事件化:append-only event store + 物化投影」同向(该方向另行立项),本条只做到写日志+吸收即可交付吞吐
- 标签: 核心
- 背景: D-364 不变式「窗口内没有写者」靠锁实现:围栏共享锁贯穿整个 bash 窗口(默认 120s/上限 600s),排他写者(req/defect/idea/decision/test_record/memory)预算仅 3s,撞上任一线的长 bash 即报错。两线 bash 窗口交叠时写者可长期挤不进去——轮末 test_record/req update 被外线 cargo build 拖住,是 D-382 修完围栏互斥后并行吞吐被吃掉的主要残余。设计基线 parallel_read_serial_write_orchestration.md §285 已预言「等全局静默会被后来的写者饿死,需要另设策略」,策略至今未落地
- 验收: 一条线 cargo build(分钟级)期间,另一条线 req update/test_record/memory_add 毫秒级完成且不被围栏误回滚;bash 越界写照旧被检出并回滚(D-364/D-368 全部回归绿);窗口内合法写+越界写混合场景回滚到合法日志终态而非窗口开点
- 优先级: P1

## R-269 浏览器工具:playwright-core 辅进程 headless 自检通道 [todo]
- refs: R-101 D-319 R-249 R-059
- 内容: ①Rust 工具起 Node 辅进程,playwright-core 以 channel 模式 launch 本机 Edge/Chrome headless(不下载 playwright 浏览器二进制),Rust↔Node 走 JSON-RPC over stdio;②能力:open(URL/本地文件)、screenshot(内置移动 viewport 预设,图片经 ToolOutput images 通道回模型——R-249 批1 已交付)、dom(可选 selector 的可读结构)、console、click/type;③自 launch 实例不碰 WebView2,天然绕开 D-319;④注册进桌面端与自举 harness 工具集,权限档位按 profiles 既有口径;⑤辅进程生命周期:空闲超时回收、工具关闭即收尾,不留僵尸 headless。拆批:批1 辅进程骨架+open+screenshot(含移动 viewport);批2 dom+console;批3 click/type 交互。
- 复杂度: 大
- 批次: 0/3
- 来源: 2026-08-16 移动端开发前置盘点。用户定调:浏览器工具属开发工具必要范畴,直接登记;技术路线经用户拍板选 playwright-core 辅进程(devDependencies 已有 ^1.62.1,e2e-smoke 同款地基);首要消费场景是移动端 UI 的自举自检,兼收 R-101/webfetch 两侧收益。
- 标签: 核心
- 边界: 不 attach WebView2(R-101 的 CDP 路线另论);不做多 tab/多上下文并发;不做网络拦截与请求 mock;无 Node 或无 Edge/Chrome 时给明确诊断,不静默降级;截图体积口径沿用 R-249。
- 验收: ①打开本地 HTML 与 http URL 各有实测轨迹;②移动 viewport 截图被模型真实消费(实测轨迹,不是单测断言);③click/type 后 DOM 变化可读回;④页面 console 错误可读;⑤缺 Node/缺浏览器时诊断明确;⑥工具生命周期结束后无残留辅进程与 headless 实例(实测进程列表);⑦附带给出 e2e-smoke 切本路线绕开 D-319 的可行性结论(只要结论,不要求实施)。
- 优先级: P1

## R-270 桥接移动化:LAN 配对/SSE/approval/PWA serve 与通知桥 [todo]
- refs: R-059 D-063 R-269 docs/design/r059_mobile_agent_communication.md
- 内容: 现状 mobile.rs 只绑 127.0.0.1、Connection: close 单线程 accept、三个 JSON 端点、单一共享 token。本条:①监听可切 LAN(默认仍回环,桌面设置页开关+显示地址);②设备配对:桌面端生成配对码/二维码(地址+一次性配对 token),每设备独立 token,设备列表可单独撤销(替换现单一共享 token);③SSE 端点 GET /v1/events 长连接实时推送,断线重连沿用既有 delivery_cursor 补发,每连接独立线程,不阻塞其它请求;④approval 通道:GET pending 权限询问(脱敏摘要)+ POST 回答,接 runner 既有 ask 流,最终门禁仍在 harness 侧;⑤静态页 serve:桥接直接 serve PWA 页面(随桌面端发版分发,不另起服务);⑥息屏通知出口:approval/失败/完成等关键事件经现成 LAN 推送桥(KDE Connect 类,具体工具实施时定)发手机系统通知。拆批:批1 LAN+配对/撤销;批2 SSE;批3 approval;批4 PWA serve+通知桥出口。
- 复杂度: 大
- 批次: 0/4
- 来源: 2026-08-16 移动端方案定案(用户逐项拍板):形态 PWA+现成通知桥(手机为 Android);实时通道 SSE;第一批含 approval 远程回答;原生壳不做——息屏通知由 LAN 推送桥零开发补齐,不为舒适性引入 Android 工具链。必要性口径:本条是移动端唯一的硬必要部分,无替代。
- 标签: 后端
- 边界: 公网监听禁止(既有定调不变);不做 TLS(LAN 自用威胁模型,token 即门);不自研推送协议,不接 FCM/Web Push 等公网推送;不开放远程 shell/write——approval 只回答既有询问,不新增能力面;协议契约沿用 docs/design/r059_mobile_agent_communication.md 阶段A字段定义。
- 验收: ①LAN 另一设备实测连通,默认回环行为不变;②撤销某设备后其 token 立即 401,其它设备不受影响;③SSE 断线重连 cursor 补发无丢终态,长连接挂着时其它端点仍可用;④移动端回答 approval 后 runner 真实放行/拒绝各有实测轨迹,harness 门禁无旁路;⑤手机浏览器打开桥接地址能加载 PWA 页面;⑥手机息屏状态收到 approval 事件的系统通知(实测);⑦既有回环+token 行为与 D-063 回归全绿。
- 优先级: P1

## R-271 移动端 PWA:配对/通知流/发消息/approval 界面 [todo]
- refs: R-059 R-269 R-270 R-267
- 依赖: R-269 R-270
- 内容: ①PWA 静态工程(与桌面 ui/ 同纪律:原生 JS、零构建、零框架),由 R-270 桥接 serve;②页面:配对(扫码/输码)、线程/会话列表与运行状态、通知流(SSE 订阅+cursor 补发)、发消息、approval 卡片(脱敏摘要+批准/拒绝);③PWA manifest+service worker:可添加到主屏、全屏打开、离线时给明确提示(不做离线数据);④移动 viewport 布局,长列表窗口化沿用 R-267 模式。拆批:批1 配对+通知流只读;批2 发消息;批3 approval 卡片+PWA manifest。开发期每批用 R-269 浏览器工具按移动 viewport 自检(截图+DOM),真机验收由用户执行。
- 复杂度: 大
- 批次: 0/3
- 来源: 2026-08-16 移动端方案定案:形态 PWA+现成通知桥(Android),承接 R-059 双向通信与通知推送两条验收的实际载体。用户手机用途定调:给电脑发消息、看运行状态、批权限——轻交互遥控器,不做重界面。
- 标签: 前端
- 边界: 不引前端框架与构建步骤;不做息屏推送(R-270 通知桥承担);不做 iOS 专属适配(Android Chrome 优先);第一批绑桥接当前项目,不做多项目切换;不做桌面端功能面的完整复刻——只做遥控器三件事。
- 验收: ①Android 真机全链路实测:配对→看通知流→发消息→批 approval,有实测记录;②锁屏/切后台再回,SSE 恢复后 cursor 补齐无丢终态;③添加到主屏后全屏打开;④每批有 R-269 移动 viewport 自检轨迹(开发期证据);⑤长通知流滚动不卡(窗口化生效);⑥R-059 双向通信与通知推送两条验收在本条+R-270 交付后可核销。
- 优先级: P1
