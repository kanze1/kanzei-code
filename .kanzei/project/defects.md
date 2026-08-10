# Defects

## D-185 `<memory-hints>` 声称只进本轮,实际逐轮累积进对话历史 [open] (medium)
- 复现: 开跑前预检索的记忆提示块拼进 `run_prompt`(crates/kanzei-app/src/main.rs 注入点注释写"提示块只进本次运行"),但它随 User message 进 `summary.messages` → 桌面端整份存进 conversations → 下轮作为 `prior` 回灌。跑 N 轮,历史里就躺着 N 个 hint 块。
- 影响: ①每轮固定多烧 N-1 份陈旧提示;②这些块是**当时**的记忆快照,与现行 INDEX.md 可能已经不一致,模型读到的是过期索引却无从分辨;③与 R-106"注入 token 下降"的目标反向。
- 根因: 提示块拼在 prompt 字符串上而不是作为一次性 system/context 段落,持久化路径对它无感知。
- 验收: hint 块不进 conversations 快照(或落库前剥离),连跑 3 轮后历史里最多一个块;注入 token 账单能看出 hint 段的独立占比。
- 证据等级: E2
- 优先级: P2
- 标签: 核心

## D-229 harvest_sop 只接了桌面端,CLI 轮末缺失同款 SOP 采集通道 [open] (medium)
- 优先级: P2
- 依据: 2026-08-10 memory 系统全量走查。crates/kanzei/src/main.rs 轮末只调 harvest_failures + harvest_entry_fact;kanzei-app/src/main.rs 轮末额外有 harvest_sop——CLI 完成条目不产 SOP 候选,R-124 采集通道双端不对称,遥测口径也随之分裂。
- 修复方向: CLI 轮末补 harvest_sop 同款调用;三个 harvest 收敛为一个共享的轮末采集函数,两端调同一入口,杜绝再次漂移。
- refs: R-124 R-105

## D-230 resident_index 预算装箱按 id 先到先得,新条目被系统性折叠 [open] (medium)
- 优先级: P2
- 依据: kanzei-tools/src/memory/mod.rs resident_index 按 load_all 的 id 升序装 3000 字预算,放不下的 continue 折叠——id 越大(越新)的条目越容易被挤出常驻索引,而新条目往往正是当前最相关的;老条目永远优先纯属枚举顺序副作用,不是价值排序。
- 修复方向: 装箱前按价值排序(decision_weight×新近度,或至少 updated 新近优先);与 prompt_hints 的口径保持同源(D-216 教训:两边必须对同一份判定)。
- refs: R-104 R-149

## D-214 SOP 候选投进全局 inbox 无人消化:manager 只读项目 inbox,7 条候选自 08-08 滞留 [open]
- 现象: ~/.kanzei/memory/inbox.md 里有 7 条 `## note` SOP 候选(最早 2026-08-08),从未被消化。
- 根因线索: harvest_sop 按设计把 SOP 候选投给 global store 的 inbox(kanzei-app main.rs ~6019),但轮末触发只查项目 store 的 pending_notes,manager 的 memory_inbox_clear 也只清项目 inbox——全局 inbox 是只进不出的死信箱。
- 修复方向(二选一): ①轮末触发与 manager 消化把 global inbox 一并纳入(pending 检查、prompt 注入、clear 都要对齐);②SOP 候选改投项目 inbox,由 manager 消化时按 scope=global 落库(R-124 本意是用户拍板采纳,注意别破坏候选箱语义)。
- 影响: R-124 SOP 提炼链路实际断裂,候选永远到不了用户面前。
- refs: R-124 R-149 (medium)

## D-217 stale 记忆无归档搬运通道:memory_system.md 承诺的 memory-archive/ 整理流程不存在 [open]
- 现象: 设计基线 §2 写「stale 后由整理流程移入 memory-archive/,带墓碑」,但代码里 archive/ 目录只被 load_archived_ids 用来保 ID 不复用,没有任何工具或触发把 stale 条目搬进去;INDEX.md 的「N stale 条待归档」永远挂着。sleep-time 空闲整理同样未实现,消化只有轮末触发与 UI 手动按钮。
- 影响: 遗忘只有「人工+墓碑」半套;stale 条目永远占主目录与 load_all 扫描;文档与实现不一致。
- 修复方向: 归档搬运做成引擎动作(同 tracker archive 哲学:搬运后回读校验),触发挂 R-150 的整理清单(零采纳候选/复发告警/stale 积压);实现时同步修订 memory_system.md 或按实现改文档。
- refs: R-150 R-107 (medium)

## D-209 对话落库粒度太粗(用户反馈,具体维度待澄清) [open] (medium)
- refs: D-208 D-185
- 原始描述: 用户 2026-08-09 原话"落库对话粒度太粗"(与活动栏回放问题同时反馈)。
- 机制现状(供收敛方向): ①对话持久化是 `conversation.updated` 事件整份 messages 快照替换,轮内不落盘,恢复只能回到轮边界;②工具轨迹 run.trace 只在收尾 flush 一次(D-179 补了停止路径,但仍是整轮一包);③episodes 是轮级摘要。三层都是"轮"粒度,轮内的中间态(改到一半、流式输出中断点)不可恢复、不可检索。
- 待澄清: 用户所指的具体痛点——候选:a) 历史恢复丢轮内进度;b) 回放时一整轮的工具轨迹糊成一批看不出先后;c) 检索/引用历史时只能按整轮拿、拿不到单条消息;d) 其他。按 D-205 教训不代用户猜死,取活前先确认。
- 验收: 待澄清后按维度改写;暂置:对话落库粒度支持轮内增量(或用户确认的等价目标),恢复/回放/检索三条消费路径至少一条受益并有实测。
- 证据等级: E2(用户反馈,机制已核实)
- 优先级: P2
- 标签: 后端

## D-207 取活顺序所见非所得:视图排序与优先级徽章都不参与取活,界面零提示 [open] (medium)
- refs: R-054 R-111
- 复现: 2026-08-09 用户反馈"取需求和缺陷的顺序看不懂了,因为侧边栏可以调整顺序"。机制现状:①取活真序 = md 文件物理顺序从上到下(dev prompt "Scan from top to bottom",schedule_entries 只后置阻塞项、不改文件);②侧栏拖拽(manual 排序+无筛选时)经 docs_update reorder **写回文件**,真的改变取活顺序;③侧栏另有 id/状态/复杂度/优先级四种视图排序(main.js filterRequirements),**只改显示**;④优先级徽章 P0~P3 完全不参与取活(prompt 明言 "Priority labels are background info, not the ordering")。
- 影响: 选了任何视图排序后,用户看到的顺序与 agent 取活顺序完全无关,界面没有任何提示;优先级徽章满屏,人天然以为按 P0→P3 取活,实际一票不投——近期把 5 条需求升 P0(576d725)在取活上零效果,用户的调度意图静默落空。三种顺序语义(文件序=取活序/视图序/优先级暗示序)混在同一个列表上,只在"manual+无筛选"时才重合。
- 根因: R-054 定了"文件顺序即开发顺序"的单一真源,后续视图排序与优先级徽章叠上去时,没有同步交代它们与真源的关系;取活规则只写在 prompt 里,UI 侧无任何投影。
- 验收: ①非 manual 排序视图下,侧栏显式提示"当前显示顺序≠取活顺序"(或等价视觉语言);②有一处能看到真序:取活预览(下一条会被拿的条目有标记,阻塞项显示跳过原因)或一键切回文件序;③优先级二选一——要么参与取活(prompt 与 schedule 同步改,并写清与文件序的优先关系),要么在 UI 上明示"仅参考,不影响取活";④用户复查确认能看懂"agent 下一个会拿哪条、为什么"。
- 证据等级: E1(代码四处机制实证 + 用户反馈)
- 优先级: P1
- 标签: 前端

- 进展: 2026-08-09 部分交付(①取活焦点可视化、②拖拽禁用提示)基础上,本轮补验收③:优先级语义二选一选 B(UI 明示)——按 M-002 用户定调 priority 只是背景信息不参与取活,不改 prompt/schedule。实现:①侧栏优先级筛选下拉 title 改为「按优先级筛选(仅参考,不影响取活顺序)」(index.html);②需求/缺陷行内优先级徽章 title 追加「(仅参考,不影响取活)」(main.js renderDocList badge.title);③i18n 新增两条词条(I18N_EN),ui-i18n-smoke 770 key 全绿;④冒烟新增断言:筛选 title 与徽章 title 均含「仅参考」,ui-runtime-smoke 222 invoke 全绿。验收④用户复查(能否看懂 agent 下一个会拿哪条)待发版安装后用户确认,与 D-210/D-211 同惯例。 2026-08-10 用户反馈验收④未过:前端把阻塞 doing(R-157)渲染成「运行中」,与取活实际脱节(§1.1 阻塞项不计 WIP、agent 跳过它)。根因:computeAgentFocus active 收集只按 status==doing/fixing,未排除 blocked。修复①:12-docs-pages.js computeAgentFocus 的 active 排除 entry.blocked,冒烟补断言,四条前端冒烟全绿,提交 c0864b0。 2026-08-10 用户再反馈:active 是集合没意义——集合是为多线程并行(每线程一条 active)设计的,现在多线程改造还远,应退化为单条。修复②:computeAgentFocus 的 active 从 Set 改为单条 id = 取活序第一个可执行的 doing/fixing(defect-first 先缺陷后需求);多余可执行 doing/fixing 只是已取未动的历史状态,不标 agent-active;11-docs-list.js 使用点同步 has()→===。冒烟新增「多条 doing 只标取活序第一条」断言,ui-runtime 243 invoke 全绿。验收④仍待用户重建 kzapp 后复查确认(ui 资源打包进 exe,需 release.ps1 重建生效)。

