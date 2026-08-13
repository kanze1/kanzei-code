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
- 阻塞: 用户: 2026-08-13 用户明确指示「先做 R-200 再发版,不是做这条」,本条暂缓。解除动作: 用户说恢复推进时按队列取活(R-200 及其后续完成后再开)。解除人: 用户。
- observed_head: d124749aabe65ec0cde4f2280c9583dd4f33be40
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786609593506

## R-183 kz 无人值守执行通道:非交互直接放行 bash + 可审计轨迹(原「预授权集」随 D-267 作废) [doing]
- **2026-08-11 改写(用户定调,随 D-267 关闭为 dropped)**: 原标题里的「permission 规则 worktree 继承主根、可审计预授权集」两项**作废**——它们服务的是 D-267 的中间档,而中间档已被砍掉(理由见 D-267 关闭说明:挡不住有意的、被绕过两次、威胁模型里没有「模型是敌人」)。**本条大幅缩小**:非交互模式下 bash 直接放行,防线整体挪到结果侧(R-186)。
  下方原「内容」「验收」保留作为历史,**实施以本节为准**。
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(2026-08-11 实测三次全失败 + 读码定位)
- refs: R-182 R-177 R-030 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 搭任务级并行实测时,**`kz run` 在 worktree 里无法无人值守跑**,是当天唯一让实验彻底停摆的硬卡点(另一个候选载体 `claude -p` 因 OAuth token 被吊销同样不可用)。任务级并行的前提是「N 条线各自跑到底」,没有这个通道就只能靠外部 CLI。
- 现状与缺口(逐点读码核实): 
  ①**EOF 落 Deny**:`crates/kanzei/src/main.rs:394-416` 的权限分支读 `std::io::stdin().read_line()`,后台运行时 stdin 是 EOF → 空行 → 落 `_ => AskReply::Deny`。不挂死,但**每一次写和每一条 bash 都被拒**,agent 寸步难行。
  ②**permission 规则的 workdir 钉死主根**:`.kanzei/kanzei.toml` 的 24 条 `[[permissions.rules]]` 里,后半段规则的 resource 是 JSON,内含 `"workdir":"c:/users/kanzei/documents/kanzei code"`。从 worktree 跑时 workdir 是 worktree 路径,**这些规则一条都匹配不上**——线启动时等于空白允许清单。
  ③**`cargo` 根本不在允许清单**:全部 24 条规则里没有任何 `cargo *`,而 Rust 任务的验证全靠它。
- 内容: ①非交互检测 + 显式策略:无 TTY 时不再落 Deny,改为按配置的**非交互默认策略**(建议三态:`deny`(现状,保守) / `rules-only`(只认预授权规则,规则外拒) / `allow-listed`(规则 + 本次运行的显式 allowlist));策略必须显式配置,**不提供"全放行"的隐式默认**。②permission 规则的 worktree 继承:worktree 内运行时,规则匹配按**主根**而非 cwd 解析 workdir(与 R-182 的主根重定向同一条原则),避免线一启动就没有任何授权。③可审计:非交互模式下每一次自动放行都落轨迹(动作、资源、命中的规则、时刻),`kz` 退出时给出汇总;拒绝同样可见(D-004 口径)。④补齐开发所需的基础规则模板(cargo/node/git 的只读与构建子集),放进新建配置的注释模板(与 R-172 同族)。
- 边界: 不做「全部自动同意」的开关——那等于把权限系统关掉,与仓库既有的硬 deny 纪律冲突。不改 profile/agent 体系。不做桌面端的无人值守(桌面端有 UI 可问,不是同一个问题)。
- 验收: ①`kz run` 在 worktree 里后台运行(stdin 关闭)能完成一次真实的「改代码 → `cargo test` → 提交」闭环,不因权限被拒而中断;②非交互默认策略三态各有测试,**缺省仍是 `deny`**(不改变现有用户的行为,旧配置无该键时行为不变);③从 worktree 运行时,主根的 permission 规则能命中(有测试直接断言同一条规则在主根与 worktree 下匹配结果一致);④每次自动放行有可查轨迹,含命中的规则原文;⑤无 TTY 检测本身有测试(不是靠"读到 EOF"倒推)。
- 依赖: 
- 取活依据: 
- 进展: 2026-08-13 让位(用户 park):D-332(两份运行评估合并的治理三硬伤)按用户指令排第一并优先解决,本条暂停。本条此前为 engine 自动认领(doing)但从未开工(无进展锚点)。
- 阻塞: 用户: 2026-08-13 用户两份运行评估明确指令「把这个分析登记成最新的缺陷,然后把这个缺陷排序成当前的第一个任务,解决并发版」——D-332 优先,本条让位。解除动作: D-332 关闭后按队列恢复本条。解除人: 用户。
- observed_head: d7236ada9b95c92e8e232aaeaaf4acf38796c323
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786611671592

## R-227 占位符测试 ID 提交门禁:tracker diff 出现 T-…xxx 即拒,存量 8 处回填或标注不可考 [doing]
- 内容: commit 门禁扫描 tracker 文件 diff,出现 T-\d+xxx 形态的占位符测试 ID 即拒绝提交;配套要求 test_record 落盘后与引用它的证据同批入库,消灭隔时凭记忆写证据;存量 8 处(requirements-archive 2 处、defects-archive 6 处)回填真值或标注不可考
- 复杂度: 小
- 来源: 2026-08-13 自举复盘(R-198/R-199 关闭证据均含占位符,复发模式)
- 标签: 流程
- 验收: ①门禁单测覆盖占位符拒绝;②存量 8 处处置完毕;③新增关闭证据无占位符
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-227
- 进展: 2026-08-16 取活。B1 完成(commit 635db58):①commit 门禁 placeholder_id_gate(git.rs)——tracker 文件 diff 出现 `T-<数字>xxx` 占位符即拒,只扫 tracker 路径,真实 10 位 ID 放行;②归档回填通道 fill_archived_placeholder(docstore.rs,与 dedupe 同锁同写路径,恰好命中一次,歧义拒绝)+ tracker 动作 archive_fill + CLI 分支(kz req archive_fill <id> <old> <new>);③存量 8 处占位符已全部定位并查明真实 ID:requirements-archive R-198 T-1786565xxx→T-1786565346、R-199 T-1786566xxx→T-1786565831;defects-archive D-219 T-1786451xxx→T-1786451434、D-266 T-1786560xxx→T-1786560588、D-279 T-1786562xxx→T-1786562463、D-281 T-1786562xxx→T-1786562856、D-282 T-1786563xxx→T-1786563655、D-316 T-1786563xxx→T-1786564679(真值均核自 tests-archive 对应记录)。验证:kanzei-tools 332 passed + kanzei 4 passed(T-1786631611)+ fmt/clippy 全过。验收①门禁单测覆盖占位符拒绝——placeholder_id_gate_拒绝占位符_放行真实_id与非tracker 测试通过;验收③新增关闭证据无占位符——门禁在 commit 层拦截,本提交无占位符。验收②存量 8 处处置:需用 archive_fill 动作回填,但当前引擎进程运行旧代码(archive_fill 报 unknown action,valid 列表不含它),归档文件又被 ruleset+managed-files 双重保护无法直改——处置待引擎重启加载新代码后执行 archive_fill。
- 阻塞: 待引擎重启加载 archive_fill 动作后执行存量 8 处回填(解除人:引擎下次启动;回填映射见进展)
- observed_head: 635db584872ab3d177751206a72fae384c33f102
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786631684828

