# Defects

## D-504 鞭挞配置双真源与 autoRounds 双计数器,四副本靠手工互拷同步 [fixing] (medium)
- 复现: crates/kanzei-app/ui/08-compose.js:1088-1097 lineAutoConfig 活动线读 DOM 复选框、其他线读 processAutoState Map;同状态另存 localStorage(kz-process-auto-state) 与后端 ui_prefs/auto_state_update(:1014-1021,:1057);autoRounds 全局(:4)与 state.auto_rounds(:337,:380) 靠 07-events.js:439/449/465 手工互拷,:1078 切线再读回
- 影响: 四副本两条同步路径,漏一处即显示 0/10 实际下一轮撞上限;历史已翻车两次
- 来源: 2026-08-18 全库勘察(主会话);D-290/D-353 历史翻车点
- 标签: 前端
- 验收: 收敛单一真源(Map/state),DOM 只做投影;切线/后台线/重启回归用例;冒烟覆盖
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-504
- 进展: 实现提交 `8f490d92` 与自动化证据已完成。已确认真实安装位 `C:\Users\kanzei\AppData\Local\kanzei\kzapp.exe` 存在且当前进程正在运行；当前窗口显示用户正在使用该应用，按发布规则不得强杀或擅自关闭。因此最后一项“已安装桌面应用退出→重启→读取持久化状态”暂记外部阻塞，待用户关闭窗口后执行真实重启链路；其余验收保持已通过。
- observed_head: 8f490d92856e1e0208efee838b55b18254d6c883
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787008359348
- 阻塞: 
- 对账: 2026-08-20 对账:用户已关闭 kzapp 窗口,阻塞解除;剩余动作=重新启动安装位 kzapp 回读持久化 auto state 完成真实重启验收(桌面窗口空闲期执行,CLI 循环亦可承接);其余验收已通过(8f490d92)
- 停车: 等待用户 kzapp 窗口空闲:剩余验收=重启安装位回读持久化 auto state,当前 PID 51368 为用户使用中进程不可接管;上一窗口期(2026-08-20 03:5x)已用于 R-101 停止 E2。解除人:用户空闲窗口+agent 执行

## D-566 cross-tree 隔离快照无回收且构建产物误报,121 目录 143MB 纯堆积 [open] (medium)
- refs: D-395 D-397 R-306
- 复杂度: 中
- 复现: .kanzei/quarantine/ 现存 121 个目录共 143MB(shell-with-log 82、cross-tree 32、bg 7),最早 2026-08-16,无任何回收路径。抽查 cross-tree-1787021081327:内容是 crates/kanzei-app/gen/schemas/desktop-schema.json 与 windows-schema.json——Tauri 构建产物,任一线跑构建即重生成,被越界检测当跨树写入取证;cross-tree-1787060772922 存的是 p16 线自己 R-299 B1 提交(7188ba76)的前置内容,合法自身工作被判越界。08-16 单日 30 次 cross-tree 隔离(两线并行互撞日)
- 影响: 误报占绝大多数,真越界信号被淹没;143MB 取证快照只进不出;构建产物类路径每次并行构建都会再触发
- 标签: 流程
- 验收: ①构建产物路径(gen/schemas 等)进入越界检测豁免清单或按内容指纹放行,有定向测试;②quarantine 提供按日期/类型的清理入口(dry-run+实际释放量),真越界证据可显式保留;③清理后 121 个存量目录处置留痕;④真实并行双线构建实测不再产生 schema 误报
- 优先级: P2
- 进展: 2026-08-20 存量处置完成:121 目录 143MB 经用户放行后清空,清单存证 scratchpad/quarantine-manifest-20260820.txt;误报最大来源已随 R-306 B1 修复(collect_tree_metadata 补 D-407 排除清单)。剩余验收:豁免/清理入口代码化(①②④)与真实并行双线实测
- observed_head: 080db353cc33509398d0746987dccf2b703fe0b1
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787174556151

## D-567 记忆 inbox 消化 10 进 0 出:manager run made no inbox progress,96 条积压 [open] (high)
- refs: D-409
- 复杂度: 中
- 复现: .kanzei/memory/inbox.checkpoint.json(updated_at_ms=1787169243689,2026-08-20 03:54):batch_id=inbox-1787169204456,status=failed,input_notes=10,success_notes=0,failure_reason=manager run made no inbox progress,pending_after=96。inbox.md 现存 96 个 note 块(106KB,08-18~08-19 产生,79% 为 bash 指纹)
- 影响: inbox 完全不消化,积压只增不减;D-409 修的是整箱塞爆(分批已生效,本次确实只喂 10 条),这次是 manager 消化端零产出,属新故障模式;记忆写入管道断裂,新知识无法晋升
- 标签: 后端
- 验收: ①定位 manager run 零进展根因(模型调用失败/discard 销账失败/门禁拒绝)并修复,失败原因可观测不再只有一句 no progress;②真实重跑一批消化,success_notes>0 且 pending 下降;③96 条积压清空或按同指纹聚类批量处置留痕;④连败告警:连续 N 批 status=failed 主动上报而非静默重试
- 优先级: P1