## D-205 快记通道无信息保真门槛:模糊输入被编造复现后落库,关键限定词丢失 [open] (medium)
- refs: D-204
- 复现: 实例即 D-204。用户输入"SOP易用程度有问题,似乎总结的不太好",快记(QuickCaptureComponent 迷你 run,crates/kanzei-app/src/main.rs)产出「复现: 查看 SOP 时」——这不是复现,是从"查看 SOP"四个字硬挤出来的伪复现;用户真实意图「**用户**查看/使用 SOP 时的易用性」(2026-08-09 对话澄清)这一关键限定完全丢失,条目读起来像在说 SOP 内容对模型的可消费性。
- 影响: 信息在源头瘦身,浪费全落下游:自举拿到「查看 SOP 时」这种复现无从下手,要么猜方向(猜错=整轮白干)要么空转;更糟的是伪复现看起来像真的,没人知道该回去问用户。快记越好用、用得越多,这个失真通道流量越大。
- 根因: 三层叠加。①prompt 只说 how to reproduce **if inferable**,没规定推断不出时怎么办,模型的默认行为就是编一个;②快记的 ask 回调把 Question 一律 Cancelled(无人应答的设计约束),模型想追问也没有通道;③落库成功判据"只看库落了新条目"(main.rs:3545 注释),条目落了就算赢,信息量无人把关。
- 修复(第一层已做): prompt 明确禁止编造——推断不出复现时如实写「待澄清: <列出需要用户回答的问题>」,并要求从原文抽取关键限定词(谁的/哪个端/什么场景)进标题或复现。机制层留给后续:落库后如何机械识别"待澄清"条目并在 UI 上提示用户补充,属产品设计,交自举承接。
- 验收: ①模糊输入(如 D-204 原文)快记产出的复现字段不再是伪复现,而是「待澄清」+具体问题清单;②含关键限定词的输入(如"用户易用性")限定词不丢;③带「待澄清」的条目在侧栏可辨识(徽标/前缀任一),用户能一眼看到哪些条目等他补话;④自举取活时跳过或优先澄清「待澄清」条目,不拿伪复现开工。
- 证据等级: E1(D-204 实例 + prompt/回调/判据三处代码实证)
- 优先级: P2
- 标签: 后端

- 进展: 2026-08-09 取活:验收①prompt 层(禁止编造、写待澄清+问题清单)为既有交付(main.rs QuickCaptureComponent system 文案,3512-3519);本轮补验收③:侧栏/文档页对「复现: 待澄清: …」形态的缺陷渲染 .clarify-badge 徽标(renderDocList,只认复现字段以「待澄清」开头,title 带具体问题清单,不误标需求),i18n 词条「待澄清」,style.css .clarify-badge(accent 色);冒烟桩数据 D-001 复现改为待澄清形态并新增断言(徽标渲染+问题提示+需求不误标),ui-runtime-smoke 222 invoke 全绿,ui-i18n-smoke 771 key 全绿,frontend_check 结构完整。验收①真实快记实证与④(自举取活跳过/优先澄清待澄清条目,属调度核心改动)留后续,④记入 R-101 批次前评估。

## D-184 commands / skills 两张注册表是死的:解析注册后无人消费 [open] (medium)
- 复现: 在 `~/.kanzei/commands/` 或 `~/.kanzei/skills/`(及项目同名目录)放 markdown,MarkdownComponent 会扫描、解析并注册(crates/kanzei-harness/src/markdown.rs:22);但全仓库对 `snapshot.commands()` / `snapshot.skills()`(crates/kanzei-harness/src/harness.rs:110、114)**零调用**——文件进了注册表就地消失,既不进提示词也不成为工具。
- 影响: 六张注册表实际在跑的只有四张。用户按目录约定放了命令/技能文件,界面与模型都不会有任何反应,也没有一行提示说"注册了但没人用",属于静默无效功能。
- 根因: 注册表与消费端分两步落地,消费端(注入提示词或转成工具 spec)始终没接。
- 验收: 要么接上消费端(commands 进提示词可调用清单、skills 按 description 与任务匹配给出加载提示,与 R-106 的 sop 匹配同源),要么显式移除这两张注册表与扫描逻辑;二选一,不留"解析了但没人读"的中间态。有测试覆盖所选方向。
- 证据等级: E2(读代码确认零调用点)
- 优先级: P2
- 标签: 核心

## D-159 memory-manager 忽略前置 pathspec fatal 并把 commit 症状误记为根因 [open] (medium)
- refs: R-105
- 优先级: P2
- 复现: 一次 `git add` 因文件名大小写/截断不匹配报 pathspec，随后 `git commit` 因无暂存内容退出 1。自动 memory-manager 生成 M-013，标题断言“Changes not staged 表示没有暂存内容”，正文进一步把根因泛化为忘记 git add；但本次真实根因是前置 git add 的 pathspec 不存在。
- 影响: 记忆把症状误当根因，未来遇到同类输出会错误建议再次 git add，而不检查前置 add 是否因 pathspec/权限失败；属于会诱导重复失败的错误长期事实。
- 标签: 核心
- 根因: 失败归纳只消费了批次末尾 `git commit` 输出，没有关联同一 bash 调用前面的 `fatal: pathspec ... did not match any files`，跨命令因果被截断。
- 证据等级: E1
- 验收: M-013 被更正或标 stale，不再声称本次根因是忘记暂存；失败提炼能优先保留同一 bash 调用中更早的 fatal/pathspec 根因，或在无法判定时只记录症状不下根因结论；有回归覆盖。

- 进展: 错误 M-013 仍处于未提交状态；已向 memory inbox 投递具名更正说明，后续修复需让 failure harvest 保留同批前置 `fatal: pathspec` 根因并补回归。本轮不把错误记忆混入 R-069 提交。

## D-174 托管项目后台 Shell 缺少可归因的文件隔离 [fixed] (high)
- 复现: 后台 Bash 启动后立即返回,后续异步进程可以在任意时刻修改 `.kanzei/project` 与 `.kanzei/memory`;Harness 无法区分后台进程写入和稍后专用工具的合法写入,也无法安全回滚。
- 根因: 现有后台进程注册表只管理 PID、日志和生命周期,没有独立工作目录、文件系统沙箱或按进程归因的写入审计。
- 影响: 若继续允许托管项目中的后台 Bash,受保护文档可能绕过专用工具契约;当前修复选择在存在 `.kanzei` 的项目中拒绝后台 Bash,因此 R-097 的后台启动能力暂时降级。
- 验收: ①后台任务运行在可隔离或可归因的文件系统边界中;②后台任务不能写入 Harness 托管路径;③专用工具的合法写入不会被后台回滚机制误伤;④覆盖启动、轮询、停止、越界写入和并发合法写入测试。
- 优先级: P1
- 关联需求: R-097、R-139
- 复杂度: 大
- 批次: 4/4
- 关闭说明: 2026-08-10 关闭(fixed)。交付提交 `4ed305d`。**能力边界先说清楚,免得下次有人看到 R-097 就以为后台任务能长驻**:此前的修复是「托管项目一律拒绝后台 bash」(R-097 的后台能力在自举仓库里等于 0);本轮换成可归因边界,能力恢复到「**单 run 内可用、跨 run 被收尾**」——后台任务生命周期 ⊆ owner run 生命周期,下一个 run 起来时上一个 run 的后台任务被终止并做终态对账。这是本轮**有意为之的安全降级**,不是终态语义。

  **逐条对照验收原文(四条)**

  ①「后台任务运行在可隔离或可归因的文件系统边界中」→ **达成(归因边界,不是内核隔离)**。新建 `crates/kanzei-tools/src/managed.rs`:托管围栏从 `bash.rs` **纯搬迁**(逻辑一字未改),前台/后台共用一套口径——仓库里不该有第二套「不能写入」的语义。`BackgroundProcess` 增 `owner: BackgroundOwner{run_id, process_id, write_key}`(background.rs:31-36/56),身份取自 `ToolCtx` 的 R-171 双键;`run_id` 缺失(CLI 未 `with_identity`)登记 `unowned` 而**不伪造 id**——归因要么真实,要么如实说不知道。spawn 之前拍托管基线,`register()` 的文档注释钉死「基线必须是 spawn 之前拍的」:晚一刻就把进程自己的副作用算进基线,围栏从此永远看不见它。另有 `workdir` 结构化围栏(`background_workdir_breach`,bash.rs:412,由 bash.rs:165 调用):校验**参数**而非命令文本——D-173 已证否「猜命令文本」这条路(`WriteAllText`/重定向/解释器一行流/`git checkout` 单文件都绕得过),所以它只是静态第一道,绝对路径写入仍由守卫的结果侧对账兜住。

  ②「后台任务不能写入 Harness 托管路径」→ **达成,但走的是「隔离 + 回滚 + 归因」而非「阻止写入」**。守卫任务 `spawn_guard`(background.rs:245)按 `GUARD_TICK = 300ms`(:26)周期对账,进程退出后再对账一次(命令可能在最后一刻写盘);`reconcile`(:262)两级探测(先比元数据,命中差异才读内容),判为越界即隔离到 `.kanzei/quarantine/bg-<id>-<ms>/` → 回滚到基线 → 生成 `BreachRecord{at_ms, touched, quarantine, restored}` 留档,并可经 `process output` 读出(测试断言输出含 `[managed-files]` 与 `owner run=run-breach`)。处置比前台硬一档(连带 kill 进程树),理由是后台进程会持续重试写入,不 kill 就是无限回滚循环。**必须如实标注的一点**:交付过程中发现 `crates/kanzei-tools/src/shell.rs` 的 `kill_tree` **从未真正击杀过任何进程树**(恒定 2.008s 后返回而目标仍 alive,`kill_on_drop` 与 2s `timeout` 叠加会在超时丢弃 future 时先把 taskkill 自己杀了),已单独登记 **D-262**。因此「越界后 kill 进程树」这一**加固项有实现、但目前无可信证据**;验收②的成立**不依赖它**,靠的是隔离 + 回滚 + 归因这条与 D-173 前台围栏同口径的路径。`shell.rs` 本轮零改动(尝试性修复未解决问题,已 `git checkout` 还原干净)。

  ③「专用工具的合法写入不会被后台回滚机制误伤」→ **达成**。新建 `crates/kanzei-harness/src/managed_fence.rs`:专用写工具白名单(`MANAGED_WRITERS`,:38)+ RAII 窗口 `tool_scope` + 窗口关闭时的吸收回调(`set_absorber`,:118),与 `harness/progress.rs` 的 task-local 同构。放在 harness 是因为 kanzei-core 不依赖 kanzei-tools,runner 侧的「合法写入信号」要传给 tools 侧的守卫只能经 harness。接线两处:`crates/kanzei-core/src/runner/tool_exec.rs:95` 与 `crates/kanzei-core/src/runner/drive.rs:888` 各包一层(**串行路径必须包**,writer 阶段走的就是那条)。`reconcile` 里用 `write_in_progress` 把本轮变化分流成「有合法解释」与「越界」,前者推进基线不报越界。

  ④「覆盖启动、轮询、停止、越界写入和并发合法写入测试」→ **达成,五场景齐备**(`crates/kanzei-tools/src/background.rs`):`场景启动_托管项目后台任务登记owner与基线`(:484)、`场景轮询_读输出不误判托管树也不产生越界记录`(:508)、`场景停止_走终止路径并做终态对账`(:561)、`场景越界_后台写托管文档被隔离回滚并归因到owner`(:598)、`场景并发_窗口内合法写入不误伤_窗口外同样写入被回滚`(:662)。场景⑤**带对照组**且对照组是重点:同样的写入放在窗口**外**必须被回滚——只测「窗口内不被误伤」的话,一个恒真放行的实现照样能让上半段假绿。

  **刻意拆掉了两条「会因为错误的原因而通过」的断言,并在测试注释里写明原因**(这件事必须留档,否则后来人会以为是漏写):(a) 停止场景不再断言「进程真的退出」——`kill_tree` 是坏的,那条断言只会因为命令自然结束而绿;改断言「走了终止 + 终态对账路径、不误报越界、托管树逐字节不变」。(b) 越界场景不再断言「越界进程被 kill」——该命令写完就自然退出,`is_running()` 为假证明不了 kill 起了作用。两处注释都写明「等 D-262 修好后应把断言按正确形态补回去」,D-262 的验收③也照这条写了。

  **其它交付项**:`finish_foreign_owners`(background.rs:313)做跨 run 收尾,真实调用方在 `bash.rs:175` 与 `process.rs:60`;`kill_background_processes`(= `background::kill_project`)升级为「终止 + 终态对账」,即租约释放前的收尾入口(不另造别名),调用方 `crates/kanzei-app/src/run.rs`。删除托管项目里的后台拒绝点,`BashTool::description()` 动态化——原静态描述在托管项目里向模型宣传一个必然失败的参数,每次浪费一轮;新文案明说后台任务「fenced and owned by the current run … finished when the next run starts — so it cannot outlive this turn」。批1 的 `#[allow(dead_code)]` 已全部摘除,每项都有真实消费方(§1.25)。

  **既有能力标注(§1.25)**:托管围栏的镜像/比对/隔离/回滚逻辑(`ManagedSnapshot::capture`、`enforce_managed_files`、4 MiB 单文件 / 2000 文件上限)是 **D-173 前台围栏的既有实现**,本轮只是从 `bash.rs` 搬进 `managed.rs` 供前后台共用,**不作为本次产出申报**;`BackgroundProcess` 注册表、输出缓冲与 256 KiB 丢头留尾同样是 R-097 既有。本次新增的是:owner 身份与基线守卫、合法写入窗口(managed_fence)、跨 run 收尾、五场景测试。

  **残余缺口去处(§1.2)**:①内核级隔离(受限令牌/低完整性/AppContainer/ACL)+ 合法写入窗口的毫秒级缝隙 + 镜像上限内的空白 → **D-258**(本轮评估为代价收益倒挂:低完整性进程连 `target/`、`node_modules/` 都写不了,而那正是后台任务的唯一用途);②跨 run 长驻的受管后台服务 + 后台日志落盘可回看 → **R-180**;③`kill_tree` 失效 → **D-262**;④租约 Drop 的精确收尾接线 → **R-173**。

  **验证**:交付时定向 kanzei-tools 213 / kanzei-harness 81 / kanzei-core 119 全通过,clippy `-p tools -p harness -p app -p core --all-targets -D warnings` 零输出,rustfmt 全净;关闭前全量 `cargo test --workspace` exit=0、524 passed(复杂度大,满足 §1.4 全量触发点①)。

