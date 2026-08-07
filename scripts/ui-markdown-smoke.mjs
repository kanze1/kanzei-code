import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync("crates/kanzei-app/ui/main.js", "utf8");
const start = source.indexOf("function escapeHtml");
const end = source.indexOf("// ---------- 消息渲染 ----------", start);
assert.ok(start >= 0 && end > start, "找不到 Markdown 渲染器源码边界");
const markdown = new Function(`${source.slice(start, end)}\nreturn { renderMarkdown };`)();

const list = markdown.renderMarkdown("- alpha\n- beta\n\n1. first\n2. second");
assert.match(list, /<ul>[\s\S]*<li>alpha<\/li>[\s\S]*<\/ul>/, "无序列表未渲染");
assert.match(list, /<ol>[\s\S]*<li>first<\/li>[\s\S]*<\/ol>/, "有序列表未渲染");

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
