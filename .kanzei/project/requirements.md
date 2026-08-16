# Requirements

## R-221 research 模式重定位:按 docs/design/research_mode.md 分批实施独立深度研究模式(文献+仓库调研,论文级产出) [doing]
- 优先级: P2
- 复杂度: 大
- 标签: 后端 前端 harness
- 来源: 2026-08-12 八维度审计维度8;设计文档 docs/design/research_mode.md(§2 八个定调点待用户逐项确认后动工)。
- 背景: research 模式骨架完整但形态错位(面向网络调研)且零使用(state.db 266 条 episodes 零调用 websearch/source/finding,.kanzei/research 全 git 历史只有空模板);真实勘察全在 dev 完成且结论无固定落点(勘察报告被 D-294 单行不变式折成单行塞进度字段);证据等级 E0-E4 被双重语义挪用;research/memory.md 是绕开记忆控制平面的第二套无校验记忆。
- 内容: 按 docs/design/research_mode.md(2026-08-16 设计基线,定调点全部过审)实施模式基座五批:批1 档位收口(桌面注册 ReadonlyProfile、bash 硬 deny+替代指引指向 latex/plot 专用工具、files/git 只读);批2 topic 工件(S-/F-/report 落 .kanzei/research/<topic>/,前端按 topic 分组);批3 证据口径(V 表双域写进 conventions);批4 回流通道(backlog 只读索引+conventions 注入、req/defect get+add 子集、finding→[todo] 草稿);批5 记忆一元化(memory_search/memory_note 进档,memory.md 停止注入)。研究引擎(四段流水线)由 R-277 承接,工具配套 R-273/R-274/R-275,前端 R-276。
- 边界: research 不可提交 git、不动既有条目状态(add 草稿除外);不做报告 schema 校验。「不可写 docs/design」一条待重推(新定位下产出是论文而非设计文档,问法需重新表述)。**dev 侧「先计划后自举」的勘察工件落点问题不由本条承接**——那是独立课题,需另立条目。
- 验收: 以设计文档 §7 总则为准——一条真实 R- 条目的 勘察→报告→登记→dev 实施 完整链路有轨迹;每批验收见设计文档 §6。
- refs: D-276 R-201 D-304 R-273 R-274 R-275 R-276 docs/design/research_mode_prior_art.md
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-221
- 进展: 2026-08-16 取活。勘察结论:R-221 的设计真源 docs/design/research_mode.md 状态为「设计基线草案(2026-08-12 八维度审计维度8 产出;定调点待用户逐项确认后转正)」——§2 的八个定调点(主形态/工件落点/证据等级 V 表/回流通道/记忆一元化/档位矩阵/可写 docs 边界/三形态收敛)全部标注「待用户确认」,括号内为本设计的默认建议。按 §1「需求边界不清楚时必须先提问确认,不允许在关键问题上自行假设后直接实现」,八个定调点未获用户拍板前实施会踩边界(如「research 不可写 docs/design」「证据等级单列 V 表」都是用户层面决策)。现状盘点(供解除阻塞后立即开工):批1 档位收口的 files/git 只读已在 R-218 完成(SubagentBase 6 件套),ReadonlyProfile 与 bash 硬 deny+替代指引是既有模式(profiles.rs:652-658 先例);批2-批6 的 topic 工件/证据口径/回流/记忆/三形态均未动。 || 2026-08-16 复核:设计已转正(2026-08-16 定调点全部过审,research_mode.md 已重写为设计基线),原阻塞对象 R-246 已 done 并归档(复核时仍为 doing,现确认已归档),阻塞解除条件全部满足,当场清空阻塞字段,按 §7 批次恢复可执行。 || 2026-08-16 让位:本轮按队列顺序取 R-186(P0 队首),本条 doing→todo 让位,待 R-186 交付后按队列轮转,届时直接开工批1(档位收口:ReadonlyProfile + bash 硬 deny+替代指引,先例 profiles.rs:652-658)。
- 阻塞: 队列让位(2026-08-16):R-186(P0 队首)本轮推进中,单 WIP 槽不足,本条让位等待队列轮转。解除动作: R-186 关闭后清本字段,直接开工批1(档位收口:ReadonlyProfile + bash 硬 deny+替代指引,先例 profiles.rs:652-658)。解除人: agent。
- observed_head: 98d7a586f38a09f5b449b75b7a3c93c62d01852f
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786835811278
- 批次: 0/5
- 状态: todo

