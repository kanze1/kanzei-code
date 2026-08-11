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
- 进展: **主体已交付,保持 open 因验收③未达成**(`dadf1ce`,经 `88b9cda` 并入 dev)。2026-08-11 任务级并行实测的线 C 产出,改动面只含 `crates/kanzei-tools/src/test_record.rs`。已达成:**①**五处生产写点全部并轨 `atomic_file::write_atomic`(快照归档两处、`record_test_run` 定点替换、`append_test_run` 追加、`initialize_refs` 回填),并加了机械守护测试(按行切到 `#[cfg(test)]` 为止、跳过注释行,复发当场红);**②**新增 `lock_test_runs()` 走 `atomic_file::lock_exclusive`,键取 `tests.md` **一把锁同时罩活动与归档**(因 `allocate_test_id`/`ensure_id_unused` 本就同时扫两边,分开锁等于没锁),`record_test_run`/`append_test_run`/`initialize_refs` 各自把「读→分配/认领→写」整段罩住,内层嵌套走 `FileLock` 同线程重入计数;三个持锁函数全是同步 fn,锁不进 async 状态机(`!Send` 由编译器兜着,未另造锁)。快照的幂等归档拆成 `archive_terminal_records` 用 `try_lock_exclusive(200ms)`,拿不到锁跳过归档照常返回读结果(与不变量 8 补注同口径),编号复用/IO 故障等真失败仍照常报错。新增三条用例:8 线程无外部串行并发登记(编号互异、记录不丢)、外部持锁期间登记必须等待且 `tests.md` 不被创建(证明罩的是整段而非只罩落盘)、快照拿不到锁时跳过而非报错;**④**D-227 既有用例全绿(`cargo test -p kanzei-tools` 217 passed),clippy `-D warnings` 干净。
  **未达成的验收③(全仓只留一套原子写原语)**:仓里仍有四处独立 tmp+rename,均不在本次改动面内——`crates/kanzei-llm/src/auth/store.rs:50`、`crates/kanzei-tools/src/architecture.rs:202`、`crates/kanzei-tools/src/files.rs:64`、`crates/kanzei-tools/src/memory/store.rs:1356`。本条据此保持 `open`,收口这四处即可关闭。
  **另记一条本次实测的设计发现(与 R-182 同源)**:`lock_path_for` 把锁文件放在目标同目录,即 `<worktree>/.kanzei/project/tests.lock`。并行工作树各有自己的 `.kanzei/`,所以**各写各的 `tests.md` 时根本不会互斥**;互斥只在同一份 checkout 被多个进程打开时才成立。这与实测「两个 worktree 相隔 10 秒各 `kz defect add` 都拿到 D-267」是同一件事的两面——**锁生效的前提是文档只有一份**,落点见 R-182 内容①②。

## D-263 自举提交时暂存了非本轮改动:应只 git add 明确文件,否则并发写入被静默卷进他人提交 [open] (medium)
- 优先级: P1
- 复杂度: 小
- 标签: 流程
- 证据等级: E1(2026-08-11 实例,提交为证)
- refs: R-181 D-264
- 复现: 2026-08-11 凌晨,自举循环取活 R-174 期间,外部 agent 正在同一批文件上工作(尚未完成)。自举的 `92879e2`(R-174 B2)与 `25ea2c0`(R-174 B3)**把外部 agent 未完成的改动一并暂存并提交**——提交标题里的「含 R-173 遗留收尾」正是被裹进去的那部分。自举本身并不知道自己提交了什么额外内容。
- 根因: 自举轮末提交时按「工作区里所有改动都是我的」暂存(`git add -A` / `git commit -a` 一类),而不是只 add 本轮实际动过的文件清单。这个假设在单写者下成立,在有外部 agent 或人手动改动时不成立。
- 影响: ①**改动归属混乱**——两个来源的改动挤进同一个提交,事后拆分只能靠人读 diff;②**回滚锚点失效**——revert 该提交会连带撤销别人的工作;③被裹进来的改动**没有经过自举自己的门禁**(本次就带进了 8 处 fmt + 6 条 clippy 红灯,见 D-264);④外部 agent 那边看到的是「我没提交,但我的改动不见了/已提交」,极易误判。
- 修复方向: 轮末提交改为**只暂存本轮明确改动过的文件**——引擎本来就知道自己调用过哪些写工具(edit/write/tracker/test_record 的目标路径都有记录),按那份清单 `git add <file>...`。若发现工作区里有清单之外的改动,**不要静默跳过也不要一并提交**,而是在提交说明或轨迹里明说「工作区另有 N 处非本轮改动,未纳入本次提交」(D-004 口径:任何不做的理由都要说出来)。
- 边界: 这条与 R-181(跨 agent 写入互斥)互补不互替——R-181 让双方知道对方在写,本条保证**即使撞上了,损伤也只停在各自的文件里**。本条更便宜、更该先做。
- 验收: ①构造「工作区有本轮之外的改动」场景,自举轮末提交**只包含本轮文件**,清单外的改动仍留在工作区;②提交说明或轨迹里对被跳过的改动有可见记录;③有回归测试覆盖「清单外改动不入暂存区」。

