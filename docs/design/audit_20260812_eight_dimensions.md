# 八维度全面审计与改进计划(2026-08-12)

- 状态: 审计报告 + 改造计划总纲(用户指令发起;19 个子代理完成——8 维度并行分析 + 11 条 critical/即时声称逐条独立反证,其中 5 条被驳回,见 §10)
- 方法口径: 每条发现要求 file:line 证据;「已登记」逐条对照 requirements/defects 及两份归档;行号为 2026-08-12 dev HEAD(d1792bc 前后,审计期间自举轮并发提交了 800d5da/6ef23cc/60f9455,受影响结论已复核修正)
- 登记映射: 缺陷 D-296~D-306、需求 R-202~R-224,逐项见各节「处置」标注与 §9 总表;未登记的进 §11 候选池
- 配套产出: [research_mode.md](research_mode.md)(维度8 的设计文档,实施条目 R-221)

## 1. 抽象水平与解耦

总评:巨石拆解(R-153/R-154/R-155)按 monolith_decomposition.md 兑现约七成,执行质量高;但「分文件」之外的熵增没有任何条目在跟踪。三个新熵点:①文档承诺「另立条目」的两处函数内部拆分从未登记,run_task 涨到约 1010 行、run_once_with_parts 约 987 行;②kanzei-tools 已成全仓最大 crate(25,430 行,含记忆控制平面 7,314 行、tracker+docstore 4,826 行)且反向依赖 kanzei-core,「工具层」坐在依赖图顶端,名实不符;③tracker.rs(2,988 行)成为新第一大文件,590 行单 match 分发+取活调度+1,616 行测试同居,恰是自举最高频改动面。trait 使用克制,无过度抽象;不足侧是公共基础 crate 缺位(atomic_file 寄居 kanzei-llm,harness 因依赖方向用不到)。

| 改进项 | 证据锚点 | 处置 |
| --- | --- | --- |
| run_task / run_once_with_parts 内部分段拆分(补登文档承诺) | monolith_decomposition.md:25/69/192;app/run.rs:26-1035;core/runner/drive.rs:47-1034 | R-202 |
| kanzei-tools 解体:memory/ 拆独立 crate | tools/src/memory/ 共 7,314 行;manager.rs:304 开 kanzei_core::SessionStore | R-203 |
| tracker.rs 拆分 actions/scheduling/测试分域 | tracker.rs:257-787 match;:956-1370 调度被 4 方消费;:1372-2988 测试 | R-204 |
| config.rs 拆出 project_root.rs 与 permission_persist.rs | config.rs:1121-1357 根发现+文件系统身份;:1067-1120 规则写回 | R-205(D-270 修复落点) |
| 前端会话运行态具名状态机 + 唯一 mutator | session_state_and_line_runtime.md §2.2 未落地;state.running 4 文件 12 处直写;08-compose.js:273-293 重复写块 | R-206 |
| worktree 生命周期从 app/processes.rs 下沉共用 | processes.rs:777-1355 桌面独占;kanzei/src/main.rs:702 自认架构债 | R-207(R-183/R-181 前置) |
| kanzei-base 零依赖 crate 承接 atomic_file/FileLock | kanzei-llm/src/atomic_file.rs:11-14 自述寄居;harness 无法依赖 | R-208(R-181 前置) |
| InMemoryBroker 死抽象清理;前端编号约定衰减(06 重号、08-compose 复胖) | core/notification.rs:7;ui/ 加载序 | 候选池 |

## 2. 实现效率