## R-216 记忆写入侧质量三闸:近似去重下沉 store.add 双 scope、[fp:] 指纹一致性校验、tracker 交付状态内容拒收 [doing]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 记忆
- 来源: 2026-08-12 八维度审计(§5)。M-055/M-056 于近似去重上线当天英文复述 M-044 并携带编造指纹——「假指纹立即污染注入」经反证驳回(FingerprintIndex 只收 active 且不扫标题),但穿透与伪造本身实证成立;另有 6 条交付状态类内容落进记忆与 tracker 重复。
- 内容: ①classify_novelty 的 FTS 语义探测下沉进 store.add 作为硬闸(Uncertain 即拒并返回候选),查重范围扩到双 scope;②新条目携带的 [fp:] 必须与来源 note 中引擎生成的指纹逐字一致,拒绝自造;③标题/subject 命中「R-/D- 编号+已交付/勿重复/验收边界」形态时拒绝并指路 tracker(或强制挂 refs 并随条目关闭自动 deprecate)。
- 验收: ①复刻「英文改写 M-044」场景被拦并指路 memory_update(单测);②伪造指纹的 add 被拒;③存量 6 条交付状态记忆逐条处置;④各拦截路径有单测。
- refs: R-194 R-195 R-196 D-299 D-282
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-216
- 进展: 2026-08-16 收口此前引擎取活留下的半成品实现(工作树未提交,6 测试红)。已完成:①三闸实现确认完整(store.rs add 内:交付状态拒收 has_tracker_id、指纹一致性 fp_markers、语义探测下沉 classify_novelty 双 scope);②修复 6 个失败 fixture(5 个自造指纹被新指纹闸拦——merge_gate/find_by_marker/merge_conservative/merge_自动搬运 注入来源 note 或 force,1 个 novelty_gate 语义断言适配 R-216 口径);③新增 3 个验收单测:自造指纹的add被拒_来源note指纹放行、交付状态内容被拒并指路tracker、英文改写被add硬闸拦截返回候选。验证:memory 95 passed + kanzei-tools 346 passed + clippy/fmt 全过。验收对照:①英文改写被拦并指路 memory_update——英文改写被add硬闸拦截返回候选 测试(Uncertain 返回候选);②伪造指纹的 add 被拒——自造指纹的add被拒_来源note指纹放行;③存量 6 条交付状态记忆逐条处置——**未做**(验收③数据工作);④各拦截路径有单测——3 条新增。 || 2026-08-16 复核:原阻塞对象 R-195 已 done 并归档,阻塞解除条件全部满足,当场清空。剩余工作=验收③:逐条查 memory 库定位 6 条交付状态记忆并归档/改写(数据工作,不需要用户拍板),完成即可关闭本条。 || 2026-08-16 让位:本轮按队列顺序取 R-186(P0 队首),本条 doing→todo 让位,待 R-186 交付后按队列轮转,届时直接做验收③(存量 6 条交付状态记忆逐条处置)并关闭本条。
- 阻塞: 队列让位(2026-08-16):R-186(P0 队首)本轮推进中,单 WIP 槽不足,本条让位等待队列轮转。解除动作: R-186 关闭后清本字段,直接做验收③(存量 6 条交付状态记忆逐条处置)并关闭本条。解除人: agent。
- observed_head: 98d7a586f38a09f5b449b75b7a3c93c62d01852f
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786835811578
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

