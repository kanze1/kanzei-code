# 工作目录管理:新建项目、项目状态事实与「模型在等你」

- 身份: live_design
- 状态: 设计基线(2026-09-26 起草并随 ui2/workdir 分支实施,本文描述的是已落地的实现)
- 日期: 2026-09-26
- 上游文档: [model_autonomy_and_harness_intensity.md](model_autonomy_and_harness_intensity.md)(鞭挞判定顺序与模型停机权)、[ui_surface_stack.md](ui_surface_stack.md)(弹层唯一写法)、[ui_color_semantics.md](ui_color_semantics.md)(语义色表)
- 关联需求: 无(用户 2026-09-26 UI2 问题清单第 13 条「工作目录的管理现在也有问题,你看到上下文了吗?」;tracker 条目由自举循环登记)
- 关联缺陷: 无
- 关联决策: 无(本文 §8 记录本轮用户与主会话的拍板)
- 一句话: 新建项目只落了一个空壳、agent 不知道这是空项目也不知道有没有 Git 与工具链、鞭挞看不出模型在等用户、`\\?\` 路径前缀在手动轮与鞭挞轮是两种写法——现在由引擎一处探测「项目状态事实」,同时喂给 agent 上下文与界面;新建项目走对话框(默认建 Git 库);模型在等你时鞭挞停下但保持开着,你回复后照常续跑;路径只有一种写法。

## 1. 现场与根因

用户在桌面上新建了「MD文件保存」让 agent 做一个手机用的 Markdown 上下文库。截图 15:agent 派子代理「勘察现有仓库」,发现是空目录、没有 Flutter,最后反过来问用户「请提供实际应用仓库的路径」;鞭挞先后发了「继续推进」和一条写死 `defects.md` 的推进指令;工具行摘要是「退出码 0 · state.db-wal」。截图 16 的活动面板还有四条失败:`conventions get` 报文件不存在、`git status` 报 `rev-parse --show-toplevel failed (exit 128)`、`req update` 的批次字段被拒(`批次: 0/5; B1 核心模型…`)、自主轮里 `choco install flutter` 因需要管理员失败。

| # | 根因 | 位置(修前) |
|---|---|---|
| 1 | 新建项目只建目录与 `.kanzei`:不 `git init`、不写运行时文件的忽略规则、事后也不提示 | `projects_init`、09-sessions `initProject`(两次纯文本输入) |
| 2 | agent 上下文只有 `Environment: OS/cwd/root/shell` 一行,没有空项目 / Git / 技术栈 / 工具链事实 | `base.rs` `core/env` |
| 3 | 工具链缺失没有探测与处置契约;`which` 只按精确文件名查启动时的 PATH | `shell.rs`,dev 提示 |
| 4 | 鞭挞判定看不出模型在等用户(只看工具画像) | harness `AutoRunCtx` |
| 5 | dev 提示说「纯文本回复就是停止鞭挞的信号」,引擎对纯文本轮却先 Nudge 一次 | `dev.rs`,`decide()` |
| 6 | Nudge 文案写死 kanzei 的队列(`defects.md`)与语气(「不要再做可行性判断」) | `nudge_prompt(WorkPriority)` |
| 7 | 鞭挞续跑轮写死 agent `dev`:结伴线勾鞭挞后续跑轮按自主档跑 | 08-compose-runtime `sendAutoToSession` |
| 8 | `\\?\` 前缀从进程身份键漏到运行路径:鞭挞轮的系统提示、bash 工作目录、绝对路径权限规则、取活顺序存储键都与手动轮不同 | `normalized_project_root` 用 `std::fs::canonicalize` |
| 9 | 路径归一有 7 份各自的剥前缀实现,口径不一 | projects / worktree / orchestration / file_checkpoints / session / project_root |
| 10 | 非 Git 项目悄悄失效:改动条直接清空;git 工具对**上级仓库**做 status/stage/commit;并行线要到 `git worktree add` 才失败,确认框还写死 Rust 的 target/ 冷编译 | `git_status`、`ensure_repository`、`createWorktreeLine` |
| 11 | 非 Git 项目的真实进展签名是常量(只剩 tracker 字节会变),连续三轮只写代码会被零产出熔断 | `progress_signature` |
| 12 | bash 摘要把表格最后一格(`state.db-wal`)当成人话 | 05-tool-summary `bashHighlight` |
| 13 | pwsh 按系统代码页(GBK)输出、bash 工具按 UTF-8 解码:中文项目路径在 agent 看到的输出里是 6 个 U+FFFD(库里存的就是乱码,不是显示问题);ANSI 着色码也原样进了上下文 | `bash.rs`、`shell.rs` |

## 2. 项目状态事实(一处探测,两处消费)

`crates/kanzei-tools/src/project_state.rs`:`probe(root) -> ProjectFacts`、`render(&ProjectFacts) -> String`。

- **布局**:`greenfield`(`.kanzei` 与 `.git/.idea/.vscode/.dart_tool/node_modules/target/build/dist/__pycache__/.venv`、`desktop.ini/Thumbs.db/.DS_Store` 之外没有文件)/ `sparse`(≤3 个文件且没有清单)/ `existing`。遍历深度 ≤3、条目 ≤2000。
- **Git 三态**:`repo {branch, has_commits}`(根上有 `.git` 目录或工作树的 `.git` 文件;HEAD/refs/packed-refs 判断有无提交,读 `commondir`)/ `parent {toplevel}`(自己不是仓库,某个上级目录是)/ `none`。**不 spawn git**。
- **技术栈**:根与下一层的清单文件(Cargo.toml→rust、package.json→node(按锁文件定 npm/pnpm/yarn)、pubspec.yaml→flutter、pyproject/requirements/setup.py→python、go.mod→go、pom/gradle→java、sln/csproj→dotnet)。空项目时从活动需求的文字里按词边界匹配关键词推「计划栈」并注明来源条目(`flutter(R-001)`)。
- **工具链**:识别栈 ∪ 计划栈所需的可执行文件加 git,按 **PATH + PATHEXT + 注册表用户/系统 PATH** 查找(`shell::fresh_path` / `find_executable`,`flutter.bat` 能找到);有缺失时列出本机可用的 winget/choco/scoop 与**用户级**安装提示(flutter 没有官方 winget 包,给官方 zip 解压到 `%LOCALAPPDATA%` 或 `git clone -b stable`)。
- **缓存**:按根缓存 30 秒;bash 每跑完一条命令、git init 之后作废(`project_state::invalidate`)。
- **只要 Git 状态时**用 `git_state_of(root)`(只读 `.git`/HEAD/refs,不遍历目录、不找工具链):`git_status`、建线入口的后端闸、非 Git 进展签名都走它,不跑整套探测。

消费:

1. **agent 上下文** `core/project-state`(BaseComponent 注册的轮内刷新源,所有档位)。`<project-state>` 块不超过 600 字符,只依赖**会改变 agent 行为的状态**(布局类别、Git 三态、栈、工具链有无;排序、无时间戳)——状态不变文本逐字节不变,不打断 prompt 缓存。**文件数不进文本**(`layout: 已有工程` / `layout: 几乎为空(无工程清单)——在这里搭工程`),只留在 `ProjectFacts.files` 给界面:这个块放在系统提示末尾、所有消息之前,bash 每跑完一条就作废探测缓存,文本里带文件数的话 agent 每建/删一个文件整段对话的 prompt 缓存都失效(复核 major,单测「加一个文件文本逐字节不变」守着)。空项目写明「就在这个目录搭工程,不要向用户索要『实际仓库路径』」;无 Git 写明哪些功能不可用、需要时用 `git action=init`;上级仓库写明 git 工具拒绝操作上级仓库。工作树线按 cwd 探测(工作树自己的 `.git` 文件与清单)。`core/env` 的路径同样是 simplify 形态。
2. **桌面端** `project_facts` 命令(JSON 即 `ProjectFacts`),横幅、并行线入口与建线确认文案读它;`git_status` 带 `repo: own|none|parent`(+`toplevel`),非独立仓库时不再把上级仓库的分支与改动当成本项目的。`projects_create` / `project_git_init` / `project_facts` 是 async 命令,阻塞工作(git 子进程、目录遍历、PATH × PATHEXT 扫描、注册表)在 `spawn_blocking` 里做——同步命令跑在 Tauri 主线程上,对话框与切项目会卡住 WebView。

## 3. 路径只有一种写法

`crates/kanzei-base/src/path_form.rs`(零依赖):

- `simplify` / `simplify_str` / `canonical` / `canonical_or_simplified`:**可以继续当路径用**的形态,规则同 dunce——只有剥掉前缀后语义不变才剥:总长 >260(UTF-16)、组件 >255、DOS 保留名(含 `con.txt`)、组件以点或空格结尾、非法字符、`.`/`..`、空组件、没有根目录的 `\\?\C:`、`\\?\Volume{…}` 一律原样。比 dunce 多一条:`\\?\UNC\srv\share\…` 在同样条件下化为 `\\srv\share\…`。
- `strip_verbatim`:**比较键**用,无条件剥(原来 7 份实现的公共部分;orchestration、file_checkpoints、session_identity、project_root::dir_key、worktree 的 `git_arg_path`/`worktree_key` 都改为调用它,各自的大小写/分隔符口径不变)。

出口:`normalized_project_root` 改用 `canonical`——它是进程 id(`d|<根>`、`p3|<根>`)、`ProcessInfo.project_dir/origin_project` 与鞭挞轮 `projectDir` 的唯一来源。前端 `sendAutoToSession` 用 `origin_project`(再防一道旧形态),取活顺序键统一经 `workPriorityKeyFor`。bash 子进程的工作目录用 simplify 形态。

**存量迁移(schema v25)**:`processes` 的主键与 `origin_project/project_dir/worktree_path`、`retired_processes`、`file_checkpoints` 的 `tree_root/abs_path/process_id`、`sessions.project_root` 改写成 simplify 形态;主键冲突(同一位置两种写法各一行)保留时间戳较新的一行。`session_id` 本来就由剥前缀后的身份哈希得出,历史不丢。读时兜底:`list_processes` / `list_retired_process_ids` 按「原样 / simplify / verbatim」三种写法一起匹配并把结果归一。

迁移范围里的**另外两份持久化副本**(复核补齐):

- **偏好** `app.json`:`load_prefs` 把 `process_auto_state` 与 `work_priority` 的键同样 simplify。两种写法并存时**保留去前缀的那条、丢掉带前缀的**:只有本版本才写去前缀的键,它一定是较新的写入;带前缀的只可能是升级前的存档或本地 localStorage 旧映射回灌的陈值(起草时写成「带前缀的赢」,复核指出陈值会在下次启动把用户升级后的设置——比如关掉的鞭挞——改回去)。前端本机存的 `kz-process-profile` 与 `kz-process-auto-state` 键同样归一(`normalizeProcessKeyed`,同一口径:新键赢)。
- **研究课题登记表** `~/.kanzei/research-library.json`:`storage_root` 与 `linked_projects` 是 `normalized_project_root` 输出的持久化副本(本机 17 条全是 verbatim 形态)。不迁移的话身份根换形态后 `register` 一条都认不出,第一次进研究空间就把每个课题以新编号再登记一遍、从此列两遍。`edit_library` 读出后先 `simplify_paths`:两个字段改写成 simplify 形态,同一课题(比较键 = `strip_verbatim` + 小写的存储根 + topic;无 topic 的再加 kind,与 `register` 认领口径一致)有两条的合并进编号最小的一条(关联项目取并集),`next_id` 不动;`register` 与 `link_projects` 的去重同样按这个比较键,不再按原串相等。迁移结果与那次编辑一起写回。

## 4. 新建项目与打开已有目录

- **新建项目对话框**(项目卡菜单「新建项目…」、项目总览页头、命令面板同一入口,`#new-project-overlay`,00-surface `openDialog`):名称(校验路径分隔符、保留名、尾点)/ 位置(默认上次用过的位置,否则当前项目的上级目录;「浏览…」调原生文件夹选择)/ 预览「将创建:…」/ ☑ 初始化 Git 仓库(默认勾选)/ 一句话描述(可选,创建后放进输入框当第一条消息的**草稿**,不自动发送)。失败(目标已存在且非空等)留在对话框里说原因。
- **`projects_create {parent, name, gitInit, description}`**:目标存在且除 `.kanzei` 外非空时拒绝并指向「打开文件夹…」;建目录与 `.kanzei`(含先行调研骨架)、写 `.kanzei/.gitignore`;勾了 Git 就 `git init -b main`(旧 git 回退 `init` + `symbolic-ref`),**本机配了 git user.name/email 时再做一次首提交**(`git add -A` + `--allow-empty`,并行线要 HEAD),没配就只建库并在反馈里说明「没有做首次提交——并行线要等第一次提交之后才能用」;git 不在 PATH 上时项目照样建好、反馈给警告。
- **`.kanzei/.gitignore`**(只在缺失时写,不改用户根 .gitignore):`state.db`、`state.db-*`、`state.db.v*.bak`、`*.lock`、`*.tmp`、`.write-log/`、`artifacts/`、`summaries/`、`quarantine/`、`file-annotations.json`、`memory/index.db`、`memory/index.db-*`、`memory/inbox.md`、`memory/inbox.checkpoint.json`、`project/auto-run-alerts.jsonl`。`project/*.md` tracker、`memory/*.md`、`kanzei.toml` 照常入库。已有 `.kanzei` 的老项目不补写(多半在自己的根 .gitignore 里管着,kanzei 仓库就是)。
- **项目事实横幅**(`#project-facts`,侧栏项目卡下方的 `.project-warn-slot`,一次一条,D-170 隔离告警在场时让位):上级仓库(琥珀「需要注意」:点名上级仓库 [在此初始化独立仓库](先确认嵌套仓库)[不再提示])> 不是 Git 仓库(中性 [初始化 Git][不再提示])> **仓库还没有提交**(中性「Git 已初始化但还没有提交:并行线要等第一次提交」[知道了])> 空项目(中性「agent 会直接在这个目录里搭工程」[知道了])。「不再提示/知道了」按「项目|类别」记进 `ui_layout.project_facts`。「还没有提交」一类是复核补的:新建项目时本机没配 Git 身份就只 init 不提交,原先这件事只在一闪而过的 toast 里说过,之后横幅只剩「空项目」,并行线为什么不可用只藏在建线入口的 title 里。
- **`project_git_init`**:给已有的非 Git 项目建库 + 补忽略规则,**不自动提交**(已有文件里可能有不该进库的东西)。前端按后端结果提示:`created:false`(本来就是仓库,横幅过期了)说「本项目已经是 Git 仓库,没有重复初始化」,不再照样报「已初始化」。
- **「无 Git」芯片**(`#ctx-git`,输入区上下文带分支位旁,中性 `.kz-ctl`):`git_status.repo` 为 none/parent 时出现,读屏名说明不可用的功能;点开菜单的标题行是说明,唯一动作是「初始化 Git / 在此初始化独立仓库」。**研究空间不显示**(`renderGitChip` 判 `active_space`,切空间到下一次 `refreshGit` 之间由 `body[data-space="research"] .ctx-git` 兜住;不用 `data-space-only`——03-workspaces 会无条件切掉它的 `.hidden`):独立课题在 `~/.kanzei/research-workspaces/…` 下本来就不是仓库,在那里「初始化 Git」等于把研究工作区建成仓库。权限侧同样收口:research 档把 `git init` 与 stage/commit/merge_ff/finalize 一起硬拒绝,readonly 档硬拒绝 `git init`(不落到默认 Ask)。
- **新建项目对话框的主按钮**用专用键「创建项目」(英文 Create project);原先复用「创建」,它的英文是容器状态 `created`。
- **并行线入口**:事实不是「有提交的独立仓库」时 `#worktree-add`/`#lines-add` 标 `aria-disabled` + 原因(无 Git / 上级仓库 / 还没有提交),点击只提示原因;后端 `create_process_with_tracker` 同样先按事实拒绝并说清原因。建线确认框的「每线独立 target/ 目录…首次冷编译需数分钟」只在识别到 rust 栈时出现,其余工程说「每条线是一份完整的工作树检出」。

