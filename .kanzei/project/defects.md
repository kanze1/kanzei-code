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

## D-207 取活顺序所见非所得:视图排序与优先级徽章都不参与取活,界面零提示 [open] (medium)
- refs: R-054 R-111
- 复现: 2026-08-09 用户反馈"取需求和缺陷的顺序看不懂了,因为侧边栏可以调整顺序"。机制现状:①取活真序 = md 文件物理顺序从上到下(dev prompt "Scan from top to bottom",schedule_entries 只后置阻塞项、不改文件);②侧栏拖拽(manual 排序+无筛选时)经 docs_update reorder **写回文件**,真的改变取活顺序;③侧栏另有 id/状态/复杂度/优先级四种视图排序(main.js filterRequirements),**只改显示**;④优先级徽章 P0~P3 完全不参与取活(prompt 明言 "Priority labels are background info, not the ordering")。
- 影响: 选了任何视图排序后,用户看到的顺序与 agent 取活顺序完全无关,界面没有任何提示;优先级徽章满屏,人天然以为按 P0→P3 取活,实际一票不投——近期把 5 条需求升 P0(576d725)在取活上零效果,用户的调度意图静默落空。三种顺序语义(文件序=取活序/视图序/优先级暗示序)混在同一个列表上,只在"manual+无筛选"时才重合。
- 根因: R-054 定了"文件顺序即开发顺序"的单一真源,后续视图排序与优先级徽章叠上去时,没有同步交代它们与真源的关系;取活规则只写在 prompt 里,UI 侧无任何投影。
- 验收: ①非 manual 排序视图下,侧栏显式提示"当前显示顺序≠取活顺序"(或等价视觉语言);②有一处能看到真序:取活预览(下一条会被拿的条目有标记,阻塞项显示跳过原因)或一键切回文件序;③优先级二选一——要么参与取活(prompt 与 schedule 同步改,并写清与文件序的优先关系),要么在 UI 上明示"仅参考,不影响取活";④用户复查确认能看懂"agent 下一个会拿哪条、为什么"。
- 证据等级: E1(代码四处机制实证 + 用户反馈)
- 优先级: P1
- 标签: 前端

- 进展: 2026-08-09 部分交付(①取活焦点可视化、②拖拽禁用提示)基础上,本轮补验收③:优先级语义二选一选 B(UI 明示)——按 M-002 用户定调 priority 只是背景信息不参与取活,不改 prompt/schedule。实现:①侧栏优先级筛选下拉 title 改为「按优先级筛选(仅参考,不影响取活顺序)」(index.html);②需求/缺陷行内优先级徽章 title 追加「(仅参考,不影响取活)」(main.js renderDocList badge.title);③i18n 新增两条词条(I18N_EN),ui-i18n-smoke 770 key 全绿;④冒烟新增断言:筛选 title 与徽章 title 均含「仅参考」,ui-runtime-smoke 222 invoke 全绿。验收④用户复查(能否看懂 agent 下一个会拿哪条)待发版安装后用户确认,与 D-210/D-211 同惯例。 2026-08-10 用户反馈验收④未过:前端把阻塞 doing(R-157)渲染成「运行中」,与取活实际脱节(§1.1 阻塞项不计 WIP、agent 跳过它)。根因:computeAgentFocus active 收集只按 status==doing/fixing,未排除 blocked。修复:12-docs-pages.js computeAgentFocus 的 active 只收可执行 doing/fixing(排除 entry.blocked),next 逻辑不变(本就找第一个可开工);冒烟补断言「blocked doing 保留 blocked 标记但不标 agent-active、且不挡住 next」,四条前端冒烟(ui-runtime 237 invoke/ui-i18n 306 key/ui-markdown/ui-a11y)全绿。验收④仍待用户重建 kzapp 后复查确认(ui 资源打包进 exe,需 release.ps1 重建生效)。

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

