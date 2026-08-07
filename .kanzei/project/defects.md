# Defects

## D-151 冒烟 harness 对 class 结构失明,按 class 的断言长期假通过 [fixed] (high)
- 复现: 2026-08-08 做 R-123 时发现。冒烟里 `document.querySelectorAll(".documents-list .doc-item[data-doc-id]")` 恒返回 0,而 `#documents-req-list .doc-item` 返回 2——同一批节点,只是换了按 class 找就找不到。
- 根因: 三处叠加。①harness 按 `id="..."` 正则造节点时**只取 id,完全不读 class 属性**,index.html 里写死的 class 一个都没进 DOM;②`setAttribute("class", ...)` 直接写 `_attributes` 而不经过 className setter,ClassList 的内部集合与属性脱节,于是第一次 `classList.toggle()` 回写就把已有 class 整体抹掉;③选择器引擎的 `matchesOne` 把复合选择器整段当一个 class 名比,`.doc-item[data-doc-id]` 这种写法恒不命中,而 main.js 里到处是这种写法。
- 影响: 比"漏测"更糟——**假通过**。任何依赖 class 结构的断言(视图切换、分组、面板显隐、列表内元素定位)都在悄悄返回空集然后判定通过,历史上"UI 冒烟通过"的可信度因此被高估。修复后同一份脚本的初始化 invoke 数从 35 涨到 39,说明之前有整段代码路径根本没被执行到。
- 验收: ①按 id 造节点时同时取回 class;②setAttribute("class") 走 className setter,保持 ClassList 与属性同源;③选择器支持复合形式(`.a.b`、`.a[attr]`、`div.a`),closest 同步;④修复后既有断言全部仍通过,且新增的按 class 断言能真实生效。
- 优先级: P1
- refs: R-084 R-101 R-123
- 阶段: 2
- 不变量: 测试:护栏必须真的会失败
- 证据等级: E2
- 备注: 落地位置 scripts/ui-runtime-smoke.mjs(节点构造正则、setAttribute、matchesCompound)。这类"护栏形同虚设"与 D-138 同源,属同一类问题的第二次发作。
- 标签: 流程

