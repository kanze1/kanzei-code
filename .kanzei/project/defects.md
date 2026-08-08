# Defects

## D-170 项目隔离失效:需求在不同项目之间串 [fixed] (high)
- 复现: 2026-08-08 用户实测。用「＋ 添加项目目录」加了新项目后,不同项目看到同一批需求。
- 根因: `projects_add` **只把路径记进偏好,不在该目录建 `.kanzei`**;而后端一律用 `discover_project_root` 解析根,它会**沿目录树向上找 `.kanzei`,找不到再退到最近的 `.git`**。于是任何未初始化的项目目录都会解析到某个祖先——两个共用同一祖先(或同属一个 git 仓库)的目录被当成同一个项目,读写同一份 `requirements.md`/`defects.md`,连会话也共用。向上遍历对 CLI 是对的(`kz` 在子目录里跑要找到项目根),但桌面端的项目是用户**显式选定**的目录,向上走等于把他的选择悄悄换成了祖先。
- 影响: 最严重的一类——跨项目数据污染。用户会看到别的项目的需求,在错误的项目里改条目,而且完全无从察觉。
- 验收: ①`projects_add` 就地创建 `.kanzei`,新项目从加入那一刻起自成一根;②存量项目**不静默迁移**——改根会让 `project_session_id` 变化、历史对话看起来消失,那会造成第二次"数据丢失"惊吓;改为新增 `project_root_info` 如实报出"所选目录 vs 实际生效的根",不一致时侧栏顶部醒目告警并给出实际路径;③提供 `project_detach` 一键在本目录建立独立空间,**只建空间不搬数据**——祖先目录里的条目属于祖先项目,替用户搬等于替他做决定;④回归覆盖"同一上级下两个目录先共用、分离后互不可见,且上级数据不被动"。
- 优先级: P0
- 阶段: 1
- 不变量: 项目:用户显式选定的目录就是该项目的根
- 证据等级: E2
- 备注: 落地位置 crates/kanzei-app/src/main.rs(projects_add 建 .kanzei、project_root_info、project_detach)、ui(侧栏告警与一键分离)。回归:Rust 侧 `同一上级下的两个项目必须各自独立不串数据`,冒烟 5 项。
- refs: D-058
- 标签: 后端


## D-169 切到独立文档页后需求整列消失,界面无任何说明 [fixed] (high)
- 复现: 2026-08-08 用户实测。切换到独立文档页,需求列表变空,而筛选控件显示的是"全部" —— 看起来像数据丢了。
- 根因: 两层叠加,**是 R-115 的筛选持久化把一个潜伏矛盾激活的**。
  ①`syncTagFilter` 在"保存的标签不存在于当前条目"时,只把**下拉的显示值**回落成 `all`,不回写筛选状态。于是 `reqFilters.tag` 仍是那个不存在的标签,`filterRequirements` 照它筛,结果为空——而界面显示"没有筛选"。R-115 之前标签不持久化,每次启动都是 `all`,这个矛盾永远碰不到;一持久化就必然触发。
  ②空状态判断是 `entries.length === 0 && archivedCount === 0` 才显示"(空)"。有归档条目时被筛空则**连"(空)"都不显示**,渲染出纯一片空白,更像数据没了。
- 影响: 用户会认为需求被删了。这个项目此前真丢过 8 个缺陷条目,这种"看起来像数据丢失"的表现代价极高——会触发不必要的恢复操作。
- 验收: ①`syncTagFilter` 返回实际生效值,三处调用方一律回写状态,做到显示与状态同源;②任何列表在**筛前非空、筛后为空**时必须显示"N 条被当前筛选隐藏"并给一键清除,不得留白;③冒烟守住"列表不得无声变空"这条不变量。
- 优先级: P1
- 阶段: 3
- 不变量: 前端:列表不得无声变空——空了就要说清为什么
- 证据等级: E3
- 备注: 落地位置 crates/kanzei-app/ui/main.js(syncTagFilter 返回值 + 三处回写、renderDocList 被筛空分支)。已反验:去掉回写,冒烟报「筛选状态应回落成「全部」而不是筛空」并失败。
- refs: R-115
- 标签: 前端