总评:单线对话主路径健康(WAL+NORMAL、帧级节流、D-283 后事件驱动为主)。效率债集中三处:①「刷新即全量」读路径——docs_snapshot 单次调用把两份归档(~4.8MB)解析约 6 遍外加一次 git log,挂在每次文档刷新与每次 git 提交事件后,极可能就是 R-193 勾选延迟的机制底座;conversation_list/trace_get/按序号恢复全量解析整张 session_events(主会话实测 4333 条/8.9MB)且 run.trace 无清理通道,成本单调增长。②落库形态——每条工具事件新开一条 SQLite 连接;run.trace 9.8MB 中 95% 来自 279 条非增量整包(单条最大 945KB),这给 D-209 提供了新的量化方向(对话快照只占 0.05MB,轨迹层才是大头)。③空间治理缺位——state.db 82MB 里约 68MB 是 freelist 死页,9 份迁移备份 59MB 永不回收,.kanzei 数据库相关占用 ~145MB 而活数据仅 ~11MB。

| 改进项 | 证据锚点 | 处置 |
| --- | --- | --- |
| docs_snapshot 重复解析归档 6 遍 + git log(反证确认) | app/docs.rs:146-290;docstore 每次 open+load 无缓存 | D-296 |
| session_events 全量解析 + run.trace 无保留策略(反证确认) | app/conversation.rs:74-178;core/store/events.rs:24-35;整包 flush state.rs:203-230 | D-297 |
| state.db freelist 68MB + 迁移备份 59MB 无回收 | 实测 page_count/freelist_count;无 VACUUM 调用 | D-298 |
| 工具事件落库每事件新开连接 | app/state.rs:267-276;调用点 run.rs:345-485 | 候选池(修 D-297 时顺做) |
| 协作块每步对每条其它运行线起 1-4 个 git 子进程 | drive.rs:192-193 → collaboration.rs:110-150 | 候选池(TTL 缓存) |
| 每步 2 次全量历史序列化+1 次深克隆;启动链 6 步串行;归档分卷滚动 | drive.rs:212/273/296;18-startup.js:22-36;docstore.rs:340-375 | 候选池 |

## 3. 前端交互

总评:R-197/D-283 后的会话状态投影扎实,历史债大都已还清。两类系统性断层:①「设计文档宣称已交付、代码里没有」——parallel_lines_ui.md 状态头称 P1~P6 随 R-184 全部上线,实际 P1 的「删『下一个』推断值+backlog 标『被取得』」一处都没发生、P6 的按 agent 设置未实现,虚标会让后续自举轮漏做或重复申报,D-207 的病根(界面展示推断值)继续存活;②同一动作双入口纪律不一致——侧栏「隔离工作树」保留绕过收活五格「必须人读 diff」强制格的独立合并路径。状态可见性余留:停止缺 stopping 中间态(闪跳)、空闲线路残留旧 stage 文案(反证确认)、autonomous 轮权限被拦零聚合信号(「自动放行」写着本次实际跨重启持久化)、后台线路挂起询问在线路按钮上零可见。鞭挞与模式的耦合与 interaction_modes.md「勾连跑自动切」的承诺相反。

| 改进项 | 证据锚点 | 处置 |
| --- | --- | --- |
| parallel_lines_ui.md 状态头虚标 P1/P3/P6(与维度7 交叉确认) | 该文:4;ui/11-docs-list.js:347 isAgentNext 仍在;16-settings.js 无按线设置 | D-304 |
| 侧栏工作树独立合并入口绕过收活五格 | 09-sessions.js:20-94;20-lines.js:500-520 仅 window.confirm | D-305 |
| 空闲线路残留上一轮 stage 文案(反证确认) | 01-core.js:52-55 写入、:88-102 终态不清;09-sessions.js:147 轮询回填 | D-306 |
| 权限被拦聚合呈现 + 自动放行常驻徽标与语义对齐 | 07-events.js:432-436 只进隐藏日志;D-281 记连撞三轮 | R-223(配套 D-281) |
| 鞭挞勾选自动切自主(兑现 interaction_modes.md:49) | 08-compose.js:605-616 拒绝+toast 三步 | R-224 |
| stopping 中间态(并入 R-206);线路待答徽标;live-* 孤儿管线清理;键位语义对齐;P1 残余(并入 D-304 修复) | 08-compose.js:796-833;07-events.js:472-521;06-activity.js:780-812 | R-206 / D-304 / 候选池 |