## D-264 定向测试口径漏掉新增集成测试所在 crate:cargo test 全绿但 fmt/clippy 从未跑到 [open] (medium)
- 优先级: P2
- 复杂度: 小
- 标签: 流程
- 证据等级: E1(2026-08-11 实例,已修但机制未修)
- refs: D-263 R-152
- 复现: 2026-08-11 自举交付 R-174 批1-3,进展里写「定向:core 119/harness 82/tools 213/app 67 全绿」与「cargo test --workspace 全量全绿」——都属实。但它本轮**新增的两个集成测试**落在 `crates/kanzei/tests/`(`task_cancel_parallel.rs`、`max_tasks_parallel_dispatch.rs`),而 `cargo test --workspace` 会跑它们、`cargo fmt --all --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` **从头到尾没被跑过**。结果:8 处 fmt + 6 条 clippy 红灯随提交进库(已由 `06a2b87` 收口)。
- 根因: conventions §1.3/§1.4 的定向验证口径是「动了 crates/ 跑 `cargo test -p <改动 crate>`」——它只提测试,**没提 fmt 与 clippy**,而 CI(`.github/workflows/ci.yml`)与发版门禁(`scripts/verify.ps1`)两处都把 fmt/clippy 列为必过项。规则层与门禁层不同步:按规则做到位,推上去照样红。
- 影响: 红灯要等 push 后 CI 才暴露,而自举一轮可能提交多次;更糟的是**发版门禁会当场拦下**(本轮 `package.ps1` 的验证证据门禁就是这样拦住的),排查时要回溯好几个提交才找得到源头。
- 修复方向(二选一或都做): ①把 fmt/clippy 写进 §1.4 的定向清单——每次提交前对**改动文件**跑 `rustfmt --edition 2021 <file>` 与对**改动 crate** 跑 `cargo clippy -p <crate> --all-targets -- -D warnings`(注意:本次那 6 条 clippy 只在编译 `-p kanzei` 时才暴露,只跑改动最多的那个 crate 不够,新增了测试文件就要连它所在的 crate 一起跑);②做成代码强制而非规则:轮末提交前引擎自动跑一次 fmt/clippy 定向检查,红了不许提交(conventions §4「任何『规则』能用代码强制的绝不只写进提示词」)。**推荐 ②**,因为 ① 已经写在规则里过一次而这次仍然漏了。
- 验收: ①构造「新增文件带 fmt/clippy 违规」的场景,提交前被拦住并明说违规位置;②conventions §1.4 的定向清单与 CI/verify.ps1 的门禁清单**逐项对齐**,两处任一新增门禁时另一处必须同步(可加一条守护测试比对两份清单);③有回归覆盖。