## D-168 配置页与实际生效的配置之间有三处静默不一致 [fixed] (high)
- 复现: 2026-08-08 用户配 DeepSeek 全过程暴露。设置页 primary 明明显示 `deepseek:deepseek-chat`,发消息时日志却是 `[鉴权] anthropic:claude-sonnet-5` + `provider 'anthropic' 需要环境变量 ANTHROPIC_API_KEY`——界面显示的和实际跑的是两份东西,而且没有任何线索指向"你改的那个没生效"。
- 根因: 四处叠加,都是"看得见的"与"生效的"脱节。
  ①**表单不保存不生效,却无提示**:设置页是一张普通表单,填完不点保存只活在 DOM 里;运行时读的是磁盘。用户以为改了。
  ②**settings_get 只读全局文件**:而运行时是 `全局 + 项目` 合并。项目级 kanzei.toml 一旦也设了 models,设置页显示的就是个不生效的值,同样零提示。
  ③**模型角色是自由文本框**:手打 `provider:model`,拼错一个字母要到真正发消息时才炸,那时早已离开设置页,联系不到是刚才填错的。保存路径不做任何校验。
  ④**merge() 漏了 models.reasoning**:primary/fast/providers/proxy/profile/permissions 都合了,唯独 reasoning 没有——同一个 `[models]` 表里有的键管用有的不管用,是最难查的那类不一致。
- 影响: 配置这条链路整体不可信。用户按文档一步步配完、连通性测试还过了,一发消息用的还是旧 provider,且报错完全指不到原因。
- 验收: ①表单与磁盘不一致时显示「未保存」;②settings_get 同时返回合并后的生效值与项目配置路径,不一致时界面明示"被项目级配置覆盖,本页改动不会生效";③模型角色改为下拉,选项来自各 provider 的探测结果,保留手填兜底,且**探测不到的已存值必须原样保留**(否则一进设置页就被悄悄改掉,保存一次配置就坏了);④保存前用 `resolve_model` 校验 provider 确实存在,不存在直接拒绝并说明格式;⑤merge 补上 reasoning;⑥鉴权失败的报错带上本次解析到的模型,并提示检查保存与项目级覆盖。
- 优先级: P1
- 阶段: 3
- 不变量: 配置:界面显示的必须等于实际生效的,不等就要说破
- 证据等级: E2
- 备注: 落地位置 kanzei-harness/config.rs(merge reasoning)、kanzei-core/assemble.rs(错误带模型)、kanzei-app/main.rs(settings_get 返回 effective、validate_model_roles)、ui(下拉、未保存徽标、覆盖提示)。回归:Rust 侧 2 项(保存前校验、models 全字段合并),冒烟 8 项。「已存值不被下拉吃掉」已反验:去掉保留分支即失败。
- refs: D-156 D-157 R-115
- 标签: 模型

## D-167 加了 OpenAI 兼容 provider 却选不出任何模型 [fixed] (high)
- 复现: 2026-08-08 用户按指引在设置页添加 deepseek(protocol=openai, base_url=https://api.deepseek.com/v1, api_key_env=DEEPSEEK_API_KEY),顶栏「模型」下拉里一个 deepseek 模型都没有,只有 primary/fast 两个角色项。
- 根因: `models_list` 只硬编码枚举四种情况——primary/fast 角色、`auth="codex"`(3 个写死型号)、`auth="claude"`(3 个写死型号)、`base_url` 含 11434 的 Ollama(查 /api/tags)。**其余 provider 直接落到分支尾部,贡献 0 个模型**。而配置层是完全开放的:任何 OpenAI 兼容端点都能配进去。于是"能配 provider"与"能用 provider"之间断了一环,DeepSeek/OpenRouter/Kimi/自建 vLLM 全中招。
- 影响: provider 配置形同虚设——配好了、连通性测试也过,就是没法在界面上选中它的模型。用户只能去改 kanzei.toml 的 `[models]` 硬指,顶栏下拉这条主路径不通。
- 验收: ①protocol 为 openai / openai-responses 的 provider 走标准 `GET {base_url}/models` 探测,带上 api_key(直填优先于环境变量),遵循全局代理设置;②探测失败静默跳过,不阻断其余 provider 的列举——端点可能没实现 /models,或 key 尚未配好;③提供手填兜底「＋ 手填模型…」,输入 `provider:model` 直指,校验格式后落盘并持久留在下拉里;④Ollama 仍走原生 /api/tags(它的 /v1/models 不全),抽成 `push_ollama_models` 避免两处重复。
- 优先级: P1
- 阶段: 3
- 不变量: 配置:能配进来的 provider 就必须能在界面上用起来
- 证据等级: E2
- 备注: 落地位置 crates/kanzei-app/src/main.rs(models_list 新增 openai 分支 + push_ollama_models)、ui/main.js(手填入口与持久化)。冒烟新增 4 项断言:手填入口存在、落盘、回到下拉、非法格式被挡。
- refs: R-115
- 标签: 模型

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