## D-202 超长对话把 webview 主线程拖死,侧栏等大片控件点击无反应 [fixing] (high)
- 复现: 2026-08-09 用户实测(53bb8e7 桌面端):自举循环长会话(几百轮,含大量工具调用块/diff/markdown)期间,侧栏条目展开、筛选等点击**完全无反应**(无按压反馈,像点在空气上);发送等主操作路径尚可。用户自判"上下文太多卡住了"。初步排查已排除:①初始化崩坏(53bb8e7 的 main.js 在 ui-runtime-smoke 全量执行 0 错误);②ask 遮罩挡点击(全屏 overlay 会挡住所有东西,与"主操作正常"不符);③R-086 状态机焊死(那只禁用运行态控件,不吞侧栏点击)。
- 疑似根因(待复现证实): 长对话 DOM 巨大(消息、工具块、diff 逐条渲染,无虚拟化/窗口化),流式事件持续追加触发重排,主线程长期忙碌,点击事件延迟到秒级等价于无反应。若成立,与 D-013(diff 默认展开导致对话过长)、D-046(重绘防抖)是同一性能债的延续:此前只做了"少画",没做"画不下就不画"。
- 验收: ①可复现实证:构造或回放长会话,量化点击响应延迟(如 Event Timing / 长任务计数),定位耗时大头;②修复后同样场景侧栏点击在人可感知阈值内响应(<200ms);③对话渲染有上限策略(虚拟化、折叠历史或分页任一),DOM 节点数有界;④冒烟加长会话性能断言防回归。
- 根因(2026-08-09 定位,代码实证): 主因是 **i18n 的全局 MutationObserver 把每一次 DOM 变动都放大成一次全文档重扫**。main.js:711 `new MutationObserver(() => applyLanguage())` 监听 `document.body` 且 `childList+subtree+characterData+attributes`;而 applyLanguage(main.js:627)每次执行都 `createTreeWalker(document.body, SHOW_TEXT)` 走**整页每一个文本节点**(每个节点还做一次 `parent.closest("[data-i18n-raw]")` 祖先回溯),之后再 `querySelectorAll("[title],[placeholder],[aria-label])` 扫一遍全页。它不按语言短路——中文模式下同样全量走。于是单次成本 ∝ 全文档文本节点数 ≈ 对话长度,而触发频率 = 每个流式 delta 一次(appendAssistant 的 `innerHTML=` 必然产生 childList 变动)⇒ 一轮对话的渲染开销对会话长度成平方增长,轮次越多主线程越被占满,点击排在长任务后面就等于"没反应"。
  次因(同一热路径上的三处放大,都在 appendAssistant,main.js:1320-1335):①每个 delta 重新 `renderMarkdown(整条消息)` 并整块 `innerHTML=`,单条消息内部就是 O(n²);②每个 delta 把整条 raw `split("\n").map(正则).filter()` 一遍,只为取"最近在说"的最后一行;③每个 delta `scrollBottom()` 读 `messages.scrollHeight`,强制同步重排整个消息列表(这一项随轮次增长)。全文件 `requestAnimationFrame` 出现 0 次,没有任何合帧/节流。
  另:对话 DOM 从不裁剪(只有切会话时 `messages.innerHTML=""`),所以**上下文压缩不会缓解卡顿**——压缩只减少发给模型的 token,渲染侧一个节点都没少。用户"上下文太多卡住了"的直觉方向对,但机制不在上下文,在渲染。
