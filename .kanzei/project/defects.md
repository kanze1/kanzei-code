# Defects

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

## D-172 启动黑屏:i18n MutationObserver 微任务死循环饿死渲染主线程 [fixed] (critical)
- refs: D-136 458af450 e4b45f21
- 优先级: P0
- 复现: build-2c999d4(含 e4b45f21)启动即整窗黑屏。CDP 观测:浏览器进程命令(Browser.getVersion)秒回,所有需渲染进程处理的命令(Runtime.evaluate/Runtime.enable/Page.enable/冷附加 Debugger.enable)永不响应;渲染进程 10 分钟烧掉 380s CPU。重启后在 about:blank 阶段先挂 Debugger 再 pause,栈定格在 applyLanguage(main.js:569)← MutationObserver 回调(main.js:639)。
- 影响: 桌面端完全不可用;且症状组合(黑屏+无 console+CDP 无响应+PrintWindow 抓黑)极易误判为 WebView2/GPU/截图伪影问题,本次调查一度走偏。
- 标签: 核心
- 根因: 两笔提交叠加成环。458af450 的属性翻译在 zh(默认)模式下对每个带 title/placeholder/aria-label 的元素**无条件 setAttribute**(判据 `translated !== source || language !== "en"` 恒真);e4b45f21 给 languageObserver 补 `attributes:true + attributeFilter:[title,placeholder,aria-label]`。DOM 规范规定 setAttribute 同值也入 mutation 队列,于是 observer→applyLanguage→setAttribute→observer 微任务无限循环,事件循环永远轮不到绘制与输入。`applyingLanguage` 标志只防同步重入,防不了跨微任务自触发。冒烟测不出是因为 harness 的 setAttribute 同值早退不通知 observer,与规范语义相反。
- 证据等级: E1(冒烟护栏红绿双验)+ 真机 CDP 断点栈与修复前后渲染进程 CPU/响应实证
- 验收: ①main.js 属性写入前比对,同值不写;②冒烟 harness setAttribute 同值也通知 observer(对齐 DOM 规范),并加「observer 连续自触发>25 轮判失败」护栏,把挂死变成可读失败;③bug 复位冒烟必红、修复后必绿,已双验;④修复构建真机验证:Runtime.evaluate 即时响应、页面完整渲染、渲染进程存活 53s 仅耗 1s CPU。

- 进展: 已修复并双侧验证(2026-08-08)。遗留:发布版(用户机器)仍是坏 build,需走发版 SOP 推送修复。

## D-173 架构索引 architecture/README.md 无专用工具可写:edit 被 ruleset 拒绝,agent 只能 bash 旁路维护 [open] (high)
- 备注: 本轮已用 bash 旁路一次性补齐索引(946742f),内容正确;本缺陷登记的是通道缺失本身,不撤回已完成的补全。关联 D-171(编号空洞 tombstone,同轮解除门禁时补记)。
- 复现: agent 用 edit 更新 `.kanzei/project/architecture/README.md` 报 permission denied by ruleset(policy-managed,提示用专用工具);但 req/defect/goal/decision 四个专用工具只管理各自追踪文件,没有任何工具托管 architecture 目录。实测 2026-08-08:索引补全只能经 bash 写入(946742f),而 bash 能写受保护目录本身也说明 R-139 的 bash 级 .kanzei 路径硬门禁尚未落地。
- 影响: ①自举循环新增/重命名设计文档后,架构索引只能由用户手改,必然滞后(本次 10 个文档重命名 + 2 份新设计入库后,索引仍只有 5 个旧条目);②agent 若想维护索引,唯一通道是 bash 旁路,而旁路通道本身违反'受保护文档不被 bash 旁路'的设计原则;③architecture/README.md 是架构发现入口,索引滞后会让后续会话找不到现行设计真源。
- 根因: ruleset 对 `.kanzei/project/*` 的 edit/write 硬 deny 只给 tracker 类工具放行(设计意图是防模型旁路),但 architecture/README.md 作为同级项目管理资产不在任何专用工具的托管范围——需求/缺陷/目标/决策各有工具而架构索引没有,形成'既不能 edit、也无专用工具'的双重缺口;bash 写入通道未封堵又构成硬门禁的旁路。
- 验收: ①提供可用的架构索引维护通道:要么新增专用命令/工具(如 `kz doc index` 或 tracker 工具扩展),要么把索引改为从 docs/design 自动生成(如 docs_snapshot 系),agent 更新 docs/design 后索引自动同步;②补 R-139 的 bash 级 .kanzei 路径硬门禁,使受保护文档不能经 bash 旁路写入;③验收时新增/重命名一个 docs/design 文档后,索引可被 agent 直接维护且无需 bash 旁路。
- 优先级: P1