## D-265 dev 构建的更新检查谎报「已是最新」:release_is_newer 对 dev 直接返回 false,用户永远不知道该手动装 [open] (medium)
- 优先级: P1
- 复杂度: 小
- 标签: 发布
- 证据等级: E1(用户实测截图 + 代码形态自证)
- refs: D-145 D-004
- 复现: 2026-08-11 用户装完 build-22a927c 后打开设置页「版本与更新」,显示 `当前版本 dev` + 点「检查更新」得到 **「已是最新(build-22a927c)」**。它明明已经取到了最新发布的 tag,却告诉用户不用更新;用户据此以为自己已经在新版上,实际左边栏没有子代理面板、「更多」里没有勘察复核开关——新代码一个都没在跑。
- 根因(代码自证): `crates/kanzei-app/src/update.rs` 的 `release_is_newer` 第一行短路——`if current_hash == "dev" || tag.is_empty() || tag.contains(current_hash) { return false; }`。dev 构建(`KANZEI_BUILD_INFO` 未设,即 `release.ps1` / `cargo build` 产出的那份)直接判定「没有新版」,`update_check` 于是回 `status: "latest"`,前端 `16-settings.js` 按这个渲染成「已是最新」。
- 影响: **一旦落到 dev 构建上,应用内更新通道就永久失效且无声**——启动时的静默检查不会弹 toast,手动点「检查更新」还会得到一句反向的保证。这正是 D-145「发布了但仍在跑旧版」那一族,只不过上次的成因是两份副本、这次的成因是版本比较把 dev 当终点。用户唯一的出路是有人告诉他去手动装 setup.exe。
- 为什么当初这么写(不要简单删掉那个分支): dev 构建没有可比的时间戳(`build_stamp` 需要 `KANZEI_BUILD_INFO` 的第二段),硬跟发布版比会得出无意义的结论。所以 `return false` 在**比较语义**上没错,错的是把「无法比较」渲染成「已是最新」。
- 修复方向: 让 `update_check` 区分三态而不是两态——`latest`(真的最新)/ `update`(有新版)/ **`incomparable`**(本地是 dev 构建,无法与发布版比较)。第三态的文案必须明说:「本地为开发构建(dev),无法与发布版比较;最新发布是 build-xxxx,需要手动运行安装器」,并给出下载入口。D-004 口径:任何不做的理由都要说出来,绝不静默。
- 验收: ①`KANZEI_BUILD_INFO` 未设时,设置页不再显示「已是最新」,而是明说无法比较 + 最新发布 tag + 手动安装指引;②发布构建的既有两态行为不变(有既有单测的保持绿);③`release_is_newer` 的三态判定有单测覆盖(dev / 同 hash / 更新的发布各一条);④启动时的静默检查在 dev 构建下也给出一次可见提示(不弹窗打扰,但设置页要能看到)。

## D-266 setup.exe 的 /S 静默安装在 kzapp 运行时静默无效:退出码 0、文件没换、无任何提示 [open] (medium)
- 优先级: P1
- 复杂度: 中
- 标签: 发布
- 证据等级: E1(2026-08-11 用户实测,三条独立证据)
- refs: D-265 D-145 D-004
- 复现: 2026-08-11 连发两版(build-c4c7300、build-22a927c),两次都按流程执行 `kanzei-setup-<hash>.exe /S`,退出码 0、无任何输出。用户开 kzapp 后发现新功能一个都没有。实测取证:
  ①`%LOCALAPPDATA%\kanzei\kzapp.exe` 的 `LastWriteTime` 是 **2026-08-10 21:07:01** —— 早于两次发版(00:40 与 01:47),文件从未被替换;
  ②在该 exe 里按字节搜 `22a927c` → **不存在**;而发布树刚构建的 `target/release/kzapp.exe` 里搜得到 `22a927c 20260810174535`,证明构建产物本身是对的、`KANZEI_BUILD_INFO` 也确实传进了二进制;
  ③设置页版本徽章显示 `v0.1.0 (dev)`,即跑的是 `release.ps1` 装的开发构建(它不设 KANZEI_BUILD_INFO)。
  用户改为**双击运行安装器(不带 /S)**后立即装上,新功能出现。