## 5. Git 工具与其它工具修复

- **只操作项目自己的仓库**:`repository_state` 比较 `rev-parse --show-toplevel` 与代码树根(主根,或工作树线的 cwd;双方 canonical 后比较)。非仓库或上级仓库时 `status/diff/log` 返回一条**事实**(ok,不是失败行:「not a git repository: … 需要时 git action=init」/「not an independent git repository: … 位于上级仓库 X」),其余动作报错并给出路(init,或把上级仓库根登记为项目)。
- **`init` 动作**:在代码树根 `git init -b main` + 补 `.kanzei/.gitignore`,不提交;默认权限 Ask(自主轮 NonInteractive 即拒)。已是仓库时什么都不做。
- **`conventions get`** 在没有 `conventions.md` 时返回 `exists: false` 与「按通用工程做法工作即可,不必再查」(ok),不再是红色失败。
- **批次字段**:`split_batch_value` 认出重复的键名前缀与 `k/N` 之后的说明(`批次: 0/5; B1 核心模型…` → `0/5` + 附注),写入侧归一成 `k/N` 并把附注放进「批次计划」字段(已有则追加);开头不是 `k/N` 的仍拒收,错误信息写明可接受的写法。读路径同样宽容。「批次计划」登记在 `tracker/fields.rs` 的 `FIELD_REGISTRY`(叙事类、无消费者),结构化读取不标灰、不计入未知字段。
- **bash**:每次运行按注册表合并新鲜 PATH(只追加不删);pwsh/powershell 命令前加 UTF-8 输出前导(`[Console]::OutputEncoding`/`$OutputEncoding` 设为无 BOM UTF-8、`$PSStyle.OutputRendering='PlainText'`),cmd 用 `chcp 65001`;权限判定、展示与回喂模型的仍是原始命令。跑完即作废项目状态缓存。
- **bash 摘要**(05-tool-summary):成功兜底先判表格/列表(表头分隔行、Format-List「键 : 值」占多数、≥3 行且几乎全是单 token 的非中日韩行),是就「输出 N 行」;否则末行必须是**句子**(中日韩 ≥4 字,或空格分词 ≥3 且其中 ≥2 个字母词,单个路径/文件名不算)才显示。`isProse` 本身不改。
- **非 Git 进展签名**:`progress_signature` 在没有独立仓库时并入树指纹(`.kanzei` 与构建目录之外文件的路径/大小/修改时间,深度 ≤8、条目 ≤5000),只写代码也算进展。