## 4. 测试构建与记录

总评:骨架扎实(约 700 条测试、9 个 CLI E2E、六条前端冒烟、verify.ps1 十步门禁、提交四重硬门禁)。结构性短板:①门禁清单三处维护(verify.ps1/ci.yml/git.rs)承诺逐项同步,守护测试只比对 2 项,已实际漂移(ui_lint 只进了 verify 侧,CI 缺 npm ci);②提交门禁 cargo check 与 clippy 全 workspace 串行都跑,语义冗余,小步提交付双份全仓分析;③记录方式落差大——设计文档承诺的 VerificationRun 体系零实现,source_test_gate 只验新近度不验相关性(任何 passed 记录可背书任何源码提交),tests-archive 371 条仅 23% 带关联字段,全链路无耗时数据;④偶发红治理空白,D-293 验收没有可执行载体(本轮读码把怀疑面收敛:docstore 条失败形态与 load() NotFound 宽容分支+Windows rename 替换窗口高度吻合,read 条的 temp 不唯一猜测基本排除——已回填 D-293 方向,反证确认)。D-295 死锁在审计期间已被自举轮修复归档(6ef23cc)。

| 改进项 | 证据锚点 | 处置 |
| --- | --- | --- |
| 门禁清单机械同步守护 + CI 补 npm ci/ui-lint | verify.ps1:47-50;ci.yml 清单缺项 | R-209 |
| 提交门禁去 cargo check 冗余 + verify/test_record 补耗时 | git.rs:396/470-484/584-596;verification.json 无时长 | R-210 |
| 偶发红加压脚本(D-293 验收载体;顺评 cargo-nextest) | scripts/ 无压测;D-293 验收①③无载体 | R-211 |
| source_test_gate 相关性绑定(覆盖面声明求交) | git.rs:538-547 只消费 last_passed_at | R-212 |
| D-293 定向排查方向(NotFound 短重试 / ReplaceFileW) | docstore.rs:328-338;atomic_file.rs:34-80 | 并入 D-293(本报告为证据) |
| TempProject RAII 夹具扩到所有测试临时目录 | 6+ 处手写 temp_dir().join,panic 即泄漏 | 并入 R-200 实施(扩边界建议) |
| VerificationRun 承诺诚实处置;test_record refs 软强制;ui-runtime-smoke 拆分与变异表扩容;e2e-smoke 入门禁(随 R-101) | reliability_usability_self_hosting_quality.md:300-352 | 候选池 |

## 5. 记忆系统

总评:存储层与工具面扎实,门禁密度高;控制平面(R-161~R-166)存在系统性「验收以单测可跑为准、生产链路实际断开」。反证确认的现行问题:失败指纹粒度崩塌——bash 类失败常态塌缩成 `[fp:bash|exit code:]` 全类通配键,任何 bash 失败都 Tier0 注入 M-022 并污染复发计数。结构性缺口(反证修正后仍成立的部分):promote 的 provenance 硬约束形同虚设(不校验 episode 真实性、写证据失败被 `let _` 吞掉、manager 只能编造 episode_id);五段漏斗遥测名不副实(AVAILABLE 口径写错、ACTION_CHANGED/OUTCOME_IMPROVED 永远为 0、memory_recalls 停写承诺未兑现、miss 不落库致 precision/recall 永远算不出);inbox 现存 13 条滞留、整箱清空存在并发丢 note 窗口(「结构性死锁」定性被反证驳回——steps 是模型轮次而非工具调用数,通道能推进,但按条销账改进仍然值得做);写入去重挡不住跨语言复述(M-055/M-056 复制 M-044,「假指纹立即污染注入」被反证驳回——FingerprintIndex 只收 active 条目);记忆与 tracker 边界只有提示词一句话在守,6 条交付状态类内容已落进记忆。

