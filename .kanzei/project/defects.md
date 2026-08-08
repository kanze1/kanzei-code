# Defects

## D-171 启动黑屏:孤儿 msedgewebview2 进程锁住 WebView2 数据目录 [fixing] (high)
- 复现: 父 kzapp 被强杀(更新交接、任务管理器、崩溃)时 WebView2 子进程存活,继续握着 `dev.kanzei.app/EBWebView` 数据目录的目录锁;下一个实例的 WebView 初始化失败,窗口就是一块黑。实测本机曾积累 6 个存活 7 小时的孤儿 msedgewebview2。
- 根因: 强杀父进程不会自动回收 WebView2 子进程;新实例启动时 WebView 初始化被孤儿进程的目录锁挡住,与 D-172(i18n 死循环)是两个独立的黑屏根因。
- 修复: `cleanup_orphan_webviews()`(crates/kanzei-app/src/main.rs)——只杀命令行带 `dev.kanzei.app` 的 msedgewebview2.exe,且只在**没有其他 kzapp 实例存活**时动手(别的实例在,它的 webview 就不是孤儿);在主流程窗口创建前与安装交接前调用。
- 验收: ①强杀 kzapp 后残留孤儿 webview,重启 kzapp 不再黑屏;②有其他 kzapp 实例存活时不误杀其 webview;③更新交接路径(install helper)也清孤儿,避免新实例黑屏。
- 证据等级: E1(逻辑自洽 + 注释记录实测 6 个孤儿)
- 优先级: P0
- 备注: 2026-08-08 并行环节已写好修复代码(工作区未提交),本条目补登记;此前被误判为编号空洞补过 tombstone,已撤销纠正。

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
- 优先级: P1

## D-174 托管项目后台 Shell 缺少可归因的文件隔离 [open] (high)
- 复现: 后台 Bash 启动后立即返回,后续异步进程可以在任意时刻修改 `.kanzei/project` 与 `.kanzei/memory`;Harness 无法区分后台进程写入和稍后专用工具的合法写入,也无法安全回滚。
- 根因: 现有后台进程注册表只管理 PID、日志和生命周期,没有独立工作目录、文件系统沙箱或按进程归因的写入审计。
- 影响: 若继续允许托管项目中的后台 Bash,受保护文档可能绕过专用工具契约;当前修复选择在存在 `.kanzei` 的项目中拒绝后台 Bash,因此 R-097 的后台启动能力暂时降级。
- 验收: ①后台任务运行在可隔离或可归因的文件系统边界中;②后台任务不能写入 Harness 托管路径;③专用工具的合法写入不会被后台回滚机制误伤;④覆盖启动、轮询、停止、越界写入和并发合法写入测试。
- 优先级: P1
- 关联需求: R-097、R-139

## D-175 安装器只发 kzapp 不发 kz CLI:schema 迁移后旧 CLI 直接打不开 state.db [fixing] (high)
- 复现: 2026-08-08 发布 build-0c9f903(含 store schema v4→v5)。静默装完 setup.exe 后,`kz --version` 仍是 430d6d6(SCHEMA_VERSION=4);一旦启动新 kzapp 把 `.kanzei/state.db` 迁到 v5,旧 kz 在 `SessionStore::open` → `migrate` 处命中 `version > SCHEMA_VERSION` 直接返回 UnsupportedSchema,`kz run` 完全不可用。本次靠手动 `cargo install --path crates/kanzei --force` 救回,安装器缺口未变。
- 根因: 桌面端与 CLI 是两个独立安装通道(NSIS → %LOCALAPPDATA%\kanzei;cargo install → ~\.cargo\bin),却共用同一个 `.kanzei/state.db`;package.ps1 只打包 kzapp,没有任何机制让安装器更新 CLI。以前 CLI 落后只是"旧",引入 schema 迁移后变成硬失败。
- 影响: ①任何 schema 变更发版即弄坏机器上的 kz,而 kz 是自举循环的入口;②迁移单向且此前无备份,回退到上一版 kzapp 同样打不开库,发布事实上不可回滚;③UnsupportedSchema 文案只说"不兼容",不给出路,容易诱导用户删库,而删库丢的是全部会话历史。
- 验收: ①安装包内随附与 kzapp 同一次构建的 kz.exe;②安装后首次启动 kzapp 能把 CLI 同步到 ~\.cargo\bin,且只升不降(开发者手动装的更新版本不被覆盖);③schema 升级前自动留下可打开的整库备份;④UnsupportedSchema 文案给出桌面端与 CLI 各自的升级动作并明确禁止删库;⑤上述均有自动化测试,且发一版真实安装验证 CLI 版本随 kzapp 一起前进。
- 修复进展(2026-08-08): package.ps1 打包前构建 kz 并作为 sidecar 注入(externalBin 经 `--config` 只在打包时生效,避免 tauri-build 在 build script 阶段校验 sidecar 而弄挂所有普通 cargo build);kzapp 启动调用 sync_bundled_cli 同步到 ~\.cargo\bin,标记文件走快路径、只升不降;SessionStore 升级前 `VACUUM INTO` 留 `state.db.v<n>.bak`(WAL 下直接拷 .db 会拿到残缺快照);UnsupportedSchema 改为携带 found/supported 并给出可执行指引。
- 验证(2026-08-08): kanzei-core 51 项、kzapp 33 项、kanzei-tools 82 项、kanzei-harness 38 项通过,含备份一致性、更高版本拒绝打开与文案断言、CLI 同步只升不降。验收⑤(真实安装后 CLI 版本随包前进)待本次发版装完确认,故保持 fixing。
- 优先级: P0
- 标签: 发布