- 根因: Tauri 的 NSIS 模板在目标程序运行时需要先处理占用(结束进程或提示用户);静默模式(`/S`)下无人可问,它直接放弃并**以成功退出码结束**。于是调用方(人或脚本)拿到的是「装好了」,实际一个字节没动。conventions §9.1 把「静默装 setup.exe」写成了标准做法,而这条路径在最常见的场景(应用正开着)下恰好无效。
- 影响: 这是本仓第三次栽在「发布了但仍在跑旧版」上——D-145 是两份副本,D-265 是更新检查谎报已最新,本条是安装器静默无效。三条叠起来的效果是:**发版流程每一步都报成功,而用户手上的二进制没变,且应用内更新还会告诉他已是最新**。2026-08-11 实测里三条同时命中,排查花了四个来回才定位。
- 修复方向: ①`package.ps1` 与发版检查单里的静默安装改为**装后校验**——比对安装位 exe 的 `LastWriteTime`,并在其字节里确认含本次 hash,不符即报错并明说「kzapp 正在运行,请关闭后重装」(与 `verify.ps1` 的证据绑定同一哲学:**不信退出码,信产物**);②或在静默安装前主动检测 kzapp 进程,有则拒绝并提示,不要试了才发现;③conventions §9.1 补一句:静默安装在应用运行时无效,必须先关应用或改用交互式安装。**推荐 ①**,因为它同时挡住其它未知的静默失败模式。
- 验收: ①kzapp 运行中执行静默安装,流程**当场失败并说明原因**,不再返回成功;②装后校验能对上本次 hash(有实测证据);③conventions §9.1 与实际行为一致;④与 D-265 的三态更新提示合起来,发版链路任一环节没生效时用户都能看到可见信号。

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

## D-268 background.rs 围栏测试只用进程级 Mutex 串行化:两条线并行跑同一 crate 测试时毫无保护,可假绿可假红 [open] (medium)
- 优先级: P1
- 复杂度: 中
- 标签: 核心
- 证据等级: E2(读码发现,本轮未触发;可达路径已成立)
- refs: D-262 D-227 R-182 R-184 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 任务级并行实测,线 A(D-262)在读码时发现并主动上报,**本轮未触发**——如实标注,不冒充实测。
- 复现(尚未实际触发,但路径可达): `crates/kanzei-tools/src/background.rs` 用**进程级** `tokio::sync::Mutex` 串行化围栏敏感测试,而 `managed_fence` 的「合法写入窗口」本身也是**进程级**状态。这只在单个 `cargo test` 进程内有效。任务级并行的常态是多条线共享同一个 `CARGO_TARGET_DIR`(本机 target 已 53GB、盘剩 68GB,每树独立物理上放不下,见 R-182 与 deep_parallel_dev D6),两条线同时跑 `cargo test -p kanzei-tools` 时**两个 OS 进程的托管文件窗口可以交错**。
- 影响: ①**假绿**——越界写入落在另一个进程打开的合法窗口里,围栏测试认为"没越界"而通过;②**假红**——自己的合法写入被另一个进程的窗口边界切断,测试报越界。两种方向都让围栏测试在并行开发下**不可信**,而围栏正是 D-174 交付时唯一没被拆掉的那条保障。与 D-227 同族(单进程内成立的不变量,跨进程不成立),与 R-182 实测「跨 worktree 的 FileLock 各锁各的、根本不互斥」是同一类错误。
- 边界: 不是生产代码缺陷——`managed_fence` 的生产语义在单进程内是对的。本条只针对**测试在并行下的可信度**。修复不应把进程级窗口改成全局互斥而拖慢生产路径。
- 修复方向(待定): 二选一——①测试侧用跨进程互斥(`atomic_file::FileLock` 或按 crate 取一把文件锁)把围栏敏感测试整体串起来,与 D-261 给 `test_record` 的做法同源;②让围栏窗口带上进程身份(pid/run_id),跨进程的窗口互不认账,从根上消除交错。②更彻底但改动面进生产代码,需先评估。
- 验收: ①两个 OS 进程**同时**跑 `cargo test -p kanzei-tools` 的围栏用例,结果稳定且与单进程一致,有可重复的实测证据(不是"跑了几次没复现");②假绿方向有定向反证:构造跨进程窗口交错,确认修复前该越界写入**能**混过围栏、修复后被抓;③生产路径的 `managed_fence` 性能与语义不因本次修改而变,有测试背书。