## 6. 鞭挞:模型在等你

- **`AutoRunCtx::awaiting_user` + `AutoStopReason::AwaitingUser`**,判定位置在模型段:`round_failure` 之后、`model_declared_done` 之后,所有任务判断(Nudge / ZeroOutput / GoalPending / VerifyRound)之前;两档一致;挂着目标也停但不清除目标;backlog 全阻塞、暂停、本轮后停、致命/限流仍排在它前面。
- **信号来源**(协调器轮末组装):① 主信号——本轮 `question` 以 `pending_question` 收口(自主轮 NonInteractive 下问题挂起;MetricsSink 在 ToolEnd 看 display.kind,交互轮当场回答的不算);② 兜底——本轮**最后一条**助手消息的正文**明显以向用户提问收尾**(`ends_with_user_question`,保守判据:去掉代码块后的最后一段,末段是选项列表时连同前一段;末句以 `？/?` 结尾,或含「请提供/请确认/请选择/需要你确认…」「please provide/confirm/choose…」;客套邀请「如有问题请告诉我 / feel free…」不算)。只看最后一条:它带工具调用(轮中旁白,如「先确认 X 是否存在?」+ 调工具)或没有正文(只有推理)就不判——不往前找更早的旁白(复核 minor;kanzei 库里 15 条轮末消息有 2 条只有推理)。用户接受偶尔误停:代价是回一句话。
- **界面**:`Stop/AwaitingUser` 时鞭挞保持勾选、不挂续跑定时器、停机原因槽显示「模型在等你回答」(琥珀,`waiting`)、对话里一条「⏸ 模型在等你回答(回复后自动继续)」,并滚到那个待回答的问题(question 的「回复此问题」块,否则最后一条助手回复)。**你这时手动发的消息就是回答**:`stopAutoForManualInput` 不再把它当成「手动接管」关掉鞭挞(按会话记的 `awaitingUserSessions` 标记,一次性),回答那一轮结束后引擎照常 Continue。等待期间不给「继续鞭挞」(`#auto-resume` 隐藏:鞭挞本来就开着,该做的是回答)。**标记只对紧接着的那条手动消息有效**(复核 minor):点「继续鞭挞」、手动开关鞭挞、这条线下一轮以别的结果收口(续跑、别的停机、未开鞭挞;活动线 kz:done 与后台线 `handleBackgroundSessionDone` 同一口径),标记都过期——之后的手动消息是真正的接管,照常关鞭挞。**后台线**停在这里时侧栏线路行单独写「在等你回答」(琥珀 `attention` 字形,不再混在「空闲」里),切过去原因槽给同一句提示(`applyAutoUiState` 清槽后回填)。
- **Nudge 文案由状态派生** `nudge_prompt(&NudgeFacts {selected, queues, user_blocked})`:引擎当前选中的条目(`resolve_work_selection`,与 `work next` 同一选取逻辑、不做全量交付对账)、项目里实际存在的 tracker 文件(按取活顺序)、阻塞原因指向用户的条目。不再出现项目里不存在的文件名,删掉「不要再做可行性判断」;给出「确实卡在用户决定或外部条件上 → question 问一次、写阻塞与解除条件、结束本轮,不要用散文提问」;保留「复核阻塞是否还成立」(kanzei 自举靠它复核历轮阻塞)。事实只在真的要发 Nudge 时才算(惰性),而且**在 `auto_runs` 锁外算**(复核 minor):锁内只做判定、取计数镜像与目标原文,放锁之后再序列化载荷、读 tracker、跑 git 观测——`auto_runs` 是全会话共享的 std Mutex,同步命令 `auto_state_update` 在 Tauri 主线程上等它(D-583 同一原则)。失败分支同样处理,停摆的手机通知(网络)也挪到锁外。
- **dev 提示契约**改为与引擎行为一致:全阻塞时不做凑数动作、以简短文字收尾——backlog 全阻塞引擎直接停,否则发**一次**针对性推进、下一轮仍无动作就停;要问用户用 `question` 一次(自主轮挂起 → 写 `阻塞`/`解除条件` → 结束本轮 → 引擎等回答);`<project-state>` 说空项目就在这里搭、没有 Git 就别反复 `git status`;**缺工具链(用户决定)**:用 `question` 问一次三个选项——授权我做**用户级**安装(不需要管理员:官方 zip 进 `%LOCALAPPDATA%` 加用户 PATH,或 winget `--scope user`,写明命令)/ 你自己装好告诉我 / 换技术栈;授权了就装、验证 `<tool> --version` 再继续,否则写阻塞 + 解除条件「`<tool> --version` 可用」;无人值守时绝不跑需要管理员的安装器(choco install、机器级 MSI)。dev-pair 提示同步补空项目与缺工具链两句。
- **结伴线续跑按结伴档跑**:`sendAutoToSession` 按本线实际档位传 agent(活动线读模式芯片,后台线读本线记住的档位,回落规则同 `applyProfileValue`)。**行为变化**:此前勾了鞭挞的结伴线,续跑轮一直按自主档(有 Nudge 与核查轮)运行;现在真的是轻控制(引擎不 Nudge、不插核查轮、模型说完成即停)。需要重门禁请显式切「自主推进」。
- **线档位经后端落盘**(复核 major):续跑轮既然按线档位发,档位就必须和鞭挞开关一样持久。原先档位只在 localStorage(`kz-profile` / `kz-process-profile`),本机重启即丢(D-404),而鞭挞开关在 `app.json` 里——重启/装新版后自举线仍开着鞭挞,档位却回落成结伴,续跑轮静默降级(没有 Nudge 与核查轮、backlog 不再停循环,还带上结伴提示与权限询问)。现在档位随 `process_auto_state` 每条线的 `mode` 字段(`dev-pair|dev-auto|research`)进 `app.json`:`rememberAutoUiState` 写入(当前线取模式芯片,其余线取本线记住的档位),`normalizeAutoState` 保留它;启动时 `mergeBackendAutoState` 在同一次合并里 `restoreLineModes` 回填 `processProfileUi`,再按恢复的档位重做一次活动线的芯片回显——都在 `applyProfileValue` / `lineAgent` 读它之前。回填规则:后端记着 `mode` → 以它为准;后端没有而本地有 → 补进后端;都没有但鞭挞开着 → 记为「自主推进」(升级前的续跑轮一律按自主档跑,这就是那条线一直以来实际的档位;用户要结伴就显式切一次,之后持久)。全局 `kz-profile` 只剩「从没设过档位的主线」的回落,不再迁。

