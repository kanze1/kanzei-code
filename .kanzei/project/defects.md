# Defects

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

## D-187 KANZEI_HOME 只有 memory 认,配置与 markdown 组件仍走真实 HOME [open] (medium)
- 复现: `crates/kanzei-tools/src/memory/mod.rs` 读 `KANZEI_HOME` 决定记忆根;而 `crates/kanzei-harness/src/config.rs`(全局 kanzei.toml)与 `crates/kanzei-harness/src/markdown.rs`(agents/commands/skills)直接用 `dirs::home_dir()`。设了这个变量之后,记忆搬走了、配置与组件还在真 HOME。
- 影响: 半个覆盖比不覆盖更容易骗人——用它做隔离测试或多实例并存时,会以为整个 kanzei 目录都换了位置,实际只换了记忆;两处根不一致导致的现象很难归因。
- 根因: KANZEI_HOME 是记忆模块单独引入的,没有提升为全局 home 解析入口。
- 验收: 要么全局统一(所有 `~/.kanzei` 消费点走同一个 `kanzei_home()` 函数,含 config/markdown/app.json/agent-containers),要么去掉 KANZEI_HOME 只保留 memory 内部用途并改名;不留"只覆盖一半"的状态。有测试覆盖。
- 证据等级: E2
- 优先级: P3
- 标签: 核心

## D-188 单元测试探针写进生产更新日志,稀释 D-182 的诊断入口 [open] (low)
- 复现: `%TEMP%\kanzei-update.log` 当前全部内容是 5 条"单测探针",时间 2026-08-08 23:08 与 2026-08-09 00:28——测试与生产用同一个绝对路径(crates/kanzei-app/src/main.rs:1367 附近的 update_log 测试)。
- 影响: `update_log` 超 256 KiB 是整文件删,测试写入会稀释乃至挤掉真实的更新交接记录;而这个日志正是 D-182 为"更新过程无从复盘"专门建的入口,现在打开看到的全是测试噪声。
- 根因: 日志路径写死为 `%TEMP%\kanzei-update.log`,测试没有走可注入的路径参数。
- 验收: 测试写到独立临时文件(路径可注入或按 pid 隔离),生产日志里不再出现"单测探针";补一条断言防回归。
- 证据等级: E1(本机日志内容实证)
- 优先级: P3
- 标签: 后端

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

## D-178 git 工具 stage 静默失败:normalize_resource Windows 小写化破坏大小写敏感路径 [fixing] (high)
- 复现: git stage .kanzei/memory/INDEX.md 返回 "nothing is staged after this request"。根因: git.rs:148 normalize_files 用 kanzei_harness::permission::normalize_resource(raw) 规范化路径, 该函数在 Windows 上 to_lowercase 整个路径(permission.rs:167-168), git pathspec 大小写敏感, 转小写后匹配不到磁盘上的 INDEX.md/M-016-*.md/Cargo.lock 等含大写字母的文件, git add 成功但零暂存, stage 报 nothing staged。对照: probe-test.txt(全小写) 可正常暂存。
- 影响: 任何含大写字母的路径(INDEX.md、M-016/M-017 记忆文件、Cargo.lock)都无法通过 git 工具暂存, memory 文件提交被卡; 用户直接用 bash git add 不受影响。
- 验收: git stage 对含大写字母路径能正常暂存并返回 staged_hash; 保留 normalize_resource 的安全校验(逃逸/目录检查)但传给 git 的路径保持原始大小写; 补大小写路径回归测试。
- 严重程度: high
- 优先级: P2
- severity: high

