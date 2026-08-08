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