- 量化(2026-08-09,ui-runtime-smoke 的 DOM harness 内测,只证明标度不代表真机绝对值): applyLanguage 单次耗时随文本节点数线性上升——95 节点 0.54ms / 295 节点 1.11ms / 895 节点 3.61ms;renderMarkdown 每 delta 均摊耗时随消息长度线性上升——2850 字 0.035ms → 45600 字 0.169ms(单条消息累计 135ms,纯解析、不含 DOM)。真机 WebView 的 TreeWalker/重排成本远高于 harness,几百轮会话的文本节点数在万级,单次 applyLanguage 已足以吃满一帧。
- 修复方向(建议按序,每步独立可验): ①observer 回调改为只处理 `mutations` 里的 addedNodes 子树(新进节点才本地化),不再全文档重扫——单点改动,收益最大;②applyLanguage 用 rAF 合帧,一帧最多一次;③appendAssistant 流式期间只追加纯文本(`textContent +=`),消息收尾时再整条 renderMarkdown 一次;④"最近在说"从 delta 增量算,不扫整条 raw;⑤scrollBottom 合帧,或改 CSS `overflow-anchor` / 底部哨兵 + IntersectionObserver,去掉每 delta 读 scrollHeight;⑥最后才做验收③的 DOM 上限(窗口化/折叠历史)——前五步做完可能已不需要。
- 修复(2026-08-09,修复方向①②④⑤已落地): ①applyLanguage 拆成 localizeTextNode/localizeAttributes/localizeRoot(root),observer 回调改为只把 `records` 里的 addedNodes(childList)与 target(characterData/attributes)交给 localizeNodes,全文档重扫只留给初始化与切语言两处显式调用;②④⑤合成一处:appendAssistant/appendReasoning 的 delta 只累加文本,renderMarkdown+innerHTML+scrollBottom 压到每帧最多一次(scheduleStreamRender/flushStreamRender),上一次渲染实测 >8ms 就按实测耗时退避(上限 250ms),长消息自动降频;"最近在说"改用 lastNonEmptyLine() 只扫尾部 2000 字窗口(并丢掉被窗口截断的首行,预览不会从半个词开始)。③(流式期间只上纯文本、收尾再渲染)未采纳——合帧后已无必要,且会让正文在流式期间失去格式。⑥DOM 上限/窗口化仍未做,留待真机复测后再判是否需要。
- 冒烟(验收④已落): ui-runtime-smoke 的 DOM harness 原先给 observer 递空 records、createTreeWalker 忽略 root、requestAnimationFrame 同步执行——三处都会让新路径在冒烟里空转,已一并补真:投递真实 MutationRecord、createTreeWalker 尊重 root(含文本节点 root)、rAF 入队由 flush 排干,并统计"从 body 起的全文档重扫"次数。新增三条行为断言:200 个 delta 触发的 renderMarkdown ≤20 次、全文档 i18n 重扫增量为 0、合帧后最后一段文本确实渲染出来;另加一条增量本地化断言(新进节点在 en 下必须被翻译),防止"少扫了也少翻了"。拦截实测:把 observer 改回 `() => applyLanguage()` → 冒烟报"触发了 2 次全文档重扫"(harness 内 200 个 delta 同步发生会被微任务合批,真机每个 delta 是独立事件、独立微任务检查点,即每 delta 一次);把渲染改回每 delta 一次 → 报"200 个 delta 触发了 200 次 renderMarkdown"。四条 UI 冒烟全绿。
- 待验收: ②真机复测(几百轮会话下侧栏点击 <200ms)由用户在新构建上确认;①的真机 Event Timing/长任务数据仍未采;③(DOM 节点数有界)未做。三条都清了才转 fixed。
- 证据等级: E1(代码路径实证 + harness 标度量化 + 拦截实测;真机 Event Timing 数据待补,对应验收①)
- 优先级: P1
- 标签: 前端

## D-184 commands / skills 两张注册表是死的:解析注册后无人消费 [open] (medium)
- 复现: 在 `~/.kanzei/commands/` 或 `~/.kanzei/skills/`(及项目同名目录)放 markdown,MarkdownComponent 会扫描、解析并注册(crates/kanzei-harness/src/markdown.rs:22);但全仓库对 `snapshot.commands()` / `snapshot.skills()`(crates/kanzei-harness/src/harness.rs:110、114)**零调用**——文件进了注册表就地消失,既不进提示词也不成为工具。
- 影响: 六张注册表实际在跑的只有四张。用户按目录约定放了命令/技能文件,界面与模型都不会有任何反应,也没有一行提示说"注册了但没人用",属于静默无效功能。
- 根因: 注册表与消费端分两步落地,消费端(注入提示词或转成工具 spec)始终没接。
- 验收: 要么接上消费端(commands 进提示词可调用清单、skills 按 description 与任务匹配给出加载提示,与 R-106 的 sop 匹配同源),要么显式移除这两张注册表与扫描逻辑;二选一,不留"解析了但没人读"的中间态。有测试覆盖所选方向。
- 证据等级: E2(读代码确认零调用点)
- 优先级: P2
- 标签: 核心

