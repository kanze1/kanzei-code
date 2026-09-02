import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { renderMarkdown } from "../crates/kanzei-app/ui/04-markdown.js";

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

console.log("UI Markdown 冒烟通过：列表、表格、代码语言、安全外链与 XSS 用例已覆盖");