## D-204 SOP 用户易用性不佳:总结质量/查看展示/产生时机三处都不行 [open] (medium)
- refs: D-205 R-105 R-107
- 原始描述: SOP易用程度有问题，似乎总结的不太好
- 澄清(2026-08-09 用户逐项指认): 所指为**用户**查看/使用 SOP 时的易用性,不是 SOP 对模型的可消费性。三个维度都有问题:①**总结质量差**——条目内容泛化、丢关键步骤,看了不知道怎么照做;②**查看入口/展示**——界面上找到、打开、阅读 SOP 的路径不方便,展示形式不适合阅读;③**产生时机/数量**——该沉淀的没沉淀、不该沉淀的乱沉淀,产出节律不对。检索/命中用户未勾选,暂不在范围内。
- 复现: 桌面端 Memory 页(R-107)查看 sop 类条目;对照近期自举轮次的 SOP 产出(如 inbox 里的「候选 SOP:完成 D-155 的流程」类,只有工具顺序罗列,无判断依据与边界条件)。
- 影响: SOP 是 R-105 记忆蒸馏的主要产出形态之一,人读不动就只剩模型消费一条腿;产生时机不对还会稀释记忆库信噪比。
- 验收: ①总结质量:SOP 条目有可照做的结构(适用场景/步骤/每步判断依据/边界),不再是纯工具名罗列;②查看展示:Memory 页的 SOP 有适合阅读的排版,入口可发现;③产生时机:沉淀门槛可说明(什么样的流程值得成为 SOP),乱沉淀实例(纯机械序列)被拦;④用户复查确认三个维度都有改善。
- 备注: 本条登记过程本身暴露了快记的信息保真缺陷(伪复现「查看 SOP 时」+丢"用户"限定词),已单独登记为 D-205 并修了第一层。
- 优先级: P2
- 标签: 核心

## D-219 WIP 准入把阻塞 doing 计入配额,鞭挞提示词与 §1.1 新口径不同步 [open] (medium)
- 复现: 2026-08-09 实测:R-101(用户挂起)+R-148(仅剩等用户复查)占满 2 个 doing 名额,循环以「WIP 约束不能并发开启」拒开 R-153——两个不可执行条目把新工作准入整体锁死。
- 根因: 旧 §1.1 规则「blocked doing 不占可执行槽,但仍计入 doing 总数」自相矛盾——计入总数即占用准入;DEFAULT_CONTINUE_PROMPT 规则 5「doing 最多 2 个;已满就继续推进这两项」把旧口径写死在注入文案里,且不区分可执行/阻塞。
- 已做(规则层,2026-08-09): conventions §1.1 改为「非阻塞 doing 最多 2;阻塞/挂起 doing 不计入准入配额;含阻塞总数 >4 必须先收敛存量」;R-101 转回 todo(用户挂起,不在推进中),R-148 补①类阻塞字段(等用户复查)——名额已释放,R-153 可开。
- 待修(机制层): DEFAULT_CONTINUE_PROMPT 规则 5 文案按新口径改写(区分可执行/阻塞 doing),旧默认加入 LEGACY_CONTINUE_PROMPTS 静默升级(D-163 同族,防用户存的旧默认与新契约错位);调度器/取活预览若有同口径判断(D-207 系)一并同步。
- 验收: ①注入文案与 §1.1 新口径一致,LEGACY 升级路径有测试;②构造「2 个阻塞 doing + 可做 todo」场景,循环能开新条目不再误拒;③冒烟断言防回归。
- 边界: 改动集中在 main.js 文案与 LEGACY 数组,与 R-154 拆解撞文件——微小改动,安排在 R-154 批次间隙或 08-compose 批落位后做,不与拆解批同轮;R-157 参数化规则 6 时顺路复核本条。
- refs: D-163 R-157 D-207
- 优先级: P1
- 标签: 前端

## D-227 并行 test_record 自动生成相同时间戳 ID，四条 UI 记录互相覆盖 [fixed] (medium)
- 复现: 并行调用四个同秒 test_record，均省略 id；结果生成相同 T-1786297655，archive 中标题不同但 ID 相同。
- 影响: 测试证据无法一一引用，可能破坏测试记录唯一性与归档完整性。
- 标签: 流程
- 进展: 本轮发现；后续需用串行记录或显式唯一 id 认领，先核对 tests-archive 的实际条目。
- 优先级: P2
- 复杂度: 中
- 批次: 1/1
- 关闭说明: 2026-08-10 关闭(fixed)。交付提交 `ab41df2`。

  **先纠正本条目原文的一处事实(重要,别把它当成历史结论继承下去)**:标题与影响字段写的「四条 UI 记录**互相覆盖**」**不成立**。实测核对 `.kanzei/project/tests-archive.md`:`T-1786297655` 的四条记录(:820/:825/:830/:835,标题分别是 R-153 的 i18n / a11y / Markdown / runtime 四条冒烟)**全部落盘存活,没有任何内容丢失**;`T-1786341674` 的两条同理(:1157/:1162)。wave 排他与 R-171 的写租约都真的生效了。真实损害只有一条:**ID 不唯一 → 无法按 id 引用、无法按 id 收尾**(工具要求「用显式 id 收尾」,同号时不知道收的是哪一条)。

  **根因判断(与原文的「并行」推测相反)**:根因不是并发,是分配器**只读系统时钟不读文件**——`append_test_run` 直取 epoch 秒当 ID,同一秒内的连续两次调用必然同号。**串行 ≠ 唯一**:串行保证的是「同一时刻只有一个写者」,唯一性要的是「分配前看过已发出的编号」,两件事无交集。所以 **R-171 的写租约在原理上修不掉它**,本条也不该等租约。归档实证支持这一判断:四条记录标题各不相同,连「收尾」时刻都是同一秒。

  **逐条对照原文的诉求(本条无编号验收字段,按「影响」字段的两条诉求对照)**

  ①「测试证据可一一引用 / 记录唯一性」→ **达成**。`crates/kanzei-tools/src/test_record.rs:82` 新增 `allocate_test_id(root)`:扫 `active ∪ archive` 全部 `T-<n>`,取 `max(now_secs, max+1)` 单调推进。**选单调推进而非加后缀**是有理由的——`running_age_secs`(:67)与 `last_passed_at` 都对 `T-<epoch>` 做 `parse::<u64>()`,ID 一旦带后缀这两条会**静默失效**,且 1400+ 条历史记录要跟着迁移;代价只是突发时编号领先墙钟几秒,配套把 `running_age_secs` 的 `checked_sub` 改 `saturating_sub`(:68),编号领先墙钟时 age 归 0 而不是让 `age_secs`/`stale` 两个字段整个消失。测试:`同秒串行四次登记必须拿到四个互不相同的id`(:1044)、`写排他下并发四次登记编号仍互不相同`(:1085)、`新编号必须跳过归档里已占用的最大编号`(:1134)、`编号保持纯u64且领先墙钟时不判悬空`(:1274)。

  ②「归档完整性」→ **达成,双向把关**。新登记路径 `ensure_id_unused`(:107):编号已被 active/archive 任一条占用且标题不同 → **报错拒写、不自动改号**(沿用 `docstore::repair_reused_archived_id` 的保守立场:静默改号会把编号复用伪装成一次正常写入,证据链就此不可信,D-004 口径),错误文本给全冲突编号、两个标题、所在文件、「未写入任何内容」与两条可执行的下一步;测试 `编号已被占用时拒绝再登记并说明理由`(:1153)。归档侧同样把关(:262-286):内容相同幂等跳过,内容不同报错不追加;测试 `归档已有同编号且内容不同时拒绝追加`(:1242)。另外把「用显式 id 收尾」这条纪律做成可执行的——工具输出回显本次分配的编号(`recorded T-xxx`,:635/:643;running 状态追加「跑完请用 id=T-xxx 记终态」),不回显的话该纪律在源头就无法执行,正是本缺陷反复复发的机制缺口;测试 `工具输出必须回显本次分配的编号`(:1182)。

  **顺带修掉一处勘察时没看出来的静默失败**:旧 `record_test_run` 用 `str::replace` 做块替换,`old_block` 找不到时**原样返回、照样 `fs::write` 回去并返回成功快照**——一次收尾彻底丢失且无任何提示。现在改为定点区间替换(:400-414),找不到就明确报错「未写入任何内容,请重新读取列表后重试」(D-004 口径);同时消除了「两条字节相同的块被一次连坐换掉」的问题,测试 `定点替换不得误伤内容相同的另一条记录`(:1206)。

  **设计自我修正(留档)**:原打算加「落盘前全文件编号唯一性后置校验」,写测试时发现它与定点替换**直接冲突**——那会让已含重复编号的历史文件彻底无法写入,连修复性收尾都做不了。改为只在新登记路径查占用。

  **既有能力标注(§1.25)**:`test_record` 的追加/就地收尾/归档搬运/悬空 running 标记均为既有实现;本次产出是编号分配器、同号拒写(双侧)、定点替换与输出回显。

  **残余缺口去处(§1.2)**:①**跨进程原子性(CAS)未做**——分配器的单调推进只在单进程内成立,两个 OS 进程同时 `test_record` 仍可能撞号;按裁决并轨到 R-138 新建的 `crates/kanzei-tools/src/atomic_file.rs`(仓里只留一套原子写/文件锁原语),已登记 **D-261**。②**历史 6 条重复编号未清理**(本次修复**刻意不回改历史**,需要显式的一次性修复入口)→ 已登记 **D-259**。③`test_runs_snapshot` 是只读命令却写盘且不持任何锁 → 已登记 **D-260**。

  **验证**:交付时定向 `cargo test -p kanzei-tools` 208 passed 0 failed(test_record 21 条 = 13 原有 + 8 新增,八条都是真回归闸、旧实现逐条会挂),rustfmt clean,clippy 零诊断指向本文件;关闭前全量 `cargo test --workspace` exit=0、524 passed(复杂度中,满足 §1.4 全量触发点①)。

