import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { renderMarkdown, safeMarkdownPath } from "../crates/kanzei-app/ui/04-markdown.js";
import * as parse from "../crates/kanzei-app/ui/04-structured-parse.js";

const markdown = { renderMarkdown };

const list = markdown.renderMarkdown("- alpha\n- beta\n\n1. first\n2. second");
assert.match(list, /<ul>[\s\S]*<li>alpha<\/li>[\s\S]*<\/ul>/, "无序列表未渲染");
assert.match(list, /<ol>[\s\S]*<li>first<\/li>[\s\S]*<\/ol>/, "有序列表未渲染");
const headings = markdown.renderMarkdown("# one\n## two\n### three\n#### four\n##### five\n###### six");
for (const level of [1, 2, 3, 4, 5, 6]) {
  assert.match(headings, new RegExp(`<h${level} class="md-h md-h-${level}">`), `${level} 级标题结构未保留`);
}
const nested = markdown.renderMarkdown("- parent\n  - child\n    - grandchild\n- sibling");
assert.match(nested, /<ul><li>parent<ul><li>child<ul><li>grandchild<\/li><\/ul><\/li><\/ul><\/li><li>sibling<\/li><\/ul>/, "嵌套列表缩进层级未渲染");
const inline = markdown.renderMarkdown("*italic* and _underline_ and **bold**");
assert.match(inline, /<em>italic<\/em>/, "星号斜体未渲染");
assert.match(inline, /<em>underline<\/em>/, "下划线斜体未渲染");
assert.match(inline, /<strong>bold<\/strong>/, "粗体回归失败");
const paragraphs = markdown.renderMarkdown("first\n\nsecond");
assert.equal(paragraphs, "<p>first</p><p>second</p>", "段落边界未保留");
const css = await readFile(new URL("../crates/kanzei-app/ui/style.css", import.meta.url), "utf8");
for (const [level, token] of [[1, "20"], [2, "18"], [3, "16"], [4, "14"], [5, "13"], [6, "12"]]) {
  assert.match(css, new RegExp(`\\.md-h-${level}\\s*\\{[^}]*font-size: var\\(--fs-${token}\\)`), `${level} 级标题字号 token 未定义`);
}
assert.match(css, /\.msg\.md p \{ margin: \.7em 0; \}/, "段落间距未达到 0.7em");

const table = markdown.renderMarkdown("| name | score |\n| :--- | ---: |\n| kanzei | 10 |");
assert.match(table, /<table>[\s\S]*<th[^>]*>name<\/th>[\s\S]*<td[^>]*>10<\/td>[\s\S]*<\/table>/, "Markdown 表格未渲染");
assert.match(table, /text-align:right/, "表格对齐标记未保留");

const link = markdown.renderMarkdown("[kanzei](https://example.com/docs?a=1&b=2)");
assert.match(link, /href="https:\/\/example\.com\/docs\?a=1&amp;b=2"/, "安全外链未渲染");
assert.match(link, /target="_blank" rel="noopener noreferrer"/, "外链缺少安全打开属性");

const code = markdown.renderMarkdown("```rust\nfn main() {}\n```");
assert.match(code, /<pre class="code"><code class="language-rust">fn main\(\) \{\}<\/code><\/pre>/, "代码语言标识未渲染");