## R-218 SubagentBase 只读工具面扩容:files 与 git 只读子命令入列,勘察角色能查 git 历史 [todo]
- 优先级: P2
- 复杂度: 小
- 标签: 后端 harness 并行
- 来源: 2026-08-12 八维度审计(§6)。
- 背景: task 子代理只有 read/glob/grep(tools/subagent.rs:14-25),task_spec 自述 cannot inspect git state;R-173 编排的勘察/复核角色走同一快照——查不了 git 历史、看不了文件地图,勘察质量有硬上限。
- 内容: SubagentBase 加入 files、git(限 status/diff/log 只读子命令),保持全 allow 零 ask;webfetch 暂不加。
- 验收: ①勘察角色能独立完成一个需要 git log 的勘察任务;②写类 git 子命令在子代理内被拒(定向测试);③既有只读语义测试全绿。
- refs: R-173 R-174

## R-234 代码符号/结构级视图工具:依赖关系、调用链、函数列表,填补 files 行数与 read 全文之间的粒度空白 [todo]
- 优先级: P1
- 复杂度: 大
- 标签: 核心
- 背景: 评估代码质量时粒度停在文件级+文本匹配级:files 给行数、grep 给正则命中、read 给逐行文本。中间缺符号/结构级视图(依赖关系、调用链、函数列表),导致质量评估只能「读全文(重)」或「靠行数猜(浮)」,没有中间档。本轮评估 harness 质量时暴露:靠 files 行数+测试数量下结论,未读一行代码。
- 验收: ① 对指定文件/crate 输出符号列表(函数/结构/impl);② 输出调用链或依赖关系(谁调用谁/依赖哪些 crate);③ 不必 read 全文即可定位质量热点(如 config.rs 2851 行的内部结构);④ 有真实调用方(agent 在评估/重构类任务中实际使用),不昺昺死在死代码。

## R-221 research 模式重定位:按 docs/design/research_mode.md 分批实施「先计划后自举」勘察载体 [todo]
- 优先级: P2
- 复杂度: 大
- 标签: 后端 前端 harness
- 来源: 2026-08-12 八维度审计维度8;设计文档 docs/design/research_mode.md(§2 八个定调点待用户逐项确认后动工)。
- 背景: research 模式骨架完整但形态错位(面向网络调研)且零使用(state.db 266 条 episodes 零调用 websearch/source/finding,.kanzei/research 全 git 历史只有空模板);真实勘察全在 dev 完成且结论无固定落点(勘察报告被 D-294 单行不变式折成单行塞进度字段);证据等级 E0-E4 被双重语义挪用;research/memory.md 是绕开记忆控制平面的第二套无校验记忆。
- 内容: 按设计文档六批实施:①档位收口(桌面注册 ReadonlyProfile、bash 硬 deny+替代指引、files/git 只读入列)②topic 工件落点(.kanzei/research/<topic>/)③勘察证据等级 V 表进 conventions④回流通道(backlog 只读索引注入+finding→req/defect 草稿)⑤记忆一元化⑥三形态收敛(SCOUT_ROLES/task 勘察落同一工件)。
- 边界: research 不可写 docs/design、不可提交 git、不动既有条目状态(add 草稿除外);不做报告 schema 校验。
- 验收: 以设计文档 §7 总则为准——一条真实 R- 条目的 勘察→报告→登记→dev 实施 完整链路有轨迹;每批验收见设计文档 §6。
- refs: D-276 R-201 D-304

## R-217 dev 档联网能力:websearch 注册进 dev(默认 ask),webfetch/websearch 支持域名级白名单规则 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 harness 权限
- 来源: 2026-08-12 八维度审计(§6)。
- 背景: websearch 只注册给 research(profiles.rs:552-554),webfetch 默认 Ask(base.rs:53)而 NonInteractive 下 Ask 即拒(drive.rs:876-881)——dev+autonomous 组合下模型没有一条合法联网路径,查 crate 文档、搜报错答案都做不到。
- 内容: dev 档注册 websearch(默认 ask,交互轮可放行);为 webfetch/websearch 提供域名级白名单资源形态(如 resource="docs.rs/*" allow)使自主轮可精确授权。
- 边界: 不改 Ask 在 NonInteractive 下等于 Deny 的语义(那是 R-183 的事);默认不放行任何域名。
- 验收: ①交互轮 dev 可搜索;②自主轮按域名白名单放行 webfetch 有定向测试;③白名单外域名仍走 Ask。
- refs: R-183 R-198

## R-219 context_limit 未知的 provider 启用保守压缩预算,overflow 恢复计数随成功衰减 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 harness
- 来源: 2026-08-12 八维度审计(§6)。
- 背景: known_context_limit 白名单外返回 None(config.rs:326-343),drive.rs:210 只在 Some 时做轮内预算——未知 provider 全程无主动压缩;被动恢复整个 run 只有 2 次(mod.rs:88,compaction.rs:124-141 只增不减)且一刀砍到 4000 字符,第 3 次 overflow 直接终止。
- 内容: context_limit 未知时按保守默认(如 32k)启用主动压缩并在启动告警点名「该 provider 无上下文基准」;恢复计数在成功恢复且随后 N 步无 overflow 后衰减。
- 验收: ①未知 provider 长跑不再第三次 overflow 直接终止(集成测试);②已知 provider 行为不变;③启动告警可见。
- refs: D-288

## R-215 inbox 消化协议改逐条销账:快照-消化-按条删除,并堵并发 append 与 next_id 竞态 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 记忆 并行
- 来源: 2026-08-12 八维度审计(§5)。「结构性死锁」定性经反证驳回(steps 是模型轮次而非工具调用数,通道能推进),但现存 13 条滞留 ≥1 天、并发竞态窗口真实存在。
- 背景: manager 消化是「整箱进 prompt+末尾整箱清空」(manager.rs:420-426),清空窗口内其他自举进程 append 的 note 被无痕清除;append_note 是读全文-拼接-原子写回(store.rs:1122-1162),并发追加后写覆盖先写;next_id 扫描-分配可撞号。
- 内容: 消化只删自己见过的 note(按指纹销账,discard_note 已有现成实现),新增的留箱;或按 note 一文件分片使追加天然无竞争;next_id 加同目录文件锁或冲突重试。
- 验收: ①构造 20 条积压能在数轮内收敛到 0;②并发 append+consolidate 压测零丢 note;③「消化清空吃掉新 note」窗口有定向测试封死。
- refs: R-195 D-282 D-299

## R-216 记忆写入侧质量三闸:近似去重下沉 store.add 双 scope、[fp:] 指纹一致性校验、tracker 交付状态内容拒收 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 记忆
- 来源: 2026-08-12 八维度审计(§5)。M-055/M-056 于近似去重上线当天英文复述 M-044 并携带编造指纹——「假指纹立即污染注入」经反证驳回(FingerprintIndex 只收 active 且不扫标题),但穿透与伪造本身实证成立;另有 6 条交付状态类内容落进记忆与 tracker 重复。
- 内容: ①classify_novelty 的 FTS 语义探测下沉进 store.add 作为硬闸(Uncertain 即拒并返回候选),查重范围扩到双 scope;②新条目携带的 [fp:] 必须与来源 note 中引擎生成的指纹逐字一致,拒绝自造;③标题/subject 命中「R-/D- 编号+已交付/勿重复/验收边界」形态时拒绝并指路 tracker(或强制挂 refs 并随条目关闭自动 deprecate)。
- 验收: ①复刻「英文改写 M-044」场景被拦并指路 memory_update(单测);②伪造指纹的 add 被拒;③存量 6 条交付状态记忆逐条处置;④各拦截路径有单测。
- refs: R-194 R-195 R-196 D-299 D-282

