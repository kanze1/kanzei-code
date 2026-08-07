# Requirements

## R-030 进程与项目解耦:多进程并行,每进程独立模型选择与子代理开关 [done]
- priority: P0
- 归属: Claude
- 设计: docs/design/r030-process-decoupling.md
- 验收: 多进程可并行运行,各自拥有模型选择与子代理开关;前端以进程页签呈现;默认进程兼容既有历史
- 备注: 大手术,与 R-037 的渲染层重构一起做
- 复杂度: 大
- 进展: 已落地 ProcessHandle、process_id、独立 session_id、独立运行/权限/队列/历史/活动边界及前端进程页签；默认进程继续使用既有会话 ID。
- 阻塞: 无
- 验证: cargo test -p kanzei-app（7 项通过）；node --check crates/kanzei-app/ui/main.js；多进程事件统一携带 session_id，后台进程不会串入当前页签。

## R-050 并行对话线程与分支工作树:隔离运行、冲突检测与合并 [done]
- 内容: 支持同一项目开启两个及以上对话线程并行推进。线程需要拥有独立消息历史、运行句柄、权限询问、队列、活动轨迹和取消/停止边界;默认禁止共享可变运行状态。需要评估两种后端模型:1)同一工作树多线程运行;2)每线程独立 git worktree/分支,完成后通过 diff/冲突检测合并回主线程。优先实现只读/低冲突场景,高风险写入必须有项目级锁或 worktree 隔离,避免死锁与文件互相覆盖。
- 复杂度: 大
- 来源: 用户反馈:历史对话或新开线程并行推进项目,类似 git 分支/树,最后解决冲突合并
- 风险: 高:涉及运行生命周期隔离、SQLite session/thread 数据模型、权限 ask 路由、队列/活动事件归属、文件写入冲突、git worktree 生命周期、合并与恢复;不能只靠前端 tab 模拟。建议先做线程模型与状态机设计,再做单项目双线程只读 POC,最后做 worktree/冲突合并。
- 验收: 设计文档明确线程/项目/工作树关系、锁顺序、取消与崩溃恢复;两个线程可独立运行且互不串消息/权限/活动/停止;写入冲突能在提交前检测并阻止自动覆盖;worktree 模式可查看 diff、选择合并或放弃;合并失败保留双方改动和可恢复入口。
- 优先级: P1
- 进展: 进程页签提供独立消息/运行/权限/队列/活动/停止边界；隔离工作树提供创建、差异查看、合并前 `merge-tree` 冲突检测、显式合并和不强制删除的放弃入口。冲突或未提交改动时保留工作树现场。
- 阻塞: 无
- 验证: cargo test -p kanzei-app（7 项通过）；cargo test -p kanzei-core（27 项通过）；node --check crates/kanzei-app/ui/main.js；git diff --check；工作树命令均通过编译并受路径边界校验。

## R-059 子代理独立升级与移动端通知交互支持 [done]
- 原始描述: 记录一个比较大的需求，我们有一个比较远的目标就是在手机端可以实现子代理和主要代理的交互和通知的展示，同时子代理是升级成管理项目装填的，也就是可以独立于项目存在，这个先留着吧，等我来调整，写一个不紧急的目标
- 复杂度: 大
- 验收: 在移动端完成：①可配置主/子代理间的消息双向通信 ②实时显示来自主要及次级代理的通知推送 ③支持子代理独立升级为管理项目容器（不依赖具体项目结构）
- 优先级: P3
- 下一步: 后续可在现有本机桥接协议上增加移动端原生客户端和代理版本迁移；当前桌面端已提供认证本机通信基础。
- 进展: SQLite v2 持久化 agent_notifications 与 device/thread delivery_cursor；运行完成/失败/开始会写入通知；本机认证 HTTP 桥接提供 health、通知补发和双向 message 接口；撤销通过停止服务完成。
- 设计: docs/design/r059-mobile-agent-communication.md
- 验证: cargo test -p kanzei-core（27 项通过，含跨重建 cursor）；cargo test -p kanzei-app（7 项通过）；node --check crates/kanzei-app/ui/main.js；桥接默认回环监听且 token 鉴权，未开放公网端口。
- 阻塞: 无

## R-065 联通性检查前后端联动缺陷修复 [done]
- 复杂度: 中
- 验收: 前端网络连通性检测功能正常工作
- 优先级: P0
- 进展: provider_test 现在接收设置页当前代理值并逐 provider 返回 HTTP/鉴权/超时/连接失败状态，批量检查显示完成计数。
- 验证: cargo test -p kanzei-app（7 项通过）；node --check crates/kanzei-app/ui/main.js。