- 进展: 2026-08-16 交付形态已拍板:PWA+现成通知桥(手机为 Android),原生壳不做(息屏通知由 LAN 推送桥零开发补齐);双向通信与通知推送两条验收的实施载体为 R-270(服务端)+R-271(PWA 界面),本条在其交付后按新载体核销;第三条『子代理升级为管理项目容器』与移动端无关,待用户重估是否保留。 || 2026-08-08 复核:验收三条原文要求「在移动端完成」,本仓库不存在移动端工程;2026-08-07 退回原因明确本需求保留移动端三条验收、待用户排期。桌面桥接(阶段 B)属既有能力,按退回意见应拆为独立子需求,不在本条验收范围内。 || 2026-08-16 复核:R-270(桥接移动化)与 R-271(移动端 PWA)均已 done——①双向通信:POST /v1/messages(R-270)+PWA 发消息界面(R-271 批2);②通知推送:R-270 通知桥出口(完成/失败经 LAN 推送桥发手机通知)+PWA SSE 通知流(R-271 批1)。两条验收的实施载体齐备,依赖自然解除;第三条『子代理升级为项目容器』与移动端无关,待用户重估。
- 阻塞: 2026-08-16 复核:R-270/R-271 均已 done——双向通信与通知推送两条验收的实施载体已交付,依赖自然解除(不再阻塞)。剩余阻塞仅第三条『子代理升级为项目容器』:与移动端无关,需用户重估是否保留(保留则另立范围,不保留则本条按两条核销后关闭)。解除人: 用户(第三条)。
- observed_head: 49b65e2030c7dae4958963a6f9c5babe52b703da
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786842800288

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
- 进展: 2026-08-14 批1 交付(1831239)。勘察修正了原条目的一处前提:`Part::Image` 的三协议映射早在 R-014 就通了,缺的只是**工具侧出口**,协议层零改动即可打通——不必等 R-244。实现:①ToolOutput 增 images 载荷(空 vec 与既有行为逐字节一致,53 处 `ToolOutput {` 里只有 4 个真构造点,其余是解构模式);②read 按 magic bytes 而非扩展名识图(PNG/JPEG/WebP/GIF),扩展名撒谎会让 media_type 与真实字节不符、provider 400 且报错指向请求体;③图片 Part 只能追加在所有 ToolResult 之后——Anthropic 要求 tool_result 块在 user 消息最前,而 results[i]↔calls[i] 由 note_step 的 debug_assert 锁着,中间也不能插;④provider 不支持时**在进 messages 前**降级为显式文本说明,判据收敛为 Route::supports_images() 与 client.rs 硬拒绝共用一处。新增 10 条测试。 || 2026-08-14 批2 交付:新增 ui_screenshot 工具(kanzei-app/src/screenshot.rs)。实窗验证三轮才对,两次假绿都值得记——①未声明 DPI 感知时 GetWindowRect 返回虚拟化坐标(2582px 的窗口报成 1295px),抓到的是横跨多个窗口的错误区域;②改用正确矩形后,屏幕 DC 抓取拿到的是压在上面那个应用的界面(kzapp 被完全遮挡),内容丰富所以 looks_blank 一路放行。两次都是「测试通过但抓的不是那个窗口」。最终改用 PrintWindow+PW_RENDERFULLCONTENT 离屏渲染,免疫遮挡,在完全被盖住的状态下抓到 kzapp 完整界面并经人眼与用户实拍逐项比对一致;屏幕 DC 仅在 PrintWindow 失效且本窗口为前台时作回退,不是前台宁可报错——返回别人的界面比返回错误坏得多。测试记录 T-1786705800。 || 2026-08-16 复核:批1 已解除;批3 的依赖 R-244 已 done 并归档(Tool Pipeline 契约已冻结),只余 R-245 确定图片类 artifact 的 spill 落点,而 R-245 自身仍等 R-242。当前 park 的唯一原因是 WIP 槽由 R-195 持有(用户 2026-08-16 指定)。解除动作: R-195 关闭后清本字段直接续做批2。解除人: agent(批2)/ 依赖自然解除(批3 等 R-245)。 || 2026-08-16 让位:本轮按队列顺序取 R-186(P0 队首),本条 doing→todo 让位,待 R-186 交付后按队列轮转;批1/批2(ui_screenshot/read 识图)已交付,剩余批3 等 R-245(R-242 完成后才解)。
- 阻塞: 队列让位(2026-08-16):R-186(P0 队首)本轮推进中,单 WIP 槽不足,本条让位等待队列轮转。解除动作: R-186 关闭后清本字段恢复推进;批1/批2 已交付,剩余批3 等 R-245(R-242 完成后才解)。解除人: agent。
- 验收: ①read 读 PNG/JPEG/WebP/GIF 各有定向测试,media_type 正确,非图片文件走原文本路径无回归;②ui_probe screenshot 返回的图片能被模型消费,桌面端实测有轨迹;③provider 不支持图片时有显式降级诊断;④图片 artifact 走 R-245 spill,ToolOutput 不内联超阈值 base64;⑤R-014 既有附件路径逐条无回归;⑥ToolOutput 结构变更后既有全部工具返回路径编译通过且行为不变(机械核验)。
- 优先级: P1
- observed_head: 98d7a586f38a09f5b449b75b7a3c93c62d01852f
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786835811870
- 状态: todo