## D-233 文件视图打开卡顿:同步 files_snapshot 在主线程全量读+哈希 258 个文件 [open] (medium)
- 优先级: P1
- 标签: 前端
- refs: R-148 D-202
- 复现: 2026-08-10 用户实测(build-9e09b80):桌面端切到「文件」视图明显卡顿。
- 根因(代码实证,四层叠加): ①`files_snapshot` 是**同步** Tauri command(crates/kanzei-app/src/files_view.rs:24,非 async),Tauri v2 同步 command 在主线程执行——整个扫描期间 UI 完全冻结;②每次调用都全量 `scan(&root)`(kanzei-tools/src/files.rs):对每个 ≤2MB 的代码/md 文件做 `std::fs::read` 全文读取 + 行数统计 + FNV-1a 全文哈希,当前仓库命中 **258 个文件共 4.4MB**(其中 Monaco vendor 85 个文件 1.1MB 也被逐个读+哈希——它们永远不会被标注,读了纯属浪费);③scan 还同步 spawn `git ls-files` 子进程(Windows 进程创建自带几十 ms);④前端每次切视图都重新 invoke(main.js:886 `if (view === "files") refreshFiles()`),filesSnapshotData 缓存形同虚设——切走再切回就重扫一遍。与 D-202 是同类病(主线程被长任务占死),但这次在 Rust 侧不在渲染侧。
- 影响: 每次打开/切回文件视图 = 主线程同步读 4.4MB + 258 次哈希 + 一次子进程,机械硬盘或杀软实时扫描环境下秒级冻结;仓库越大越糟,与「文件视图是分析重文件的工具」的定位自相矛盾(files_view.rs 头注自己写过"本功能恰好是分析重文件的工具,自己先别成为反例")。
- 修复方向(按序独立可验): ①`files_snapshot`/`file_preview` 改 async command(线程池执行,主线程立即解放)——单词改动收益最大;②快照会话内缓存:切回视图直接用 filesSnapshotData 渲染,后台静默刷新,显式「刷新」按钮才强制重扫;③增量重扫:按 size+mtime 粗判未变的文件复用上次的行数/哈希,只重读变了的(全文 FNV 只在标注流程里保持 D-213 的 mtime 免疫语义);④vendor/gen 等永不标注的路径跳过读内容(只 stat),树里仍显示但标记「未度量」。
- 验收: ①切到文件视图主线程无秒级冻结,切换期间其它控件可点(与 D-202 验收同口径 <200ms);②切走再切回不重扫(有缓存命中证据);③第二次打开的快照耗时比首次显著下降(增量路径生效,日志或遥测可见);④vendor 文件不再被读内容,measurable 集合缩到项目自有源码;⑤冒烟或单测覆盖 async 化与缓存路径。
- 证据等级: E1(用户复现 + 代码路径实证 + 读取量实测 4.4MB/258 文件)

## D-235 conventions.md 无专用工具可写:模型只读,引擎化交付标注无法落地 [open] (medium)
- 复现: R-157 验收⑤要求 conventions.md §1.4 标注「引擎已接管」。edit 被 ruleset 拒绝:policy-managed(用户手写的项目资产,模型只读),且无专用工具;规则明令禁止 shell 旁路(重定向/Set-Content/WriteAllText/node 单行均被检测回滚)。同 D-173(architecture/README.md 无专用工具)一类的能力缺口:需求/缺陷/目标/决策各有 tracker 工具,规范文档 conventions.md 没有对应专用写入通道。
- 影响: R-157 验收⑤(文档标注)无法由 agent 完成,条目不能按 §1.25 关闭;同类缺口将来还会卡住所有需要改 conventions.md 的条目(如引擎化交付后的标注、新决策的 §1.x 更新)。
- 优先级: P2

## D-239 取活口径漂移复现追踪:伪阻塞/伪可执行/挂起无载体 [open] (medium)
- 复现: 2026-08-10 复盘取活时发现三处阻塞/挂起口径漂移:①R-151/R-162~R-167 把非阻塞内部依赖(R-150/R-161 等,解除权在 agent)写进「依赖」字段,list 据未完成依赖判 blocked,调度器整批跳过,需求队列后半截系统性锁死;②R-157 实质卡在 D-235(conventions.md 无专用写入通道,edit 被 ruleset 拒绝),却无阻塞字段,以 doing 形态占可执行 WIP 名额、实际推不动;③R-101 用户 08-09 挂起只写在进展里,状态 todo 无阻塞字段,取活器会误取。
- 根因假设: §1.1 阻塞口径只在「触碰条目时」顺带复核,无周期机械核对;2026-08-09 WIP 口径修订后历史条目未回扫(R-151 的阻塞恰在口径修订期写入)。
- 进展: 2026-08-10 已修当前三条:R-101 补挂起阻塞字段(解除人=用户);R-157 补合法阻塞(⑤依赖 D-235,解除人=修 D-235 的 kanzei 或用户手写);R-151/R-162~R-167 清空伪阻塞依赖字段,依赖关系写进各条进展。
- 验收: ①当前三条已修,req get 各条目可见清理后口径(证据:R-101/R-157 有合法阻塞字段,R-151/R-162~R-167 依赖字段为空、进展注明解锁条件);②此后每轮取活前复核阻塞/依赖字段口径,若再次出现同类漂移(伪阻塞、伪可执行 doing、挂起无载体)→ 确认为规则缺陷,升级修 §1.1/取活器并记根因;③连续 10 轮无同类复现 → 用户确认后关闭本条。
- refs: R-101 R-157 R-151 R-162 R-163 R-164 R-165 R-166 R-167

## D-243 记忆正文读取仍未回填遥测采纳 [open] (medium)
- 复现: memory_search 返回 file 后调用通用 read，当前 read.rs 只读取文件，不调用 MemoryStore::mark_recall_fetched；memory_search 自身却在搜索返回时提前标记 fetched。
- 来源: R-161 验收②与 docs/design/memory_control_plane.md §2
- 标签: 核心
- 进展: 待随 R-161 批2修复。
- 验收: 仅在真实 read 读取 .kanzei/memory 文件后回填对应召回；memory_search 与桌面端/CLI 共用 state.db 漏斗事件；保留旧 index.db 读兼容。
- 优先级: P1

## D-244 对照页优先级/阻塞控件跨队列写并落盘:调一次覆盖另一队的持久化筛选 [open] (medium)
- 优先级: P2
- 复杂度: 小
- 标签: 前端
- refs: D-207 D-211
- 证据等级: E1(取证确认 HEAD 既有 + 探针实测)
- 复现: 对照(both)标签页上,`优先级` 与 `阻塞` 两个控件仍是启用的,它们走 14-docs-actions.js 的 applyDocFilter,而 applyDocFilter 对 docFilterTargets() 返回的每个队列都写。实测:对照页把优先级调成 P0 → `before={"req":"all","defect":"all"} after={"req":"P0","defect":"P0"} saved={"req":"P0","defect":"P0"}`;调阻塞同理。缺陷队列的筛选被覆盖并落盘。
- 取证(重要,别误判成新引入): `git show HEAD:crates/kanzei-app/ui/14-docs-actions.js` 该行 = `for (const kind of docFilterTargets()) documentFilters[kind][field] = value;`,且 HEAD 的 syncDocumentFilters 也从不给 priority/blocked 置灰——**HEAD 就有的形态**,不是 2026-08-10 侧栏重构引入的。
- 与已修 P0 的区别: 这是**用户主动调控件**、两张列表当场同时变,不是「切个标签页就被改掉」;相对 HEAD 只减不增。所以不拦发版,但按 2026-08-10 定调「对照页是只读的对照视图,不得改动任何队列的持久化筛选状态」它同样不合规。
- 修复方向(二选一,都属设计决策): ①对照页禁用这两个控件(与 status/complexity/sort/tag 一致,走中性副本);②给对照页独立的筛选状态,不与两队共享。
- 验收: 对照页调任何筛选控件后,两队的持久化筛选状态均不被改写(内存与 localStorage 都要验);有拦截实测的冒烟断言。