## 7. 门禁与测试

Rust(均实跑过变异:删掉被守护的那一行,对应测试变红):

| 位置 | 用例 | 变异 |
|---|---|---|
| kanzei-base path_form | 盘符/UNC 去前缀;超长(每段合法、总长 >260)、保留名、尾点/空格、`..`、空组件、非盘符 verbatim 原样;幂等;canonical 不带前缀 | 删总长判据 |
| kanzei-core path_migration | v24 库(默认进程、并行线 + 工作树、两组主键冲突、超长路径、退役线、检查点、会话)升级后全部 simplify、冲突保留较新行、超长保持 verbatim;读时兜底两种写法都列得出且归一 | 删迁移调用;list_processes 只匹配一种写法 |
| kanzei-tools project_state | 空项目(含 desktop.ini)/清单识别栈/Git 三态(无提交、packed-refs、上级仓库)/计划栈来自 R-001/PATHEXT 找到 flutter.bat/渲染稳定且 ≤600/树指纹/缓存作废/git init 不覆盖已有忽略规则、首提交取决于本机身份 | greenfield 判据失效 |
| kanzei-tools git | 上级仓库内:status 是事实、stage 拒绝且点名上级、上级暂存区不动;非仓库:status 是事实、commit 拒绝、init 建库后恢复正常、再 init 什么都不做;`git init` 权限为 Ask | 仓库判据退回「rev-parse 成功即可」 |
| kanzei-tools bash(Windows) | 中文目录 `MD文件保存` 下 `Write-Output`+`Get-Location` 无 U+FFFD、无 ANSI、原文与路径完整;注册表 PATH 合并只增不减去重;前导只进子进程参数 | 去掉 pwsh 前导 |
| kanzei-tools tracker / memory docstore | `批次: 0/5; B1 …` 归一为 `0/5` + 批次计划;`2/4(B3 渲染)`、`batches: 1/2` 放行;`B1 0/5`、`3/11/2` 拒收 | 删归一 |
| kanzei-tools conventions | 缺文件 get 是引导性事实 | — |
| kanzei-harness auto_run | 等用户两档都停、优先于 Nudge/GoalPending/ZeroOutput、不遮住暂停/本轮后停/致命/限流/全阻塞;Nudge 按状态生成、不含 defects.md 与「可行性判断」、点名等用户的条目 | 删 AwaitingUser 分支;Nudge 回到写死队列 |
| kanzei-app | `ends_with_user_question` 中英正反样本(含现场原句、反问后自答、客套邀请、代码块里的问号);非 Git 目录写代码后签名改变;AwaitingUser 序列化与 Nudge 事实;question 挂起才置位;身份根不带前缀且两种入参同一进程 id;偏好键迁移;新建项目(建库/忽略规则/首提交或缺身份、非空目标拒绝、名称校验) | 删客套判据;删树指纹;删偏好键迁移;身份根退回 std canonicalize;删非空目标检查 |
| kanzei-tools project_state(复核) | 已有工程多一个源文件、几乎为空的目录多一个零星文件,`render` 逐字节不变(`files` 照常变) | 把文件数放回文本 |
| kanzei-tools profiles(复核) | research 档 `git init` 与 stage/commit/merge_ff/finalize 同为硬拒绝;readonly 档 `git init` 硬拒绝且拒绝理由说「不建仓库」 | 去掉 research 的 init;readonly 规则失效 |
| kanzei-tools tracker(复核) | 归一写出的「批次计划」在结构化读取里 known、narrative | 批次计划不登记 |
| kanzei-app research_library(复核,Windows) | 登记表是 verbatim 形态(外加一条大小写不同的 dev 构建重复),连进两次研究空间:一个课题一条、编号不变、存储根与关联项目是 simplify 形态、同一目录只算一个关联项目、`next_id` 不动 | 删迁移调用;`register` 退回原串比较 |
| kanzei-app auto_run(复核) | 最后一条助手消息只有推理 / 带工具调用 → 不判(不回头读旁白);最后一条纯文本 → 就是它 | 不排除带工具调用的消息 |
| kanzei-app prefs(复核) | 两种写法并存时保留去前缀的值 | 退回「带前缀的赢」 |