| 改进项 | 证据锚点 | 处置 |
| --- | --- | --- |
| 失败指纹质量闸(反证确认现行影响) | metrics.rs:391-419 failure_kind 取首行抹数字;bash.rs:268-271 首行恒为 exit code | D-299 + R-216 |
| promote provenance 校验补真 | memory/store.rs:392-427 `let _` 吞写失败;telemetry.rs:71-77 | R-213 |
| 漏斗遥测口径修正 + miss 落库 | telemetry.rs:136-165;mod.rs:616-647 | R-214 |
| inbox 逐条销账 + 并发 append/next_id 防护 | store.rs:1122-1162 读改写;manager.rs:420-426 整箱清空 | R-215 |
| 写入侧三闸(FTS 去重下沉双 scope、[fp:] 一致性校验、tracker 边界拒收) | store.add 无语义硬闸;M-055/M-056 实例;6 条交付状态记忆 | R-216 |
| R-164 三臂对照认领(dense 启用/冻结出结论) | index.rs:15 dense 恒空;A-011 无终点 | 候选池(可并入 R-196 复核) |

## 6. harness 与上下文/工具管理

总评:骨架质量高(六注册表装配、last-match-wins+硬 deny、装配期能力校验、上下文账单、主动压缩+纪要校验、宽容 JSON 修复),对「弱模型也能照着走」有实质支撑。「autonomous 死锁寸步难行」的 critical 声称被反证驳回(§10),但方向性缺口成立:R-198(前缀白名单)+R-183(non_interactive 接线)仍是无人值守的正解组合,建议合并为一条 P0 交付口径。确定性缺陷:limits.barrier_timeout_secs 漏接 merge overlay 且 unknown_keys 名单缺失——设了静默不生效还误报未知键,代码注释两处自认「就是这么漏的」却从无条目(本轮复核 overlay! 宏 10 字段确实无它)。能力缺口:dev 档零联网(websearch 只注册给 research,webfetch 默认 Ask 在自主轮等于禁用);task 子代理只有 read/glob/grep,勘察角色查不了 git 历史;context_limit 认不出的 provider 全程无主动压缩、被动恢复仅 2 次且一刀砍到 4000 字符;AllowOnce 不跨轮;command 模板/skill 工具是设计承诺的半成品;kanzei.toml 无用户面配置参考。

| 改进项 | 证据锚点 | 处置 |
| --- | --- | --- |
| barrier_timeout_secs 双漏(overlay + unknown_keys) | config.rs overlay! 宏 10 字段无它;:363/:1032 注释自认 | D-300 |
| dev 档联网能力(websearch + 域名级白名单形态) | profiles.rs:552-554;base.rs:53;drive.rs:876-881 | R-217 |
| SubagentBase 只读工具面扩容(files/git 只读) | tools/subagent.rs:14-25;task_spec 自述 cannot inspect git | R-218 |
| context_limit 未知时保守压缩 + 恢复计数衰减 | config.rs:326-343;mod.rs:88;compaction.rs:124-141 | R-219 |
| kanzei.toml 配置参考(unknown_keys 名单驱动 + 一致性测试) | harness_m1.md:16-53 样例过时;已知键 8 顶层+limits 11 | R-220 |
| R-198+R-183 合并 P0 交付口径;AllowOnce 会话级;并行 wave 补发 PermissionResolved;command/skill 收口;权限规则卫生通道;R-199 时去 p\| 魔法前缀 | drive.rs:165-170/568-683;run.rs:129-141 | 候选池(前两者建议并入 R-198/R-183 取活时的条目修订) |

## 7. 并行系统(四轴分级)

