# Requirements

## R-153 拆解 kanzei-app/src/main.rs(6413 行→约 16 模块,main.rs 收敛为装配) [doing]
- 优先级: P1
- 复杂度: 大
- 标签: 后端
- 来源: 2026-08-09 用户定调巨石拆解;结构地图与批次表已落设计文档 §A(行号基准 c339b58,执行以符号名定位)。
- 内容: 照 files_view.rs 先例(command 加 pub、invoke_handler 全路径注册、低耦合模块零依赖 main)把 75 个 command 按域拆为 state/update/fast_model/agent_container/mobile/memory/prefs/projects/processes/settings/docs/conversation/harness_ext/subagents/run 等模块;批0 先把 818 行 update_tests 按域切开(解锁全部后续批),批1 零依赖叶子起步,批4 落 state.rs 枢纽,批10 收 run.rs,共 11 批,每批一提交。设计: docs/design/monolith_decomposition.md §A。
- 边界: 零行为变更,diff 只允许 move+use+可见性;run_task(695 行)只整体搬迁不拆内部(内部拆分另立条目);main() 开头三调用顺序、UI_PROBE 三 static 同模块、ask_seq 共享、cfg(windows) 成对搬迁等危险点清单见设计文档;拆解批与其他源码条目不得并发。
- 验收: ①main.rs ≤300 行且只含 mod 声明+main()+Builder 装配;②每批独立提交且 cargo test -p kanzei-app 绿,条目关闭前全量 cargo test --workspace 一次全绿(节奏见 conventions §1.4);③invoke_handler 78 项全数保留(拆前后清单 diff 核对)且按域分组加注释;④四条 UI 冒烟不受影响;⑤拆前后 wc -l 对照记入进展。
- refs: A-008 R-148(先例 files_view.rs)
- 依赖: R-152

- 进展: harness_ext 清理尾批已在提交 45cb9dc 完成：FrontendToolsComponent 与 QuickCaptureComponent 旧副本已删除，定向验证 T-1786287179 已登记通过。下一步进入 subagents 域，先评估 defect_review/quick_req 与 main.rs 共享依赖后再做整体剪切迁移。