前端:`scripts/ui-runtime-smoke.mjs`「分区:工作目录管理」——bash 摘要五例(截图同款表格 + 名称、纯名称列表、Format-List、单个文件名、真正的句子)、AwaitingUser(勾选保持/无定时器/提示/回复不关鞭挞)、续跑轮 agent/projectDir/取活顺序键、新建项目对话框(Git 默认勾选、预览、失败留在对话框、描述进草稿不发送、缺身份反馈)、事实横幅(no-git/初始化/无提交时建线拦截、上级仓库、让位隔离告警、空项目「知道了」按项目记住)、建线确认只对 Rust 提 target/、「无 Git」芯片(none/parent/own)。变异守卫 `wdBashTable / wdBashSentence / wdAwaitKeep / wdAwaitMark / wdAutoAgent / wdAutoProjectDir / wdNewProjectDraft / wdBannerYield / wdWorktreeGate / wdGitChip / wdRustOnlyText` 均已实跑变红。既有两处「新建项目 = 两次输入」的断言改为对话框流程。预览场景 `workdir-new / workdir-nogit / workdir-parent`(scripts/ui-preview/scenes.mjs 同名分区)。

复核补的冒烟(同一分区):等待期间 `#auto-resume` 隐藏,点「继续鞭挞」/手动开关鞭挞/下一轮以续跑收口之后,手动消息照常关鞭挞;后台线停在 AwaitingUser 时线路行写「在等你回答」(`attention` 字形),切过去原因槽回填,下一轮别的收口后回到空闲;模拟重启(本地档位全清)后 `mergeBackendAutoState` 回填档位,自主线与「鞭挞开着、没记档位」的旧存档线续跑轮都按 `dev` 发,切档位后 `ui_prefs_set` 的存档带 `mode`,`normalizeAutoState` 保留 `mode`,旧键与新键并存时新键赢;初始化 Git(无提交)后横幅换成「还没有提交」+[知道了],`created:false` 时提示「已经是 Git 仓库」;研究空间不画「无 Git」芯片;英文界面新建项目主按钮是 Create project。变异守卫 `wdModeRestore / wdModeLegacy / wdModeRemember / wdModeNormalize / wdKeyNewerWins / wdAwaitResumeHidden / wdAwaitResumeTake / wdAwaitToggle / wdAwaitExpire / wdBgAwaitExpire / wdBgWaiting / wdSwitchWaiting / wdBannerNoCommit / wdInitToast / wdGitChipResearch` 均已实跑变红;按钮专用键的守卫是手工变异(index.html 改回「创建」→ 冒烟变红)。预览场景加 `workdir-nocommit`(「还没有提交」横幅 + 一条后台线停在「在等你回答」)。