## D-176 同一目录裂成两个会话 id(扩展长度路径前缀),历史与队列互相看不见 [fixing] (high)
- 复现: 本仓库的 state.db 里同一个目录有两条会话:`ses_project_c0b8d633186c2464`(project_root `C:\Users\kanzei\Documents\kanzei code`)与 `ses_project_ce2fce953a5e4103`(project_root `\\?\C:\Users\kanzei\Documents\kanzei code`)。桌面端的运行落在后者(1090 条事件),CLI 落在前者,同一项目的历史互相看不见。
- 根因: 桌面端 `normalized_project_root` 内含 `std::fs::canonicalize`,Windows 上返回带 `\\?\` 扩展长度前缀的路径;CLI 走裸 `discover_project_root` 不做 canonicalize。而 `project_session_id` 只做 `to_lowercase()` 后哈希原字符串,不做任何路径规范化,于是两种写法哈希出两个 id。代码里 5 处 Tauri 命令带着"会话 ID 必须与运行/写入侧同源(D-058)"的注释,说明此坑踩过一次,但当时只靠"都记得调 normalized_project_root"的约定对齐,CLI 侧没跟上——约定而非门禁。
- 影响: ①历史对话在桌面端与 CLI 之间不复用,表现为"历史时有时无";②队列、输入状态、episode 画像同样分裂,跨端度量失真;③会话越多,state.db 里同一项目的孤儿会话线越多。
- 验收: ①同一目录的裸路径、`\\?\` 前缀、大小写差异、末尾分隔符四种写法收敛到同一个会话 id;②`\\?\UNC\` 映射回普通 UNC 写法;③不同目录仍是不同会话;④裸路径形态的身份串保持不变(否则既有会话集体改名、历史失联);⑤有测试锁住上述不变量,而不是继续靠注释提醒。
- 修复进展(2026-08-08): `project_session_id` 改为先经 `session_identity` 规范化——剥 `\\?\` / `\\?\UNC\` 前缀、去末尾分隔符、小写。分隔符刻意不统一:裸路径的哈希必须与历史一致,否则所有既有会话一次性改名。选型由用户定为"向后兼容、不迁移存量"。
- 验证(2026-08-08): kanzei-core 新增「同一目录的各种路径写法收敛到同一个会话id」,含向后兼容的身份串断言(不断言哈希字面量——DefaultHasher 跨 Rust 版本不保证稳定)。真实桌面端确认待发版后进行,故保持 fixing。
- 优先级: P0
- 标签: 后端
- 备注: 采用向后兼容方案后,桌面端会切回裸路径 id,`ce2fce953a5e4103` 那条线的 1090 条事件成为孤儿(数据仍在 state.db 中,未删除)。

## D-177 上下文压缩只在轮末检查,长轮与被停止的运行一次也轮不到 [fixing] (high)
- 复现: 事件流 seq 1073-1076:18:35:55 提升输入并 running,19:17:04 用户停止,`reason=stopped_by_user`,**没有 run.completed**。而压缩检查写在 run.completed 之后那一段(`estimate > limit*7/10` 才调 fast_summarize),整整 41 分钟里检查点执行 0 次。
- 根因: 上下文预算只在一轮**结束之后**评估,而长轮与自动续跑恰恰是最需要它的场景——一轮不结束就一次也轮不到,中途停止更是直接跳过收尾。轮内唯一的上下文管理是 runner 的 `recover_context_overflow`,它只在 provider 已经报 overflow 之后才动,于是实际行为是"一路涨到撞墙,撞了才被动裁剪"。另:轮末估算漏算工具 schema,而 schema 每步整份重发,在工具多的 profile 下是常驻大头。
- 影响: ①长轮的上下文成本不受控,只能靠撞墙兜底,而撞墙那次请求本身已经浪费;②被动裁剪发生在错误路径上,裁剪力度不可选;③用户观感是"跑了一大波压缩从没触发"。
- 验收: ①每步开跑前按 context_limit 主动估算并在到达预算线时就地压缩;②估算把 system、历史与工具 schema 三者都计入;③压缩保留当前用户消息,并把被裁段落沉淀为可核对轨迹;④主动压缩与撞墙后的被动恢复各记各的额度,主动让路不吃掉被动重试;⑤UI 与 CLI 能看见"何时让路、让掉多少";⑥有测试锁住估算口径与压缩效果。
- 修复进展(2026-08-08): RunnerConfig 增 context_limit;每步请求前按 CONTEXT_BUDGET_RATIO=0.7 估算并触发 `compact_messages_for_retry`,上限 3 次;新增 RunEvent::ContextCompacted,桌面端写 run.trace + kz:status,CLI 打印一行;estimate_prompt_tokens 计入工具 schema。轮末那次压缩保留作兜底。
- 验证(2026-08-08): kanzei-core 新增「上下文估算把工具schema计入并按预算线判定」「主动压缩显著缩小上下文且保留当前用户消息」;workspace 259 项通过。真实长轮触发待发版后观察 run.trace 的 context.compacted 记录,故保持 fixing。
- 优先级: P0
- 标签: 核心
- refs: D-176