## D-269 bash 权限可被历史授权提权:normalize_resource 非单射,在已批准命令的任一斜杠处插入 T/../ 即可带进任意 shell 语句 [open] (high)
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 证据等级: E1(**我在 dev HEAD 上独立复现**,非仅采信复核结论;见「实测」)
- refs: D-050 D-051 D-267 R-183 docs/design/tier1_implementation_plan.md
- 来源: 2026-08-11 第一梯队 F1 的对抗复核。复核本意是验 F1 新加的兼容垫片,**结果发现同一个洞在改动之前就存在**——F1 只是把它显式化并书面认证为"安全"。**本条与 F1 无关,是既有缺陷,已发布的 `build-ad80b2d` 里就是活的。**
- 根因: `normalize_resource`(`crates/kanzei-harness/src/permission.rs:180-215`)按 **D-050** 的设计做路径规范化——弹出 `..` 的前一段、折叠 `//` 与 `/./`、`\`→`/`、Windows 下整串小写。这些操作**故意是非单射的**(多个输入映到同一输出),对路径资源是正确的。
  问题在 `crates/kanzei-core/src/runner/drive.rs` **:545 / :604 / :764 三处**对**所有** action 的资源一律 `normalize_resource`,**bash 也不例外**。而 bash 的资源是 `{"command":...,"workdir":...}` 的 **shell 文本**,不是路径。于是:
  ①落盘的 pattern 是规范化后的串;②运行期的 value 也被规范化;③二者逐字节比较。
  **结果是一条规则准入的不是一个命令,而是 `normalize_resource` 的整个原像类。**
  垫片式修法(F1)把判据写成 `pattern == normalize_resource(value)`,并论证"确定性保证每个 V 只对应唯一 P"——**这个不变量写反了**:那是函数性,授权需要的是反方向的**单射性**(每个 P 只准入唯一 V),而 `normalize_resource` 恰恰被设计成非单射。
- 影响(提权,不是可用性): 在**任一条已批准命令**里含至少一个 `/` 时,把该 `/` 替换成 `T/../`(T 为任意不含 `/` 的串)即可注入任意 shell 语句——T 在规范化时被 `..` 整段弹掉,注入版与原版的规范化结果逐字节相等。用户配置里 21 条 bash 规则大多含 `/`。
  **D-051 的降级同时失效**:注入段里的 `*` 在 pattern 成形前就被抹掉,pattern 不含 `*`,`command_chaining_escapes` 不触发。
  命令文本确实原样执行:`crates/kanzei-tools/src/bash.rs:87` 把 `input["command"]` 逐字节放进 resource JSON,`execute` 用的是同一个 `input.command`,**中间无任何再校验**。
- 实测(2026-08-11,**我在 dev HEAD `b53b9aa` 上独立跑的**,scratch crate 依赖仓内真实 `kanzei-harness`,未改仓库任何文件): 
  已批准(取自 `.kanzei/kanzei.toml` 第 11 条真实规则):
  `git grep -n "cleanup_orphan_webviews" -- crates/kanzei-app/src/main.rs`
  注入版:
  `git grep -n "cleanup_orphan_webviews" -- crates/; Remove-Item -Recurse -Force $HOME ;/../kanzei-app/src/main.rs`
  输出:`两条命令不同 true` / `规范化后相等 true` / `evaluate(已批准命令) Allow` / `evaluate(注入命令) Allow`。
  复核方另在第 5/9/12 条规则与 `cargo --manifest-path` 上给出同形态提权链,并验证 F1 之后的新落盘形态(未 mangle 的原串,只要本身是规范化的不动点)**同样中招**——所以这不是只影响历史规则的一次性兼容窗口。
- 修复方向(待设计,勿直接照做): 根子是**对 bash 资源施加了路径语义**。正确方向是让 bash 资源**彻底不经过任何路径规范化**——`drive.rs` 三处按 action 分流,bash 走原样、其余仍走 `normalize_resource`(**只能改 bash 分支**:write/edit/read 少了 normalize 会让 D-050 的四条路径测试与 `write.rs` 的落点一致性测试同时红)。
  配套问题:既有落盘的 pattern 已经是规范化后的串,停止规范化后它们与原串失配。二选一——①加载时一次性迁移(反解或标记失效要求重新授权);②保留一个**只做逐字节相等**的兼容读取路径。**注意 ② 正是 F1 的形态,而它就是被本条否掉的那个**——若走 ②,必须证明它不引入原像类(F1 的论证是错的,不可复用)。
- 边界: 与 D-267(缺一个安全中间档)是**不同**的问题。D-267 是"偏严到没有可用中间档";本条是"偏松到历史授权可提权"。两条要分别修,不要合成一次改动。
- 验收: ①`drive.rs` 三处对 bash 资源不再调 `normalize_resource`(机械核验:该文件 bash 分支 grep 零命中);②**定向反证**:用本条实测里的那一对命令构造测试,断言注入版为 `Ask` 而非 `Allow`;再补 `cargo --manifest-path ./x/; evil ;/../y.toml` 一条同形态。③既有落盘规则的处置方案落地且有测试(迁移或兼容读取,二者都要证明不引入原像类)。④D-050 的四条路径规范化测试与 `write.rs` 落点一致性测试保持绿(证明只动了 bash 分支)。⑤D-051 的 `command_chaining_escapes` 在注入形态下重新生效,有测试。

## D-270 显式主根的 HOME 守卫仍有四处缺口:发现式取根仍纯词法、KANZEI_HOME 不参与比较、卷元数据读失败 fail-open、两条入口 trim 不一致 [open] (medium)
- 优先级: P1
- 复杂度: 中
- 标签: 核心
- 证据等级: E1(2026-08-11 对抗复核逐条实测,四条均给出可复现输入)
- refs: D-194 D-189 D-186 R-182 R-177
- 来源: 2026-08-11 批次 K1 交付「HOME 守卫改用文件系统身份比较」后的对抗复核。**主体已修好**(尾随点 `C:\Users\kanzei.`、UNC `\\localhost\C$\...`、junction、8.3 短名全部拦住,且合法项目根不误拦),本条是它**明确未覆盖**的四处残余,按份量排序。
- 缺口①(重要): **发现式取根仍是纯词法**。`discover_project_root_with_home` 用 `dir_key` 比较,没跟着升级成 `is_same_dir`。同一个物理目录换成别名走,`~/.kanzei` 立刻变回「项目根磁铁」。CLI 侧被第二道 `reject_home_as_project_root` 兜住(偏严、报错可见),桌面端路径需单独核实。
  K1 留它的理由(已记录,可作为修复时的输入):①派发单把范围限定在 `is_home_root`;②发现式取根的 cwd 来自 `current_dir()`,写不出尾随点/带 `..` 的串;③改它要为**每级祖先各做一次 canonicalize**,是每次配置加载 O(深度) 次系统调用。**所以修法不是简单替换**,要么只对最终命中的那一级做身份比较,要么加缓存。
- 缺口②(次要): **`same_dir_by_volume_metadata` 拿不到元数据时 fail-open**,与它自己的注释相反。注释声称「误判只会偏保守」,实际上 metadata 读失败、`modified()` 取不到、目标不是目录,任何一种都 `return false` = 判成不同目录 = **放行 UNC 别名**。方向必须反过来:拿不到身份就当作**可能相同**,由上层保守处置。
- 缺口③(次要): **`KANZEI_HOME` 一设,同一个碰撞就没人守了**。`is_home_root` 只跟 `dirs::home_dir()` 比,从不跟真正的全局根 `kanzei_home()` 比。把 `KANZEI_HOME` 指到某项目自己的 `.kanzei`,项目产物与全局配置/全局记忆重新落进同一个目录——**正是 D-194 声称要挡的那件事**——且零告警。反方向也不自洽:全局根已经搬走时,`--project-root` 指向真实 HOME 反而不再有危害,却仍被拦。
- 缺口④(次要): **两条入口对同一串输入给出的理由不一致**。`KANZEI_PROJECT_ROOT` 走 trim,`--project-root` 不 trim;带首尾空格的 HOME 经参数进来会被报成「路径不存在」而不是「你把主根写成 HOME 了」。两道拦截的先后顺序本来就是为了避免张冠李戴(`main.rs` 明确写了第一道打在显式输入上就是为了不被泛化报错盖过去),这里被空格破了。
- 边界: 主体(显式入口的身份比较)已交付且经实测,**本条不是回归**。符号链接形态因需管理员/开发者模式未能实测,但 canonicalize 解符号链接与解 junction 走同一条 reparse point 路径,junction 已覆盖。
- 验收: ①发现式取根对别名形态的 HOME 也拦得住,且**给出加载路径的性能实测**(不得让每次配置加载多做 O(深度) 次系统调用);②卷元数据读失败时方向改为保守(判成可能相同),有定向测试构造读失败;③`KANZEI_HOME` 参与比较,指到项目自己的 `.kanzei` 时被拦并告警,有测试;④两条入口对同一输入给出**同一条**理由,带首尾空格的 HOME 经两条入口都报「主根写成 HOME」,有测试。

## D-271 MemoryCoordinator::release_writer 在持锁临界区内 send 租约:接收端已丢弃时 lease 退回并当场 drop,回调二次锁同一把非重入 Mutex 死锁 [open] (high)
- 优先级: P0
- 复杂度: 小
- 标签: 核心
- 证据等级: E1(**我在 dev HEAD 上逐行核实代码形态**,非仅采信复核结论)
- refs: R-171 R-173 R-177 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 批次 K2' 交付时主动上报(它在 app 侧绕开了这个坑,但如实指出根因在 core 里没修)。**与本轮改动无关,是既有缺陷,已发布的 `build-ad80b2d` 里就是活的。**
- 根因(代码形态自证,`crates/kanzei-core/src/orchestration.rs` 的 `release_writer`): 交接分支里 `if let Some(tx) = w.tx { let _ = tx.send(Ok(lease)); }` 这一行在 `self.inner.projects.lock()` 的**临界区内**(锁块直到该行之后才闭合,`self.notify(pending)` 在块外)。
  `oneshot::Sender::send` 在**接收端已被丢弃**时返回 `Err(原值)`——把 `WriterLease` 原样退回。`let _ =` 当场 drop 它,而 `WriterLease` 的 Drop 回调正是 `move |released_run_id| coord.release_writer(&key, released_run_id)` → **二次进入 `release_writer` → 再锁同一把非重入 `std::sync::Mutex` → 死锁**。
- 可达性(今天就可达,不是理论): 任何**被丢弃/abort 的排队 acquire future** 都会造成"接收端已丢弃"。例如 `crates/kanzei-app/src/run.rs` 的 writer run 被停止按钮 abort 时,它排在队列里的 `w.tx` 接收端随之消失;下一个持有者释放租约、轮到唤醒它时就撞上。死锁发生在持有全局 `projects` 锁的线程上,**该项目的所有写仲裁自此永久挂死**(`acquire_writer_lease` / `release_writer` / `snapshot` 全阻塞),只能重启 kzapp。
- 影响: 项目级写仲裁整体失效且不可恢复。并行开发下暴露面被放大——排队者越多、abort 越频繁越容易撞上,而任务级并行的常态正是「多个 writer 排队 + 随时停某一条」。
- 修复方向: 把 `send` 与「send 失败后 lease 的处置」**移出临界区**。形态:锁内只把要唤醒的 `(tx, lease)` 收进局部变量(与 `pending` 事件同一手法,该函数已经在用),锁释放后再 `send`;send 失败时**显式处理**退回的 lease(此时不持锁,drop 回调可安全重入去唤醒下一个排队者),不得再用 `let _ =` 吞掉。
- 验收: ①构造「排队者的接收端已丢弃」(丢弃 acquire future 后由持有者释放租约),断言 `release_writer` **正常返回**且后续 `acquire_writer_lease` 仍能成功——该测试**在修复前必须挂死/超时**(反证);②send 失败时退回的 lease 被显式处置且**队列继续推进**(下一个排队者拿到租约),有测试;③`projects` 锁的临界区内**不再有任何可能触发 `WriterLease::drop` 的语句**(机械核验:锁块内 grep 无 `send(`);④R-171/R-173 既有写租约测试全绿。