## R-214 记忆漏斗遥测口径修正:AVAILABLE 按 active 计、miss 落库、policy_action 记真实层级、memory_recalls 按承诺停写 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 记忆
- 来源: 2026-08-12 八维度审计(§5)。
- 背景: telemetry.rs:136-141 注释写「available 为 active 记忆数」但 SQL 数的是 memory_sources 行;ACTION_CHANGED/OUTCOME_IMPROVED 两段无任何生产写入方永远为 0(:156-165);record_trigger 在 miss 时直接 return(mod.rs:616-619),recall_events 只有命中样本,trigger precision/recall 永远算不出;policy_action 按 failure_count 标注,与实际检索层级无关(mod.rs:641-647);memory_recalls「停写留读」的迁移承诺未兑现。
- 内容: 五段漏斗每段接真实数据源或在展示层明示「未实装」;miss 也落一行(hits 空、retrieved_ids=[]);retrieve 返回携带实际命中层级并原样落 policy_action;完成 memory_recalls 停写收敛。
- 验收: ①stats 漏斗五段有非测试数据源或显式 N/A 标注;②能从 recall_events 直接算出各触发类型 precision/recall;③memory_recalls 停写留读。
- refs: R-161 R-196 R-213

## R-195 candidate 记忆的晋升与清退闭环:存量 5 条无人验收,最后一次晋升停在 2026-08-10 [todo]
- 内容: 给 candidate 定一条会被执行的闸门,形态不在本条强行拍板:或按复发计数自动晋升(计数已可用),或轮末/每 N 轮让 manager 逐条判定晋升与清退,超期未处置的自动 deprecate 归档。同时把存量 5 条走一遍该流程。
- 复杂度: 中
- 来源: 2026-08-12 记忆库存清理:22 条 candidate 里 15 条是重复或空正文条目(已置 deprecated 归档,见提交 2bc5899/216120e),剩 5 条 M-034/M-035/M-037/M-038/M-040 自 2026-08-10 起无人处置;最后一次成功晋升是 M-032(2026-08-10 21:31)。
- 标签: 核心
- 现状: ①promote 有 provenance 硬约束(store.rs:promote 要求至少一条 episode 证据),但没有任何常规动作会产出这种证据,于是没人晋升得了;②三段晋升(第 2 次建 candidate、第 3 次+带修复证据晋升)依赖跨轮复发计数,而计数此前因指纹里含命令载荷永远停在 1——该病灶已在 f104890 修掉(mask_volatile_payload + normalize_fp_marker),计数从此能涨;③即便计数能涨,晋升仍需一个会真正被执行的判定动作。
- 边界: 不改「未验证不注入」的取舍(R-165):本条不是要让 candidate 参与召回,而是不让它永远躺着。已在 f104890 落地的部分(candidate 对去重与复发检测可见)不重做。
- 验收: ①存量 5 条 candidate 全部有归宿(晋升 active 或 deprecated 归档),逐条给出依据;②有机制测试:满足条件的 candidate 能被自动处置,不满足的不动;③candidate 存量不再单调增长——用 index.db 与文件数给出前后对照。
- 优先级: P2

## R-196 记忆系统三处修复的效果复核:按 index.db 遥测与修复前基线对照 [todo]
- 内容: 新版本(build-3f268a5 起)跑够样本后重跑同一组查询对照四项:①自动轮采纳率是否高于 22.5%(检索键从模板 prompt 换成取活条目标题的直接效果);②空轮比例是否下降;③recurrence_counts 是否出现 >=2 的计数(指纹归一是否真的让同坑塌成一条);④candidate 新增速度是否下降(空正文与近似重复门禁是否真拦住了)。
- 基线(index.db 遥测,2026-08-08 至 08-11,修复前): 累计 224 轮召回 / 523 条注入 / 159 条被拉取 = 采纳率 30.4%;拆开看:自动轮 161 轮 351 条注入 采纳率 22.5%,用户提问轮 63 轮 172 条注入 采纳率 46.5%;08-11 当天空轮(整轮一条也没拉)36/38;单条最极端 M-006 被注入 101 次只被拉 18 次、M-018 注入 28 次 0 采纳;recurrence_counts 里 11 个指纹全部停在 1;22 条 candidate 历史召回 0 次。
- 复杂度: 小
- 来源: 2026-08-12 提交 f104890 修了三处记忆病灶(自动轮召回检索键、失败指纹归一化、manager 跨状态去重),修复前的基线已完整量化,必须回头验证修的是不是真病灶。
- 标签: 核心
- 边界: 只做度量与结论,不在本条里改实现;若某项没改善,记录原因并另开条目,不在本条无限追修。
- 验收: ①样本量说明(建议自动轮 >=50 轮)与四项对照数据写进本条进展;②任一指标未改善的,写明判断原因并给出后续条目编号;③查询口径与基线一致(同样从 .kanzei/memory/index.db 的 memory_recalls / recurrence_counts 取数),口径不同不算数。
- 优先级: P2

## R-194 全局(用户级)记忆的上线或废弃决策:7 条候选 0 条 active,历史召回 0 次 [todo]
- 内容: 二选一并落地:①上线——给全局记忆一条可执行的晋升路径(谁在什么时候按什么证据把 U-00X 升 active),并把现有 7 条逐条处置;②废弃——明确不做用户级记忆,把检索路径里的全局 store 分支摘掉,不留一个永远空转的二级库。
- 复杂度: 中
- 来源: 2026-08-12 记忆系统运行分析(依据 index.db 遥测):~/.kanzei/memory 里 7 条条目全是 candidate、0 条 active、INDEX.md 正文为空,memory_recalls 表零行——用户级记忆自建立以来从未参与过任何一轮决策。
- 标签: 核心
- 现状: 检索只看 active(memory/mod.rs 的 prompt_hints 走 search(..., Some("active"), 3);memory_search 工具 status 默认也是 active),candidate 不进召回;全局库没有任何晋升动作被执行过,7 条自 2026-08-11 起原地不动。
- 边界: 不改「未验证不注入」的既有取舍(R-165);本条只解决全局库要么没人晋升、要么根本不该存在这个二选一。
- 验收: ①决策写进 docs/design 的记忆相关文档,给出理由;②若上线:全局库至少 1 条 active,且 index.db 的 memory_recalls 有真实召回行(不是构造的测试数据);③若废弃:检索路径不再遍历全局 store,有定向测试断言,且现有 7 条有明确去向(归档或删除)。
- 优先级: P2