## D-179 停止运行时 abort 先于收尾,整轮轨迹与 episode 全部丢失 [fixing] (high)
- 复现: 2026-08-08 一次 41 分钟的运行(事件流 seq 1073-1076)被用户停止后,该会话只留下一条 `session.status_changed {"reason":"stopped_by_user"}`——没有 run.trace、没有 episode、输入状态也没有结局。对照正常结束的轮次(seq 1084-1086)三者齐全。
- 根因: `stop_runtime_and_finalize`(crates/kanzei-app/src/main.rs)先 `handle.abort()` 再收尾,而写 run.trace / append_episode / finish_input 的代码全在被 abort 的那个 task 里,先杀后写等于什么都不写。失败轮次同理:`let summary = run_result?;` 在写轨迹之前提前返回,`run.failed` 之外一样什么都不留。
- 影响: ①最值得复盘的运行(长到不得不停)恰恰一个字节都不留,D-173 补的运行审计在这类轮次上等于没做;②工具耗时、权限决策、token 统计全丢,"时间花在哪"仍然只能靠猜;③D-177 的轮内压缩是否真的触发,在被停止的轮次里无法验证。
- 验收: ①停止时先把在飞轨迹与 episode 落库再 abort;②失败轮次同样落库;③episode 的步数与 token 取自逐步累计的真实值,不是补零;④正常收尾与停止路径不重复写(幂等);⑤有测试锁住"停止后 run.trace 与 episode 都在,且再停一次不产生第二条"。
- 修复进展(2026-08-08): SessionRuntime 增 `live: Arc<Mutex<LiveRun>>` 在飞画像(run_id/input_id/provider/model/步数/token/轨迹),挂在 runtime 上而不是 run_task 局部,停止路径才够得着;`flush_live_run` 幂等落库,停止路径在 abort **之前**调用,失败分支也调用;TurnStart/StepEnd 逐步累计步数与 token。
- 验证(2026-08-08): kzapp 新增「停止时在飞轨迹与episode先落库再abort」,断言 outcome=halted、步数取真实值、归属列齐全、重复停止不产生第二条 episode;kzapp 34 项通过。真实桌面端停止验证待发版后进行,故保持 fixing。
- 优先级: P0
- 标签: 后端
- refs: D-173、D-177

## D-180 v5 之前遗留的 promoted 输入未回填,仍会被后续停止追认为 cancelled [fixing] (high)
- 复现: 装上 v5 后查本机 state.db:`promoted 195 / cancelled 187 / running 1`。那 195 条是 v5 之前跑完的输入——当时没有 completed 终态,它们永远停在 promoted。而 `finalize_interrupt` 取消 `pending/promoted/running`,所以用户下一次按停止,这 195 条历史上早已跑完的输入仍会被一并改写成 cancelled。
- 根因: v5 只加了新状态与新写入路径,没有回填存量。新记录不再被污染,存量却仍在被反复追认——修了一半。
- 影响: 历史输入的状态位不可信,按状态做的任何统计(完成率、取消率)都失真;且每停止一次就再污染一次,不是一次性损失。
- 验收: ①迁移把存量 promoted 回填为 completed;②保护窗内(可能正被另一个进程执行)的 promoted 不回填;③promoted_at 缺失的老记录同样视为存量;④回填后再停止,已回填的记录不再被改写;⑤有测试锁住上述四条。
- 修复进展(2026-08-08): SCHEMA_VERSION 提到 6,迁移中回填 `promoted → completed`,保护窗 5 分钟(桌面端与 CLI 共用同一个库,可能有另一进程正在执行)。completed 是**迁移推断值**不是观测值:v5 之前根本没有记录结局的地方,只能按"被提升了就说明当时确实执行过"判定,已在代码注释与本条目写明。
- 验证(2026-08-08): kanzei-core 新增「迁移把遗留promoted回填为completed但不动可能在飞的输入」,含回填后再停止不被改写的断言;store 24 项通过。
- 优先级: P1
- 标签: 后端
- refs: D-173
- 备注: 回填口径(推断为 completed 而非保持现状)由用户 2026-08-08 定调。
- 续修(2026-08-08): v6 回填在真实机器上扑空——22:03 与 22:37 两次停止已把存量 promoted 全抹成 cancelled(384 条),v6 上线时一条 promoted 都不剩。唯一还留着原始状态的是 `state.db.v4.bak`(promoted 196 / cancelled 185)。经用户确认"捞",新增 v7:迁移前 ATTACH 同目录的 `state.db.v*.bak`,把**备份里是 promoted、现库是 cancelled**的输入恢复为 completed。判定以备份为权威而非猜测:备份里已是 cancelled 的是当年真取消,不动;备份里根本没有的(如 21:40 那条真被停掉的)更不动。恢复条数写入 schema_meta.legacy_inputs_recovered 供事后核对。
- 验证(2026-08-08): kanzei-core 新增「v7从备份恢复被抹掉的输入状态位且不误伤真取消」「v7在没有备份时安静通过」,含幂等性断言;workspace 269 项通过。
- 迁移与回滚: v5→v6 只有一条 UPDATE,无表结构变更;回滚把 SCHEMA_VERSION 改回 5 即可,已回填的 completed 对 v5 代码是合法值(v5 的 CHECK 已含 completed),不会打不开库。

