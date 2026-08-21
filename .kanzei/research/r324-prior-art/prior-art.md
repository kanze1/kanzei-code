---
kind: prior_art
topic: r324-prior-art
status: complete
trigger: core_requirement
entry_refs: R-324
websearch_round_limit: 4
---

# 先行方案对照

对照主题:代码符号索引的形态——重解析(AST/LSP)还是轻扫描,覆盖多少语言。

## 外部已有实现

### Universal Ctags 的正则式多语言标签

- 出处: https://docs.ctags.io/en/latest/man/ctags-optlib.7.html
- 证据等级: V1
- 事实: 用 per-language 的正则规则从源码抽取标签(函数/类/变量),不建完整 AST。
  `--regex-<LANG>` 让每种语言只声明「哪些行形态算一个定义」,新增语言=加一组正则,
  不改扫描框架。
- 差异: 本仓 `symbols` 已经是同型的行级扫描,但**只硬编码了 Rust 一种规则**
  (`symbols.rs` 里 `extension == "rs"`)。
- 决策: **采用其分层思路**——扫描循环(注释/块注释/行尾裁剪)与语言规则分离,
  加 JS 只加一个 `parse_js_symbol_line`,不复制循环。**不采用**其正则驱动的
  可配置形态:本仓只需两三种语言,手写匹配比维护正则表更好读也更好测。

### Claude Code 的工具面(无符号索引)

- 出处: https://code.claude.com/docs/en/hooks
- 证据等级: V2
- 事实: 该产品的工具面里没有符号索引类工具;定位符号靠 Grep(ripgrep)与 Read,
  钩位表可见其工具族为读写/搜索/执行/子代理,不含结构化代码地图。
- 差异: 纯 grep 定位需要模型自己构造正则并过滤定义行、注释、字符串里的伪命中,
  多花往返;但语言无关,任何仓库都能用。
- 决策: **不采用其"只靠 grep"路线**——本仓是 Rust+JS 双栈且长期自举,
  「谁调用它/定义在哪」是高频查询,值得一个索引;但**保留 grep 作为兜底**,
  symbols 不覆盖的语言不影响模型干活(这也是本条目不追求全语言覆盖的理由)。

## 仓内既有设计

### r310_repo_map_design.md 的形态取舍

- 出处: file:docs/design/r310_repo_map_design.md:1
- 证据等级: V3
- 事实: 三方案对照后取「实时按需查询」,拒绝全量注入(固定上下文成本高)与持久
  增量索引(索引维护成本高于收益)。明确「实时扫描天然随提交更新」。
- 差异: 该结论对语言无关,但当时只落到 Rust。
- 决策: **沿用**——JS 走同一条实时扫描路径,不新增持久索引,不注入上下文。

### gen-ui-lint-globals.mjs 的顶层判据

- 出处: file:scripts/gen-ui-lint-globals.mjs:24
- 证据等级: V3
- 事实: `ui/*.js` 是经典 script 按序加载,共享全局作用域的只有**列 0** 的声明;
  该脚本据此提取跨文件全局白名单供 eslint no-undef 使用。
- 差异: JS 没有 `pub`,symbols 需要一个等价的可见性判据,而仓内已经有一个且被
  lint 依赖。
- 决策: **复用同一判据**(列 0 或 export)。两处若各定各的,会出现「lint 认作
  全局、symbols 不认作公共面」的自相矛盾;测试与注释双向点名对方,钉住同源。
