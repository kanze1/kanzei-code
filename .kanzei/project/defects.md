# Defects




















































































































































































## D-061 OAuth 凭证无锁读改写且非原子覆盖,与官方 CLI 共享文件可致登录态失效 [open] (high)
- 复现: 两个 kanzei 进程(或 kanzei 与 Claude Code CLI)在令牌过期窗口内并发发起请求。
- 根因: kanzei-llm/src/auth/claude.rs:28-95、auth/codex.rs:20-101 的流程是 read_to_string → 判断过期 → POST 刷新 → `std::fs::write` 覆盖,无文件锁、无 tmp+rename 原子替换、无写前重读。这两个文件(~/.claude/.credentials.json、~/.codex/auth.json)同时被官方 CLI 读写。
- 影响: 双方各自用同一 refresh_token 刷新,而 OAuth 轮换 refresh token,后到者 invalid_grant,且先到者写入的新 token 可能被并发方以旧内容覆盖回去,登录态永久失效并殃及官方 CLI,需手动重新登录;truncate-then-write 中途崩溃会留下半截 JSON,下次解析直接报"请重新登录"。access token 约 1 小时一刷,窗口频繁。
- 验收: 刷新前后加文件锁,写入改为写临时文件再 rename 原子替换,写前重读校验;补并发刷新不互相覆盖的测试。
- 优先级: P1
- 阶段: 1
- 不变量: 配置与文档:多文件变更原子提交
- 证据等级: E2
- 阻塞: 涉及与 Claude Code/Codex 官方 CLI 共享 OAuth 凭证文件的并发写入、文件锁与原子替换，属于第三方集成/凭证高影响改动；依据 conventions.md 第 1 节需先提交方案并等待用户确认。解除条件：确认锁实现、跨进程协作与 Windows 原子替换策略。下一步：暂跳过，继续 D-059。








































































































































































- 标签: oauth,concurrency,atomic_write
- 类型: data_loss
- 领域: auth,provider,storage












## D-108 英文模式只翻译少量静态节点,动态状态与操作反馈长期中英混杂 [fixing] (medium)
- 复现: 设置语言为 English,创建/切换项目、运行任务、打开文档/设置并触发 toast;静态导航的一部分变英文,动态生成的状态、日志、错误、按钮和 300 余处中文字符串仍保持中文。再切回中文还会触发 D-092 的属性不可逆问题。
- 根因: applyLanguage 只遍历当前 DOM 文本节点和少量 title/placeholder,I18N_EN 仅覆盖有限字典;后续 JS 动态生成的文本不会经过翻译函数,也没有以 key 为中心的统一文案层。
- 影响: 英文模式无法作为完整产品能力使用,错误与权限等高风险信息尤其容易出现语义断层;R-069 原验收“所有产品/功能文案”未满足却被归档 done。
- 验收: 所有用户可见文案由稳定 key 生成,动态内容与属性同源且可逆;中英文分别跑页面/操作快照,不得出现非用户数据导致的混合语言;补缺失 key 检查。
- 优先级: P3
- refs: D-092 R-069
- 阶段: 3
- 不变量: 操作反馈:文案进入统一 i18n 资源
- 证据等级: E3





























































- 进展: 已进入 fixing（本次仅建立取活边界，尚未修改代码）：后续收口运行事件/完成与停止反馈动态文案，覆盖 main.js 运行事件订阅(kz:tool-start/tool-end/step/error/stream-restart/compacted/stopped/done)及上下文详情面板；完成后跑动态 key 缺失检查与 UI 冒烟验证。设置/工作区/文档及真实双语快照仍是后续范围。












































































































- 标签: dynamic_text,translation,runtime_feedback
- 类型: ux
- 领域: ui,i18n,feedback