## D-181 主动上下文压缩复用应急截断:一次砍掉 97% 且保留的是最旧内容 [fixing] (high)
- 复现: D-177 把主动预算线接到了应急函数 `compact_messages_for_retry` 上。该函数把除当前用户消息外的全部历史拍成一个 8000 字节文本块:deepseek 128k 的预算线是 89,600 token,触发后掉到约 2,000 token,一次砍掉 97%。且其累积循环从 index 0 正序、攒够即停,保留的是开场白,丢掉的是刚做完的工作。
- 根因: 应急路径与主动路径的定位被混为一谈。应急发生在 provider 已经拒绝请求之后,粗暴但必须一次成功,合理;主动发生在还有三成余量、也有时间的时候,没有任何理由推倒重来。另有两个附带缺陷:①`remaining = 8_000 - history.len()` 按字节算却用 `chars().take(remaining)` 取字符,中文实际超额约三倍,那个上限名不副实;②`Part::ToolCall` 被整个 skip,只留下工具输出而不知道是哪个工具、什么入参产生的。
- 影响: ①压完模型不知道自己刚做了什么,长轮大概率原地重做,压缩反而放大成本;②轮末那条像样的 `fast_summarize` 已确认长轮轮不到,于是形成"好的不跑、跑的不好";③`MAX_PROACTIVE_COMPACTIONS=3` 是假的——第一次就压成一个块,后两次只是重复截同一个块。
- 验收: ①主动压缩保住首条用户消息(任务定义)与最近工作区逐字不动,只压中段;②中段交 fast 模型出结构化纪要,要求写出具体文件/函数/标识符而非泛化;③纪要不可用时回落到截断,但只截中段;④中段为空时不计入压缩次数,不吃重试额度;⑤应急路径改为保留最近内容而非最旧;⑥按字符截断,中文不超额;⑦保留内容含工具名与关键入参;⑧有测试锁住上述各条。
- 修复进展(2026-08-08): 新增 `compact_with_digest`(三段式:head 逐字 / 中段纪要 / 近期 RECENT_VERBATIM_RATIO=0.35 逐字),抽走中段后用 `filter_message_history` 清孤儿工具部件;`digest_segment` 走 SubagentRuntime.fast 那条 route,失败回落只截中段;`clip` 统一按字符截断;应急路径改为从最近往回收并纳入 ToolCall。fast 模型调用由用户 2026-08-08 明确批准。
- 验证(2026-08-08): kanzei-core 新增「主动压缩保住任务定义与近期工作并只压中段」「应急压缩保留最近内容而非最旧内容」「clip按字符截断且中文不超额」;workspace 266 项通过。**纪要质量未经真实模型验证**——测试里 subagent=None 走的是截断回落,而 fast 是本地小模型 ollama:qwen3.5:4b,能否保住标识符待实测,故保持 fixing。
- 优先级: P0
- 标签: 核心
- refs: D-177

