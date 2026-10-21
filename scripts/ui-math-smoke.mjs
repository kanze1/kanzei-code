import assert from "node:assert/strict";
import { renderMarkdown, renderInlineMarkdown, splitTableRow } from "../crates/kanzei-app/ui/04-markdown.js";

const expressions = [
  String.raw`$P(X_A\mid E)=\frac{1}{3}$`,
  String.raw`\(\sqrt{x^2+y^2}\)`,
  String.raw`$$\sum_{i=1}^{n}x_i$$`,
  String.raw`\[\begin{pmatrix}a&b\\c&d\end{pmatrix}\]`,
];
for (const source of expressions) {
  const html = renderMarkdown(source);
  assert.match(html, /class="katex"/, `公式未渲染: ${source}`);
  assert.match(html, /<math /, "数学公式缺少可访问的 MathML");
}
const product = renderInlineMarkdown("$f(x)=a * b * c$");
assert.match(product, /class="katex"/);
assert.doesNotMatch(product, /<em>/, "公式中的乘号被 Markdown 吞掉");
assert.deepEqual(splitTableRow(String.raw`| $|x|$ | $\|v\|$ |`), ["$|x|$", String.raw`$\|v\|$`], "公式中的竖线或范数被表格拆开");
const fenced = renderMarkdown("```latex\n$x^2$\n\\[y\\]\n```");
assert.doesNotMatch(fenced, /class="katex"/, "围栏代码被当成公式");
assert.doesNotMatch(renderMarkdown("`$x^2$` ``\\(x\\)``"), /class="katex"/, "行内代码被当成公式");
assert.doesNotMatch(renderMarkdown(String.raw`价格 $5 和 $10；转义 \$x\$。`), /class="katex"/, "价格或转义美元被误识别");
assert.match(renderMarkdown("价格 $5 和 $10，**正常强调**。"), /<strong>正常强调<\/strong>/, "未配对的货币符号吞掉了后面的 Markdown");
const unfinished = renderMarkdown("$$\n\\frac{1}{");
assert.doesNotMatch(unfinished, /katex-error|md-math-error/, "流式未闭合公式不应提前报错");
assert.ok(unfinished.includes("\\frac{1}{"), "未闭合公式的原文丢失");
const invalid = renderMarkdown(String.raw`$\frac{1}{$`);
assert.match(invalid, /md-math-error/, "闭合但无效的公式缺少错误状态");
assert.ok(invalid.includes("\\frac{1}{"), "错误公式必须保留源码");
const unsafe = renderMarkdown(String.raw`$\href{javascript:alert(1)}{x}$ $\htmlClass{injected}{x}$`);
assert.doesNotMatch(unsafe, /<a |class="injected"|<script/i, "公式允许了不可信 HTML 或链接");
const multiline = renderMarkdown(String.raw`公式：

$$
\begin{aligned}
a &= |x| \\
b &= \frac{1}{2}
\end{aligned}
$$

后文 **保留**。`);
assert.equal((multiline.match(/class="katex"/g) || []).length, 1, "多行公式没有作为完整块解析");
assert.match(multiline, /后文 <strong>保留<\/strong>/, "公式吞掉了后续正文");
const quoted = renderMarkdown("> $$\n> \\frac{1}{2}\n> $$\n\n- 概率 $p=\\tfrac12$");
assert.equal((quoted.match(/class="katex"/g) || []).length, 2, "引用或列表中的公式未走共享解析器");
const links = renderMarkdown("[成本](https://example.com/$x$) [$x^2$](docs/report.md)");
assert.match(links, /href="https:\/\/example.com\/\$x\$"/, "公式识别改变了链接目标");
assert.equal((links.match(/class="katex"/g) || []).length, 1, "链接标签内的公式未正确渲染");
console.log("Math parser PASS: delimiters, Markdown isolation, tables, code, streaming, errors, quotes and links");