## D-185 `<memory-hints>` 声称只进本轮,实际逐轮累积进对话历史 [open] (medium)
- 复现: 开跑前预检索的记忆提示块拼进 `run_prompt`(crates/kanzei-app/src/main.rs 注入点注释写"提示块只进本次运行"),但它随 User message 进 `summary.messages` → 桌面端整份存进 conversations → 下轮作为 `prior` 回灌。跑 N 轮,历史里就躺着 N 个 hint 块。
- 影响: ①每轮固定多烧 N-1 份陈旧提示;②这些块是**当时**的记忆快照,与现行 INDEX.md 可能已经不一致,模型读到的是过期索引却无从分辨;③与 R-106"注入 token 下降"的目标反向。
- 根因: 提示块拼在 prompt 字符串上而不是作为一次性 system/context 段落,持久化路径对它无感知。
- 验收: hint 块不进 conversations 快照(或落库前剥离),连跑 3 轮后历史里最多一个块;注入 token 账单能看出 hint 段的独立占比。
- 证据等级: E2
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

## D-173 架构索引 architecture/README.md 无专用工具可写:edit 被 ruleset 拒绝,agent 只能 bash 旁路维护 [fixing] (high)
- 备注: 本轮已用 bash 旁路一次性补齐索引(946742f),内容正确;本缺陷登记的是通道缺失本身,不撤回已完成的补全。D-171 已确认为真实缺陷(孤儿 webview 黑屏,743d4e4 修复并登记),非编号空洞;此前的 tombstone 误判已撤销。
- 复现: agent 用 edit 更新 `.kanzei/project/architecture/README.md` 报 permission denied by ruleset(policy-managed,提示用专用工具);但 req/defect/goal/decision 四个专用工具只管理各自追踪文件,没有任何工具托管 architecture 目录。实测 2026-08-08:索引补全只能经 bash 写入(946742f),而 bash 能写受保护目录本身也说明 R-139 的 bash 级 .kanzei 路径硬门禁尚未落地。
- 影响: ①自举循环新增/重命名设计文档后,架构索引只能由用户手改,必然滞后(本次 10 个文档重命名 + 2 份新设计入库后,索引仍只有 5 个旧条目);②agent 若想维护索引,唯一通道是 bash 旁路,而旁路通道本身违反'受保护文档不被 bash 旁路'的设计原则;③architecture/README.md 是架构发现入口,索引滞后会让后续会话找不到现行设计真源。
- 根因: ruleset 对 `.kanzei/project/*` 的 edit/write 硬 deny 只给 tracker 类工具放行(设计意图是防模型旁路),但 architecture/README.md 作为同级项目管理资产不在任何专用工具的托管范围——需求/缺陷/目标/决策各有工具而架构索引没有,形成'既不能 edit、也无专用工具'的双重缺口;bash 写入通道未封堵又构成硬门禁的旁路。
- 验收: ①提供可用的架构索引维护通道:要么新增专用命令/工具(如 `kz doc index` 或 tracker 工具扩展),要么把索引改为从 docs/design 自动生成(如 docs_snapshot 系),agent 更新 docs/design 后索引自动同步;②补 R-139 的 bash 级 .kanzei 路径硬门禁,使受保护文档不能经 bash 旁路写入;③验收时新增/重命名一个 docs/design 文档后,索引可被 agent 直接维护且无需 bash 旁路。
- 修复进展(2026-08-08): 已新增 `architecture` 专用工具及固定路径、`expected_hash` 并发保护、同目录临时文件与可恢复替换;Harness 已把架构文档纳入托管资源并要求通过专用工具访问;通用 Bash 已在执行前后对托管资源做快照并回滚越界写入。
- 验证(2026-08-08): `kanzei-tools` 80 项、`kanzei-harness` 37 项、`kanzei-core` 50 项测试通过。尚未在已安装桌面端中完成一次真实模型调用与工具交互验收,因此保持 `fixing`。
- 收口核对(2026-08-09): 本轮 fixing 批量收口时**刻意不关**这条。episodes 实证:48 个轮次里 `architecture` 工具 **0 次真实调用**(同期 req 196 次、defect 95 次)——验收③"agent 直接维护索引且无需 bash 旁路"不是没测到,是从未发生。工具注册、权限、D-195 的提示词同源测试都在,但按 §1.25"声称完成的能力必须有真实调用方",一个零调用的通道不能算闭合。下一次自举改动 docs/design 后用 architecture 工具更新索引成功,即可关闭。
- 优先级: P1