## D-114 自举运行验证节奏低效:git 查询过密、全量测试时机不当、已知位置缺陷仍派子代理 [fixing] (low)
- 复现: 2026-08-07 完整落库轨迹:30 次终端调用中约 13 组 git status/diff/show 密集重复且常一次塞多条;文件仍处换行损坏时跑过全工作区测试;D-082 单文件已知函数缺陷启动子代理,28 次内部读查后因网络错误失败返回,主 agent 重查一遍。
- 根因: dev 提示词无验证节奏与子代理适用边界约束;runner/工具层对重复查询、无变化重测、已知位置探索无任何检测。
- 影响: 单轮约 14~18 次可避免的终端调用(占 47%~60%);重复输出稀释上下文,推高 token 成本与轮次时长。
- 验收: 提示词纪律落地(已完成);R-099 度量显示同类任务终端调用数与 edit 未命中率显著下降;若提示词不足以收敛,按 R-100 落 runner 层机械提醒。
- 优先级: P2
- refs: D-113 R-099 R-100
- 阶段: 1
- 不变量: 工具:每次调用都有信息增量
- 证据等级: E2
- 进展: a7892e1 已在 dev 提示词加入验证节奏纪律(git 检查按里程碑并合并查询、先定向测试后全量、无变化不重测)与子代理适用边界(已知文件+函数直接读);1d5e294 的 edit/bash 门禁已消除编辑连败与整文件重写两类根源。剩余闭环依赖 R-099 的度量数据,机械化提醒按 R-100 决策。
- 进展(追加): 鞭挞 14~17 轮暴露微切片新浪费:D-108 每轮只翻 2~3 处文案(拖到 34 步),且纯前端改动每轮跑全量 cargo test。按用户定调落地:①鞭挞默认提示词规则 2 改为「一轮一个完整条目,同构批量改动一轮吃完整类别」,旧默认进 LEGACY_CONTINUE_PROMPTS 静默升级;②conventions §1.3 增粒度与「验证匹配改动面」规范;③dev 提示词同步测试选择规则。生效需重装 kzapp(前端与提示词打包在二进制内)。






























































































- 标签: verification,cadence,tool_efficiency
- 类型: governance
- 领域: process,testing,agent










## D-121 kz run 被权限拦停后退出码仍为 0,自动化调用方无法感知失败 [open] (medium)
- 复现: 非交互终端执行 kz run(stdin 为管道),agent 首个 bash 调用触发权限询问,read_line 读到 EOF 判 Deny,输出 "(stopped: permission declined)" 后进程 exit 0。
- 根因: run_cli 的 halted_by_user 分支只打印提示即正常返回 Ok(crates/kanzei/src/main.rs:343-345),权限拦停与正常完成共用同一退出语义。
- 影响: 脚本/CI/其他 agent 调用 kz 时无法区分"完成"与"被拦停",按成功继续会基于半途结果做后续动作。
- 验收: halted_by_user 以非零退出码结束(如 3)并在 stderr 说明;正常完成保持 0;补退出码断言测试。
- 优先级: P2
- refs: R-102
- 阶段: 1
- 不变量: 会话控制:终态语义在退出码可见
- 证据等级: E2
- 备注: 2026-08-07 用 kz 做前端分析实测发现(用户确认立项)。









- 标签: exit_code,permission_denied,automation
- 类型: contract
- 领域: cli,automation,permission








## D-122 裸 bash 通配规则的降级告警与实际放行行为不一致 [open] (low)
- 复现: 项目 kanzei.toml 写 `action="bash", resource="*", effect="allow"`;kz 启动提示"检测到 1 条旧 bash 权限规则;将降级为逐次询问",随后整轮 bash 全部直接放行,一次也没询问。
- 根因: legacy_bash_rules 把所有非 command/workdir JSON 的 bash 规则识别为旧规则并告警,但评估路径对用户显式配置的整体 `*` 保留 yolo 语义——告警文案与评估行为出自两套判断。
- 影响: 用户被告知会逐次询问,实际却全量放行:轻则困惑,重则误以为有询问兜底而放松了对通配规则的警惕。
- 验收: 告警与评估行为一致——显式 `*` 要么如实提示"将全量放行(yolo)",要么真的降级询问;补两种规则形态的启动提示测试。
- 优先级: P3
- refs: D-051
- 阶段: 1
- 不变量: 权限:授权范围精确可解释
- 证据等级: E2
- 备注: 2026-08-07 kz 前端分析实测发现(用户确认立项)。