## D-182 应用内更新静默失败:交接 helper 就是安装器要替换的 kzapp.exe,镜像被锁 [fixing] (high)
- 复现: 2026-08-08 22:43 用户点设置页「检查更新」升 build-ea6d058。安装包完整落到 `%TEMP%\kanzei-setup.exe`(9,564,216 字节),但 `%LOCALAPPDATA%\kanzei` 三个文件的时间戳仍停在 21:35,一个都没换,界面上也没有任何失败提示。
- 排除项: ①安装包本身没问题——同一个文件指定目录能完整装出 kzapp+kz(exit 0);②下载没问题——字节数与 release 资产一致;③不是权限问题——kzapp 未运行时同一个包装得进去。
- 根因: `update_install` 用 `Command::new(current_exe())` 起交接 helper,而 `current_exe()` 就是安装器要替换的 `%LOCALAPPDATA%\kanzei\kzapp.exe`。父进程 `app.exit(0)` 之后 helper 仍在跑同一个镜像文件,Windows 全程锁着它,NSIS 覆盖不了。`run_install_helper` 里安装器非 0 退出就 `return` 且**不删安装包**——TEMP 里那个文件还在,与这条路径吻合。
- 诊断为何困难: helper 只用 `eprintln!` 报错,而 GUI 进程没有可见 stderr,失败原因无处可查。这是本缺陷真正卡住排查的地方,与根因同等重要。
- 影响: 应用内更新整条通道失效,只能靠手工静默安装;且失败无声,用户以为装上了,实际仍在旧版——本次正是如此。
- 验收: ①helper 跑安装目录之外的副本,安装目录内无任何被本进程锁住的文件;②helper 名字不同于 kzapp.exe(避免被安装器的关闭运行实例逻辑连带处理);③交接全过程写入 `%TEMP%\kanzei-update.log`,含父进程退出与否、安装器退出码与 stdout/stderr、安装后 exe 时间戳、拉起结果;④安装器报 exit=0 也要回读 exe 时间戳核对,不能只信退出码;⑤helper 副本由下次启动回收;⑥有测试锁住 helper 落点与日志落盘。
- 修复进展(2026-08-08): 新增 `update_helper_path()`(`%TEMP%\kanzei-update-helper.exe`),`update_install` 先复制再起该副本;`run_install_helper` 改用 `output()` 捕获安装器 stdout/stderr,全程 `update_log` 落盘,并在安装后回读 exe mtime;`startup_update` 顺手回收残留的 helper 副本。
- 验证(2026-08-08): kzapp 新增「更新交接helper跑在安装目录之外」,断言落点在 TEMP、名字不叫 kzapp.exe、日志真的落盘;kzapp 35 项通过。真实「检查更新」验证待本版发出后由用户执行,故保持 fixing。
- 优先级: P0
- 标签: 发布
- refs: D-175
- 备注: 排查期间我用 `/D=<临时目录>` 做探针,误以为它只影响本次安装;NSIS 会把该路径写成新的 InstallLocation,导致随后一次 `/S` 装到了临时目录、看起来像"exit 0 却什么都没换"。已用 `/D=%LOCALAPPDATA%\kanzei` 装回并核对注册表。该探针也毁掉了 22:43 当时注册表值这条证据。

## D-183 发版不核对提交区间:并发运行的提交被夹带发布且无人察觉 [fixing] (high)
- 复现: 2026-08-08 发布 build-ea6d058 与 build-96acfdf 时,`49634b7..HEAD` 区间里含 f73ae6c(21:41)与 5223dc6(21:52)两个并发自举运行的提交。发布流程照常 `merge --ff-only` + `package.ps1 -Publish`,全程无任何提示,发布说明里也只是把它们混在变更列表里,没人核对过。事后靠用户追问才发现。
- 根因: 发布流程只认 HEAD,不看区间。本仓库有并发自举运行提交到同一分支,而它的提交作者、邮箱与人手动提交完全一致,git 元数据分辨不出来——没有任何自动信号,只能靠发布者主动核对,而"靠记得"不是门禁。
- 影响: ①发出去的内容可能包含未经审阅的改动,发布说明失真;②本次两个提交只碰 `.kanzei/` 文档没动 crates/,二进制未受影响,但同一条路径下一次就可能夹带源码;③与 D-173 系列建立的"提交范围可核对"原则自相矛盾——工具层拦住了提交夹带,发布层却没有对应门禁。
- 验收: ①package.ps1 构建前摊开 `<上个 build-* 标签>..HEAD` 的完整提交清单,并标出每条是否触碰源码;②发布者必须用 `-Ack <条数>` 明确确认,不传直接中止、传错也中止;③release notes 与门禁使用同一个区间变量,两处口径不得各算各的;④发布树源码工作区不干净时中止,避免构建产物与标签不对应;⑤门禁本身经过实测(不传 -Ack、传错数各一次)。
- 修复进展(2026-08-08): package.ps1 新增 `-Ack` 参数与区间清单打印([源码]/[文档] 标注)、`$range` 变量供 notes 复用、发布树 crates/scripts/ui 脏工作区检查。
- 验证(2026-08-08): 截取门禁段落在发布树上实跑两次——不传 -Ack 报"核对上面 2 个提交…加 -Ack 2 重跑";传 -Ack 99 报"你确认的是 99 个提交,实际区间里有 2 个"。两条路径均按预期中止。
- 优先级: P1
- 标签: 发布
- refs: D-175
- 备注: 这是流程缺陷不是代码缺陷,由本次发布者(AI 助手)在自检中发现并主动登记;此前两次发布已既成事实,不追溯撤回。