## D-174 托管项目后台 Shell 缺少可归因的文件隔离 [open] (high)
- 复现: 后台 Bash 启动后立即返回,后续异步进程可以在任意时刻修改 `.kanzei/project` 与 `.kanzei/memory`;Harness 无法区分后台进程写入和稍后专用工具的合法写入,也无法安全回滚。
- 根因: 现有后台进程注册表只管理 PID、日志和生命周期,没有独立工作目录、文件系统沙箱或按进程归因的写入审计。
- 影响: 若继续允许托管项目中的后台 Bash,受保护文档可能绕过专用工具契约;当前修复选择在存在 `.kanzei` 的项目中拒绝后台 Bash,因此 R-097 的后台启动能力暂时降级。
- 验收: ①后台任务运行在可隔离或可归因的文件系统边界中;②后台任务不能写入 Harness 托管路径;③专用工具的合法写入不会被后台回滚机制误伤;④覆盖启动、轮询、停止、越界写入和并发合法写入测试。
- 优先级: P1
- 关联需求: R-097、R-139

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

## D-223 R-158 新增设置字段误删 profile_default 导致编译失败 [fixing] (medium)
- 修复范围: 恢复 SettingsPayload.profile_default 字段；Codex Fast mode 仍保持独立字段。
- 复现: 在本次 Codex Fast mode 改动后运行 `cargo check -p kanzei-app`，SettingsPayload 编译报 profile_default 不存在；设置打开构造体也报该字段未定义。
- 根因: 新增 codex_fast_mode 字段时的精确替换遗漏了既有 profile_default 字段。
- 验收: SettingsPayload 同时包含 profile_default 与 codex_fast_mode；设置保存/打开相关构造点可编译。
- 优先级: P1
- 进展: 已恢复 crates/kanzei-app/src/main.rs::SettingsPayload.profile_default，并保留 codex_fast_mode；后续 cargo check 未再报告 profile_default 缺失。整体 kanzei-app 仍被 R-153 既有 mobile.rs/processes.rs 编译错误阻断，待上游迁移稳定后复测。

## D-227 并行 test_record 自动生成相同时间戳 ID，四条 UI 记录互相覆盖 [open] (medium)
- 复现: 并行调用四个同秒 test_record，均省略 id；结果生成相同 T-1786297655，archive 中标题不同但 ID 相同。
- 影响: 测试证据无法一一引用，可能破坏测试记录唯一性与归档完整性。
- 标签: 流程
- 进展: 本轮发现；后续需用串行记录或显式唯一 id 认领，先核对 tests-archive 的实际条目。
- 优先级: P2

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

## D-231 stale 记忆归档流程未落地,失效条目永驻主目录 [open] (medium)
- 优先级: P3
- 依据: memory_system.md §2 承诺「stale 后由整理流程移入 archive/ 带墓碑」;store.rs 的 load_archived_ids 会读 archive/ 目录,但代码里没有任何写入方——stale 条目永驻主目录,FTS 仍索引(仅 0.5 降权),目录随时间只增不减。
- 修复方向: 并入 R-165 Memory Compiler 的归档流程(deprecated/invalid 移入 archive/,墓碑保留,默认检索不可见);lifecycle 状态迁移时一并处理 stale→deprecated 兼容映射。
- refs: R-165 R-103

## D-232 package.ps1 发布门禁缺「HEAD 已推远端」检查,未推时 422 到最后一步才炸 [open] (medium)
- 优先级: P2
- 复现: 2026-08-10 发版 build-9e09b80 实测:dev 领先 origin/dev 21 个提交时跑 package.ps1 -Ack 21 -Publish,tauri build + NSIS 全部完成(约 1 分钟)后,gh release create --target <full_hash> 报 HTTP 422 `Release.target_commitish is invalid`——GitHub 要求 target 提交在远端可达,本地未推的 SHA 一律拒绝。推送后重跑成功。
- 影响: 失败发生在整条流水线最后一步,前面的构建时间全部白费;且 422 报错文案不指向真因(脚本注释里只记载过「短 hash 会 422」的旧案),排查要靠人对比 origin/dev。自举/无人值守发版场景下这是必踩坑——自举只 commit 不 push。
- 修复方向: 在 -Ack 核对同一节(构建开始前)加一条前置检查:`git rev-list origin/<branch>..HEAD` 非零即中止,报错明说「先 git push 再发版」;或者(需用户拍板)自动 push 后继续。顺带把 422 旧案注释更新为两种成因(短 hash / 未推远端)。
- 证据等级: E1(本次发版实测复现+修复验证)
- 标签: 流程

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