## D-245 R-170 把 kanzei.toml [cadence] 变成死配置:设置页照写,无任何消费方送进模型 [open] (high)
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- refs: R-157 R-170 D-242
- 证据等级: E1(全仓 grep 零命中 + config.rs merge 缺分支,两处独立实证)
- 复现: R-157 交付了 kanzei.toml `[cadence]` 五个字段 + 设置页透传 + 把生效节奏渲染进继续文案。R-170(eb7ae42)按剥离清单删掉了 cadenceVerificationText 与 applyCadenceSettings ——**渲染点没了,配置就再也到不了模型**。两处实证:①`grep -rn "\.cadence|Cadence" --include=*.rs crates/` 除 settings.rs 存取与 config.rs 定义本身外零命中,JS 侧除 16-settings.js 表单存取外只剩 02-i18n.js 一段已失真的说明文案;②crates/kanzei-harness/src/config.rs 的 merge() 只合并 models/providers/proxy/profile/permissions/limits,**没有 cadence 分支**,load() 从 KanzeiConfig::default() 起手,所以 `config.cadence` 恒为默认——文件里写了也到不了运行时。
- 影响: R-157 整条交付变成惰性资产:设置页改得动、存得住、读得回,唯独不生效。用户按界面调节奏后行为不变,属于「只展示不接真实数据源」的反面(§1.25 明令这类不算完成)。与 D-242 同源——都是 R-170 剥离时误判「真源已在别处」。
- 修复方向: ①config.rs merge() 补 cadence 分支,让文件值真能进 KanzeiConfig;②给节奏一条到模型的通路(注入 system prompt,或让引擎按配置直接决定跑不跑全量,后者更符合「能代码强制的绝不只写进提示词」);③conventions §1.4 的「交付后本节标注引擎已接管」在通路补回前**不得标注**——现在标了就是假话。
- 验收: ①改 kanzei.toml 的 [cadence] 后,实测行为随之变化(轨迹或日志为证);②config.rs merge 有 cadence 单测;③设置页改参数→保存→重开生效且真作用于验证节奏;④R-157 的验收⑤有明确归宿。

## D-246 内置 provider 删不掉:fill_defaults 无条件回填五个,UI 上删了下次打开又回来 [open] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 依据: 2026-08-10 设置页全字段走查。本轮修好了**自定义** provider 的删除持久化(settings_apply_providers 按「载荷非空即权威」剪枝,有单测钉死空清单不删);但 crates/kanzei-harness/src/config.rs fill_defaults 用 entry().or_insert() 无条件注入 anthropic/ollama/codex/claude/deepseek 五个内置 provider,而 settings_get 在 fill_defaults 之后才列表——删掉这五个中任何一个,配置文件里的子表确实被删了,下次打开设置页它仍会由默认回填重新出现。
- 影响: 用户感知是「删了又回来」,会以为删除功能坏了(实际是自定义 provider 已修好、内置的按设计回填)。与 D-173 的 context_limit 兜底同源。
- 修复方向: 二选一——①UI 上把内置 provider 标成不可删(或删除按钮改「恢复默认」);②给一句「已恢复为内置默认」的说明。不建议改 fill_defaults 本身,那是配置可用性的兜底。
- 验收: 内置 provider 的删除入口不再给出「已删除」的错误预期,用户能看懂为什么它还在。

## D-247 代理选「指定地址」却留空时静默降级成 env,界面零提示 [open] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 依据: 2026-08-10 设置页全字段走查(用户定调「加提示」,登记交自举)。设置页代理模式选「指定地址」但地址框留空时,crates/kanzei-app/src/settings.rs 按空串当 `env` 处理,静默降级,界面没有任何提示——用户以为自己指定了地址,实际走的是环境变量。
- 影响: 静默降级是本仓反复吃亏的模式(D-004「任何拒绝发送的理由都要说出来,绝不静默」同族);代理配错时表现为「设了没用」,排查要一路读到 settings.rs 才看得出来。
- 验收: ①选「指定地址」而地址为空时,界面给出可见提示(表单校验或保存时提醒任一),说明将回落到环境变量;②不静默改写用户选择;③冒烟或单测覆盖该分支。

## D-248 applyProfileValue 切进程时写全局 kz-profile,把用户的全局档位选择静默降级 [open] (medium)
- 优先级: P2
- 复杂度: 小
- 标签: 前端
- 证据等级: E1(取证 HEAD 逐字一致 + 探针实测)
- 依据: 2026-08-10 持久化面全面审计(35 个写入点逐条枚举)顺带查出。crates/kanzei-app/ui/08-compose.js 的 applyProfileValue 把**进程级**档位写进**全局**键 `kz-profile`。实测:
  用户全局选了 `dev-auto` → switchProcess 到一个 research 进程 → 全局 `kz-profile` 被写成 `research`。
  而该函数上方的回退分支只认 `dev-pair`/`dev-auto`,`research` 被写进去等于**把用户的全局选择降级成 dev-pair**。
- 取证: `git show HEAD:crates/kanzei-app/ui/08-compose.js` 的 applyProfileValue 与工作区**逐字一致**——HEAD 既有行为,不是 2026-08-10 侧栏重构引入的。
- 影响: 切个进程看一眼就把全局偏好丢了,且丢法不可见(下次启动才发现档位变了)。与本轮治的那一族同病:非用户主动的操作改掉并落盘了用户的持久化状态。
- 修复方向: 进程级档位不应写全局键——要么进程档位单独存(按 session/进程 id),要么只在用户主动改档位时写全局。注意别破坏「新进程继承全局默认」的既有语义。
- 验收: 切进程不改写全局 `kz-profile`;用户主动改档位仍正常持久化;有拦截实测的冒烟断言。

## D-249 docs_snapshot 把读失败静默降级成空列表:unwrap_or_default 叠加 docstore 非原子写,前端拿到「成功但空」的快照 [fixed] (high)
- 优先级: P1
- 复杂度: 中
- 标签: 后端
- refs: R-138 D-244
- 证据等级: E1(四处代码实证 + 竞态探针实测)
- 依据: 2026-08-10 持久化面全面审计。四层叠加构成一条「瞬态空快照」通道:
  ①`crates/kanzei-tools/src/docstore.rs:307` 是 `std::fs::write(&self.path, text)`——**截断后重写,非原子**;
  ②同文件 285-291 的 `load()` 对空文件/少条目一律返回 `Ok`,不报错;
  ③`crates/kanzei-app/src/docs.rs:96` 是 `store.load().unwrap_or_default()`——**任何读失败(含 Windows 文件占用)静默降级成空列表**;
  ④`docs.rs:87-89` 每次 `docs_snapshot` 开头都跑 `archive_terminal()`,**它自己就在写这几个文件**,而它只在「有条目刚进终态」时才写——正是 `refreshDocsSoon` 被触发的同一时刻。
  于是一次 `refreshDocs`(用户点标签页)与一次 `refreshDocsSoon`(agent 事件,400ms 去抖 + IPC)完全可以同时在飞:一个在写,一个读到被截断的文件,前端拿到一份**「成功但空」**的快照。
- 影响: 不止筛选——计数归零、列表闪空都从这里来;且因为它长得像"成功",所有下游都不会重试或报警。本轮已在前端加了两道收窄(D-169 回落加空列表守卫、refreshDocs 换项目重认),但截断读到「部分条目」时 `entries.length` 仍 > 0,**前端只能收窄不能封死**。
- 修复方向: ①`DocStore::save` 与 `archive_terminal` 改 tmp+rename 原子写(与 R-138 同一件事,可并轨);②`docs_snapshot` 别把读失败 `unwrap_or_default()` 成空列表——读失败要么向上报错让前端保留上一份快照,要么显式区分「真的没有条目」与「读不到」。
- 验收: ①并发写 + 读的压测下,前端不会收到「成功但空」的快照;②读失败有可见信号(不静默降级);③原子写落地后 tracker 文件不会被读到截断态;④有回归测试。
- 批次: 1/1
- 关闭说明: 2026-08-10 关闭(fixed)。交付提交 `b4bda5c`,与 **R-138** 并轨交付——本条描述的是同一条竞态通道的四层叠加,①③层归 R-138(原子写),②④层归本条。

  **逐条对照验收原文(四条)**

  ①「并发写 + 读的压测下,前端不会收到『成功但空』的快照」→ **达成**。回归测试 `docs_snapshot_并发刷新不会返回成功但空的快照`(`crates/kanzei-app/src/state_tests.rs:295`):一个写线程反复整文件重写 `requirements.md`,主线程连做 60 次 `docs_snapshot`,每次断言条目数恒等于 6(不是「>0」——那样缺条目也能绿)。打的是完整通道:`docs_snapshot` 自己开头就会写(`archive_terminal`),正是依据字段第④层描述的重叠窗口。**反证**:回退原子写后跑同一用例,立刻打出「读到了截断态:条目数 0,只可能是 3 或 30」——本条描述的「成功但空」逐字复现。

  ②「读失败有可见信号(不静默降级)」→ **达成**。`crates/kanzei-app/src/docs.rs` 的 `docs_snapshot` 签名从 `-> serde_json::Value` 改为 `-> Result<serde_json::Value, String>`,**4 处** `unwrap_or_default()` / `map_or(0, ..)` 全部换成 `?` + `read_failed(kind, e)`(点名读不到的是哪个文件)——**比本条依据字段描述的多 3 处**:依据只点了 `docs.rs:96` 的 `store.load().unwrap_or_default()`,实际同函数里 `batch_ids` 收集、`archived`(load_archive 计数)、`archived_entries`(load_archive 取值)、`load`(每个 kind 的条目)四处都是同一个洞。回归测试 `docs_snapshot_读失败向上报错而不是空列表`(state_tests.rs:258,`#[cfg(windows)]`):先确认正常路径读得到 1 条(免得夹具不对导致假阳性),再用 `share_mode(0)` 独占占用文件,断言 `expect_err` 且错误文本含 `requirements.md`,最后解除占用断言自动恢复、不留粘滞状态。**反证**:回退该改动后,panic 里直接打出 `"requirements": Array []` 的成功载荷。

  ③「原子写落地后 tracker 文件不会被读到截断态」→ **达成,由 R-138 交付**(§1.25 显式标注:这一条不是本条的产出)。`crates/kanzei-tools/src/docstore.rs` 的四个整文件写点全部改 `crate::atomic_file::write_atomic`(:348 save / :461 与 :523 archive_terminal / :608 void_id 及 :392 repair_reused_archived_id 路径),读者只可能看到旧全量或新全量。

  ④「有回归测试」→ **达成**,即上面两条(state_tests.rs:258 与 :295),加上 R-138 侧 `crates/kanzei-tools/src/atomic_file.rs` 的 7 条与 `tracker.rs:2354` 的并发闸。

  **关键设计口径:开头那次 `archive_terminal` 明确不挂写租约**。它走 `DocStore::try_lock(200ms)` 限时文件锁(docs.rs 内 `store.try_lock(Duration::from_millis(200))`),拿不到就**跳过归档但正常返回读结果**,失败进新增的 `warnings` 数组 + `tracing::warn!`,**不参与整体失败**——原先是 `let _ =`,归档失败连一行日志都没有。不挂租约的理由:`MemoryCoordinator::acquire_writer_lease` **无超时**,挂上去会让文档面板在 agent 跑一轮期间整段卡死,等于拿一个更严重的问题换一个更轻的。该口径已写进 `docs/design/parallel_read_serial_write_orchestration.md` **不变量 8 的 2026-08-10 补注**(提交 `79852a5`,文件 :108-109):**代理发起的写动作走租约;界面读路径顺手做的幂等维护走文件锁**。后续 D-260 照此口径修 `test_runs_snapshot`。

  **零 `.js` 改动,以及为什么这成立**(这是本方案的关键收益,不是省事):前端两个消费点 `crates/kanzei-app/ui/14-docs-actions.js` 的 `refreshDocs`(:11)与 `refreshDocsSoon`(:45)**现成就是「保留上一份快照」**——`invoke` 抛错时走 `catch`,`renderDocsSnapshot` 根本不会被调用,`latestDocsSnapshot` 与屏幕上那份列表原样留着,只多一个 `toastError` / `console.error`。旧实现给的是「成功但空」,前端**没有任何办法**分辨,只能被迫重绘成空;改成抛错之后,降级方式正好落在前端已有的正确路径上。新增的 `warnings` 字段也不需要改 `.js`:前端忽略未知键。冒烟 `ui-runtime` 743 invoke 0 错误,证实零 `.js` 改动成立。

  **既有能力标注(§1.25)**:前端两处 `catch` 的「不重绘即保留上一份」语义是既有实现(D-169/D-250 系列改动带来的),本条只是让后端不再把读失败伪装成成功,不重复申报前端行为为本次产出。

  **残余缺口去处(§1.2)**:同类「只读命令顺手写盘且不持锁」的最后一处在 `test_runs_snapshot` → 已登记 **D-260**;`test_record.rs` 五处裸 `std::fs::write` 未并轨 `atomic_file` → 已登记 **D-261**。

  **验证**:交付时定向 kanzei-tools 208 / kanzei-app 53 全绿,clippy `-D warnings` 零输出,ui-runtime 冒烟 743 invoke 0 错误;关闭前全量 `cargo test --workspace` exit=0、524 passed(复杂度中,满足 §1.4 全量触发点①)。