## 8. 用户与主会话的拍板(2026-09-26)

- 新建项目的一句话描述进输入框当**草稿**,不自动发送。
- 勾选初始化 Git 时,本机配了 git 身份就做首提交;没配就只 init 并在反馈里说明。
- 缺工具链:自主档里 agent 用 question 问**一次**;用户授权后可以做**用户级**安装(不需要管理员);否则写阻塞 + 解除条件。
- 「最后一句明显在问你」的散文兜底:接受,偶尔误停可以接受。
- 结伴档勾鞭挞后的续跑轮真的按结伴档跑(行为变化,写进发版说明)。

## 9. 未做与后续

- `default_conventions.md` 里 kanzei 专属的 cargo/compile_gate/verify.ps1/CI 条款仍注入所有项目(用户尚未拍板迁回 kanzei 仓库自己的 conventions.md)。
- 非 Git 项目仍没有回滚入口:R-366 的文件检查点只有采集,bash 写入也不进检查点;横幅里没有写这句(等恢复入口落地再说)。
- `project_facts` 未进 `scripts/ipc-contract.json`:工具链的 `found` 与 Git 状态随机器与项目而变,形状取样不稳定;字段由 `ProjectFacts`(serde)一处定义,前端夹具按同名字段写。
- 协调器里 ToolEnd → `note_question_end` 的接线由 MetricsSink 单测覆盖函数本身,接线点(`build_event_handler` 的 ToolEnd 分支)没有端到端用例。
- 鞭挞 AwaitingUser 的「滚到问题」在假 DOM 里只验调用不报错,真实滚动靠预览截图人工看过。
- 状态栏 `#status-git` 在非 Git 项目仍是空白(上下文带已有芯片,状态栏不重复)。
- 协调器失败分支的进展签名(`progress_signature`,含 git 观测)仍在 `auto_runs` 锁内算:它只在鞭挞武装时才需要,挪出锁要分两次取锁(先判要不要重试再算签名),本轮复核只把 Nudge 事实与手机通知挪了出去。
- readonly 档的 stage/commit/merge_ff/finalize 仍落默认 Ask(本轮只按复核意见硬拒绝了新加的 `init`)。