总评:机械强制层质量高(七阶段迁移表穷举锁死、屏障唯一通路、租约三层保证+Drop 兜底、只读子代理构造层锁死、R-182 树级分桶、托管文档 FileLock)。四轴完成度:R-173 已交付且测试锚扎实;R-174 仅剩验收⑦待发版实测;R-175/R-176 零起步。缺口:①编排派发路径没有 per-role 墙钟,设计承诺的「双层有界」内层在唯一生产路径上不存在,单角色挂死拖满 1800s 外层屏障;②TaskCancellations 死 token(timeout drop future 跳过清理,stop_task 对已死子代理误报成功);③停止/异常路径 writer 审计断档(桌面协调器未装配 observer);④R-176 条目仍按已被 R-182 撤销的不变量 3/4/5 撰写,照单实施会与树级租约死锁——取活前必须重写;⑤主树多写入者(自举×kz CLI×外部 agent)机制护栏为零,唯一防线是提示词纪律(R-181 已登记,建议尽快落「声明+检测」最小版);⑥收活五格门禁非合并前置、合并后无全量步。

四轴下一级最小可交付建议:R-174 发版后收验收⑦;R-175 第一级只做「派发不阻塞+终态通知」(复用 agent_notifications)不做续跑;R-176 重写条目后第一级只做单个写子代理的归因与单独回滚;面板轴补泳道单条停止。

| 改进项 | 证据锚点 | 处置 |
| --- | --- | --- |
| per-role 墙钟缺失 | phase.rs:365-368 注释承诺;phase_pipeline.rs:294-311 无 timeout;rt.timeout_secs 唯一消费点 drive.rs:520 | D-301 |
| TaskCancellations 死 token | runner/subagent.rs:267-323;drive.rs:520-533 timeout drop | D-302 |
| writer 审计断档(observer 未装配) | state.rs:400 MemoryCoordinator::new();core/orchestration.rs:132-139 notify 丢弃 | D-303 |
| 收活五格补防线(门禁前置确认 + 合并后全量) | 20-lines.js:287-294/334-339/388-391;设计 §5 | R-222 |
| R-176 条目按 R-182 新口径重写(取活前) | requirements.md R-176 引已撤销不变量;orchestration.md:131-133 划掉 | 建议(见 §9 注) |
| R-185 最小版尽快交付;R-181「声明+检测」最小版;roster_cap 截断提示;ReadParallelWriteSerial 配置通道表述对齐 | main.rs:559 coordinator None;phase_pipeline.rs:230-231 | 候选池(前两条既有条目已覆盖方向) |

## 8. research 模式

维度8 的全部素材与设计定调见 [research_mode.md](research_mode.md)。核心结论:research 模式骨架完整但形态错位(面向网络调研)且零使用(266 episodes 零调用 websearch/source/finding);真实勘察全部发生在 dev 模式且结论没有固定落点;证据等级 E0-E4 被双重语义挪用;research/memory.md 是绕开记忆控制平面的第二套无校验记忆。重定位为「先计划后自举」的勘察载体,实施条目 R-221。

## 9. 登记映射总表

缺陷(按建议取活序,已插入 defects.md 头部):

| 编号 | 一句话 | 维度 | 反证 |
| --- | --- | --- | --- |
| D-296 | docs_snapshot 重复解析归档 6 遍+git log,挂在每次刷新与提交后(R-193 机制底座) | 效率 | 确认 |
| D-297 | session_events 全量解析(8.9MB)+run.trace 无保留策略单调增长 | 效率 | 确认 |
| D-298 | state.db 68MB freelist+59MB 备份无回收 | 效率 | 实测 |
| D-299 | 失败指纹塌缩 [fp:bash\|exit code:] 全类通配,Tier0 错配注入 | 记忆 | 确认 |
| D-300 | barrier_timeout_secs 漏 overlay+unknown_keys 双漏 | harness | 本轮复核 |
| D-301 | 编排派发无 per-role 墙钟,内层有界承诺不存在 | 并行 | 读码 |
| D-302 | TaskCancellations 死 token,stop_task 误报成功 | 并行 | 读码 |
| D-303 | 桌面协调器无 observer,停止路径 writer 审计断档 | 并行 | 读码 |
| D-304 | parallel_lines_ui.md 状态头虚标 P1/P3/P6 已上线 | 前端/并行 | 双维度交叉 |
| D-305 | 侧栏工作树合并入口绕过收活五格已读 diff 强制格 | 前端 | 读码 |
| D-306 | 空闲线路残留上一轮 stage 文案 | 前端 | 确认 |