## R-235 存量 28 条零证据 active 记忆逐条复核:保留(存量豁免)或降级 candidate,用户拍板 [todo]
- 优先级: P3
- 内容: 对 28 条零证据 active 记忆逐条复核:保留(存量豁免,接受不可计量)或降级 candidate(严格符合无来源不入 active,代价是不可检索注入)。复核结果与依据落到 memory 系统设计文档或本条目关闭证据。
- 复杂度: 小
- 来源: R-213 关闭时盘点发现(R-213 验收③处置的承接)
- 标签: 后端
- 背景: R-213 盘点:state.db 311 条 episode、memory_sources 0 行,project 域 28 条 active 记忆(M-001~M-063)全部零证据(global 域无条目)。这些是 provenance 门禁上线前由用户/交互会话/manager 产生的既有资产,source 字段均无机器可链接的 run_id,历史回填=变相伪造,不可行。R-213 的处置定为存量豁免+文档化,但控制平面「用数据判断记忆是否改善决策」对这些条目无法计量,保留还是逐条降级应由用户拍板。
- 验收: ①28 条清单逐条给出保留/降级结论与依据;②结论落地(设计文档或关闭证据);③如选择降级,操作后搜索不再命中 candidate 条目。
- 阻塞: 用户: 28 条零证据 active 记忆保留(存量豁免)或降级 candidate 需用户逐条拍板,解除权不在 agent。解除动作: 用户给出拍板结论(全部保留 / 逐条降级清单)后按结论落地并关闭。解除人: 用户。

## R-206 前端会话运行态收口具名状态机:唯一 mutator,全局 running 降为派生视图,补 stopping 中间态 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 前端
- 来源: 2026-08-12 八维度审计(§1/§3);session_state_and_line_runtime.md §2.2 承诺的具名状态机未落地。
- 背景: 现状是 6 个布尔标志(ui/03-shell.js:78-88)被 4 个文件 12 处直写,全局 running 与 per-session 状态双真源;08-compose.js:273-283 与 :288-293 是一对紧邻重复写块(R-197 叠在旧块上的残渣)。新增任何事件类型都要手工复刻 6 标志更新规则,漂移一次就复发 D-283 类「运行中显示空闲」。停止交互缺设计基线的 stopping 态:本地乐观复位被在途进度事件翻回「运行中」,状态闪跳。
- 内容: 提供唯一 mutator(applySessionEvent/applyLocalIntent),按设计 §2.2 把 6 标志折算成具名状态字段;删除重复写块;全局 running 改为派生;补 stopping 投影(点停止后按钮转「停止中…」禁用,进度事件不得翻回运行中,仅 kz:stopped/kz:idle/终态错误能离开)。
- 验收: ①grep ui/ 目录 state.running 直写仅剩 mutator 一处;②D-283 两条反证冒烟保持绿;③「长工具运行中点停止无状态闪跳」冒烟断言;④删除 08-compose 重复块。
- refs: D-283 R-197 R-199 D-306
- 进展: 2026-08-12 R-226 已落具名 `phase`、统一 `transitionSession`、`stopping` 中间态及按活动 session 的运行控件投影；兼容旧路径仍保留 `running/converged/live_running` 字段，故本条不虚标完成，后续验收为删除全部直接字段写入并让全局 running 彻底只读派生。

## R-224 鞭挞勾选自动切自主推进:兑现 interaction_modes 的「直接勾连跑自动切」承诺 [todo]
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 来源: 2026-08-12 八维度审计(§3);interaction_modes.md:49 定案「想让它自己跑再切自主(或直接勾连跑,自动切)」,实现是拒绝+toast 让用户走三步且第一步必然失败(08-compose.js:605-616),模式选择器还藏在二级「更多」菜单。
- 内容: 结伴模式下勾鞭挞自动切换到自主推进并落一条 notice 说明(research 下仍拒绝);若用户否决自动切,则至少把模式选择器提回顶栏一级。
- 验收: ①空闲结伴态到鞭挞就绪 ≤1 次交互;②notice 可见,取消勾选切回;③冒烟断言。
- refs: R-036

## R-190 fast 本地模型 Ollama 的自动开启与常驻运行状态 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-12 dev HEAD)
- 来源: 2026-08-12 用户原话:「fast 的本地模型 ollama 需要能我们自动安装开启,能看到运行状态」。同一条消息里的另两项已登记为 R-188/R-189,本条是当时漏登的第三项(见 D-279)。
- 既有能力(§1.25 显式标注,不得重复申报为本次产出): **「自动安装」已经做完了,不要重做**——R-136 [done] 交付 `fast_model_setup`(crates/kanzei-app/src/fast_model.rs:36-150):winget 静默装 Ollama(:46-76)→ 起 `ollama serve` 并轮询 20 秒(:78-103)→ `/api/pull` 流式拉模型、百分比与 MB 进度发 `kz:fast-setup` 事件(:105-147),每步幂等、失败停在哪步说清下一步;`fast_model_status`(:12-31)返回 installed/serviceUp/modelPresent/ready 四态;设置页有状态行 `#fast-status` 与「一键就绪子代理」按钮(ui/index.html:446-449、ui/16-settings.js:373-421)。另 D-278 已把同源就绪文案接进子代理面板头部(fastStatusText 共享函数)。
- 现状缺口(本条只补这两件): ①**没有「自动开启」**——服务只在用户手点「一键就绪」时被拉起(16-settings.js:409),应用启动时既不检测也不拉起。开机后没起 Ollama、或服务中途退出,fast 子代理杂活(记忆整理/快速记录)就**静默失效**,除非用户主动翻到设置页才看得见。②**运行状态不是常驻的**——`refreshFastStatus()` 只有两个调用点(16-settings.js:420 一键完成后、:542 设置视图打开时),是一次性快照文本,既不轮询也不随事件更新;D-278 把它扩到了子代理面板,但仍是「打开面板时查一次」,面板开着期间服务挂掉不会反映。
- 内容: ①**启动即保活**:应用启动时,若 fast 解析到本地 Ollama 且 CLI 已安装但服务未运行,自动拉起 `ollama serve`(复用 fast_model.rs 既有起服务分支,不新写一套);服务在运行期掉线时能重新拉起或至少把状态如实翻红。②**常驻运行状态**:fast 运行态在设置页与子代理面板之外也看得见(状态栏或活动栏指示),且**状态随真实探测变化而更新**,不是打开某个视图才刷一次。③状态语义沿用 R-136 四态(未安装/服务未运行/模型未拉取/就绪)与 D-278 的 `fastStatusText` 共享函数,不新造第三套口径。
- 边界: 不改一键安装链路本身(R-136 既有)。**自动开启只覆盖「起已装的服务」**——安装 Ollama(数百 MB)与拉模型(以 GB 计)仍需用户点一次,R-136「未经确认的后台大流量下载不可接受」的设计取舍不推翻。fast 指向非本地 Ollama 的 provider 时一律不托管(fast_model.rs:152-161 既有行为),启动路径同样零动作。不做跨平台安装(winget 为 Windows 专用,现状如此)。
- 验收: ①自动开启实测:Ollama 已安装、服务未运行的状态下启动应用,服务被自动拉起、状态转就绪,有实测轨迹或日志证据(只断言函数返回不算);②**不越界**:未安装 Ollama 时启动应用**不触发** winget 安装、不拉模型,只如实报告缺环,有定向测试;③fast 指向外部 provider 时启动路径零动作,有测试;④运行状态在设置页/子代理面板之外可见,且把 Ollama 服务停掉后界面状态能转为「服务未运行」、重新起来后能转回就绪——**不需要重开视图**,有实测证据;⑤前端改动有冒烟断言(`node --check` + `node scripts/ui-runtime-smoke.mjs`),新增的状态指示与状态刷新各有断言(§1.3);⑥R-136 既有 Rust 2 项(拉取进度行解析、服务探测对未监听端口不悬挂)与冒烟 6 项、D-278 的 fastStatusText 断言全部保持绿。
- refs: R-136 D-278 D-167 D-279
- 依赖: 