- 标签: wildcard,legacy_rule,warning
- 类型: contract
- 领域: permission,bash,provider







## D-123 usage 与 --help 不展示 agent/模型/profile 选择方式,能力不可发现 [open] (low)
- 复现: kz --help 只列出 run 与 tracker 子命令及一行 env 提示;KANZEI_AGENT 可选值(dev/dev-pair/research)、KANZEI_MODEL 语法、profile 档位含义均无从查起。
- 根因: usage() 是 5 行手写文本(crates/kanzei/src/main.rs:55-60),agent 清单在 harness 快照里,帮助文本不读取它。
- 影响: 只有读过源码的人知道怎么切 agent;dev-pair 这类对话型入口实际不可发现。
- 验收: --help 列出可用 agent(名称+一句话用途)、KANZEI_MODEL 语法示例与 profile 说明;或提供 kz agents 列表命令。
- 优先级: P3
- 阶段: 1
- 不变量: 操作反馈:能力可自发现
- 证据等级: E2
- 备注: 2026-08-07 kz 前端分析实测发现(用户确认立项)。











- 标签: help,agent,model,profile
- 类型: ux
- 领域: cli,docs,discoverability






## D-124 应用内更新不先退出自身,安装必败且僵尸安装器锁死后续重试 [open] (high)
- 复现: 2026-08-08 0:17 实录:app 运行中点「下载并安装」→ 安装器无法替换正在运行的 kzapp.exe,界面报 "另一个程序正在使用此文件。(os error 32)";失败的 kanzei-setup.exe 进程(%TEMP%,PID 15036)不退出,持续握着 kzapp.exe 句柄;用户重启 app 后重试仍报同一错误,直到手动杀掉僵尸安装器。
- 根因: 更新流程是"下载 setup → 直接运行",没有"退出自身再交给安装器"的交接;NSIS 遇文件占用时挂在隐藏对话框而非失败退出,进程成为僵尸;重试路径也不检测/清理既有安装器进程与 %TEMP% 残件。
- 影响: 应用内更新在最常见场景(app 开着点更新)必然失败,且首次失败后连关闭重开都救不回,普通用户会卡死在 os error 32,只能求助或手杀进程。
- 验收: 更新流程改为"下载校验 → 启动安装器(静默)→ 立即退出自身",安装器带完成后自启;启动更新前检测并清理残留的 kanzei-setup 进程与临时文件;失败时报错含可操作指引;补更新交接的可测覆盖(至少流程状态机单测)。
- 优先级: P0
- refs: D-121
- 阶段: 1
- 不变量: 版本与更新:更新流程可靠交接,失败可自愈
- 证据等级: E3
- 备注: 手动恢复路径已验证:杀僵尸安装器 → 温和关闭 app → 静默装 setup → 重启,已把用户从 c2cf358 更到 79e532b。












- 标签: self_exit,installer,process_lock
- 类型: bug
- 领域: release,update,windows





## D-125 独立文档页列表末尾与底部状态栏重叠,末行被遮挡不可读 [open] (low)
- 复现: 打开「需求与工作/缺陷」独立管理页,列表滚到底:最后一行(截图中为 P0 的 E2 harness 条目)与底部状态栏重叠,文字被状态栏盖住;列表容器无底部安全间距。
- 影响: 排在末尾的条目(常是新加的)难以阅读与点击;用户 2026-08-08 截图反馈。
- 验收: 列表容器 padding-bottom 预留状态栏高度(或滚动区域计算扣除状态栏);滚到底时末行完整可见可点;顺带核查同布局的其余独立页。
- 优先级: P3
- 阶段: 3
- 不变量: 界面状态:内容不被固定元素遮挡
- 证据等级: E3






- 标签: scroll,footer_overlap,last_row
- 类型: ux
- 领域: ui,layout,documents