## D-250 refreshDocs 的 catch 里 clearPendingJump 没有项目守卫:旧项目刷新失败会作废新项目刚排的跳转高亮 [fixed] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- refs: D-249
- 证据等级: E1(探针实测 pendingJumpId 从 "R-901" 变 null)
- 依据: 2026-08-10 收口验证顺带发现。crates/kanzei-app/ui/14-docs-actions.js 的 refreshDocs 本轮加了「await 前后各认一次项目」的守卫,但**只加在成功路径**;catch 里的 clearPendingJump() 没有同样的守卫。于是替旧项目发出的那次刷新若在用户切走之后才抛错,会把**新项目刚排上的**跳转高亮一并作废。
- 影响: 只丢高亮,不动数据——用户点了条目引用跳过去,却看不出落在哪一条。窄,但属同一条路径上的不对称(成功路径按项目收敛了、失败路径没有)。
- 修复方向: catch 里同样比对 forProject === currentProject,只作废属于自己那次刷新的挂起跳转。
- 验收: 旧项目的刷新失败不影响新项目已排的跳转高亮;有拦截实测的冒烟断言。
- 批次: 1/1
- 关闭说明: 2026-08-10 关闭(fixed)。交付提交 `36ce685`,与 **D-251** 同批(同一族「await 前后项目身份不一致」)。

  **逐条对照验收原文(两条)**

  ①「旧项目的刷新失败不影响新项目已排的跳转高亮」→ **达成**。`crates/kanzei-app/ui/14-docs-actions.js` 的两条 `catch` 各加项目守卫,改法与成功路径对称:`refreshDocs` 的 catch 改成 `if (currentProject === forProject) clearPendingJump();`,`refreshDocsSoon` 的 catch 同改。**同族第二处一并修**——`refreshDocsSoon` 由 agent 的文档变更事件驱动、自带 400ms 合并窗口,定时器落地时用户早就可能切走,撞上的概率比 `refreshDocs` 更高;冒烟里早有实证「只修一处、整套照样全绿」,所以两处必须各自单独钉。`toastError` / `console.error` 保持**无条件**:刷新确实失败了,与用户此刻停在哪个项目无关,该看见就得看见(D-004 口径)。

  ②「有拦截实测的冒烟断言」→ **达成**。`scripts/ui-runtime-smoke.mjs` 新增两块:`旧项目的刷新失败不得作废新项目刚排的跳转高亮(D-250)`(:3776)与 `refreshDocsSoon 的失败路径同样要按项目收敛(单独钉,D-250)`(:3830),复用 :3645 起的「甲乙两个项目 + 闸门」夹具。**反证结果(把修复改回旧代码必须判红)**:S1 `refreshDocs` 去守卫 → 红 2 条;S2 `refreshDocsSoon` 去守卫 → 红 2 条;**且 S1 与 S2 各自只红自己那块**,证明两条路径被互相独立地钉住,不是一块断言顺手覆盖了两处。

  **验证**:`node --check` 三文件 OK;ui-runtime 743 invoke 0 错误(本批新增 4 个断言块,invoke 589 → 743)、ui-a11y 22 icon-btn、ui-i18n 871 key、ui-markdown 全通过。复杂度小,按 §1.4 全量测试**非必需**;本轮仍随同批条目跑了全量 `cargo test --workspace` exit=0、524 passed。

## D-251 kz-worktrees 键在 await 之后才取:切项目撞上 IPC 会把甲项目的工作树写进乙项目 [fixed] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- refs: D-249 D-250
- 证据等级: E2(代码形态实证 + `git show HEAD:` 确认既有)
- 依据: 2026-08-10 持久化面审计。crates/kanzei-app/ui/09-sessions.js:67 与 :82 的 `kz-worktrees:${currentProject}` 是在 `await invoke(...)` **之后**才取键的——与本轮修掉的 refreshDocs 同一类跨项目错写:切项目撞上 IPC 时,甲项目新建/丢弃的工作树路径会写进乙项目的键。
- 取证: `git show HEAD:crates/kanzei-app/ui/09-sessions.js` 形态相同,**HEAD 就有,不是 2026-08-10 侧栏重构引入的**。
- 影响: 比文档刷新那条窄得多——要用户点「建/弃工作树」按钮后立刻切项目才撞上;但一旦撞上,工作树清单会长期错位(它是纯前端 localStorage 清单,不从 `git worktree list` 发现,见 R-050 退回原因④)。
- 修复方向: 与 refreshDocs 同一改法——await 前把 currentProject 存成局部量,await 后比对,不一致就丢弃本次写入。
- 验收: 切项目时的在途工作树操作不写进新项目的键;有回归覆盖。
- 批次: 1/1
- 关闭说明: 2026-08-10 关闭(fixed)。交付提交 `36ce685`,与 **D-250** 同批。

  **逐条对照验收原文(两条)**

  ①「切项目时的在途工作树操作不写进新项目的键」→ **达成**,`crates/kanzei-app/ui/09-sessions.js` 三处全部改成 await 前认领局部量 `forProject`:`refreshWorktrees`(键与逐条 `worktree_diff` 的 `projectDir` 都用 `forProject`,末尾再加 `if (currentProject !== forProject) return;` 不把旧项目清单画进新面板)、`handleWorktreeAction`(函数首行 `const forProject = currentProject;`,`worktree_merge` / `worktree_discard` 的实参与放弃后的 `kz-worktrees` 读写全走它)、`worktree-add` 点击处理(同上)。**依据字段只点了 :67 与 :82 两处;`refreshWorktrees` 的 `projectDir` 原本也在 for-await 循环里逐次重取,清单越长在飞越久,一并收敛**——这是超出条目原文的第三处。

  ②「有回归覆盖」→ **达成**。`scripts/ui-runtime-smoke.mjs:3890` 起的 `在途的工作树操作不得写进已经切走的项目的键(D-251)`,覆盖新建与放弃两条路径,**每条配正反两个方向的断言**。

  **一处对验收的解释,显式记录(不悄悄改)**:采用「**写进认领项目的键**」而非条目「修复方向」字段写的「不一致就**丢弃**本次写入」。理由:工作树清单是**纯前端 localStorage 清单,不从 `git worktree list` 发现**(R-050 退回原因④),而工作树在磁盘上已经真的建出来 / 真的放弃了;丢弃写入等于把「错位」换成**更难恢复的「丢失」**——错位至少还能在另一个项目的清单里看见,丢失则任何一次刷新都不会把它找回来。验收**原文**是「不写进新项目的键」,写进认领项目照样满足,不缩小范围。**反证实测支持这个选择**:S5「新建改成丢弃写入」→ 红 1 条(正是「没落进它真正所属的项目甲的键」那一条)。

  **反证结果(把修复改回旧代码必须判红)**:S3 新建键改回 await 后取 → 红 2 条;S4 放弃键改回 → 红 2 条;S5 新建改成「丢弃写入」→ 红 1 条。

  **反证过程补掉的一个断言盲区(值得留档)**:只做值断言时 S4 那一侧**恒绿**——放弃路径把甲的改动写进乙时,被过滤掉的是乙**根本没有的**路径,写回去逐字不变,纯值断言看不出发生过错写。于是在冒烟里加了**写入去向探针**(包住 `localStorage.setItem`,记录本次写了哪些 `kz-worktrees:*` 键),错写才无处可藏,S4 随即判红。

  **既有形态标注**:本条是 HEAD 既有缺陷(`git show HEAD:crates/kanzei-app/ui/09-sessions.js` 形态相同),不是 2026-08-10 侧栏重构引入,已在依据字段取证。

  **顺带说明(不属本条,别误以为被一起修了)**:同文件 :86 的 `}("click", refreshWorktrees);` 破损行(`worktrees-refresh` 按钮全仓无监听器)仍在,归 **D-257**。

  **验证**:`node --check` 三文件 OK;ui-runtime 743 invoke 0 错误、ui-a11y 22 icon-btn、ui-i18n 871 key、ui-markdown 全通过。复杂度小,按 §1.4 全量测试**非必需**;本轮仍随同批条目跑了全量 `cargo test --workspace` exit=0、524 passed。

