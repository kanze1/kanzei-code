# 前端迁移原生 ESM:勘察结论与迁移前置条件

- 状态: **B1/B2 前置条件已完成，正式 ESM 迁移未收口**。对应条目 R-264(P3)。
- 本文是 2026-08-15 全量审计的更新版：当前前端规模为 24 个文件、15,528 行；B1/B2 已在现有经典脚本冒烟 harness 上完成，剩余迁移风险集中在 B3 生产测试钩子边界。

## 一、结论先行

前端本身不是障碍。B1/B2 harness 前置条件已经完成，但正式 ESM 迁移仍受**测试钩子与跨文件求值语义**约束：`ui-runtime-smoke.mjs` 仍须同时守住经典脚本和 ESM 的验证能力。

而且收益的一半已经被 `20467db` 拿走了:`gen-ui-lint-globals.mjs` 曾把函数体内缩进的局部声明
误收进「跨文件全局」白名单(历史审计中 1364 个条目里 777 个是这么来的,且都是 `el`/`row`/`text`/`id`/`key`
这类极常见名字),导致 `no-undef` 对它们全线失效。该缺陷已修复；现有符号清单与编号约定继续作为迁移基线。
"补偿机制不可靠"这条动机因此不再成立。

**对自举无收益**:ESM 不影响 cargo 任何耗时,前端六个冒烟合计约 4 秒。唯一收益是模型读代码时
`import` 语句自带溯源；现有符号清单 + `01-core`/`02-i18n` 编号约定已覆盖大部分阅读溯源需求。

## 二、迁移前置条件与当前完成度

B1/B2 已完成 harness 前置改造；B3 仍是正式 ESM 迁移前必须解决的生产测试钩子边界。

### B1（已完成） `scripts/ui-sources.mjs` 重写 —— 含一个静默失效陷阱

第 11 行的正则 `/<script\s+src="([^"]+)"[^>]*>/g` 要求 `src` **紧跟**在 `<script ` 之后。
ESM 写法 `<script type="module" src="...">` 让 `type=` 抢在前面 → 零匹配 → 第 12 行 throw,
四个冒烟(runtime/a11y/i18n/markdown)一起红。

**这条好修,修完才是真陷阱**:单入口 ESM 下 `index.html` 只剩一个 `src`,于是
`loadUiSources().joined`(现为 24 个文件全文拼接)只剩入口那点内容。a11y/i18n/markdown
三个冒烟全部对 `joined` 做正则/`includes` 断言——断言"存在"的会红一片,而断言
**"不存在"的全部恒绿**(`assert.doesNotMatch`、`!includes`、`missingKeys` 为空)。

正确改法:`loadUiSources` 不再从 HTML 取清单,改为遍历 `ui/*.js` 全目录并排序;
**并加文件数下限断言**(如 `>= 24`),防止再次静默退化。

### B2（已完成） `scripts/ui-runtime-smoke.mjs` 的执行模型重建