## R-264 前端迁移原生 ESM(勘察已完成,方案见 docs/design/ui_esm_migration.md) [doing]
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
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-264
- 批次: 3/4
- 进展: 批2 完成(6c90aba)+工具链(ef89d20/e80114a/66098a6/634c9dc/b9ac558)。**批3 跨模块写全覆盖推进(2026-08-16,已提交 98d7a58 并 push)**:①renderProjects 完整 setter 化(currentProject/activeProcessId/activeSessionId,03-shell setter 定义+09 import 固化);②14-docs-actions documentsKind/dependencyViewOpen setter 化(12-docs-pages setter 定义固化);③工具链特殊修复点完善(languageSelect const→let+defer/value 并入 defer/09 setter import+赋值/03-shell setter 定义/12-docs-pages setter 定义,迁移重跑不丢失);④冒烟时序适配:DOMContentLoaded 触发后 await flush(20) 让 async 初始化推进,classic 路径不受影响(六冒烟全绿)。**新定位**:withSessionRender(R-267,01-core)写 5 个跨模块状态(currentAssistant/currentReasoning/currentReasoningHead/renderingBackground/activePane)——设计文档 §三 只列了读没列写,是深层持续工程。已回退 ui 到 HEAD(批2 稳定六冒烟全绿)。 || 2026-08-16 让位:本轮按队列顺序取 R-186(P0 队首),本条 doing→todo 让位,待 R-186 交付后按队列轮转,届时做剩余批4(withSessionRender setter 化、B3、defer 时序与冒烟断言适配、删补偿)。P3 留档(设计文档自述对自举无收益)。
- observed_head: 98d7a586f38a09f5b449b75b7a3c93c62d01852f
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786835812161
- 状态: todo
- 阻塞: 队列让位(2026-08-16):R-186(P0 队首)本轮推进中,单 WIP 槽不足,本条让位等待队列轮转。解除动作: R-186 关闭后清本字段,做剩余批4(withSessionRender setter 化、B3、defer 时序与冒烟断言适配、删补偿)。P3 留档。解除人: agent。