## D-256 applyBatch 在 for-await 循环里逐次重取 currentProject,切项目会把旧项目条目 id 写进新项目 [open] (medium)
- 优先级: P1
- 复杂度: 小
- 标签: 前端
- 证据等级: E1(代码形态实证 + `git show HEAD:` 确认既有)
- refs: D-250 D-251 D-249
- 复现: crates/kanzei-app/ui/11-docs-list.js 的 `applyBatch`(39-70 行)对批量选中集逐条 `await invoke("docs_update", …)`,而 `projectDir` 取的是**循环体内当场读的全局 `currentProject`**(:52),不是进入批量前认领的局部量。批量操作进行中切项目,剩余条目就会拿着**旧项目的条目 id** 去写**新项目**:选中 R-001、D-001 这类在两个项目里都存在的 id 时,新项目里的同号条目会被真的改状态、改标签。
- 取证(别误判成新引入): `git show HEAD:crates/kanzei-app/ui/11-docs-list.js` 的 applyBatch 与工作区**逐字一致**(HEAD=36ce685,同样是 39-70 行、`projectDir: currentProject` 落在 :52)——**HEAD 就有的形态**,不是 2026-08-10 侧栏重构、也不是 D-250/D-251 收口引入的。
- 影响: 与 D-250/D-251 同族(await 前后项目身份不一致),但**危害高一档**:D-250 只丢跳转高亮、D-251 只错写 localStorage 工作树清单,本条是**真数据错写**——新项目的 tracker 条目被改状态/改标签并经 docs_update 落盘,用户事后看不出是谁改的。批量越大、切得越早,错写条目越多。
- 待定(产品决策,**不代用户拍板**): 中途换项目时,剩余批量操作应当**整批中止**,还是**继续按认领的旧项目做完**?两种语义都自洽——中止 = 最保守,不再动任何项目;按认领项目做完 = 用户本意就是对旧项目那批条目生效,只是人走开了。取活前必须先向用户确认(D-205 教训:不代用户猜死),确认后再按所选语义改写本条验收③。
- 修复方向: 无论选哪种语义,`projectDir` 都必须在进入循环**之前**认领成局部量(与 36ce685 对 refreshDocs / handleWorktreeAction 的改法同源),循环内每次 await 后比对;差异只在比对不一致时是 `break` 还是继续用认领的局部量。
- 验收: ①批量操作进行中切项目,**不得有任何一条**写进新项目(逐条核对 docs_update 的 projectDir 实参);②有拦截实测的冒烟断言(scripts/ui-runtime-smoke.mjs 构造「await 中途改 currentProject」的桩,断言后续 invoke 的 projectDir 一律不是新项目);③所选语义在 UI 上对用户可见——选中止要给出「已切换项目,剩余 N 条未执行」之类的明确反馈,选按认领项目做完要说明这批改动落在哪个项目。

## D-257 worktrees-refresh 刷新按钮全仓无监听器:addEventListener 前半段被重构吃掉,只剩 no-op 逗号表达式 [open] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 证据等级: E1(按钮存在、全仓零绑定、git log -S 定位引入提交,三处独立实证)
- refs: D-211
- 复现: 侧栏「隔离工作树」区块标题右侧的刷新按钮(↻)**点了没反应**。
- 依据①(按钮确实存在): crates/kanzei-app/ui/index.html:79 —— `<button id="worktrees-refresh" class="icon-btn" title="刷新工作树差异" aria-label="刷新工作树差异">↻</button>`,位于 `#worktrees-section` 的 section-title 内。**注意 id 是 `worktrees-refresh`(复数 worktrees),不是 `worktree-refresh`**:按单数形式 grep 会一无所获并误判成「元素已删除」。
- 依据②(全仓零绑定): `grep -rn "worktrees-refresh" crates/ scripts/` 只命中 index.html:79 那一行,没有任何 JS 绑定它。
- 依据③(破损行): crates/kanzei-app/ui/09-sessions.js:86 是 `}("click", refreshWorktrees);` —— `$("worktrees-refresh").addEventListener` 的前半段丢了。`}` 结束的是上方 `async function handleWorktreeAction(item, action)` 的函数**声明**,其后的 `("click", refreshWorktrees);` 成了一条独立的、合法但完全 no-op 的逗号表达式语句(函数声明不是表达式,不会被调用)。`node --check crates/kanzei-app/ui/09-sessions.js` **通过**——语法检查抓不到它,正是 conventions §1.3「前端改动不得只以 node --check 作为验证证据」说的那类漏网。
- 取证: `git log -S 'worktrees-refresh").addEventListener' -- crates/kanzei-app/ui/` 与 `git log -S '}("click", refreshWorktrees);'` 共同指向 **7c5f022「增加工作树操作失败重试入口」(2026-08-07)**;`git show 7c5f022 -- crates/kanzei-app/ui/main.js` 的 diff 逐字为 `-$("worktrees-refresh").addEventListener("click", refreshWorktrees);` / `+}("click", refreshWorktrees);`——把工作树操作抽成 `handleWorktreeAction` 时,新函数的收尾 `}` 覆盖掉了下一行的 `$("worktrees-refresh").addEventListener` 前缀。R-154 B5(9349b45)切出 09-sessions.js 时原样带了过来。**HEAD 既有**(HEAD=36ce685 的 :86 仍是同一形态),不是本轮改动引入。
- 结论(纠正勘察分歧): 按钮**没有被删**——这**不是删残留,是真正的按钮失效**,修法是恢复绑定而不是清理死代码。
- 影响: 工作树差异清单只剩自动刷新路径(handleWorktreeAction 成功后 09-sessions.js:81、worktree-add 成功后 :99、以及 14-docs-actions.js:16 与 02-i18n.js:754 的整体刷新),用户看到过期状态时**没有手动刷新手段**。危害窄(工作树本身低频),但属于「界面承诺了能力却没有能力」,与 D-211 同族。
- 修复方向: 把 09-sessions.js:86 还原成两行——函数声明收尾的 `}`,以及独立一行 `$("worktrees-refresh").addEventListener("click", refreshWorktrees);`。
- 验收: 二选一,不留中间态。**优先①**——①按钮真能刷新:点击 `#worktrees-refresh` 后 refreshWorktrees 被调用且工作树清单重渲染,scripts/ui-runtime-smoke.mjs 有对应冒烟断言(断言点击后触发 worktree 相关 invoke);或②按钮与 09-sessions.js 的 no-op 残留一起清理干净(index.html 不再有该按钮、JS 不再有那条逗号表达式)。选②等于删掉用户可见的界面能力,属缩小范围,需先经用户同意。

## D-258 后台任务缺内核级文件隔离:归因+回滚拦不住合法写入窗口的毫秒级蒙混 [open] (medium)
- 优先级: P2
- 复杂度: 大
- 标签: 核心
- 证据等级: E1(读码核实 crates/kanzei-tools/src/managed.rs 与 background.rs,2026-08-10 dev HEAD)
- 来源: 2026-08-10 D-174 交付时的残余转出。D-174 本轮做的是「**按进程归因 + 越界回滚**」:`ManagedSnapshot::capture` 在动作前后各拍一次托管目录镜像(managed.rs),改了就隔离留证 + 整体回滚;后台任务登记 `BackgroundOwner{run_id, process_id, 写仲裁键}`。这是**结果侧**判定,故意不靠命令文本匹配(`WriteAllText`、重定向、python/node 一行流、`git checkout` 单文件都能避开任何字符串匹配)。
- 未做的部分与理由: **内核级隔离**(受限令牌 / 低完整性进程 / AppContainer / 托管路径 ACL)被评估为**代价收益倒挂**——低完整性进程连 `target/`、`node_modules/` 都写不了,而那正是后台任务的唯一用途(跑 build、跑 dev server、跑测试)。为了堵一个窄缝把功能整个杀掉,不划算,所以本轮明确不做,转出为独立条目待重新评估。
- 残余缺口(本条要解决的): ①无内核级边界:后台进程在操作系统层面**仍有权限**写托管路径,拦截全靠前后两次镜像比对。②**合法写入窗口的毫秒级蒙混**——专用工具正在合法写托管文档的那个窗口里,后台进程若同时写同一批文件,前后镜像比对无法区分哪一笔是合法的、哪一笔是后台进程的,回滚要么误伤合法写入、要么放过越界写入。③镜像本身有上限:单文件 >4 MiB 只记指纹(能检测不能回滚)、目录 >2000 文件直接放弃镜像(既不检测也不回滚,只在输出里如实说明),这两条边界内后台进程可以自由写。
- 修复方向(择一或组合,取活前先评估代价): ①托管路径 ACL:给后台进程一个专用身份,对 `.kanzei/project`、`.kanzei/memory` 拒绝写——比整进程低完整性精确得多,不影响 `target/`;②合法写入窗口内改走独占文件锁(R-138 的 `FileLock`),让镜像比对不必在窗口内做判定;③镜像上限内的空白改为**显式拒绝后台任务**而不是静默放行。
- 验收: ①存在一条不依赖前后镜像比对的机械边界,后台进程写托管路径在**操作系统层面**失败(或有等价的、不靠事后比对的拦截),有实测证据;②后台任务仍能正常写 `target/`、`node_modules/` 等非托管路径(不得为了堵缝把功能杀掉),有回归;③专用工具的合法写入窗口内,后台进程的越界写入被识别且合法写入不被误伤,有并发用例覆盖;④镜像上限(4 MiB 单文件 / 2000 文件)被突破时的行为是**显式拒绝或显式告警**,不是静默放行,有测试。
- refs: D-174 R-097 R-139 R-180