需求(已插入 requirements.md,位于 R-199 之后、R-174 之前;取活序可在侧栏调整):R-202~R-208(抽象/解耦)、R-209~R-212(测试)、R-213~R-216(记忆)、R-217~R-220(harness)、R-221(research)、R-222(并行)、R-223~R-224(前端)。

注:R-176 取活前重写(§7 ④)不新开条目——那是对既有条目的修订动作,留给用户或取活轮按本报告 §7 证据执行。

## 10. 反证驳回记录(防止后续轮复报)

以下声称经独立反证代理核查**不成立或严重度夸大**,后续自举轮遇到同类观察请先读这里:

1. 「autonomous 档位权限死锁,自举轮寸步难行」(critical)——驳回。分析只数了项目层 kanzei.toml,漏看全局层(config.rs:454-461 先并全局,permissions.rules 是 extend 追加);且 D-295 已在审计期间修复归档(6ef23cc)。R-198/R-183 作为正解仍然成立,但现状不是寸步难行。
2. 「live-* 四个写入点指向已删除 DOM,信息静默消失」——驳回定性。c611f90(R-197 批6)**有意**删除这些元素,写入点带 `el?.` 守卫;残余是死代码清理项,不是故障。已入候选池。
3. 「inbox 整箱消化+10 步预算 = 结构性死锁」——驳回核心算术。steps 是 LLM 轮次而非工具调用数,一轮可执行多个调用,13 条积压不必然超限;但 13 条滞留 ≥1 天、整箱清空的并发丢 note 窗口、按条销账改进仍成立(R-215)。
4. 「manager 伪造指纹立即污染 Tier0 注入」——驳回影响链。FingerprintIndex 只收 active 条目且只从 frontmatter/正文取指纹,标题不扫描;M-055/M-056 是 candidate,假指纹进不了索引。跨语言去重穿透本身成立(R-216)。
5. 「勘察简报轮内即逝、无持久化」——驳回措辞。简报进当轮 User message 并随 conversation.updated 落库;真缺口是「无固定勘察工件落点」(research_mode.md 主题)。
6. 「D-295 解除动作单边,门禁退化纯自报」——已过时。D-295 修复归档;source_test_gate 自始只消费自报记录,威胁模型明文不防说谎的模型。「成对放行 cargo test」的关切并入 R-198/R-183 取活时验收。

## 11. 候选池(本轮未登记,按需转正)

抽象:InMemoryBroker 死抽象清理;前端编号约定收敛+08-compose 三域拆分。效率:工具事件长连接;协作块 git 采样 TTL 缓存;预算估算增量维护;启动链并行化;归档分卷滚动。前端:线路待答徽标;live-* 孤儿管线清理;键位语义对齐(Enter 提示、权限弹窗 Esc≠拒绝)。测试:VerificationRun 诚实处置;test_record refs 软强制;ui-runtime-smoke 拆分+变异表全量入 verify;e2e-smoke 入门禁(随 R-101)。记忆:R-164 三臂对照认领。harness:R-198+R-183 合并 P0 口径;AllowOnce 会话级;并行 wave 补发 PermissionResolved;command/skill 半成品收口;权限规则卫生通道;p| 魔法前缀移除(随 R-199)。并行:R-185/R-181 最小版加速;roster_cap 截断提示;ReadParallelWriteSerial 表述对齐。