## R-179 深并行 UX:worktree diff 接入既有目录树渲染器、合并放弃确认流、线页签仪表 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 前端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 调度顺序: 锦上添花,排在 R-177/R-178 之后。工作量已被 R-133 与 D-096 大幅削减(见「既有能力」)。
- 来源: 2026-08-10 用户对 docs/design/deep_parallel_dev.md §6 逐条拍板后,R-050 关闭拆条的第三条(= 该文 P3)。
- 既有能力(§1.25 显式标注,不得重复申报为本次产出): ①**D-096 已 [fixed]**——`worktree_diff` 已返回真实 `git diff --no-ext-diff --binary`(crates/kanzei-app/src/processes.rs),不再是 `status --porcelain` 文件名列表弹 toast;②**R-133 已 [done]**——`crates/kanzei-app/ui/06-activity.js` 已有可折叠的 diff 目录树渲染器(`buildDiffTree`、`renderDiff`,含并排视图与长行自身列滚动);③`worktree_merge` 的 `git merge-tree --write-tree` 冲突预检真实可用;④`worktree_discard` 失败时"已保留以便恢复"的兜底已存在。**本条是把这些接起来,不是重造。**
- 内容: ①把 `worktree_diff` 的输出接进 06-activity.js **已有**的 diff 目录树渲染器——不造新查看器。②合并 / 放弃的确认流:合并前展示 `merge-tree --write-tree` 冲突预检结果的可读形态(哪些文件冲突、哪边改的),放弃前明确说清"树删了、分支留着"。③线页签徽标:分支名 / running 状态 / 每线 token 计数(episodes 已记,取出来显示)。④建线 UI 上落 D6 定案的提示:每树独立 `target/` = 磁盘 ×N + 首次冷编译数分钟。⑤`worktree_discard` 在 Windows 因文件句柄占用失败时,把现有兜底延伸到 UI 提示(§5 风险 3)。
- 顺手修: `crates/kanzei-app/src/processes.rs` 的 `worktree_field(root, worktree, field)` 的 `field` 参数是死分支——`if field == "branch"` 与 `else` 两支返回同一个 `branch`,`else` 里只有一句 `let _ = root;`;两个调用点(`worktree_diff` 与 `worktree_merge`)都只传 `"branch"`。要么去掉 `field` 与 `root` 两个参数,要么让 else 分支真的返回别的东西,不留假分支。
- 边界: 不做图形化 DAG / 画布式线管理(§2.3 与 R-111 的克制一致)。不做跨线自动任务分派。合并策略按 N2 定案保持 `merge --no-ff`,不改成 rebase。
- 验收: ①线的 diff 在应用内用 06-activity.js 的目录树渲染器显示(前端有断言证明走的是既有渲染器,不是新写的一份)。②不离开应用完成 review → merge → 清理全流程;合并失败时双方改动保留且有可恢复入口(R-050 原验收原文)。③冲突预检结果在界面上可读:列出冲突文件,不只是一句"有冲突"。④线页签显示分支名与 running,每线 token 计数取自真实 episodes 数据(§1.25:不得是常量占位)。⑤建线 UI 出现磁盘/冷编译成本提示。⑥`worktree_field` 的死分支消失(全仓 grep 无同值双分支)。⑦前端改动跑 `node --check` + `node scripts/ui-runtime-smoke.mjs`,新交互(打开 diff、确认合并、确认放弃)各有冒烟断言(conventions §1.3)。⑧800/1024/1280 三档布局检查。
- refs: R-050 R-133 R-177 R-178 D-096 D-257 docs/design/deep_parallel_dev.md

## R-187 面板与提示音管理功能设置 [todo]
- priority: P2
- 原始描述: 设置面板+各类提示音管理
- 复杂度: 中
- 归属: kanzei
- 标签: 核心
- 验收: 用户可配置界面面板及各类通知音效的设置与管理

## R-188 架构浏览直观化:代码生成的架构图渲染工具(harness/skills,非文生图) [todo]
- acceptance: ①工具从真实数据源(代码结构/依赖/设计文档)生成架构图,纯代码渲染,禁止文生图与预置图片;②架构浏览页显示生成的架构图,并随数据刷新;③图不可用时降级为现有文字视图,不空白;④图上节点可点击/定位到对应文档或代码;⑤生成链路有可运行的自动化验证。
- complexity: 中
- content: 架构浏览(architecture 索引/设计文档)当前只有文字树与索引列表,读起来很不直观。需要一个代码生成的架构图渲染工具(harness 或 skills 形态),自动从真实代码/文档数据生成架构图并嵌入浏览界面。硬约束:架构图必须是代码生成的(如 mermaid/graphviz/SVG),不是文生图,也不是预置的静态图片。
- label: 核心
- priority: P2
- status: todo
- 既有能力(§1.25 显式标注,不得重复申报为本次产出): 架构浏览页本身已存在(R-122)——后端 `architecture_snapshot` 只读命令供数据(架构索引正文 + docs/design 文档清单),前端 crates/kanzei-app/ui/19-arch.js:8-104 渲染「索引 + 设计文档树」,按索引状态分层(已入册的按索引章节分组、未入册的单列,让「有文档没入册」的缺口在界面上直接可见),点击条目走既有 Markdown 查看器 openDocViewer。本条是在这份既有数据源与视图之上**加图**,不是另起一个架构页;验收③的降级文字视图就是这棵既有的树,不要重写。
- 现状(2026-08-12 读码核实,dev HEAD): 19-arch.js 全文 129 行,**无任何图形渲染**——零 svg/canvas/mermaid/graphviz 依赖,输出是纯 DOM 文本树;`architecture_snapshot` 也只回「索引文本 + 文档名/标题列表」,**不含依赖边、调用关系、模块归属等成图所需的结构化数据**。所以本条的第一道工作量在**数据侧**(从 crates 依赖、模块引用或设计文档里抽出可成图的节点与边),渲染选型(mermaid/graphviz/自绘 SVG)是第二道;验收①的「真实数据源」指的就是这层抽取,不能拿手写的图字面量顶替。