## 变更记录

- 2026-09-26:起草并实施(ui2/workdir 分支,UI2-0926 #13):`dbb70979` 路径形态与 v25 迁移、`97471d1f` 项目状态事实与工具修复、`7720f349` 鞭挞等你回答 / Nudge / 新建项目后端、`1e2347e3` 界面与冒烟。
- 2026-09-26:独立复核修复(3 major + 9 minor 全部处理;`a189f177` 后端、`ced41f3c` 界面与冒烟):项目状态文本去掉文件数(prompt 缓存)、研究课题登记表并入 `\\?\` 迁移、线档位随 `process_auto_state.mode` 经后端落盘;Nudge 事实与停摆通知挪出 `auto_runs` 锁、兜底判据只看最后一条助手消息、偏好键并存时新键赢、「批次计划」登记、research/readonly 档拒绝 `git init`、研究空间不画「无 Git」芯片、三条项目命令改 async、「还没有提交」横幅与按结果提示、等待标记的过期与后台线「在等你回答」、新建项目主按钮专用键。

## 验证证据

- `cargo fmt --all -- --check`;`cargo clippy -p kanzei-base -p kanzei-core -p kanzei-harness -p kanzei-memory -p kanzei-tools -p kanzei-app --all-targets -- -D warnings`;`cargo test` 同六个 crate(kanzei-app 317、kanzei-tools 615(另 1 条既有 ignored)、kanzei-core 324、kanzei-harness 180、kanzei-memory 175、kanzei-base 29,全部通过);`cargo check -p kanzei --all-targets`。
- `node scripts/ui-a11y-smoke.mjs`、`ui-i18n-smoke`、`ui-markdown-smoke`、`ui-lint-smoke`(含浏览器冒烟)、`ui-connectivity`、`parallel-lines-regression`、`ipc-event-smoke`、`check-design-freshness`、`ui-narrow-layout-smoke`、`ui-workspace-smoke`、`node --experimental-vm-modules scripts/ui-runtime-smoke.mjs` 全部退出码 0;11 条前端变异与 14 条 Rust 变异逐条实跑变红。
- 预览截图:1333×695@1.5 与 1600×900@1.25,暗/亮各三景(新建项目对话框、无 Git + 模型在等你回答、上级仓库 + 芯片菜单)。
- 复核修复:`cargo fmt --all -- --check`;`cargo clippy -p kanzei-tools -p kanzei-app --all-targets -- -D warnings`;`cargo test -p kanzei-tools -p kanzei-app`(kanzei-tools 616(另 1 条既有 ignored)、kanzei-app 319,全部通过);`cargo check -p kanzei --all-targets`;上列 11 个前端门禁全部退出码 0;8 条 Rust 变异、15 条前端变异与 1 条手工(index.html)变异逐条实跑变红;截图 1333×695@1.5 与 1600×900@1.25 暗/亮四景(新加 `workdir-nocommit`),另英文两景——据截图顺手修了三处:横幅折行孤字(`text-wrap: pretty`)、线路行「在等你回答」状态词跟字形同为琥珀、状态栏「日志」在长用量串挤压下竖排成两行(`.statusbar-btn` 不收缩不换行);英文停机原因槽与线路状态词改成放得下的短句(Waiting for your answer / Needs reply)。