## R-154 拆解 kanzei-app/ui/main.js(7020 行→18 个有序 classic script) [todo]
- 优先级: P1
- 复杂度: 大
- 标签: 前端
- 来源: 2026-08-09 用户定调;不引 ES modules 的机制论证与文件清单已落设计文档 §B(行号基准 c339b58)。
- 内容: B0 使能批**只改四个冒烟脚本**:从 index.html 解析 `<script src>` 清单按序读入,runtime 冒烟逐文件 vm.runInContext(与浏览器多 script 语义一致含 TDZ,拼接执行会掩盖前向引用 bug),静态断言用 join 串,探针注入按累计命中≥2 判定——此批 main.js 一字不动、四冒烟必须仍绿;随后 B1~B9 从尾部往前切出 18 文件(01-core…18-startup):readJson/writeJson 上提 01(现存唯一前向引用硬风险 L3244)、启动 IIFE 锁死末位、04/05 相邻保 markdown 冒烟切片边界;index.html 仅 script 标签区改为按序 18 个 `<script defer>`;同步 deep_parallel_dev.md:283 的 node --check 改遍历。设计: docs/design/monolith_decomposition.md §B。
- 边界: 不引 ES modules/打包器/框架(A-008);style.css 零改动;tauri.conf.json 无需改(frontendDist 整目录);拆解批与其他前端条目不得并发。
- 验收: ①B0 后单文件形态四冒烟仍全绿(机制改造零行为变化);②每批 node --check(遍历 ui/*.js)+四条冒烟全绿;③最终 main.js 消失,18 文件按 index.html 顺序加载,单文件 ≤1000 行;④发版后真机复查主视图/发送/设置页可用(E3 残余,不阻塞关闭,进展注明);⑤拆前后行数对照记入进展。
- refs: A-008
- 依赖: R-152

## R-155 拆解 kanzei-core runner.rs(3240 行)与 store.rs(1972 行)为子模块目录 [todo]
- 优先级: P1
- 复杂度: 大
- 标签: 核心
- 来源: 2026-08-09 用户定调;外部 API 面已 Grep 核实(外部三 crate 零处使用模块路径,全走顶层再导出),划分与危险点清单已落设计文档 §C(行号基准 c339b58)。
- 内容: runner/ 按 B1 event→B2 metrics→B3 redundancy→B4 context→B5 compaction→B6 tool_exec→B7 subagent→B8 drive 八批;store/ 按 S1 拆壳(connection/path 转 pub(crate))→S2 episodes→S3 notifications→S4 events→S5 inbox→S6 session→S7 schema(migrate 原样搬不重构)→S8 测试分域八批;mod.rs pub use 平铺保持 kanzei_core:: 顶层再导出零变更;测试随域下沉不建大 tests.rs,共享测试辅助建 #[cfg(test)] pub(crate) mod testutil。设计: docs/design/monolith_decomposition.md §C。
- 边界: 零行为变更;run_once 保持 boxed 签名(与 run_subagent 递归的断点,改 async fn 立刻 E0072,两处加注释锁死);run_once_with_parts(778 行)只整体搬迁;不删零调用 pub 方法;唯一允许的非 move 改动是给 RedundancyWatch::note_step 加 debug_assert_eq!(calls.len(), results.len()) 与三处下标不变式注释。
- 验收: ①每批独立提交且定向绿(cargo test -p kanzei-core + cargo check -p kanzei -p kanzei-app -p kanzei-tools),条目关闭前全量 cargo test --workspace 一次全绿(节奏见 conventions §1.4);②lib.rs 与外部三 crate(kanzei/kanzei-app/kanzei-tools)全程零改动仍编译(以①的 cargo check 为每批断言);③runner.rs/store.rs 单文件消失,各子文件 ≤900 行;④下标不变式 debug_assert 与注释落位;⑤拆前后行数对照记入进展。
- refs: A-008
- 依赖: R-152

## R-156 全仓 fmt 收敛并启用 fmt 闸门 [todo]
- 优先级: P2
- 复杂度: 小
- 标签: 流程
- 来源: 2026-08-09 实测 cargo fmt --all -- --check 有 435 处 diff——fmt 闸门首日启用 CI 必红;且全仓格式化会使拆解设计文档的行号地图漂移,故单列一条排在拆解之后。
- 内容: ①`cargo fmt --all` 单独一个纯格式化提交(不混任何业务/重构改动);②取消 ci.yml 与 scripts/verify.ps1 里注释着的 fmt 步骤(两处同启);③实测闸门会拦:临时引入一处格式漂移验证非零退出后撤销。
- 边界: 与 R-146(clippy)同理必须避开在飞的源码工作;两条可同轮或相邻轮做,均在 R-153~R-155 完成之后。
- 验收: ①cargo fmt --all -- --check exit=0;②ci.yml 与 verify.ps1 的 fmt 步骤启用且 CI 全绿;③拦截实测记入进展;④格式化提交 diff 零逻辑变更(全量测试全绿佐证)。
- refs: R-152 R-146
- 依赖: R-153 R-154 R-155

## R-157 验证与提交节奏引擎化:kanzei.toml 可调参数并注入循环 [todo]
- 优先级: P1
- 复杂度: 中
- 标签: 核心
- 来源: 2026-08-09 用户定调:全量测试触发频率与 git 提交频率明显拖慢开发效率,应做成参数可调("稳定性不错"但每提交一次全量把验证成本乘在提交频率上)。规则层默认值已先行落 conventions §1.4(立即生效),本条把参数做进引擎。
- 内容: ①kanzei.toml 新增节奏配置节(如 [cadence]):full_test(entry_close|every_commit|every_n_batches(n)|release_only)、targeted_test(every_commit|off)、commit(per_batch|per_entry)、push(per_commit|per_entry|periodic);serde default 取 conventions §1.4 当前默认,旧配置无该节行为不变(conventions §4 向后兼容);②设置页透传全部字段,保存不丢字段;③鞭挞/自主循环把生效节奏渲染进注入提示词——DEFAULT_CONTINUE_PROMPT 规则 6 的验证文案参数化,LEGACY_CONTINUE_PROMPTS 静默升级机制同步(防 D-163 类契约错位);④push=periodic 与 R-143 并轨,不重复造。
- 边界: 发版门禁(verify.ps1 全量)与 CI push 全量不受参数影响(A-010 底线);动 main.rs/main.js 的部分不与拆解批并发。
- 验收: ①full_test 各档在注入文案里可见且实测生效(轨迹证据);②旧 kanzei.toml 无节奏节时行为与 §1.4 默认一致(serde default 单测);③设置页改参数→保存→重开生效且不丢字段;④鞭挞文案参数化后 LEGACY 升级路径有测试;⑤conventions §1.4 标注「引擎已接管,改参数走设置页/kanzei.toml」。
- refs: R-143 A-010 R-152
- 依赖: R-153 R-154

## R-050 并行对话线程与分支工作树:隔离运行、冲突检测与合并 [todo]
- 复杂度: 大
- 优先级: P2
- 来源: 用户反馈:历史对话或新开线程并行推进项目,类似 git 分支/树,最后解决冲突合并
- 验收: 设计文档明确线程/项目/工作树关系、锁顺序、取消与崩溃恢复;两个线程可独立运行且互不串消息/权限/活动/停止;写入冲突能在提交前检测并阻止自动覆盖;worktree 模式可查看 diff、选择合并或放弃;合并失败保留双方改动和可恢复入口
- 已完成: 线程隔离(=R-030 进程页签)真实可用,消息/权限/队列/活动/停止按 session 隔离并有 POC 测试;worktree 后端命令 create/diff/merge/discard 存在,merge 前的 `git merge-tree --write-tree` 冲突预检真实实现(kanzei-app/src/main.rs:671-684);设计文档 deep_parallel_dev.md(含附录早期 POC)覆盖线程/工作树关系与锁顺序,是 R-050 方案的唯一承载。
- 退回原因: 2026-08-07 验收核查发现核心组合未成立,勾不该打。①worktree 与线程完全脱节:ProcessHandle.worktree_path 恒为 None(main.rs:164/523,全仓库无 Some 赋值),process_create 不接受 worktree 参数,run_prompt 校验进程必须属于主项目目录(2605-2607)——没有任何线程能在 worktree 里运行,所有并行线程写同一工作目录;应用内无流程会在 worktree 分支产生提交,"合并"在闭环内空转。②多进程同一工作树无任何写冲突检测,设计承诺的项目写锁/git 锁/docstore 版本哈希在代码中完全不存在。③"可查看 diff"实为 git status --porcelain 文件名列表弹 toast(见 D-096)。④崩溃恢复仅设计文字,worktree 清单存 localStorage 不从 git worktree list 发现。
- 下一步: 按 deep_parallel_dev.md 分阶段推进:先让进程可绑定 worktree 并在其中运行(打通 worktree_path),再补同工作树并行的写冲突防护,最后接 diff 查看器。注意该文 §6 决策点 D1~D7 未经用户定案前不得动工(G-003 门禁)。
- 遗留质量问题: worktree 四个命令零测试;worktree_field 的 field 参数是无效分支(main.rs:605-610 两分支返回同值);frontend_phase3.md 的 POC 章节重复粘贴两遍且第一遍路径写错。
- refs: R-030 D-096
- 阶段: 5
- 证据等级: E2+E3
- 设计定位: 功能需求(2026-08-08 用户定调:R-093 的"质量先行"阶段门槛作废,按普通优先级参与取活)

- 标签: 核心

- 进展: 2026-08-08 复核:门禁仍成立,保持 todo 不启动;定案前如队列触达本条目只可完善设计文档或调研,不改代码。
- 阻塞: 用户: 需先对 docs/design/deep_parallel_dev.md §6 决策点 D1~D7 逐条定案(G-003 门禁明文:未定案前不得开始实施)。解除动作:用户审阅设计文档并逐点拍板 D1~D7(可全部采纳或逐条修改),定案后本条目解除。

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

## R-101 桌面端/前端 E2 测试 harness 与延期 E2 清单 [todo]
- 复杂度: 大
- 优先级: P0
- 归属: kanzei
- 背景: 多条缺陷按 conventions §1.2「可用即关闭」关闭,其验证增强项收拢至此,不再阻塞缺陷与需求推进;此前反复出现的阻塞原因是仓库无 package.json、无浏览器测试 harness,无法安全启动真实 Tauri UI。
- 验收: 建立可在测试基座安全启动真实 Tauri UI(或等价 WebView 驱动)的 E2 harness;逐项补齐延期 E2:D-051 桌面权限弹窗真实 UI E2;D-055 切回进程补发 pending ask 前端 E2;D-056 运行中切项目→终态复位 E2;D-060 update/close/reorder 手写内容保留与并发写入回归;D-064 注入故障的 run_task 收尾 E2;D-066 真实 Tauri Window/provider 停止 E2;D-086 runner 级 task→subagent read 拦截执行回归;R-139 bash 硬门禁桌面端真实模型工具调用 E2(2026-08-08 R-139 关闭时转入,验收条款外残余验证)。
- 拆批(2026-08-08 用户定调「拆出能先做的部分」): **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。**留待 R-086**——依赖会话事件路由的三条:D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位。基座 + 四条 E2 交付即可关闭本条,剩余三条并入 R-086 验收。
- refs: R-086
- 阶段: 3

- 标签: 流程

- 拆批: 2026-08-08 用户定调「拆出能先做的部分」: **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。基座 + 四条 E2 交付即可关闭本条;R-086 已于本轮按 §1.2 可用即关闭关闭,原「并入 R-086 验收」的三条桌面 E2(D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位)留在本条目验收清单执行。

- 进展: 2026-08-09 取活:本轮目标 = harness 基座 + 四条 E2(D-060/D-086/D-064/D-066);三条桌面 E2(D-051/D-055/D-056)属后续批次。 2026-08-09 卡点定位:CDP 驱动真实 WebView——窗口已改 setup 手动创建并注入 --remote-debugging-port(9a3cfca 已提交);实测参数被 WebView2 接受(进程命令行含 remote-debugging-port)但端口未监听(19 个 webview 进程 netstat 0 监听,fetch 全拒)。 2026-08-09 用户定案:选 A——改用微软/Playwright 官方标准路径,由 E2 脚本设环境变量 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 后拉起 kzapp,保证首个 browser 进程带参;放弃 additional_browser_args 通道(疑似被 WebView2 静默忽略)。 挂起(用户定调):本条先挂起,优先修小缺陷 D-188→D-187→D-185→D-184,修完再回来走 A。探针 scripts/probe-webview-cdp.mjs(v13)留工作区未提交。
- 状态纠正(2026-08-09): doing→todo。用户已挂起本条,实际不在推进中,却按旧 §1.1 口径占用 doing 名额,与 R-148 一起把 R-153 拒之门外(见 D-219)。恢复推进时再转 doing;挂起前提的小缺陷中 D-185/D-184 仍 open。

## R-102 CLI 只读运行档位:分析类任务免配权限直接跑 [todo]
- 复杂度: 中
- 优先级: P2
- 归属: kanzei
- 背景: 2026-08-07 用 kz 做只读前端分析:agent 首选 bash 触发询问,非交互场景直接拦停;唯一出路是给沙盒放行 `bash *`,权限粒度与"只是分析别动文件"的意图之间缺一层表达。
- 验收: 提供只读档位(如 `kz run --readonly` 或 profile):read/glob/grep/task 放行,write/edit 硬 deny,bash 限制为无副作用或直接禁用并提示替代工具;非交互终端下只读任务可零配置完整跑完;补档位权限快照测试。
- 设计定位: 让"问问题/做分析"成为 kz 的零门槛入口
- 阶段: 4

- 标签: 核心

- refs: D-121

## R-103 Memory 系统总纲:文件优先、分级、子代理管理 [todo]
- 移交: 2026-08-08 用户宣布移交自举循环。M1~M4 已落地并在实测中,后续完善由循环承接;设计基线见 docs/design/memory_system.md,改动不得偏离其 §0 品味决策(文件优先、不引向量库/图谱、读写分离)。
- 复杂度: 大
- 优先级: P0
- 归属: kanzei
- 来源: 2026-08-08 用户定调的下一个大规划(用户为记忆研究方向,taste 已对齐)
- 内容: 以 docs/design/memory_system.md 为设计基线。五个目标:提高易用性、上下文管理更精准、用户个性化持久化、常用轨迹效率提高、agent 工作效率提高。核心决策(不再重议):文件优先(markdown 真源,可编辑可透明,git 可恢复);不用向量库/知识图谱/Mem0 类框架,给 agent 好的搜索工具(FTS5+结构化过滤);记忆写读分离,写路径由 memory-manager 子代理专管;分级 = scope(global/project) × category(preference/habit/fact/sop/episode);agent 既是用户,验收全部取自举轨迹实证。
- 验收: R-104~R-107 四期全部落地;连续自举轮次中出现"写入→检索命中→避免重复探索"的闭环实证;记忆内容全部可 git 恢复;SQLite 仅存可重建派生物(FTS 索引/hits/episode 表)。
- 设计: docs/design/memory_system.md
- refs: R-098 R-099 D-088 D-114 R-104 R-107
- 阶段: 4
- 设计定位: 记忆作为 first-class primitive 的总纲与门禁
- 依赖: 

- 标签: 核心

- 进展: 依赖复核:R-105(M2)、R-106(M3)均已关闭,依赖已满足移入 refs,阻塞字段清空。R-103 总纲本身的推进依赖子代理/写工具现状的重新盘点,下一轮接手时以 docs/design/memory_system.md §0 品味决策为准。

## R-111 需求缺陷依赖的组织与可视化 [todo]
- 标签: 后端
- 复杂度: 中
- 优先级: P2
- 归属: kanzei
- 来源: 2026-08-08 用户:依赖设计合理,但求更好的组织形式与可视化
- 内容: 现状 refs/依赖 是自由文本,无校验无方向语义。改造:①字段语义分立——`依赖:`(阻塞关系,本条完成前置)与 `refs:`(关联参考,不阻塞)在引擎侧校验 ID 存在且区分方向;②tracker 输出条目时附带反向链接(谁依赖我);③独立文档页给依赖视图:按依赖拓扑分层的列表(可做层/被阻塞层),点击条目高亮其依赖链;暂不做图形化 DAG 画布(重,收益存疑,列表+高亮已覆盖主要场景)。
- 验收: 依赖引用不存在时工具告警;条目详情含正反向链接;文档页有"被谁阻塞/阻塞谁"视图且切换流畅;循环依赖检测告警。
- refs: R-054 D-112
- 阶段: 4

## R-112 需求缺陷分类体系标准化 [todo]
- 标签: 流程
- 复杂度: 中
- 优先级: P3
- 归属: kanzei
- 来源: 2026-08-08 用户:需求和缺陷应该要分类
- 内容: 现状标签(标签/类型/领域)是自由文本,同义词发散不可聚合。改造:①收敛为两级受控词表——`领域`(单选:engine/provider/session/permission/tracker/memory/ui/release/process)与 `类型`(单选:功能/质量/性能/安全/体验/流程),词表在 conventions 定义、引擎枚举校验,自由 `标签` 保留但降为辅助;②既有条目批量归一(同义词映射表);③文档页按领域/类型双维筛选与计数。
- 验收: 词表外的领域/类型被引擎拒绝并提示合法值;存量条目 100% 归一;文档页双维筛选可用;quick capture 自动建议分类。
- refs: R-054
- 阶段: 4

## R-117 子代理运行状态的可观察性 [todo]
- 复杂度: 中
- 优先级: P3
- 原始描述: 添加触发后弹出浮层显示最近开发和当前进展列表
- 范围界定: 2026-08-08 用户澄清真实意图是"子代理能对当前运行状态进行观察",并明确表示在 R-095 的呈现优化落地后不确定是否仍需要独立入口。
- 待定: 本条挂在 R-095 之后再定去留。R-095 交付后由用户判断:若活动面板的筛选折叠与后台任务操作已足够观察子代理状态,则本条关闭;若仍缺子代理各自的进度维度,则按缺口重写验收。
- 依赖: R-095

- 标签: 前端

## R-122 构建可视化架构浏览与维护内存设置功能 [todo]
- priority: P2
- 原始描述: 缺少一个架构浏览，也是要让agent维护，可视化做好一点，和设置记忆这些同级目录，要慎重选取技术栈
- 复杂度: 中
- 归属: kanzei
- 验收: 实现可视化架构图/浏览器，支持维护记忆等配置信息，并完成技术栈选型评估报告

- 标签: 前端

## R-128 全部阻塞时停止鞭挞的逻辑设计 [todo]
- priority: P2
- 原始描述: 如果全部阻塞，应该要停止鞭挞，需要更多的设计鞭挞停止的逻辑
- 复杂度: 中
- 归属: kanzei
- 验收: 当全部条目处于阻塞状态时,系统自动停止鞭挞,不再触发催办;阻塞解除后可恢复

- 标签: 核心

## R-129 单页阅读信息记忆困难优化 [todo]
- priority: P3
- 原始描述: 记忆单页阅读信息太复杂，有阅读障碍
- 复杂度: 中
- 归属: kanzei
- 验收: 提供分段展示/摘要功能帮助用户理解单一页面内容，减少认知负荷

- 标签: 前端

## R-130 测试用例记录触发机制与缺陷迁移 [todo]
- 原始描述: 测试用例相关的记录似乎没有触发机制，然后是把测试移动到缺陷下面，然后需要一次性记录存性
- 复杂度: 中
- 归属: kanzei
- 验收: 实现基于事件的或手动触发的测试用例记录机制，并在系统中建立测试到缺陷的映射关系，完成现有机现有测验数据的批量导入和初始化。
- 优先级: P2

- 标签: 后端

## R-131 设置页面部分内容支持折叠(如操作命令) [todo]
- 原始描述: 设置页面的一些显示该折叠折叠比如操作命令
- 复杂度: 小
- 归属: kanzei
- 验收: 设置页面中操作命令等较长内容默认折叠展示,点击可展开/收起
- 优先级: P2

- 标签: 前端

## R-132 mem单页手动触发整理功能 [todo]
- priority: P2
- 原始描述: mem单页应该有个可以手动触发的整理，这个需要详细设计，先记录吧
- 复杂度: 中
- 归属: kanzei
- 验收: mem单页提供手动触发整理的入口，触发后执行整理流程并给出结果反馈

- 标签: 核心

## R-133 diff树渲染优化 [todo]
- 原始描述: diff树的显示很丑，标记颜色并且不要重叠
- 复杂度: 中
- 归属: kanzei
- 验收: 实现color标记的git diff树，解决重叠问题确保视觉清晰
- 优先级: P2

- 标签: 前端

## R-134 需求和缺陷记录需要分类 [todo]
- priority: P2
- 原始描述: 需求和缺陷记录的时候需要分类
- 复杂度: 小
- 归属: kanzei
- 验收: 实现需求/缺陷记录的类型区分机制

- 标签: 后端

## R-135 开发与缺陷修复进度动画显示 [todo]
- 优先级: P0

- 标签: 前端

## R-137 Anthropic thinking 块协议回放:signature 原样回传,多轮工具不再 400 [todo]
- 背景: direction_taste 复刻清单·高:CC 按协议要求回放 thinking 块;kanzei 现状 anthropic.rs:97 Part::Reasoning => None 丢弃全部 Reasoning,thinking+工具第二轮必 400(R-094 只做了请求侧思考强度,未做响应侧回放)。
- 设计定位: 复刻 CC 基线行为:thinking 块按协议要求回放
- 证据等级: E2
- 阶段: 1
- 验收: anthropic 通道多轮工具调用时:①thinking 块的 signature 在后续请求中原样回传;②thinking+工具第二轮不再 400;③非 thinking 模型的 reasoning 文本以可见 assistant 文本保留(与 R-094 结论一致);④补 anthropic 多轮含 thinking 的协议契约测试。

- 优先级: P0

- 标签: 模型

## R-138 docstore 原子写与跨进程文件锁:tmp+rename + 独占句柄,并发写不丢不撞 [todo]
- 背景: direction_taste §5.2 地基债:docstore 整文件重写无原子替换与跨进程锁,D-064 类 lost-update 真实存在;deep_parallel_dev §3.3 P4 也要求 docstore 进程级文件锁收口主根 .kanzei 的最后一个共享写点。
- 设计定位: tracker 文档写入的原子性与并发安全
- 证据等级: E2
- 阶段: 1
- 验收: docstore save 改 tmp+rename 原子替换(临时文件与目标同目录);跨进程文件锁(Windows std::fs 独占句柄,毫秒级持有);并发写 tracker 的压测不丢条目不撞 ID;失败时保留现场可重试。

- 优先级: P0

- 标签: 核心

## R-140 i18n 架构迁移:chrome/content 分离、t(key) 渲染点翻译、MutationObserver 退役 [todo]
- 背景: direction_taste 定调二(用户明确):i18n 保留换架构。现行词典+MutationObserver 已产出 8 条缺陷家族(D-092/D-108/D-129/D-135/D-136/D-142/D-157/D-160)并篡改模型输出显示;D-172 只修了死循环,未换架构。四铁律:chrome/content 分离、翻译发生在渲染点 t(key)、模型输出语言是 prompt 问题、漏译可机械检出。
- 设计定位: i18n 架构迁移:先止血再渐进 key 化
- 证据等级: E2+E3
- 阶段: 1
- 验收: ①消息容器子树整体豁免词典替换(立即止血,终结数据篡改);②静态 DOM 改 data-i18n 一次性应用、JS 动态字符串经 t(key,params) 产出,禁止事后全文档扫描改写;③MutationObserver 退役;④漏译回落中文原文,冒烟脚本加 key 覆盖率断言;⑤按 A-003 粒度一轮吃一个界面域直至词典机制退役。

- 优先级: P0

- 标签: 前端

## R-141 ToolCtx 显式主根绑定:消除发现式取根与 worktree 锁键歧义 [todo]
- 背景: direction_taste §5.4 与 D-170 教训:ToolCtx::new 仍发现式取根(harness/src/tool.rs:13-17),worktree 线若命中 worktree 内 .kanzei 副本会拿到过期身份;并发锁键语义(tool.rs:19-28)只拼 project_root,两棵树同路径会撞锁。deep_parallel_dev §3.2 明确选 A:显式主根、不做根发现。
- 设计定位: 深并行前置:线进程显式携带主根,消除发现式根解析事故面
- 证据等级: E2
- 阶段: 1
- 验收: ToolCtx 构造支持显式传入 project_root(不再无条件 discover);线路径全程显式传根;补断言测试:worktree 内运行时 project_root 必须等于主根;并发锁键区分 worktree 实例。

- 优先级: P0

- 标签: 核心

## R-142 前端最低配 ESLint:no-undef 防手误,无构建步骤 [todo]
- 背景: direction_taste §5.2 地基债:前端 main.js 6254 行无任何 lint,手误靠运行时发现(报告 E3);no-undef 是最小有效护栏。
- 设计定位: 前端静态检查最低配,防未定义变量类回归
- 证据等级: E1
- 阶段: 1
- 验收: 引入最低配 ESLint(flat config,只开 recommended+browser env 的 no-undef 类规则),不引入构建步骤;main.js 无未定义变量错误;新增/修改前端文件后 lint 可跑且纳入冒烟脚本。

- 优先级: P0

- 标签: 流程

## R-143 自举循环定期自动 push:完成批提交后自动推送,失败可见不阻断 [todo]
- 背景: direction_taste §5.2 地基债:自举循环完成工作后依赖 agent 自觉 push,工作树长期不推风险堆积;定期自动 push 作为基线保障。
- 设计定位: 自举循环的提交自动推送保障
- 证据等级: E1
- 阶段: 1
- 验收: 自举循环每完成一批提交后自动 git push(或提供周期性的 push 时机),push 失败可见且不阻断后续轮次;与既有手动 push 流程共存不冲突。

- 优先级: P0

- 标签: 流程

## R-144 验收核查周期化:鞭挞每关 N 条自动插入只读核查回合 [todo]
- 背景: direction_taste §5.5:08-07 式事件性审计(R-092 手动按钮)应变成常驻节律——鞭挞每关 N 条自动插入一轮只读核查回合,复用现有只读子代理,把验收打假从人工触发变为自动循环的一部分。
- 设计定位: 自举质量的常驻核查节律(§5.5)
- 证据等级: E1
- 阶段: 2
- 验收: 鞭挞/自主推进每关闭 N 条(可配)自动插入一轮只读核查(复用 SubagentBase read/glob/grep):核对已完成条目的验收证据与真实调用方;发现问题时生成候选缺陷或退回依据;核查不进入主 conversation/queue;触发频率与 N 可配置。
- 优先级: P0

- 标签: 流程

## R-145 Memory 闭环实证:发版后轨迹命中与 token 基线对比 [todo]
- 优先级: P2
- 内容: 承接 R-105 验收①(连续自举轮次完整闭环实证:轮末写入→后续轮命中→避免重复探索,以轨迹为证)与 R-106 验收①(同类任务每轮注入 token 较基线下降且无因信息缺失导致的返工)。两者均需发版后在真实自举循环中取轨迹对比,不可本机验证;代码项已随 R-105/R-106 交付,本条目只跟踪实证落地。
- 复杂度: 小
- 标签: 流程
- 阶段: 5
- 验收: 自举循环发版运行 N 轮后,提供轨迹证据:①轮末记忆写入被后续轮检索命中且避免重复探索;②同类任务注入 token 较基线下降且无信息缺失返工。证据形式:episodes 落库记录、context_report 账单查询结果、轨迹摘录。

## R-146 clippy 警告清零并设闸门,此后不再悄悄回涨 [todo]
- 优先级: P2
- 复杂度: 小
- 标签: 流程
- 阶段: 2
- 依赖: R-152 R-153 R-154 R-155
- 依赖说明(2026-08-09): 闸门落点定为 ci.yml/verify.ps1 里注释着的 clippy 步骤(R-152 落地);lint 收敛的全仓 diff 会与巨石拆解大搬迁撞车并使 monolith_decomposition.md 行号地图漂移,故排在 R-153~R-155 之后,与 R-156(fmt)相邻轮做。
- 来源: 2026-08-09 用户定调「加需求里让他自举」。当前 `cargo clippy --workspace --all-targets` 0 error、约 23 条 warning(needless_borrow×7、redundant_clone×3、map_or 可简化×2、redundant closure×2、sort_by_key×2、too_many_arguments×2、复杂类型/手写字符比较/可写成 for 循环/两处 unused assignment 等)。此前 deny 级 never_loop 曾让整个 workspace 的 clippy 编译不过(D-197 顺带修掉),warning 不清则同类问题混在噪声里看不见。
- 内容: ①清零:逐条修掉现存 warning;确属合理的(如参数多但拆结构体属 churn 的 too_many_arguments)用 `#[allow]` 就地压制**并写明理由**,不许裸 allow。②设闸门:让 warning 无法再悄悄回涨——scripts 或 CI 任一位置跑 `cargo clippy --workspace --all-targets -- -D warnings` 并在非零退出时失败;package.ps1 构建前挂上即可。
- 边界(必须遵守): 纯 lint 收敛,**禁止顺手重构**——不改函数签名、不拆结构体、不动行为;每类 lint 一个提交或全部一个提交均可,但 diff 里只允许 lint 相关改动;改完跑全量测试,任何测试变红即回退该处改法。挑没有其它源码工作在飞的时段做,避免与并发提交撞车。
- 验收: ①`cargo clippy --workspace --all-targets -- -D warnings` exit=0;②每个 `#[allow]` 带一行理由注释;③闸门落地且实测会拦(临时引入一条 warning 验证非零退出后撤销);④workspace 全量测试通过,无行为改动(git diff 审计不含逻辑变更)。

## R-147 增加使用手册与作者话内容板块 [todo]
- 复杂度: 中
- 归属: kanzei
- 验收: 页面顶部新增独立区块，展示项目使用手册和来自作者的说明文字
- 优先级: P1

## R-148 文件导览:VSCode 级浏览页 + files 工具 + AI 用途标注 [doing]
- 优先级: P1
- 复杂度: 大
- 标签: 前端
- 阶段: 2
- 归属: kanzei
- 来源: 2026-08-09 用户定调,并明确"由当前会话直接交付,不走自举"。两条设计决策:①文件预览要 VSCode 对标级别(引 Monaco vendor,不做简化实现;项目无零依赖约束——那是 frontend.rs 单工具的局部取舍,不得外推);②度量中性呈现,不点名"该拆"——页面定位是人类主动分析与 agent 辅助分析的架构地图,行数多不必然要拆,拆分判断结合 AI 用途标注由人/agent 做。
- 内容: 一份扫描器两个消费者。①扫描器(kanzei-tools):git ls-files 拿清单(尊重 .gitignore,非 git 目录退化为过滤遍历),每文件度量=大小+代码行数(按扩展名)/md 字数(字符数),>2MB 只 stat 标「过大」;按目录聚合文件数/总大小/总行数。②agent 工具 `files`:文本树输出,支持 path 子树与 top-N(按行数降序),只读 Allow;已有 AI 标注时随树输出——弱模型不必逐个 read 就知道每个文件是干嘛的。③桌面端新主视图「文件」:树形浏览器(展开/折叠/排序),行尾度量徽章,目录行聚合值;点击文件用 Monaco(vendored,只读)打开,语法高亮/行号/折叠/搜索原生。④AI 用途标注:fast 模型按文件头部 60 行生成一句话用途,目录级聚合;缓存 .kanzei/file-annotations.json 按内容 hash 失效;页面手动「标注」按钮触发,后台增量,不自动烧。
- 验收: ①files 工具:树/子树/top-N 三种调法有单测,输出含度量与(已有的)标注;②页面:真实仓库(184 文件)树渲染流畅,目录聚合正确,Monaco 打开 6000+ 行文件语法高亮可用;③标注:首次全量后改动单文件只重标该文件,缓存命中不调模型;④冒烟覆盖新视图切换与树渲染;⑤用户复查"直观"达标。
- 进展(2026-08-09 代码全交付): ①扫描器+files 工具(kanzei-tools/src/files.rs,3 单测:扫描度量/树与 top 渲染/标注缓存回环),注册进 BaseComponent 全 profile 可用,权限 Allow;②Tauri 侧独立模块 files_view.rs(不再往 6400 行的 main.rs 里堆):files_snapshot/file_preview(canonicalize 逃逸检查+二进制识别+4MB 截断,2 单测)/files_annotate(fast 模型逐文件一句话+目录聚合,增量按指纹,每 8 个落盘,进度事件);③Monaco vendored 5MB(min/vs 裁掉 language 智能服务 7M 与 8 个非中文 nls——语法高亮在 basic-languages,只读预览不需要补全):懒加载,暗色主题,只读,minimap;④前端新主视图「文件」:树形展开/折叠/键盘可达,名称/行数双排序,目录聚合行,标注随行显示,i18n 23 词条;⑤运行时冒烟新增树渲染断言(目录聚合/展开后文件度量/md 字数/标注)。workspace 310 项全绿+四条前端冒烟绿。剩余验收⑤(用户复查直观达标)与真实仓库 Monaco 实测待发版安装后确认。
- refs: D-173 R-126
- 阻塞: 用户: 剩余仅验收⑤——build-cd85360 已发布并含本功能,用户在桌面端打开「文件」视图复查"直观达标"并实测 Monaco 打开 6000+ 行文件即可解除;复查通过即按验收关闭。(2026-08-09 补记:属 §1.1 ①类外部阻塞,按新口径不占 WIP 准入配额)

## R-150 记忆决策价值 P2:空闲整理与 UI 消费零采纳与复发清单 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 前端
- 阶段: 2
- 依赖: R-149
- 来源: 同 R-149,P2 移交自举循环。
- 内容: 消费 R-149 产出的决策价值信号:①空闲整理(sleep-time)把「零采纳候选」(召回≥3 采纳=0)与「复发告警」纳入整理清单,处置走既有墓碑机制(降级/修订/归档),不静默删;②Memory UI 页展示每条目的召回/采纳率与复发告警,零采纳候选有显式标记;③与 R-145 并轨:发版后取自举轨迹验证「写入→命中→避免重复探索」闭环,并复核 R-149 降权参数(0.6/0.7/阈值 3)是否合适——复核须计入两个采纳率低估通道:「看索引行即用」与「直接 read 记忆文件不经 memory_search 不计采纳」(后者可考虑给 read 加记忆目录钩子回填 mark_recall_fetched);同批决定 hits 因子去留——hits 奖励「常被搜到」(自增强)与采纳率权重惩罚「召回未采纳」方向冲突,候选处置:退役或降为平局破除器。
- 验收: ①空闲整理清单包含零采纳与复发两类候选且处置有墓碑;②Memory 页可见召回/采纳数据(800/1024/1280 三档可用);③降权参数复核结论落回 docs/design/memory_decision_sufficiency.md 变更记录。
- refs: R-103 R-107 R-125 R-145

## R-151 用户约束的机械捕获通道:对话定调不再靠主 agent 自觉投 note [todo]
- 优先级: P3
- 复杂度: 中
- 标签: 核心
- 阶段: 2
- 依赖: R-150
- 来源: 2026-08-09 R-149 全环节评审结论:论文里决策价值最高的信息形态(用户在对话里随口说的约束,如「以后别动 production」)目前完全依赖主 agent 自觉 memory_note,是写入环节唯一没有机械通道兜底的缺口;用户拍板「占位,等 R-150 遥测数据积累后再评估值不值得做」。
- 内容: 占位。方向:轮末由引擎对本轮用户消息做机械提取(候选形态:祈使+否定/「以后」「必须」「不要」类定调句),投 preference/habit 候选进 inbox,由 manager 判 NOOP/ADD——引擎只采集不判语义,与 harvest_failures 同哲学。是否立项取决于 R-150 遥测:若真实轨迹里出现「用户说过但没进记忆、后续违反」的实例,则升优先级动工;若 memory_note 自觉率足够,关闭本条。
- 验收: 先出判定报告(基于 R-150 遥测与轨迹实证,给出做/不做结论与依据);若做,再补机械提取的功能验收。
- refs: R-149 R-105