## R-189 亮色主题:前端渲染器换色结构化评估与第二套配色 [todo]
- acceptance: ①前端渲染器颜色来源结构评估:颜色集中在可换色层(变量/类)还是散落硬编码,评估结论写入需求进展或设计文档;②亮色主题完整可用:全局一键切换暗/亮并持久化;③亮/暗两套主题在 800/1024/1280 与纯键盘下均可达可用对比度;④换色改动不引入新框架,沿用现有渲染器结构。
- complexity: 中
- content: 当前桌面端只有暗色主题。需要先评估现有前端渲染器代码的颜色来源是否结构化(颜色是否集中在可换色层如 CSS 变量/主题类,还是散落硬编码),再设计并落地一套亮色主题。
- label: 前端
- priority: P2
- status: todo
- 现状评估(2026-08-12 读码核实,dev HEAD;直接对应验收①): **结构上适合换色,工作量在收口不在重构。** ①**颜色只有一处**——`crates/kanzei-app/ui/style.css`(1519 行)之外零颜色:20 个 ui/*.js 与 index.html 里的 hex/rgb/hsl 字面量各 **0** 处,没有任何 JS 在计算或写入颜色,所以换色是纯 CSS 的事,不用碰渲染逻辑。②**可换色层已经存在**——`:root`(style.css:3-18)定义 22 个**语义命名**的 token(--bg/--panel/--panel2/--input/--border/--border-soft/--fg/--fg-strong/--dim/--accent/--accent-hover/--accent-soft/--ok/--err/--warn/--info/--alert/--danger/--muted/--statusbar/--statusbar-run/--line-color),全文 **639 处 var() 引用 vs 116 处颜色字面量**,约 85% 已 token 化;命名是语义而非色值(不是 --gray-1 那种),亮色版可直接换值、不改任何引用点。③**剩余 116 处字面量分四类,处置不同**:(a) 由调色板派生的半透明叠加与阴影——rgba(208,104,78,.16)=--err、rgba(14,99,156,.18)=--accent、#0008 阴影等约 30 处,换成 `color-mix(in srgb, var(--x) N%, transparent)` 即可自动跟随主题,不需新增基色;(b) **暗色专属的徽章/状态胶囊底色**——#3a221c/#3d2a10/#16283a/#3d3113/#22301c(:382-392)与 #2a8a4233/#b34a4a33/#8883(:279-281)约 15 处,**亮色下不能复用,必须成对给值**,是本条真正的设计工作量;(c) 未 token 化的框架面——活动栏 #1a1a1a(:52)、滚动条 #383838/#4a4a4a(:46-47)、代码块与工具输出底 #0a0a0a/#0d0d0d(:1009/1146/1167/1170)、danger 按钮 #5a2c1a(:632)等约 20 处,机械提升为新 token 即可;(d) diff/语法着色 #a5c98f/#dd8d72/#f7768e/#e0af68(:1176-1177 等)约 15 处,与写死的 `color:#fff`(强调按钮/状态栏,:584/630/649/658 等)约 10 处——后者需要一个 --on-accent。④**两个 CSS 够不着的换色面(最易漏,单列)**:`color-scheme: dark`(style.css:17)决定原生控件(勾选框/下拉弹层/日期选择/文本光标)按深色还是浅色渲染,必须随主题走,否则亮色下原生控件仍是深色变体——该行注释里记着 D-154 的教训;Monaco 编辑器 `theme: "vs-dark"` 写死在 ui/17-files.js:223,CSS 变量到不了它,切主题时须同步 `monaco.editor.setTheme`。⑤**落地路径**:`:root` 拆成 `[data-theme="dark"]`/`[data-theme="light"]` 两组 token(默认 dark 保持现状零回归)+ 把 (a)(c)(d) 约 65 处字面量提升为 token + (b) 约 15 处成对给亮色值 + 上述两个非 CSS 面挂到同一开关;全程不引入新框架,与验收④一致。

## R-193 plan勾选响应延迟优化需求 [todo]
- 复杂度: 中
- 标签: 前端
- 验收: plan勾选项点击后实现即时视觉反馈和状态更新
- 优先级: P2

## R-147 增加使用手册与作者话内容板块 [todo]
- 复杂度: 中
- 归属: kanzei
- 验收: 页面顶部新增独立区块，展示项目使用手册和来自作者的说明文字
- 优先级: P1

## R-160 README添加项目设计目标说明 [todo]
- priority: P2
- 原始描述: readme里加一些设计目标，比如专为永久工作设计等等
- 复杂度: 中
- 归属: kanzei
- 验收: README中包含明确的设计目标和开发指南，如永久工作支持等核心特性说明

## R-172 新建配置文件的注释模板补齐各节骨架示例 [todo]
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 归属: kanzei
- 来源: 2026-08-10 设置页全字段走查。settings_open 原先在新建配置时把 `codex_fast_mode = false` 合成进载荷写死(已作为缺陷修掉),现改为写纯注释模板。用户定调:**保留注释模板**(不回退成 0 字节空文件),但当前模板只有三行注释,全新环境下打开「配置原文」看不到有哪些节可写,第一次上手缺线索。
- 内容: 把新建配置的注释模板补成带各节骨架的注释示例(至少覆盖 [models]、[providers.X]、[limits]、[proxy]、[cadence] 的键名与取值范围),全部以注释形式给出——**不得写成生效的显式值**,否则会被当成用户设定、绕过 fill_defaults 的默认(这正是被修掉的那个 bug 的形态)。
- 边界: 只动模板文本;不改 settings_open 的写入时机与「留空即默认」语义;模板内容写进文件、不是界面文案,不受 ui-i18n-smoke 约束。
- 验收: ①全新环境下 settings_open 产出的文件含各节骨架注释;②解析后配置仍等价于全默认(有单测:模板文件 load 后与 KanzeiConfig::default() 一致);③不引入任何生效的显式值。

## R-220 kanzei.toml 用户面配置参考:由 unknown_keys 已知键名单驱动生成,测试锁定一致 [todo]
- 优先级: P3
- 复杂度: 小
- 标签: 文档 harness
- 来源: 2026-08-12 八维度审计(§6)。
- 背景: harness_m1.md:16-53 的配置样例停在 M1(缺 limits/cadence/embeddings/permissions.non_interactive 全部新节,profile 取值没提 readonly);用户只能读 config.rs 源码猜键名。
- 内容: 生成配置参考(文档或 kz config schema 命令),覆盖全部可调键、一句话说明与默认值;加测试断言文档键表与 unknown_keys 已知键名单一致,防两处漂移。
- 验收: ①全部已知键有说明与默认值;②单侧增删键时一致性测试变红;③D-300 修复后的 barrier_timeout_secs 在参考里可见。
- refs: D-300 R-172

## R-208 新建 kanzei-base 零依赖底层 crate:承接 atomic_file 与 FileLock,解开 llm 寄居 [todo]
- 优先级: P3
- 复杂度: 小
- 标签: 后端 核心
- 来源: 2026-08-12 八维度审计(§1);atomic_file.rs:11-14 自述因 llm 是依赖图最底层只能放这里(D-261 决策),消费方横跨 tools 与 llm;kanzei-harness 不依赖 llm,其 orchestration.rs:34/41 只能在注释里引用 FileLock 行为——R-181 的跨进程 lease 契约在 harness、原语在 llm,照单实施会撞依赖方向墙。
- 内容: 新建 kanzei-base(或 kanzei-fs)零依赖 crate 承接 atomic_file/FileLock;llm/tools 改从它取;harness 增加对它的依赖。
- 边界: 纯搬迁零行为变更;过渡期保留 re-export 避免大面积改 use。
- 验收: ①kanzei-llm 不再导出文件系统原语;②kanzei-harness 可直接依赖该 crate;③全仓测试绿。
- refs: R-181 R-203
- 进展: 2026-08-13 D-332 B3 存量收敛:本条标题 [open] 为非法 lifecycle 污染(requirement 合法枚举 todo/doing/done/dropped),经 normalize 识别并修正为 todo;标题状态标记已剥离。原状态为 todo(未开工)。
- observed_head: 7f3822b37f847661673732dc8df1154d421aa1f8
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786612470593

## R-203 kanzei-tools 解体第一步:memory/ 子树拆成独立 crate,tools 不再依赖 kanzei-core [todo]
- 优先级: P3
- 复杂度: 大
- 标签: 后端 核心
- 来源: 2026-08-12 八维度审计(§1);kanzei-tools 已 25,430 行成全仓最大 crate(超 app 的 15,994 与 core 的 11,781),memory/ 7,314 行寄居其中且 manager.rs:304 直开 kanzei_core::SessionStore、mod.rs:595 实现 kanzei_core::RecallPolicy——「工具层」坐在依赖图顶端,与 lib.rs 自述「内置工具+双模式 profile 组件」脱节,记忆控制平面(R-161~R-167 主战场)没有独立编译/测试边界。
- 内容: memory/ 拆成 kanzei-memory crate(依赖 core+harness);kanzei-tools 回落到纯工具实现。
- 边界: 纯搬迁行为零变更;pub API 经再导出保持调用点零改动;不与 R-204 同批。
- 验收: ①kanzei-tools 不再依赖 kanzei-core;②memory 子系统独立编译与测试;③全仓测试绿。
- refs: R-204 R-208

## R-207 worktree 生命周期下沉 kanzei-tools:建线/回执/回滚/合并预检桌面与 CLI 共用 [todo]
- 优先级: P3
- 复杂度: 大
- 标签: 后端 并行
- 来源: 2026-08-12 八维度审计(§1);app/processes.rs 1,786 行四域混杂,worktree 业务(:777-1355)桌面独占并自带 git plumbing,与 tools/git.rs 双轨(全仓非测试 spawn git 35 处);kanzei/src/main.rs:702 注释自认「桌面端独占能力架构债」;R-183(kz 无人值守)与 R-181(外部 agent 入局)都需要 CLI 侧线管理能力。
- 内容: worktree 生命周期(create/receipt/rollback/merge 预检/状态)下沉到 kanzei-tools 的 git 域或新 worktree 模块,桌面与 CLI 共用同一实现;processes.rs 只剩 Tauri 接线与 AppState 交互。
- 验收: ①kz CLI 能调用同一实现完成建线/合并预检;②processes.rs 收敛;③既有 worktree 测试(含跨进程并发建树)全绿。
- refs: R-183 R-181 R-179

## R-205 config.rs 拆出 project_root.rs 与 permission_persist.rs:D-270 修复的结构落点 [todo]
- 优先级: P3
- 复杂度: 中
- 标签: 后端 harness
- 来源: 2026-08-12 八维度审计(§1);config.rs 2,684 行混装配置 schema/TOML 合并/权限规则持久化(:1067-1120)/项目根发现与文件系统身份判定(HOME 守卫全部实现,:1121-1357)四域,改权限形态(R-198)、改根发现(D-270)、改 schema 三类互不相干的工作在同一文件冲突。
- 内容: 拆出 project_root.rs(根发现+文件系统身份,D-270 四缺口的修复落这里)与 permission_persist.rs(append_allow_rule/generalize_resource/digest);config.rs 收敛到 schema+merge+resolve。
- 边界: pub API 经 lib.rs 再导出零变更;D-300 是两行修不必等本条,先行。
- 验收: ①三文件职责如上;②API 面零变更;③全仓测试绿。
- refs: D-270 D-300 R-198

## R-204 tracker.rs 拆分:action 分发、取活调度、测试三域分离,调度成为独立可审计模块 [todo]
- 优先级: P3
- 复杂度: 中
- 标签: 后端 核心
- 来源: 2026-08-12 八维度审计(§1);tracker.rs 2,988 行为全仓第一大文件:execute 的 match 从 :257 到 :787 十余臂内联,取活调度(schedule_entries/dependency_states/block_reasons/backlog_status/workable_titles,:956-1370)被 auto_run/CLI/docs/memory 四方消费,:1372 起 1,616 行测试同文件——恰是自举最高频改动面,取活语义(D-207 抱怨的源头)散落在工具文件里无人能单独审计。
- 内容: 拆成 actions/(每 action 一函数)+ scheduling 独立模块(供四方统一消费)+ 测试分域下沉;execute 只剩路由。
- 边界: 四个既有消费方调用点零改动;行为零变更。
- 验收: ①调度逻辑有独立测试文件;②execute 只剩路由;③全仓测试绿。
- refs: D-207 R-203

## R-202 run_task 与 run_once_with_parts 内部分段拆分:补登 monolith_decomposition 的「另立条目」承诺 [todo]
- 优先级: P3
- 复杂度: 大
- 标签: 后端 核心
- 来源: 2026-08-12 八维度审计(§1);monolith_decomposition.md:25/69/192 三处写明两函数「只整体搬迁,内部拆分另立条目」,从未登记,现已分别涨到约 1010 行(app/run.rs:26-1035,20+ 参数挂 too_many_arguments)与约 987 行(core/runner/drive.rs:47-1034)。
- 内容: run_task 按 装配/事件循环/轮末收尾 三段抽函数;run_once_with_parts 按 请求重试/工具批执行/收尾 分段。
- 边界: 行为零变更;外部签名与 pub API 不变;不与功能改动同批。
- 验收: ①每段可独立单测;②cargo test --workspace 全绿;③两函数主体各降到 300 行以下。
- refs: R-153 R-155

## R-174 子代理面板与并发度口径:独立 Running/Finished 面板、单条停止与完整 transcript [doing]
- 优先级: P0
- 复杂度: 中
- 标签: 前端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 来源: 2026-08-10 用户看过 Claude Code 的后台子代理面板后定调:kanzei 的子代理要走这个执行模型,而且比它更激进。四个轴——①后台化(跨轮存活、主代理派完不阻塞、完成发通知)②子代理能写(打破只读白名单、自持写租约)③并发度放开(远不止现在的 8)④可对话(给正在跑的子代理发消息带原上下文续跑)——**都要,但必须分级实现**(用户原话:「都要,但是你说的这些点得分级实现,确实改动大,风险多」)。参照物形态(Claude Code 面板实测):独立 Background tasks 面板,分 Running / Finished N 两区;每条显示名称、类型、已运行时长、累计 token、工具调用次数、当前正在用的工具名、View transcript 链接、单条停止按钮;面板有 Clear。本条是四轴里最便宜、可独立交付的一段(可观察 + 并发度口径),不依赖后台化。
- 设计定位: 四轴分级第 1 级——先把子代理变成「看得见、停得住、查得到」的对象;后台化(R-175)与写权(R-176)在此之上叠加
- 既有能力(§1.25 显式标注,不得重复申报为本次产出): 并发度**已经是可配项**——`max_tasks_per_turn` 在 crates/kanzei-harness/src/config.rs:59 是 `Option<usize>` 字段,:90-92 `unwrap_or(8).max(1)` 给默认 8 且**无上限钳制**;设置页已有「单轮子代理数上限」输入框(crates/kanzei-app/ui/index.html:469-470 `set-max-tasks`、ui/16-settings.js:187 与 :420、crates/kanzei-app/src/settings.rs:287-288/497/519/531);往返单测已钉死「没填的键不写进文件 / 没填走内置默认 8」(settings.rs:749-752)、serde default 单测已存在(config.rs:918-920、:936-939 越界回落 1)。因此「从固定 8 改为可配」这件事**无需再做**。
- 关键现状(本组三条需求的共同前置): 桌面端主对话**根本不注册 task 工具**——crates/kanzei-core/src/runner/drive.rs:57 只在 `subagent.is_some() && !config.execution_policy.is_serial_writer()` 时 push `task_spec()`,而 crates/kanzei-app/src/run.rs:107-108 给主对话**无条件**设 `ExecutionPolicy::ReadParallelWriteSerial`(`is_serial_writer()==true`,见 crates/kanzei-harness/src/orchestration.rs:21-23)。所以并行子代理在桌面端当前是**全禁**状态:drive.rs:410-503 的轮内并发批与 crates/kanzei-core/src/runner/subagent.rs:163-176 的读槽登记代码不可达,`max_tasks_per_turn` 配了也没有生效路径。该回归由 **R-173**(阶段编排对象)的阶段感知策略修复,是本条与 R-175/R-176 的共同前置——本条的面板与并发度实测必须在 R-173 修复后才能在桌面端取证。
- 内容: ①并发度口径收口(**不是重做配置**):复核默认 8 是否上调(用户要「远不止 8」),并在设置页把该值与「桌面端当前不生效」的事实对用户说清;溢出分支文案沿用 drive.rs:441-444 既有实现。②新增**独立「子代理」面板**——不再只作为活动面板(#bg-panel,ui/index.html:630-650)里 `bg-type-filter=agent` 的一个筛选项:Running / Finished N 分区,每条显示 名称 / 类型 / 已运行时长 / 累计 token / 工具调用次数 / **当前正在用的工具名** / 单条停止 / 打开 transcript,面板有 Clear。③单条停止通道:现状 ui/06-activity.js:261 注释明写「子代理没有单条停止通道,只能停整轮」,本条要消灭这个缺口。④可查看单个子代理的**完整 transcript**(工具调用序列 + 每次调用的入参与输出),不再只有 R-095 的摘要维度(内部调用数 / 当前步骤 / 成败 / 耗时)。
- 边界: 不做后台化(R-175)、不做写权(R-176);面板本条只需渲染**轮内并发**的子代理,跨轮存活条目待 R-175 提供数据后再接。不改 `max_tasks_per_turn` 的配置通道本身(已可用),只调默认值口径与设置页说明。
- 验收: ①并发度实测:`kanzei.toml [limits] max_tasks_per_turn = N`(N 取远大于 8 的值)后,同轮派发 N 个 task 全部执行、第 N+1 个才落 drive.rs:441-444 的溢出错误,有轨迹或日志证据;②旧配置无该键时行为不变——config.rs 既有 serde default 单测保持绿(若本条上调默认值,须同步更新 :918-920 断言并保留「缺键=内置默认」语义),settings.rs:745-752 往返单测保持绿(保存不丢字段);③面板存在且分区正确,每条的 名称/类型/时长/token/工具调用数/当前工具名 六个字段**均取自真实 RunEvent**(ToolStart/TaskProgress/ToolEnd),冒烟脚本用桩事件逐字段断言渲染出真实值而非常量占位;④单条停止真能停:点击后该子代理不再产出 TaskProgress、以「被停」终态收尾、读槽被释放,有实测证据(仅改 UI 类名/状态不算通过);⑤transcript 有真实数据源:能查看单个子代理的完整工具调用序列与每次调用的入参/输出——§1.25 明令「只展示但未接入真实数据源的界面壳不算完成」,不得以摘要冒充 transcript;⑥前端改动有冒烟断言:`node --check` + `node scripts/ui-runtime-smoke.mjs`,分区切换、单条停止、打开 transcript 三个新交互各有对应断言(§1.3);⑦桌面端可达性:R-173 修复前置回归后,在桌面端主对话实测面板真出现子代理条目(不能只在 CLI 或单测里成立)。
- refs: R-095 R-117 R-173 R-175 R-176
- 依赖: 
- 进展: 批1-3已提交(9179ae8/68ee84ec/25ea2c0),cargo test --workspace 全量全绿。验收①并发度实测✓(集成测试+轨迹)、②旧配置无键行为不变✓(serde default 测试绿)、③面板分区与六字段真实数据✓(冒烟逐字段断言)、④单条停止✓(stop_task + task_cancel_parallel.rs 实测)、⑤transcript 真实数据源✓(TaskTrace.input 渲染,冒烟断言入参)、⑥冒烟断言✓(分区切换/停止/transcript/被停终态/Clear 均有断言)。仅剩验收⑦「桌面端主对话实测面板真出现子代理条目」未闭环——需要构建新版 kzapp 安装,2026-08-11 用户定调:先不装,等下次发版一起实测。本条保持 doing 待发版,不占可执行槽位。
  2026-08-11 本次发布补齐面板生命周期：停止→已完成→已关闭→删除；关闭/删除只改变当前 UI 条目，后端 transcript/审计保留，真实 stop_task 仍是停止通道。主代理权限边界在系统提示与子代理 task_spec 中显式固化：子代理仅 read/glob/grep，写入、比对、合并和发版由主代理负责。发版后验收⑦转为用户桌面实测。
  ①**前置回归已解除**——「桌面端主对话根本不注册 task 工具」那条(本条与 R-175/R-176 共同记录的前置)已由 R-173 批4.5 修掉(`e933262`),验收⑦现在可以真去桌面端取证了。
  ②**验收③已部分交付**——R-173 收尾时把编排派发的勘察/复核子代理接上了活动面板(`ff287c4`):按 `input.phase` 分「勘察/复核」两组、显示角色名与**当前工具名**(取 `kz:task-progress` 的 `trace.name`)、运行时长、内部调用数,超时与失败分开成两种终态,冒烟有 6 组反证锁死。**它刻意复用 `#bg-list` 没有新建平行面板**——本条要做的独立面板应当在此之上演进,不是另起炉灶。仍缺:累计 token、Clear、Running/Finished 两区(现在是按阶段分组,不是按运行状态)。
  ③**验收④单条停止的最小改法已备**:目前 `dispatch_roles` 的 future 集合由屏障统一驱动,没有对外暴露的 per-role cancel handle。改法 = 每角色配一个 `CancellationToken` + 新 Tauri 命令按 role 触发,取消后该角色以 `ScoutOutcome::Failed("cancelled")` 进终态——屏障照常收敛,不会挂住。
  ④**两条形态决策留给本条拍**:(a) 编排派发的 8 条同时也会在**主对话**里各生成一个工具块(`chatToolStart` 无条件调用),信息没丢但每个自主推进轮多 8 个块,可能偏吵;(b) 前端条目的 `id` 就是角色名,而角色跨轮复用,所以当前实现是**每角色只留最新一轮**(跨轮定格的 bug 已修成"原地复位")。要保住历史轮次得让后端给 `role@round` 之类的唯一键。
  另:验收①②的「并发度可配」部分是**既有能力**(见本条「既有能力」字段),不要重做。

- 批次: 3/5

- 阻塞: 用户: 2026-08-11 定调「先不装,等下次发版一起实测」验收⑦(桌面端主对话实测面板真出现子代理条目)。解除动作: 下次发版构建新版 kzapp 后,用户在桌面端主对话实测面板出现子代理条目,确认后关闭本条。解除人: 用户。

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

- 阻塞: 用户: 2026-08-13 定调 park 本条、专注 D-318,B1 基座验证(重建 kzapp 跑 node scripts/e2e-smoke.mjs)暂停。解除动作: 用户说恢复推进时,重建 kzapp 后继续验证 B1 基座。解除人: 用户。

- 批次: 0/8

## R-135 开发与缺陷修复进度动画显示 [todo]
- 优先级: P0

- 标签: 前端

- 进展: 2026-08-11 扫描:本条仅存标题/优先级/标签,缺 内容/验收/背景——边界不清无法开工(无验收就无从交付,违反 §1.25)。待用户补全条目内容后按序恢复取活,不占可执行槽位。

- 阻塞: 用户: 本条条目内容缺失(仅存标题/优先级/标签,缺 内容/验收/背景),边界不清无法开工(无验收就无从交付,违反 §1.25)。解除动作: 用户补全条目内容(至少验收原文)后按序恢复取活。解除人: 用户。

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