## D-259 tests-archive 历史重复编号未清理:T-1786297655 四条同号、T-1786341674 两条同号 [open] (low)
- 优先级: P3
- 复杂度: 小
- 标签: 流程
- 证据等级: E1(实测统计 + 读码核实分配器与拒写逻辑,2026-08-10 dev HEAD)
- 复现: `grep -o "T-[0-9]*" .kanzei/project/tests-archive.md | sort | uniq -c | sort -rn` → `4 T-1786297655`、`2 T-1786341674`,其余编号各 1 条。同号记录标题不同,按 id 无法区分是哪一次测试。
- 来源与边界(别重复修已修好的部分): **D-227 已修好分配器**——`crates/kanzei-tools/src/test_record.rs` 现在扫描已有集合单调推进(不再同秒撞号)、同号拒写(`ensure_id_unused`)、归档侧内容不同时拒绝追加第二条同号记录(test_record.rs:275-283)。**新的重复不会再产生**;本条只管**历史存量**。
- 为什么不自动清理: 参照 `crates/kanzei-tools/src/docstore.rs:392` `repair_reused_archived_id` 的保守立场——静默改号会把编号复用伪装成一次正常写入,证据链就此不可信(D-004:拒绝的理由必须说出来,绝不静默)。所以 D-227 的修复**刻意不回改历史**,需要一个**显式的一次性修复入口**。
- 影响: 窄。测试证据按 id 反查时,这 6 条记录里有 4+2 条互相指不清;条目关闭时引用「T-1786297655」无法确定指的是哪一次。另注:归档解析用 `BTreeMap` 按 id 收敛,重复条目在解析层被折叠成一条,所以既有代码路径不会因此报错,问题只在人工反查。
- 修复方向: 给 `test_record` 加一个显式的一次性修复动作(参照 tracker 的 `repair_reused_id`:必须显式指定 id、必须说明改成什么、结果打印出来),把历史同号记录逐条改成未占用编号并保留原标题/内容;不得静默批量改。
- 验收: ①`tests-archive.md` 里每个 `T-` 编号唯一(同一条命令可机械核验:`grep -o "T-[0-9]*" ... | sort | uniq -d` 输出为空);②改号动作是显式入口、有输出说明哪条改成了什么,不是自动触发;③改号后原记录的标题、状态、命令、summary、关联字段一字不丢,有测试;④D-227 已修好的分配器与拒写逻辑不被本条改动破坏(既有测试保持绿)。
- refs: D-227 D-004

## D-260 test_runs_snapshot 只读命令却写盘且不持任何锁:绕过不变量 8 的最后一个写点 [open] (medium)
- 优先级: P2
- 复杂度: 小
- 标签: 后端
- 证据等级: E1(读码核实两处调用链,2026-08-10 dev HEAD;行号以实读为准,R-138 的代理正在改 docs.rs)
- 复现: `crates/kanzei-app/src/docs.rs` 的 `test_runs_snapshot` 是**同步只读命令**,直接转调 `kanzei_tools::test_record::test_runs_snapshot(&root)`,**不取任何锁**。而被调方在 `crates/kanzei-tools/src/test_record.rs` 里会真的写盘:发现 active 里有终态记录时,`std::fs::write(&archive_path, ...)` + `std::fs::write(&active_path, ...)` **改两个文件**。
- 对照(同文件的两个兄弟命令都做了): 同一 docs.rs 里的 `test_run_record` 与 `test_runs_init_refs` 都先 `acquire_writer_lease` 再写(R-171 批4 模式,注释明写「不能绕过协调器」)。只有 `test_runs_snapshot` 这条读路径顺手写盘却什么都不持。
- 影响: 这是设计不变量 8(「`test_record` 等写入口不得绕过协调器」,见 docs/design/parallel_read_serial_write_orchestration.md)的残留缺口。用户点开测试面板的那一刻,可以与 agent 那边的 `test_record` 写入撞在一起,两个写入者同时整文件重写 `tests.md` / `tests-archive.md`——与 D-249 描述的 `docs_snapshot` 竞态**同构**。
- 修复口径(照抄 R-138 对 `docs_snapshot` 的处置,**不要挂写租约**): R-138 本轮对同文件 `docs_snapshot` 的处置是**毫秒级文件锁 + 限时 `try_lock`**(`crates/kanzei-tools/src/atomic_file.rs` 的 `FileLock`,拿不到就跳过归档、落 `warnings`),而不是挂写租约——`MemoryCoordinator::acquire_writer_lease` **无超时**,挂上去会让面板在 agent 跑一轮期间整段卡死,等于拿一个更严重的问题换一个更轻的。判据已写进不变量 8 的 2026-08-10 补注:**代理发起的写动作走租约;界面读路径顺手做的幂等维护走文件锁**。本条属后者。
- 验收: ①`test_runs_snapshot` 的归档写盘被限时文件锁保护,拿不到锁时**跳过归档但正常返回读结果**(不阻塞面板、不报错弹窗),有测试;②并发「面板刷新 + agent `test_record`」的用例下,`tests.md` / `tests-archive.md` 不丢条目、不出现截断态,有回归;③归档写盘走原子写(与 D-261 并轨,不各写各的);④`test_runs_snapshot` 不引入写租约(有断言或注释锁定这条口径,防下一个人"顺手改成和兄弟命令一致"把面板卡死)。
- refs: R-138 D-227 D-249 D-261 docs/design/parallel_read_serial_write_orchestration.md

## D-261 test_record 五处 std::fs::write 未并轨 atomic_file:跨进程 CAS 缺失,仓里两套写原语 [open] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 核心
- 证据等级: E1(全文件 grep 实证 + 读码核实 R-138 新原语,2026-08-10 dev HEAD)
- 来源: 2026-08-10 D-227 交付时的残余转出。D-227 本轮只做了 ①编号分配器(扫描已有集合单调推进,串行也保证唯一)与 ②拒写/定点替换(`ensure_id_unused` + 归档侧同号内容不同即拒);**跨进程 CAS 未做**。按裁决要**并轨到 R-138 新建的 `crates/kanzei-tools/src/atomic_file.rs`**,仓里只留一套原子写原语。
- 复现(实证): `crates/kanzei-tools/src/test_record.rs` 的生产路径仍是**裸 `std::fs::write`** 五处(测试代码另计),全文件对 `atomic_file` 零引用。而 `crates/kanzei-tools/src/docstore.rs` 的四个整文件写点已经全部改成 `crate::atomic_file::write_atomic`。**同一个仓库里因此并存两套写语义**,这正是 atomic_file.rs 头注明令禁止的:「仓里只能有**一套**原子写/文件锁实现……两套原语意味着两套失败语义,并发排查时没人说得清哪一份才是真的」。
- 影响: ①`std::fs::write` 是**先截断再写**,写到一半时另一个进程(kz CLI / 自举循环 / 第二个 kzapp)读到零长度或半截 `tests.md`——与 D-249 第①层同病;②「读 → 算下一个 id → 写」这段没有跨进程 CAS,分配器的单调推进只在**单进程内**成立,两个 OS 进程同时记录仍可能撞号(D-227 修的是同秒时间戳,不是跨进程竞态);③失败时没有 `atomic_file` 的"保留临时文件供排查"语义。
- 修复方向: 五处生产写点全部改走 `atomic_file::write_atomic`;「读 → 分配 id → 写」整段用 `atomic_file` 的 `FileLock`(`lock_exclusive` / `try_lock_exclusive`)罩住,与 docstore 的 `TrackerTool` 写动作分支同源。注意 `FileLock` 是 `!Send`,不得跨 await 点持有。**不要另造锁**。
- 验收: ①`crates/kanzei-tools/src/test_record.rs` 的生产路径不再出现裸 `std::fs::write`(可机械核验:该文件非 `#[cfg(test)]` 区域 grep `fs::write` 零命中);②「读→分配→写」整段持锁,两个进程并发 `test_record` 不撞号、不丢记录,有跨进程或多线程压测覆盖;③全仓只有 `atomic_file` 一套原子写/文件锁原语(grep 无第二处 tmp+rename 或独占句柄实现);④D-227 已交付的分配器与拒写逻辑既有测试保持绿。
- refs: D-227 R-138 D-249 D-260

## D-262 shell::kill_tree 从未真正击杀进程树:2 秒 timeout 叠加 kill_on_drop 反而先杀死 taskkill 自己 [open] (medium)
- 优先级: P1
- 复杂度: 中
- 标签: 核心
- 证据等级: E1(实测可复现 + 代码形态自证)
- refs: D-174 R-097 R-139
- 复现: 2026-08-10 交付 D-174 写「停止后台任务」测试时暴露,三条实测证据:
  ①`kill_tree(pid)` 恒定耗时 **2.008 秒**(正好是它自己的超时)后返回,目标进程 `alive_after=true`;
  ②把超时去掉单独跑,内层 `taskkill` 阻塞约 **27 秒**(直到目标进程自然结束)才返回 `exit=128`;
  ③`current_thread` 与 `multi_thread` 两种 tokio runtime 都复现;换 `std::process` + `spawn_blocking`、去掉 `hide_console_async` 均不解决。
- 根因(代码形态自证,`crates/kanzei-tools/src/shell.rs` 的 `kill_tree`): `command.kill_on_drop(true)` 与 `tokio::time::timeout(2s, command.output())` 叠在一起——**超时丢弃 future 的那一刻,`kill_on_drop` 把 taskkill 进程本身杀了**。于是每次调用的实际行为是「启动 taskkill → 两秒后杀掉 taskkill → 返回」,目标进程树毫发无伤。返回值还被 `let _ =` 吞掉,失败完全不可见。
  次因待查:证据②说明 taskkill 在本机确实需要远超 2 秒才返回(疑似 `output()` 等待管道关闭,而管道被目标进程树里的某个成员继承着——典型的「grandchild 继承 stdout 导致 output() 挂住」形态)。若属实,则超时值调大也治不好,应改为不捕获输出(`status()` 而非 `output()`)或显式给 taskkill 的 stdio 设 `null`。修复前必须先证实这一条,不要只把 2 秒改成 30 秒。
- 影响(超出 D-174 的范围): ①`process stop` 名义返回 stopped、实则进程还在跑,用户以为停了;②`bash` 工具的超时击杀同样失效,超时只是让工具调用返回,被击杀的进程继续持有文件与端口;③D-174 的后台越界处置里「回滚后 kill 进程树」这一加固项目前无效——该条已在 D-174 交付时如实标注,其验收②靠的是隔离+回滚+归因这条与 D-173 前台围栏同口径的路径,不依赖 kill。
- 边界: `shell.rs` 在 D-174 交付时未被修改(尝试性修复未解决问题,已 `git checkout` 还原干净),本条是独立缺陷。
- 验收: ①`kill_tree` 调用后目标进程树**真的消失**(实测断言 `alive_after == false`,不是断言函数返回);②taskkill 失败/超时不再被静默吞掉,至少有 `tracing::warn!` 级别的可见信号(D-004 口径);③`process stop` 与 `bash` 超时两条路径各有一条断言进程真的退出的回归测试——注意 D-174 交付时**刻意拆掉了两条会因为错误的原因而通过的断言**,本条修复后要把它们按正确形态补回去;④非 Windows 分支保持可编译。