## D-568 记忆 INDEX 描述串号污染:M-014/M-015 描述抄错条目,毒化 FTS 检索 [open] (medium)
- 复杂度: 小
- 复现: .kanzei/memory/INDEX.md:M-014 标题「HTML 静态文案必须登记进资源表」但描述整段是 M-009 的「edit 报 old_string not found 时必读…」;M-015 标题「SSE 流内 context overflow」描述却是 M-029 的「处理 bash git 拦截…结构化工具显式 stage」。index.db 的 memory_fts 索引 description 字段,错配描述使这两条在错误查询下被召回
- 影响: FTS 检索被毒化:错误主题命中错误记忆;INDEX 是每会话注入的真源,串号直接影响召回质量
- 标签: 后端
- 验收: ①M-014/M-015 描述修正与源文件 description 一致;②全量 INDEX 行与对应 M-*.md 的 description 做一次机械一致性核对,输出不一致清单并修复;③重建 index.db FTS 后检索抽查不再串号;④INDEX 生成/更新路径补一致性断言防复发
- 优先级: P2

## D-569 tracker 完整性退化复发(D-331 同形态):归档标题双状态标记与非法 severity 再现 [open] (high)
- refs: D-331 D-553 D-554 D-555
- 复杂度: 中
- 复现: defects-archive.md:6822 的 D-553 标题行为「... [open] (small) [fixed]」——同行两个状态标记且 (small) 非法(合法 severity 为 high/medium/low);D-554 同样「[done] (small) [fixed]」;defects-archive.md:6829 引擎在取活依据字段写入「[tracker integrity degraded] D-555: invalid defect lifecycle [done]」,该告警全库出现 4 次。D-331(已 fixed,high)当时修的正是「归档终态无法安全修正且非法状态污染缺陷标题」,b140322 加了跨 DocKind 状态标记校验+fix_terminal,08-20 同形态再现说明校验有漏洞或写入方绕过
- 影响: 归档数据被污染且按 D-331 教训会扩散:完整性门禁降级告警混入取活依据字段,畸形条目无法被 list/get 正确解析;M-012 类机制恶化可能拒绝所有 tracker 写操作
- 标签: 核心
- 验收: ①定位本次畸形写入的具体路径(哪个写入方绕过了 b140322 校验)并封堵;②用 fix_terminal/repair 修正 D-553/D-554/D-555 存量畸形行,integrity 告警清零;③D-331 的回归测试补上本次形态(双状态标记+非法 severity+污染取活依据);④复发计数落档:同形态第 2 次,若再现第 3 次升级为门禁硬拒
- 优先级: P1
- 对账: 2026-08-20 勘察补充:同类完整性脏数据另见 requirements-archive.md 的 R-221 条目——标题 [done] 但残留字段「- 状态: todo」,修复时一并纳入存量清理与回归形态

## D-570 research 上下文注入读 flat 路径而写入走 topic 路径,B2 后新增 S-/F- 对 agent 不可见 [open] (high)
- refs: R-221 R-248 R-277
- 复杂度: 小
- 复现: crates/kanzei-tools/src/profiles/research.rs:199-246 的 index_of 用 DocStore::open(&ctx.project_root, kind) 读死 .kanzei/research/sources.md 平铺路径;而 tracker.rs:316-329(R-221 B2)强制 source/finding 写入 .kanzei/research/<topic>/。结果 research agent 每轮 <research-docs> 索引只看到 2026-08-16 那批 flat 遗留 19 S/11 F,B2 之后 topic 目录里新写的条目永不出现
- 影响: 「引用在收集时绑定」被釜底抽薪:研究员看不见自己刚收集的来源;因 08-17 后 research 零使用一直未被撞见,R-248 prior-art 上线后必触发
- 标签: 后端
- 验收: ①index_of 按当前 topic 聚合注入(topic+flat 遗留两段均可见);②新写 S-/F- 在同会话下一轮注入中可见的定向测试;③r221-chain 等存量 topic 目录条目回读验证
- 优先级: P2