## R-273 LaTeX 编译工具通道:Tectonic 侧车+系统发行增强 [doing]
- refs: R-221 R-249 docs/design/research_mode_prior_art.md
- 内容: ①Tectonic CLI 侧车:随包分发官方 exe(或首启下载校验),封装 latex 编译工具(输入 .tex 与工作目录,输出 PDF+诊断);预热常用宏包后默认 --only-cached 免每次联网核对 bundle(上游 #1224),失败再放开网络重试;②bib 路线:默认 natbib/bibtex(Tectonic 内置纯 Rust 实现,循环全自动),biblatex 仅在检测到 biber 二进制时可用并向 agent 显式声明;③系统发行版增强:PATH 检测 kpsewhich/latexmk,检测到 MiKTeX/TeX Live 优先用(全量宏包+biber),否则回落 Tectonic,不要求用户装;④PDF→PNG 回传:pdfium-render + pdfium.dll 侧车,编译产物页面转 PNG 经 ToolOutput images 通道回模型(R-249 已交付);⑤编译错误诊断透传(行号+上下文),支持 agent 编译回环修错(AI Scientist v1 先例)。拆批:批1 侧车+编译工具+诊断;批2 PDF→PNG 回传;批3 系统发行检测增强+bib 收口。
- 复杂度: 中
- 批次: 3/3
- 来源: 2026-08-16 用户定调 research mode 配套必备(「我们肯定还需要latex绘制」);技术路线依据 docs/design/research_mode_prior_art.md §2 调查:Tectonic 2026 年活跃维护、Windows 官方预编译、CLI 侧车优于嵌 crate(官方认证的脆构建链)、biber 不内置。
- 标签: 核心
- 边界: 不嵌 tectonic crate;不内置 biber;不做 Typst 通道(调查给出诚实对比,是否加挂另行评估);编译工作目录限研究工件目录与显式指定目录;不做 SyncTeX 编辑器联动。
- 验收: ①无系统 TeX 的机器上编译含数学公式+图+bibtex 参考文献的 .tex 成功出 PDF(实测);②PDF 页面转 PNG 被模型消费有轨迹;③断网时 --only-cached 编译已预热文档成功,未预热宏包给明确诊断;④检测到系统发行版优先用之、缺失回落 Tectonic,两路径各有测试;⑤编译错误诊断含行号不静默;⑥侧车 exe 缺失时给下载指引不崩溃。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-273
- 进展: 2026-08-16 取活开工(复杂度中,设计冻结先行)。**勘察结论**:①本机 MiKTeX 全量已装,无 tectonic;②设计文档 §2 定调:Tectonic CLI 侧车、预热+--only-cached、bibtex 内置/biber 检测、PDF→PNG 走 pdfium-render;③边界:编译工作目录限研究工件目录与显式指定目录。**设计冻结**:不变式——系统发行版优先、缺失回落 Tectonic、侧车缺失给下载指引不崩溃;权威数据源——PATH 检测结果与编译诊断(含行号)。 || **批1 完成(839b76c)**:latex_tool.rs(发行检测+compile_latex+行号诊断)+base.rs 注册。单测 3 条,kanzei-tools 297 passed。 || **批2 完成(275f2ef)**:PDF→PNG 回传(to_png 参数,pdftoppm 首页转 PNG 经 images 通道回模型)。单测 2 条,kanzei-tools 299 passed。 || **批3 完成(2026-08-16,提交待定)**:①断网 --only-cached 预热语义(验收③)——compile_tectonic 先 --only-cached,失败给「未预热需先联网预热」指引(假 tectonic 脚本模拟已预热成功/未预热诊断);②bib 路线声明(验收④ Tectonic 路径)——biber_available 检测,biber 可用声明 biblatex、缺省 natbib+bibtex(内置纯 Rust);③单测新增 2 条:tectonic已预热_onlycached成功、tectonic未预热_明确诊断含bib声明;kanzei-tools 301 passed、workspace 全量 15 段 ok(T-1786844158),clippy/fmt 通过。**关闭前核对**:验收①系统路径 MiKTeX 实测(含公式图 bibtex 出 PDF)+Tectonic 路径假脚本;②PDF→PNG 经 images 通道(批2 单测 PNG 魔数+清理);③--only-cached 预热语义(批3 假脚本两路径);④系统优先/回落 Tectonic 两路径各有测试(批1 系统实测+批3 假脚本);⑤错误诊断含行号(批1 l.3);⑥侧车缺失给下载指引(批1 Missing 分支)。按 §1.2 可用即关闭,准备 req update done。
- observed_head: 275f2efa2fecf946133954f1378e1da60578fdfc
- observed_worktree_hash: fnv1a64:bdfa2f9fe6284ae6
- recorded_at: 1786844168213

## R-274 科研绘图工具通道:Vega-Lite+PGFPlots 双轨 [todo]
- refs: R-221 R-249 R-273 R-275 docs/design/research_mode_prior_art.md
- 依赖: R-273
- 内容: ①主轨 Vega-Lite:agent 产 JSON spec,vl-convert 独立 CLI 侧车渲染 SVG/PNG(不嵌 crate,避开 deno_runtime/v8 编译负担);spec 先 JSON 校验,错误给 agent 可一轮修复的诊断;②终稿轨 PGFPlots/TikZ:走 R-273 Tectonic 通道,零新增依赖,图字体与论文正文一致;③增强轨 matplotlib+scienceplots:检测到 uv/Python 才启用(uv run --with matplotlib,scienceplots 按需环境化),检测不到明确降级;④色板注入与 R-275 对接:Vega-Lite 经 spec config/scale.range,matplotlib 经 rcParams 前导代码;⑤输出统一转 PNG 回模型(R-249 通道),原始 SVG/PDF 落盘给用户。拆批:批1 Vega-Lite 主轨;批2 PGFPlots 轨+统一落盘回传;批3 matplotlib 增强轨+色板对接。
- 复杂度: 中
- 批次: 0/3
- 来源: 2026-08-16 用户定调「科研绘图,这个绘图工具也是很重要的」;路线依据 docs/design/research_mode_prior_art.md §2 七方案对比:Vega-Lite(vl-convert)是最优纯 Rust 零安装路线且 JSON 规格对 agent 最友好、PGFPlots 投稿场景不可替代、matplotlib 是检测到 Python/uv 时的上限增强;plotters(无抗锯齿)/gnuplot/charming/plotly.rs 排除。
- 标签: 核心
- 边界: plotters/gnuplot/charming/plotly.rs 不引入;不做交互式图表与图表编辑 UI;图产物目录限研究工件目录与显式指定;不做动画/3D。
- 验收: ①零外部安装机器上 Vega-Lite spec→PNG 实测成功且被模型消费(轨迹);②同一数据 PGFPlots 轨出 PDF 实测;③检测到 uv/Python 时 matplotlib 轨出图、检测不到时明确降级诊断(两路径测试);④注入指定色板后图中系列颜色与色板逐色一致(机械断言);⑤构造一个非法 spec,诊断可让 agent 一轮修复(实测轨迹);⑥辅进程无残留。
- 优先级: P1

## R-275 调色板子系统:内置科学配色/推荐校验/用户导入 [todo]
- refs: R-274 docs/design/research_mode_prior_art.md
- 内容: ①内置科学配色打包:ColorBrewer(Apache-2.0,需致谢)/viridis 系(CC0)/Crameri Scientific Colour Maps(MIT)/Paul Tol(BSD-3)/Okabe-Ito(注出处)/cmocean(MIT)/petroff10(CC0),一次性转内部规范 JSON(name/type[seq|div|qual|cyclic]/colors[]/max_classes/source_url/license),零运行时联网;②推荐规则机械化:无序分类→qual(≤12 色)、有序连续→seq、有中点→div、周期→cyclic(Vega-Lite 按字段类型默认规则先例);硬禁忌机械拒绝(jet/rainbow 用于连续量、定性板插值);③校验链 Rust 本地实现:CVD 模拟(Machado 矩阵)→两两 CIEDE2000(palette crate 内置)→WCAG 图形对比度≥3:1→连续板亮度单调性,导入即评分;④用户导入:粘贴 hex 列表/GIMP .gpl/Adobe .ase 统一转内部 JSON;定性板不够长默认拒绝并提示改分面/高亮,兜底循环+线型区分,绝不插值;⑤对 R-274 暴露统一色板查询接口(按 type+色数返回,用户板同类型优先)。拆批:批1 内置数据+规范 JSON+查询接口;批2 推荐规则+校验链;批3 用户导入三格式。
- 复杂度: 中
- 批次: 0/3
- 来源: 2026-08-16 用户原话「科研绘图要支持调色版推荐,我给AI一些调色版,他自己做,这里可能还需要爬取一些配色网站的方案」;调查结论(prior_art §3):内置源许可证全干净且机器可读、爬配色网站砍掉(Coolors ToS 明确禁爬、Adobe API 已死、ColorHunt 灰色;纯色值组合无版权,风险在 ToS;开源聚合库覆盖更优)、Rust 生态足以本地实现全部校验。
- 标签: 核心
- 边界: 不爬配色网站(用户原「可能爬取」的想法经调查以免爬替代落地:官方源+开源聚合质量更高,「自己喂色板」由粘贴/导入入口覆盖);colorcet(CC-BY 要求署名)不入首批;不做色板编辑器 UI;不做专色/CMYK 印刷流程。
- 验收: ①内置各族色板与上游源逐色一致(抽查断言),license 与致谢字段齐全;②四类数据特征各返回正确类型色板,jet 用于连续量被拒(定向测试);③构造红绿不安全板,校验链给低分并点名冲突色对(实测输出);④hex/.gpl/.ase 三种导入各有测试,非法输入诊断明确;⑤定性板超长请求默认被拒并给分面建议;⑥R-274 注入联通实测(图中颜色与用户板一致)。
- 优先级: P1

## R-276 research 模式前端:双面板/计划审批/来源呈现 [todo]
- refs: R-221 R-267 R-273 R-274 docs/design/research_mode_prior_art.md
- 依赖: R-221
- 内容: ①布局:「会话+文档」双面板(Gemini 式)——左会话右报告文档,研究步骤折叠于报告下方,层级明确「结果>过程」;②计划先行:研究计划树是一等 UI 对象,开跑前可编辑、运行中可转向(Gemini/ChatGPT 先例;与 research_mode 定调「计划审批闸口」对应);③来源呈现:内联数字引用+来源卡+独立 Sources 页三处冗余(Perplexity 式),引用点击回源——文献 URL 与代码 file:line 双形态;④进行中活动流:检索/阅读步骤滚动展示,完成后折叠成紧凑卡;⑤topic 工件浏览:paper.tex/figures/refs.bib 浏览与图预览(figures 用 R-274 产物,PDF 预览用 R-273 的 PNG 转换)。**设计先行**:批1 只出交互设计稿(四组件权重取舍+页面流),经用户过审后才进批2-4 实施。拆批:批1 设计稿过审;批2 双面板+报告阅读;批3 计划树编辑+活动流;批4 来源交互+工件浏览。
- 复杂度: 大
- 批次: 0/4
- 来源: 2026-08-16 用户「researchmode的前端设计这些比较复杂」;设计输入为 prior_art §1 前端横评(Gemini 报告至上双面板/ChatGPT 计划编辑与运行中转向/Perplexity 来源三处冗余/Manus 过程至上反例)与四组件通用 schema(document/steps/sources/annotations)。
- 标签: 前端
- 边界: 不做协作/分享/导出站外;不做在线 LaTeX 编辑器(Monaco 已有);research 下连跑禁用沿用 interaction_modes 既有定调;长报告渲染沿用 R-267 窗口化模式,不另造。
- 验收: ①批1 设计稿经用户过审(含四组件权重取舍的明确理由);②计划编辑→运行→中途转向全链路可操作有轨迹;③引用点击回源双形态各实测(URL 与 file:line);④长报告与长活动流滚动不卡(窗口化生效);⑤与桌面既有 UI 风格与 i18n 纪律一致。
- 优先级: P2

## R-277 research 引擎:计划审批/检索反思环/大纲写作/引用校验 [todo]
- refs: R-221 R-273 R-274 R-276 docs/design/research_mode.md docs/design/research_mode_prior_art.md
- 依赖: R-221
- 内容: 四段流水线:①澄清+计划——产出显式研究计划树数据结构,经用户审批/修改后才跑(UI 由 R-276 承接);②检索-阅读-反思环——串行迭代+有限并发检索,子任务隔离上下文、回传前 RCS 式压缩(相关分+带出处摘要),原始网页/工具输出不直接进主上下文;信息写入 findings.md 时即绑定来源(STORM 信息表先例);反思步找知识缺口决定补搜;③综合写作——先 outline.md 后分节单点一次性生成,重课题写 paper.tex 走 R-273 编译回环修错;④引用校验——FACT 式论断-出处逐条核验(文献=URL 内容支撑,代码=file:line@commit 存在且语义支撑),抽查不过重写该节。支撑件:预算显式旋钮(轮次/token 上限,超限收敛写作而非报错);tantivy 本地全文索引(文献+代码)与 symbols 反查挂同一检索接口(文献论断↔代码实现互证是现有系统空白,kanzei 独有优势);断点续跑(单机状态文件,强杀可恢复)。拆批:批1 计划数据结构+澄清段;批2 检索环+压缩回传+来源绑定;批3 大纲写作+LaTeX 回环;批4 引用校验+预算旋钮;批5 tantivy 索引+symbols 同接口+断点续跑。
- 复杂度: 大
- 批次: 0/5
- 来源: 2026-08-16 research mode 定调点全部过审后按 docs/design/research_mode.md §5 立项;架构采纳先行对照(prior_art §1)全行业收敛结论:四段流水线、研究并行写作串行、引用收集时绑定、预算显式旋钮、计划给人审。
- 标签: 核心
- 边界: 不做真·多 agent 并行编排(先行对照:15 倍 token 单用户不值,隔离+压缩回传同样解上下文冲突);不做 RL 专训模型(纪律放系统侧);不做常驻知识库服务(索引随课题建随课题用);不做模拟审稿与自动选题;计划审批前端由 R-276 承接,本条只出数据结构与状态机。
- 验收: ①一个真实课题走完整链路(计划→审批→检索→带引用报告)有轨迹;②FACT 式抽查:随机抽论断,文献 URL 与代码 file:line 逐条支撑(实测,不接受自评);③预算旋钮实测:设小预算提前收敛出报告不崩;④机械核验原始工具输出不进主上下文(只有压缩摘要);⑤文献与代码经同一检索接口命中各有实测;⑥中途强杀重启可恢复续跑;⑦轻课题(只产 report.md)与重课题(paper.tex 编译通过)各走通一次。
- 优先级: P1