const unsafeHtml = markdown.renderMarkdown("<img src=x onerror=alert(1)> [x](javascript:alert(1))");
assert.doesNotMatch(unsafeHtml, /<img|href="javascript:/i, "Markdown XSS 回归：危险 HTML 或协议未被拦截");
assert.match(unsafeHtml, /&lt;img/, "原始 HTML 未安全转义");

// ---------- UI-0926 #10:markdown-lite 补语法(引用/分隔线/任务列表/删除线/裸链接/路径链接) ----------
{
  const quote = markdown.renderMarkdown("> 第一行 **粗**\n> 第二行 <script>alert(1)</script>\n\n正文");
  assert.match(quote, /<blockquote class="md-quote">第一行 <strong>粗<\/strong><br>第二行 &lt;script&gt;/, "引用块未合并连续 > 行或未转义");
  assert.doesNotMatch(quote, /<script>/, "引用块里的 <script> 未转义");
  assert.equal(markdown.renderMarkdown("a\n\n---\n\n* * *\n\nb"), "<p>a</p><hr><hr><p>b</p>", "分隔线未渲染(或 * * * 被当成列表项)");
  const tasks = markdown.renderMarkdown("- [ ] 待办\n- [x] 完成\n- 普通");
  assert.match(tasks, /<li class="md-task-item"><span class="md-task" data-checked="false">☐<\/span> 待办<\/li>/, "未勾选任务项未渲染");
  assert.match(tasks, /<li class="md-task-item"><span class="md-task" data-checked="true">☑<\/span> 完成<\/li><li>普通<\/li>/, "已勾选任务项未渲染或误伤普通项");
  assert.doesNotMatch(tasks, /<input/, "任务列表不得渲染成可交互 input");
  const inline = markdown.renderMarkdown("~~删掉~~ 见 https://example.com/a?b=1&c=2。以及 `https://code.example` 和 [站](https://x.com)");
  assert.match(inline, /<del>删掉<\/del>/, "删除线未渲染");
  assert.match(inline, /<a href="https:\/\/example\.com\/a\?b=1&amp;c=2" target="_blank" rel="noopener noreferrer">https:\/\/example\.com\/a\?b=1&amp;c=2<\/a>。/, "裸 URL 未自动成链或吞了句末中文标点");
  assert.match(inline, /<code>https:\/\/code\.example<\/code>/, "代码段里的 URL 不该被自动成链");
  assert.equal((inline.match(/<a /g) ?? []).length, 2, "已成链接里的 URL 被二次自动成链");
  assert.match(markdown.renderMarkdown("see (https://docs.rs/tokio)."), /\(<a href="https:\/\/docs\.rs\/tokio"[^>]*>https:\/\/docs\.rs\/tokio<\/a>\)\./, "裸 URL 吞了右括号或句点");
  const pathLink = markdown.renderMarkdown("[05-chat-render.js:531](crates/kanzei-app/ui/05-chat-render.js:531)");
  assert.equal(
    pathLink,
    '<p><a class="md-path" data-path="crates/kanzei-app/ui/05-chat-render.js" data-line="531" title="crates/kanzei-app/ui/05-chat-render.js:531">05-chat-render.js:531</a></p>',
    "相对路径链接未渲染成 a.md-path(或带了 href)",
  );
  assert.doesNotMatch(pathLink, /href=/, "路径链接不得带 href(交给 structuredNav,不让 WebView 自己导航)");
  // XSS:危险协议仍不产生 href;带引号的路径目标不产生属性注入;盘符路径可用。
  const xss = markdown.renderMarkdown('[x](javascript:alert(1)) [y](a/b.rs"onmouseover=1) [z](a/b.rs" onmouseover=1) [w](vbscript:x.md)');
  assert.doesNotMatch(xss, /href=|onmouseover="|data-path="a\/b\.rs"|<a class="md-path"/, `路径链接 XSS 回归:${xss}`);
  assert.match(markdown.renderMarkdown("[f](C:/p/x.rs:3-9)"), /<a class="md-path" data-path="C:\/p\/x\.rs" data-line="3" title="C:\/p\/x\.rs:3-9">f<\/a>/, "盘符路径链接未识别");
  assert.equal(safeMarkdownPath("#anchor"), null, "锚点链接不是路径");
  assert.equal(safeMarkdownPath("mailto:x@y.z"), null, "mailto 不是路径");
  // 链接文字里的行内码是嵌套占位符(架构索引就是这种写法):不能把 `md-0` 漏到界面上。
  const codeLabel = markdown.renderMarkdown("- [`memory_control_plane.md`](../../../docs/design/memory_control_plane.md):基线。 [`kz`](https://x.com/kz)");
  assert.match(codeLabel, /<a class="md-path" data-path="\.\.\/\.\.\/\.\.\/docs\/design\/memory_control_plane\.md"[^>]*><code>memory_control_plane\.md<\/code><\/a>/, `链接文字里的行内码未展开:${codeLabel}`);
  assert.match(codeLabel, /<a href="https:\/\/x\.com\/kz"[^>]*><code>kz<\/code><\/a>/, "外链文字里的行内码未展开");
  assert.doesNotMatch(codeLabel, /\u0000|md-\d/, "占位符漏到了渲染结果里");
}

// ---------- UI-0926 #10:结构化纯解析(04-structured-parse.js)的真实样例夹具 ----------
{
  const circled = parse.splitCircledList("①所有二期子条目有明确依赖;②Wave 0～4 各有 Go/No-Go 记录;③联合闭环可回溯;④二期结项时无相互矛盾状态。");
  assert.equal(circled?.items.length, 4, "①②③④ 验收未切成 4 项");
  assert.equal(circled.items[1], "Wave 0～4 各有 Go/No-Go 记录", "编号项未去掉尾部分隔符");
  assert.equal(parse.splitCircledList("①只有一个标记"), null, "只有一个 ① 不该成列表");
  const marked = parse.splitMarkedList("以 docs/design/phase2_system_upgrade.md 为二期真源维护五批:批1 设计/依赖/需求映射;批2 P0 事实恢复(D-409 与 memory backlog);批3 research+memory 引擎 E2");
  assert.deepEqual(marked?.items.map((item) => item.label), ["批1", "批2", "批3"], "批N 标记列表切分错误");
  assert.equal(marked.items[1].text, "P0 事实恢复(D-409 与 memory backlog)", "批2 的正文切分错误");
  assert.equal(marked.intro, "以 docs/design/phase2_system_upgrade.md 为二期真源维护五批", "批N 列表的引导语丢失");
  assert.deepEqual(parse.splitMarkedList("B1 基座；B2 事件；B3 回归")?.items.map((item) => item.label), ["B1", "B2", "B3"], "B1/B2 列表未切分");
  assert.equal(parse.splitMarkedList("B1-B4 已完成并通过验证"), null, "B1-B4 区间不是列表标记");
  // 括号里的「与 B4 同版发布」是引用不是新批次;B4 在后文真正出现时才切(R-364 内容字段的真实写法)。
  const nested = parse.splitMarkedList("B1 字符账单;B2 tool_search、提示词改写(与 B4 同版发布);B3 原生 defer_loading 探针;B4 CLI/桌面同口径回归");
  assert.deepEqual(nested?.items.map((item) => item.label), ["B1", "B2", "B3", "B4"], `括号内的 B4 被错切成新项:${JSON.stringify(nested?.items)}`);
  assert.equal(nested.items[1].text, "tool_search、提示词改写(与 B4 同版发布)", "B2 正文不该在括号处断开");
  assert.deepEqual(parse.splitMarkedList("B1 基座;B2 事件,回归见 B1 说明;B3 收尾")?.items.map((item) => item.label), ["B1", "B2", "B3"], "重复出现的标记不该切新项");
  assert.deepEqual(parse.splitSemicolonList("第一段内容比较长；第二段也比较长"), ["第一段内容比较长", "第二段也比较长"], "全角分号列表未切分");
  assert.equal(parse.splitSemicolonList("短；段"), null, "过短的分号段不该成列表");
  assert.equal(parse.splitSemicolonList("单段没有分隔"), null, "单段不该成列表");
  const timeline = parse.splitTimeline("R-101 B3 已提交 d1cc0006 || 2026-08-20 B3 收口:用户关闭 kzapp 窗口后通过");
  assert.equal(timeline.length, 2, "|| 时间线未切成 2 段");
  assert.equal(timeline[1].date, "2026-08-20", "时间线段首日期未抽出");
  assert.equal(timeline[1].text, "B3 收口:用户关闭 kzapp 窗口后通过", "时间线段正文错误");
  const parked = parse.parseConditionField("排队:排在 R-340 之后;恢复人:agent;解除条件:R-340");
  assert.equal(parked.owner, "agent", "停车恢复人未解析");
  assert.equal(parked.release, "R-340", "停车解除条件未解析");
  assert.ok(!parked.reason.includes("恢复人") && parked.reason.includes("R-340 之后"), `停车原因切分错误:${parked.reason}`);
  // 发现记录:requirements-archive.md 的真实 7 键单行 JSON(值有删减)。
  const discovery = parse.parseJsonish('{"Intent":"在 edit/insert/write 后先做低成本局部结构校验","Explicit":"按文件类型选择 parser/formatter check","Assumptions":"复用已有 parser、formatter、lint 与 smoke","Ambiguities":"验证器选择矩阵待代码勘察","领域对象":"changed region、validation plan/result","最小成功闭环":"对 JS、Rust、VM 三类真实 edit 路径给出局部命令","延后决策":"完整语言矩阵"}');
  assert.equal(Object.keys(discovery ?? {}).length, 7, "发现记录 JSON 未解析出 7 键");
  assert.equal(parse.DISCOVERY_LABEL_KEYS.Explicit, "用户原话", "发现记录键标签映射缺失");
  const outcome = parse.stripToolOutcome("[tool_outcome=needs_correction code=EDIT_ANCHOR_NOT_FOUND]\n请重读锚点");
  assert.deepEqual([outcome.outcome, outcome.code, outcome.body], ["needs_correction", "EDIT_ANCHOR_NOT_FOUND", "请重读锚点"], "tool_outcome 前缀未剥离");
  assert.equal(parse.stripToolOutcome("普通结果").body, "普通结果", "非前缀文本应原样返回");
  const richSource = "见 R-283 与 crates/kanzei-app/ui/05-chat-render.js:531 以及 https://arxiv.org/abs/2310.08560。0.7em、B1/B2、普通中文";
  const tokens = parse.tokenizeRich(richSource);
  assert.deepEqual(tokens.filter((token) => token.type !== "text").map((token) => token.type), ["ref", "path", "url"], "富文本实体未依次识别为 ref/path/url");
  assert.equal(tokens.find((token) => token.type === "path").line, 531, "路径行号未解析");
  assert.equal(tokens.map((token) => token.value).join(""), richSource, "富文本 token 拼不回原文");
  assert.equal(parse.tokenizeRich("0.7em 与 B1/B2 以及 v1.2").filter((token) => token.type !== "text").length, 0, "0.7em / B1/B2 被误判成实体");
  const pretty = parse.prettyPath("C:\\Users\\kanzei\\Documents\\kanzei-rel-0926\\crates\\a\\b.rs:12", "C:/Users/kanzei/Documents/kanzei-rel-0926");
  assert.deepEqual([pretty.rel, pretty.line, pretty.short], ["crates/a/b.rs", 12, "…/a/b.rs"], `prettyPath 解析错误:${JSON.stringify(pretty)}`);
  assert.equal(parse.stripAnsi("\u001b[32mok\u001b[0m"), "ok", "ANSI 未剥离");
  const httpError = parse.parseErrorText('provider returned HTTP 400: {"error":{"message":"bad param","type":"invalid_request_error","code":null}}');
  assert.deepEqual([httpError.status, httpError.message, httpError.fields.type, httpError.head], ["400", "bad param", "invalid_request_error", "provider returned HTTP 400"], "provider HTTP 错误体未解析");
  assert.ok(!("code" in httpError.fields), "null 字段不该成 chip");
  const chain = parse.parseErrorText("transport error: error sending request for url (https://api.example.com/v1/chat): client error (Connect): tcp connect error: 由于目标计算机积极拒绝");
  assert.ok(chain.chain.length >= 3 && chain.json === null, `transport 错误链未切分:${JSON.stringify(chain.chain)}`);
  const perm = parse.parsePermissionResource("bash", '{"command":"cargo test","workdir":"C:/p"}');
  assert.deepEqual([perm.kind, perm.command, perm.workdir], ["command", "cargo test", "C:/p"], "bash 权限资源未解析成命令形态");
  assert.equal(parse.parsePermissionResource("edit", "src/a.rs").kind, "path", "路径资源未识别");
  assert.equal(parse.permissionResourceText("bash", JSON.stringify({ command: "cargo test --workspace\ncargo fmt", workdir: "C:/p" })), "bash · cargo test --workspace …", "权限短文本不对(多行命令只取首行)");
  assert.deepEqual(
    ["observed_head", "优先级", "refs", "停车", "验收", "进展", "状态纠正-2", "发现记录"].map(parse.classifyTrackerField),
    ["engine", "meta", "refs", "condition", "list", "timeline", "timeline", "prose"],
    "tracker 字段分类错误",
  );
  assert.equal(parse.formatEpoch("1788804121"), 1788804121000, "10 位秒时间戳未识别");
  assert.equal(parse.formatEpoch("1786925390809"), 1786925390809, "13 位毫秒时间戳未识别");
  assert.equal(parse.formatEpoch("2026-09-26"), null, "非时间戳不该被识别");
  const fingerprint = parse.parseSourceFingerprint("v2 crates/kanzei-core/src/runner/drive/assembly.rs@63ae5885281c,crates/kanzei-tools/src/research_write.rs@cbe6c9d2c518");
  assert.deepEqual(fingerprint?.items.map((item) => item.hash), ["63ae5885281c", "cbe6c9d2c518"], "v2 源码指纹未逐文件拆开");
  assert.equal(parse.parseSourceFingerprint("c11614f9a8d710e2")?.hash, "c11614f9a8d710e2", "旧格式指纹未原样保留");
  const diff = parse.parseUnifiedDiff("diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n fn a() {}\n-let x = 1;\n+let x = 2;\n ctx\ndiff --git a/ui/b.js b/ui/b.js\nnew file mode 100644\n--- /dev/null\n+++ b/ui/b.js\n@@ -0,0 +1,2 @@\n+a\n+b\n");
  assert.deepEqual(diff.map((file) => [file.path, file.additions, file.deletions, file.language]), [["src/a.rs", 1, 1, "rust"], ["ui/b.js", 2, 0, "javascript"]], "unified diff 未按文件拆分计数");
  assert.deepEqual(diff[0].lines.map((line) => [line.kind, line.old_line, line.new_line]), [["ctx", 1, 1], ["del", 2, null], ["add", null, 2], ["ctx", 3, 3]], "diff 行号推进错误");
  // hunk 内以 `---`/`+++` 开头的是被删/新增的内容行(SQL/Lua 注释 `-- x` 删掉后写成 `--- x`),不是文件头。
  const sqlDiff = parse.parseUnifiedDiff("diff --git a/x.sql b/x.sql\n--- a/x.sql\n+++ b/x.sql\n@@ -1,2 +1,2 @@\n ctx\n--- old comment\n+-- new comment\n");
  assert.deepEqual(sqlDiff.map((file) => [file.path, file.additions, file.deletions, file.lines.length]), [["x.sql", 1, 1, 3]], `hunk 内的 --- 行被当成文件头:${JSON.stringify(sqlDiff.map((file) => [file.path, file.additions, file.deletions]))}`);
  // 带空格的项目根下的绝对路径按根整体认领,不在空格处被切断。
  const spaced = parse.tokenizeRich("see C:\\Users\\kanzei\\Documents\\kanzei code\\crates\\a.rs:12 now", { roots: ["C:\\Users\\kanzei\\Documents\\kanzei code"] });
  assert.deepEqual(spaced.filter((token) => token.type === "path").map((token) => [token.path, token.line]), [["C:\\Users\\kanzei\\Documents\\kanzei code\\crates\\a.rs", 12]], `带空格根下的路径被切断:${JSON.stringify(spaced)}`);
  assert.equal(spaced.map((token) => token.value).join(""), "see C:\\Users\\kanzei\\Documents\\kanzei code\\crates\\a.rs:12 now", "带根的富文本 token 拼不回原文");
  assert.ok(parse.looksLikeNoise("1 // 这是一段中文源码注释说明") && !parse.looksLikeNoise("3 通过") && !parse.looksLikeNoise("12 files"), "合并空白后的「行号 + 源码」判据错误");
  assert.equal(parse.resolveRelativePath(".kanzei/project/architecture", "../../../docs/design/x.md"), "docs/design/x.md", "索引相对链接未解析成项目相对路径");
  assert.deepEqual(parse.mismatchFacts({ mismatch_count: 2, mismatches: [] }), { count: 2 }, "验收对账事实未解析");
}

console.log("UI Markdown 冒烟通过：列表、表格、代码语言、安全外链与 XSS 用例已覆盖;markdown 补语法与结构化纯解析夹具已覆盖");