## D-571 websearch 端点本机不可达且无降级提示,轮次预算对直调不设防 [open] (medium)
- refs: R-277 R-248
- 复杂度: 中
- 复现: ①findings.md F-011(V3 本地实测):DuckDuckGo HTML 端点直连与本地代理均不可达,arXiv API 可用;websearch.rs 无可达性探测、无 arXiv/webfetch 降级提示,首次调用撞 30s 超时。②websearch.rs/webfetch.rs 对 research_loop 零引用:R-277 验收④「原始输出不进主上下文/轮次上限」只约束走 begin_search 的路径,模型绕开 loop 直调 websearch 无机械拦截
- 影响: research 检索主力工具在本机是坏的且失败模式是静默超时;预算旋钮是登记式约束非机械闸
- 标签: 后端
- 验收: ①端点不可达时给明确诊断并指引 webfetch+arXiv 通道(F-011 结论进代码);②websearch/webfetch 纳入 loop 预算或对绕行直调设轮次闸;③真实网络环境下降级路径实测
- 优先级: P3

## D-572 投影真源切换后正常收尾仍新增 conversation.updated 快照 [fixing] (medium)
- refs: R-242 R-243
- 复现: 默认五条 projection gate 已启用时运行一轮无压缩或 CLI 收尾，检查 .kanzei/state.db 仍能看到新的 conversation.updated；桌面端 crates/kanzei-app/src/run/persistence.rs:476-483，CLI crates/kanzei/src/cli/run/finalize.rs:53-58。
- 影响: legacy snapshot 未降为只读，事件投影与快照继续双写，无法核销 R-242 验收⑦；后续恢复可能误把新快照当作模型 prior。
- 来源: self-found during R-242 acceptance-⑦ reconciliation
- 标签: 核心
- 优先级: P1
- 进展: 已修复待提交：桌面正常收尾删除 `conversation.updated` 写入，见 `crates/kanzei-app/src/run/persistence.rs:450-482`；CLI 正常收尾删除快照写入，见 `crates/kanzei/src/cli/run/finalize.rs:90-101`；mobile 改为 typed user fact，见 `crates/kanzei-app/src/mobile.rs:324-389`；T-1786922726501 全部定向测试通过，生产 grep 仅保留 legacy 读取/测试构造。
- observed_head: 080db353cc33509398d0746987dccf2b703fe0b1
- observed_worktree_hash: fnv1a64:93aecba29cd59060
- recorded_at: 1787176206420

## D-573 CLI 压缩结果在停止 conversation.updated 双写后未进入 compaction surface 事务 [fixing] (high)
- refs: R-242 R-243
- 复现: CLI `kz run` 进入 context overflow/压缩后，运行收尾不再写 `conversation.updated`，但 CLI 没有调用 `append_compaction_transaction`；检查 state.db 的 typed facts 与 compaction_* 事件，压缩后的 surface 未持久化，重启 prior 丢失压缩纪要。
- 影响: 停止 legacy snapshot 双写后，CLI 压缩结果可能只存在进程内 Vec<Message>，重启后恢复到未压缩 typed history或缺失本轮 surface，违反 R-242 验收⑦并造成已发生上下文事实丢失。
- 来源: self-found while updating stale CLI context-overflow tests after R-242 snapshot write removal
- 标签: 核心
- 优先级: P1
- 进展: 已修复待提交：CLI `persist_cli_compaction_surface_if_changed` 比较当前 typed projection 与 summary，压缩差异追加完整 `compaction_started→compaction_summary→surface_replaced→compaction_ended` 事务，见 `crates/kanzei/src/cli/run/finalize.rs:29-68`；context overflow 两条真实集成测试改为从 typed facts + compaction surface 回放，见 `crates/kanzei/tests/integration/context_overflow_recovery.rs:144-166`；T-1786922726501 集成32 passed。
- observed_head: 080db353cc33509398d0746987dccf2b703fe0b1
- observed_worktree_hash: fnv1a64:93aecba29cd59060
- recorded_at: 1787176207042

## D-574 CLI 在写入当前 user fact 后恢复 prior 导致本轮输入重复 [fixing] (medium)
- refs: R-242 D-573
- 复现: CLI `run` 在 `TypedSessionWriter::user_message` 已写入当前输入事实后才调用 `recover_cli_prior`，投影 prior 会包含本轮当前 user message；随后 runner 再追加同一输入，轮末 `session.shadow_compared.equal=false`。
- 影响: CLI runner prior 混入当前轮输入，可能造成用户消息重复、shadow gate 误报，破坏 runner prior 从上一 segment 恢复的语义。
- 来源: self-found from R-242 CLI integration regression after projection migration
- 标签: 核心
- 优先级: P1
- 进展: 已修复待提交：CLI 将 `recover_cli_prior` 移到 `TypedSessionWriter::user_message` 之前，见 `crates/kanzei/src/cli/run.rs:285-302`；拒绝权限回归的 shadow equal=true 已通过，T-1786922726501 全部定向测试通过。
- observed_head: 080db353cc33509398d0746987dccf2b703fe0b1
- observed_worktree_hash: fnv1a64:93aecba29cd59060
- recorded_at: 1787176207648