`:1104` 的 `vm.runInContext(instrumented, sandbox, ...)` 只能跑 classic script,源码里出现
`import`/`export` 立即 SyntaxError,被 `:1106` 的 catch 变成「ui/*.js 顶层执行抛异常」——
6799 行断言一条都到不了。

注意 `:1086-1087` 的注释:逐文件执行是**刻意设计**,为了复刻浏览器多 `<script>` 的 TDZ 语义
(拼接后一次执行会把函数声明提升到整串顶部,浏览器下会炸的 ReferenceError 在 vm 里反而跑通)。
这正是本仓真实存在的一类 bug——见下面 §三 列出的顶层跨文件读。**替换执行模型时必须保住这个能力**,
否则冒烟会开始放过它本来专门拦的东西。

两条路:`node --experimental-vm-modules` + `vm.SourceTextModule` 自建 linker(保住同 context
与 `runInContext` 探针能力),或改成动态 `import` 真模块(需先把 DOM 桩装进 `globalThis`,
且会让现有全部 `sandbox.X` / `runInContext("X")` 探针作废,须靠 export 重建)。

### B3（未完成） `__kzTest` 钩子被迫进生产代码

`:1112-1120` 把一段字符串当第 22 个脚本注入同一 context,靠共享全局词法作用域闭包捕获
`08-compose.js` 的模块级 `let`(`autoRounds`/`noActionRounds`/`autoStopReason`/`autoContinueTimers`/
`processAutoState`/`autoStopAfterRound`/`autoPaused`/`cancelAutoContinueTimer`),再挂到
`globalThis.__kzTest`,后续 49 处读写鞭挞内部状态。

ESM 下这些是模块私有,注入脚本看不见 → 求值即 ReferenceError。只能改成从 `08-compose.js`
`export`——而该处注释明写着「它属于冒烟注入层,**不属于生产代码**」。**这是一次设计decision的反转,
不是顺手改**:测试钩子将永久留在生产代码里。

## 三、前端侧的真实约束(审计实测)

规模:24 个文件、15,528 行、**44 处 `typeof X === "function"` 守卫**、**零重名冲突**。
每文件 import 量温和(多数 1–30 个符号,最重的 `07-events.js` 是 84 个)。

**零内联事件处理器** —— `index.html` 950-970 全是外链 `src=`,无内联代码块;
全量 grep `on(click|change|input|submit|error|load|keydown)=` 在 24 个 js 上零命中。
迁移的常见头号杀手在本仓是空的。

### 顶层跨文件读(ESM 下的 TDZ 硬约束,顺序不能乱)

| 消费方 | 读什么 | 提供方 |
|---|---|---|
| `05-chat-render.js:18` | `messages.addEventListener` | `01-core.js:166` |
| `08-compose.js:325-327,539` | `promptBox.addEventListener` | `01-core.js:167` |
| `08-compose.js:820,833` | 顶层 `new Map(Object.entries(readJson(...)))` | `01-core.js:140` |
| `03-shell.js:528` | 顶层 `initTheme()` → `applyTheme()` → `t()` | `02-i18n.js:846` |
| `07-events.js:151` | 顶层 `t("点击查看上下文成分")` | `02-i18n.js:846` |
| `08-compose.js:664,685` | 顶层 `renderAutoStatus()`/`syncAutoRunState()` 读 `let activeSessionId` | `03-shell.js:69` |
| `14-docs-actions.js:138-140` | 顶层读 `bgFilters.type/.status/.role` | `06-activity.js:165` |
| `14-docs-actions.js:186` | `bindGroupToggle` 内 `sync(apply(null))` 立即执行,读 `documentFilters` | `12-docs-pages.js:121` |
| `14-docs-actions.js:142` | 顶层把 `applyBatch` 当值传进 `addEventListener` | `11-docs-list.js:40` |
| `20-lines.js:798` | 顶层 `addEventListener("click", createWorktreeLine)` | `09-sessions.js:171` |

`18-startup.js` 必须最后(所以它在 `index.html` 里排在 19/20 之后,不是编号错乱):
它是立即执行的 async IIFE,同步段就用 `invoke`/`$`/`LANGUAGE_PREFERENCES`。

`04-markdown.js` 无任何顶层可执行语句,位置自由。

### 当前 `typeof X === "function"` 守卫（共 44 处，ESM 下语义反转,必须改成真 import）

`01-core.js:83,110,118,121,124`(`refreshParallelTaskProjection`/`handleBackgroundSessionDone`/
`cancelAutoContinueTimer`/`refreshConversationLists`)、`02-i18n.js:938`
(`markLanguagePreferenceDirty`)、`03-shell.js:565-573`(`fastStatusText`)。

这些守卫依赖「未定义的裸标识符求值为 `undefined`」。ESM 下未 import 的裸标识符是
**ReferenceError**,`typeof` 守不住。全部要改成显式 import。

### 循环依赖

存在真实环(如 `10-docs-core.js:16-17` 的 `filterFields`/`saveDocFilters` 引用定义在更晚的
`12-docs-pages.js:117,121` 的 `DOC_FILTER_DEFAULTS`/`documentFilters`——因为都在函数体里才没炸)。
ESM 靠函数声明提升能容忍这类环;**真正会炸的只有 §三 表格里那些"模块求值期读跨模块顶层状态"**。

## 四、若要动工,唯一正确的顺序

1. **B1/B2 已完成**：现行经典脚本 harness 与 24 文件下限继续作为迁移前回归基线。
2. 先解决 B3 的 `__kzTest` 生产钩子边界，再逐文件迁移；每迁一个跑一次全套冒烟并保住跨文件求值断言。
3. 最后删 `gen-ui-lint-globals.mjs` + `ui-lint-globals.json` + `ui-lint-smoke.mjs` 的同步校验,
   `eslint.config.js` 改 `sourceType: "module"`。

**绝不能反过来**:先迁前端再修 harness,等于在 12k 行重构期间验证能力全黑,
而且其中三个冒烟是静默变绿而非报错。

## 五、不该顺手做的事

- **不要为了瘦身而删 `vendor/monaco/basic-languages/`**(656 KB / 90 种语言)。它与 ESM 无关,
  Monaco 是预构建 AMD bundle,打包器也 tree-shake 不动;删语言包是独立决策,别混进来放大风险面。
- **不要借机上打包器**。收益(minify/tree-shake 业务代码)在本仓不成立,代价是 devDependencies
  从 3 个变几百个 + `cargo build` 经 `beforeBuildCommand` 依赖 npm 构建。原生 ESM 已足够。