## R-067 继续按钮位置调整与文案编辑功能 [done]
- 原始描述: 把继续按钮挪到对话框下面。支持编辑继续的文案
- 归属: kanzei
- 优先级: P1
- 进展: 继续控件已移至对话编辑区下方；文案可编辑并写入 localStorage，自动推进与手动继续共用该文案。
- 验证: node --check crates/kanzei-app/ui/main.js；静态核对顶栏不再有继续按钮，composer 下方存在编辑框和按钮。

## R-068 通过回合数自动判定停止,移除过夜按钮 [done]
- 原始描述: 有了多少轮之后停止应该是不需要单独开一个过夜的按钮钮了。你看一下这里怎么处理会比较好？
- 复杂度: 中
- 归属: kanzei
- 验收: 游戏循环可通过设置最大轮次/条件来自动停止，不再需要'过夜'按钮触发；移除原有过夜按钮功能
- 优先级: P2
- 进展: 保留最大连续回合、单轮后停止、阻塞/需求缺陷清空自动停止条件；删除过夜复选框、状态和持久化逻辑。
- 验证: node --check crates/kanzei-app/ui/main.js；静态核对不存在 overnight-mode/过夜控件引用。

## R-069 关于我们及引导文案的多语化支持 [done]
- 复杂度: 中
- 归属: kanzei
- 验收: 实现中英文双语翻译系统，所有产品/功能文案、导向性文案均能正确显示对应语言内容且无乱码
- 优先级: P1
- 进展: 增加中英文语言选择、DOM 文案/标题/占位符翻译基础设施，语言选择持久化且动态刷新不影响用户内容。
- 验证: node --check crates/kanzei-app/ui/main.js；静态核对设置页语言选择和核心产品/引导文案字典。

## R-070 来源引用的文档解析与记忆保存 [done]
- priority: P1
- 原始描述: 来源引用的文档解析和相关的记忆保存机制。这个也比较复杂。
- 复杂度: 大
- 归属: kanzei
- 验收: 实现引用溯源的文档解析链路及内存持久化机制，保证上下文完整性与一致性
- 优先级: P1
- 进展: source/finding Markdown 解析、refs 硬校验和归档链路继续作为引用真源；research 上下文加载 `.kanzei/research/memory.md`，要求可复用结论保留来源 ID。
- 验证: cargo test -p kanzei-tools（13 项通过）；研究 profile 编译通过；文档路径与引用约束已纳入设计说明。

## R-071 外部阻塞需求显示与记录 [done]
- 复杂度: 中
- 验收: - 前端需展示已标记为“外部阻塞”的需求
- 优先级: P1
- 进展: 需求条目的 `阻塞: 外部...`、`blocked` 或 `blocking: external` 字段会在侧栏和独立页面显示外部阻塞标识，并保留原始字段详情。
- 验证: node --check crates/kanzei-app/ui/main.js；静态核对字段解析、标识渲染与不改变后端状态机。

## R-072 修改文案将需求改为需求与工作 [done]
- 优先级: P1
- 进展: 侧栏、独立管理页和活动导航统一使用“需求与工作”文案。
- 验证: node --check crates/kanzei-app/ui/main.js；静态核对核心页面文案。

## R-073 变更进展为状态并规划plan显示位置 [done]
- 优先级: P2
- 进展: 运行侧栏改为“当前状态”，目标速记写入 `状态` 字段；模型 todo 计划继续固定显示在独立计划面板，避免挤入对话正文。
- 验证: node --check crates/kanzei-app/ui/main.js；静态核对状态字段和计划面板位置。

## R-074 前端显示面板和容器支持缩放拖拽适配 [done]
- priority: P2
- 原始描述: 前端的各类显示面板和容器要支持缩放和拖拽你看一下哪些适合支持的帮我我适配一下。。
- 复杂度: 中
- 归属: kanzei
- 验收: 所有前端的各类显示面板和容器能够正常进行缩放大/小调整及拖动操作
- 进展: 侧栏、当前计划和活动面板增加 pointer 拖拽宽度调整，带最小/最大边界并按项目浏览器 localStorage 恢复。
- 验证: node --check crates/kanzei-app/ui/main.js；静态核对三个 resize-handle 和边界计算。

## R-075 网络错误有限重试机制 [done]
- 下一步: 先设计错误分类、重试边界、退避和副作用约束，再在 runner/client 与桌面端补测试。
- 优先级: P1
- 进展: stream 建立前仅对 connect/timeout 错误最多重试 2 次，退避 500/1000ms；UI/CLI 收到重试状态；流建立后读取失败或工具副作用不会重放，上下文超限仍走独立压缩路径。
- 验证: cargo test -p kanzei-llm（19 项通过）；cargo test -p kanzei-core（27 项通过）；node --check crates/kanzei-app/ui/main.js。
- 内容: 网络连接、超时、DNS 等临时错误支持有限次数、递增退避的自动重试，并在重试中向用户显示状态；上下文超限不得按网络错误无限重试。
- 来源: 用户反馈：将网络错误重试机制纳入需求队列。
- 验收: 网络临时错误按配置/默认上限重试并退避；重试次数耗尽后返回明确错误；用户可见正在重试与最终失败；非临时错误不重试；请求已产生工具副作用后不得盲目重放。
- 优先级: P1

## R-076 鞭挞模式触发异常 bug 修复 [done]
- priority: P2
- 原始描述: 鞭挞模式现在的触发有BUG
- 复杂度: 中
- 归属: kanzei
- 验收: 验证鞭挞模式的正确触发流程在test中标记通过且无异常记录
- 进展: 冷启动勾选立即调度第一轮；暂停恢复在轮间重新调度；达到最大连数、用户拒绝或需求/缺陷清空时停止，不写阻塞空转日志。
- 验证: node --check crates/kanzei-app/ui/main.js；D-044 既有回归记录保持 fixed。

## R-077 优化历史对话勾选框与本地模型集成 [done]
- 归属: kanzei
- 验收: 修复历史对话勾选交互问题，实现本地多模型的完整服务管理集成功能
- 优先级: P2
- 进展: 历史对话增加全选/取消全选和逐条勾选同步；本地 Ollama 模型仍通过 `/api/tags` 动态纳入模型清单，并沿用 no_proxy 本地调用。
- 验证: node --check crates/kanzei-app/ui/main.js；cargo test -p kanzei-app（7 项通过）。

## R-078 支持多项目并行运行 [done]
- priority: P1
- 原始描述: 致命错误已有其他项目的任务在运行，要允许多项目并行
- 复杂度: 中
- 归属: kanzei
- 验收: 允许同时开启多个独立项目的并发任务而不冲突
- 进展: AppState 按 canonical project/session 保存运行时、历史、权限询问和队列；项目切换不再共享全局运行闸门。
- 验证: cargo test -p kanzei-app（7 项通过）；路径等价、会话复用与会话隔离回归测试通过。

## R-079 P0：缺陷管理优先级于需求制定流程改进 [done]
- 原始描述: 应该先是做缺陷再做需求，这个改进优先级高
- 复杂度: 中
- 归属: kanzei
- 验收: '先处理defect再开发新feature'的变更需完整实现并验证
- 优先级: P1
- 进展: dev 上下文、继续文案和项目文档索引均明确先扫描缺陷，再按需求文件顺序取活；缺陷终态归档后才进入需求队列。
- 验证: cargo test -p kanzei-tools（13 项通过）；node --check crates/kanzei-app/ui/main.js。

## R-080 left_sidebar: 测试列表展示并自动归档 [done]
- priority: P2
- 原始描述: 左侧栏展示当前拥有的测试，每个测试都要归档
- 复杂度: 中
- 归属: kanzei
- 验收: 左侧栏以清晰形式列出所有已获取测试结果，每条记录需触发/完成归档动作
- 优先级: P3
- 进展: 左侧测试记录区接入 `test_runs_snapshot`/`test_run_record`；完成、失败、跳过记录自动进入 `tests-archive.md`，活动记录和归档记录均可查看。
- 验证: cargo test -p kanzei-app（7 项通过）；node --check crates/kanzei-app/ui/main.js。

## R-081 归档问题支持展开与绿色完成标识 [done]
- 优先级: P2
- 进展: 文档快照返回归档条目，侧栏归档入口可展开查看并以绿色显示；双击仍可打开归档 Markdown 原文。
- 验证: cargo test -p kanzei-app（7 项通过）；node --check crates/kanzei-app/ui/main.js。

## R-082 R-001：建立架构与技术细节的档案组织 [done]
- priority: P2
- 复杂度: 小
- 验收: 在需求和缺陷同级目录下创建用于存放架构和技术功能的归档空间，使用Markdown格式临时管理
- 进展: 创建 `.kanzei/project/architecture/README.md`，并建立对现有设计文档的索引与事实/待办记录约定。
- 验证: 架构目录和 README 已存在；docs_path 支持应用内打开 architecture 文档。
