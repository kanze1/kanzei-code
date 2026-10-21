// Production message/viewer rendering in Edge; IPC and clipboard are isolated.
/* global window, document, navigator, getComputedStyle, DataTransfer, ClipboardEvent */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = "output/playwright/math";
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const results = { checks: [], errors: [], failedAssets: [] };
await mkdir(output, { recursive: true });
function check(name, actual, expected = true) {
  assert.deepEqual(actual, expected, name); results.checks.push(name);
}
const sample = String.raw`## 条件概率与矩阵

行内公式 $P(X_A\mid E)=\frac{P(E\mid X_A)P(X_A)}{P(E)}$ 与 \(f(x)=a * b * c\)。

$$
\begin{aligned}
P(E)&=\frac12\times\frac13+\frac13=\frac12 \\
P(X_A\mid E)&=\frac{\frac12\times\frac13}{\frac12}=\frac13
\end{aligned}
$$

\[\begin{pmatrix}a&b\\c&d\end{pmatrix}\]

后文 **完整保留**。`;
const table = String.raw`| 对象 | 公式 |
| --- | --- |
| 绝对值 | $|x|$ |
| 范数 | $\|v\|$ |`;
const quote = String.raw`> 概率 $p=\tfrac12$。`;
const code = "行内 `$x^2$` 与 ``\\(y\\)``。\n\n```latex\n$$\\frac{1}{2}$$\n```";
const long = `$$${Array.from({ length: 50 }, (_, i) => `x_{${i}}`).join("+")}$$`;
async function mount(page, theme, width) {
  await page.setViewportSize({ width, height: 1000 });
  await page.goto(`${server.origin}/?scene=empty&theme=${theme}`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async samples => {
    const { activePane } = await import("/01-chat-panes.js");
    const { addMessage, setFollowLatest } = await import("/05-chat-render.js");
    const { renderMarkdownInto } = await import("/04-markdown.js");
    setFollowLatest(false); activePane.replaceChildren();
    for (const [name, raw] of Object.entries(samples)) {
      const message = addMessage("assistant md", raw);
      message.id = `math-${name}`; message.dataset.raw = raw;
      renderMarkdownInto(message.querySelector(".message-body"), raw);
    }
    window.__mathWrites = [];
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: {
      writeText: async text => { window.__mathWrites.push(text); },
    } });
    await document.fonts.ready;
  }, { sample, table, quote, code, long });
}

const page = await browser.newPage();
page.on("pageerror", error => results.errors.push(error.message));
page.on("response", response => {
  if (response.url().includes("/vendor/katex/") && response.status() >= 400) results.failedAssets.push({ url: response.url(), status: response.status() });
});
try {
  for (const [theme, width] of [["light", 1440], ["dark", 1440], ["light", 420], ["dark", 420]]) {
    await mount(page, theme, width);
    const layout = await page.evaluate(() => {
      const sample = document.querySelector("#math-sample"), long = document.querySelector("#math-long .md-math-display");
      const visual = sample.querySelector(".katex-html");
      return {
        formulas: sample.querySelectorAll(".katex").length,
        accessible: sample.querySelectorAll("math annotation[encoding='application/x-tex']").length,
        display: sample.querySelectorAll(".katex-display").length,
        noMarkdownDamage: !sample.querySelector(".md-math em") && sample.textContent.includes("完整保留"),
        tableCells: [...document.querySelectorAll("#math-table tbody tr")].map(row => row.cells.length),
        tableFormulas: document.querySelectorAll("#math-table .katex").length,
        literalCode: !document.querySelector("#math-code .katex") && document.querySelector("#math-code pre code").textContent === "$$\\frac{1}{2}$$",
        localFonts: document.fonts.check("16px KaTeX_Main"),
        mathVisible: visual.getBoundingClientRect().height > 0,
        contrast: getComputedStyle(sample.querySelector(".md-math")).color === getComputedStyle(sample.querySelector(".message-body")).color,
        longScrolls: long.scrollWidth > long.clientWidth,
        bounded: long.clientWidth <= long.closest(".message-body").clientWidth,
        noPageOverflow: document.documentElement.scrollWidth <= window.innerWidth + 1,
      };
    });
    for (const [key, expected] of [["formulas", 4], ["accessible", 4], ["display", 2], ["tableCells", [2, 2]], ["tableFormulas", 2]]) check(`${theme}/${width}: ${key}`, layout[key], expected);
    for (const key of ["noMarkdownDamage", "literalCode", "localFonts", "mathVisible", "contrast", "longScrolls", "bounded", "noPageOverflow"]) check(`${theme}/${width}: ${key}`, layout[key]);
    await page.locator("#math-sample").screenshot({ path: `${output}/${theme}-${width}.png` });
  }
  await mount(page, "light", 1440);
  await page.locator("#math-sample > .msg-actions > .copy-btn").click();
  check("whole-message copy keeps original TeX and delimiters", await page.evaluate(() => window.__mathWrites.at(-1)), sample);
  await page.locator("#math-quote .quote-copy").click();
  check("quote copy emits each formula exactly once", await page.evaluate(() => window.__mathWrites.at(-1)), String.raw`概率 $p=\tfrac12$。`);
  const selectedCopy = await page.evaluate(() => {
    const root = document.querySelector("#math-quote .md-quote-content");
    const range = document.createRange(); range.selectNodeContents(root);
    const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
    const clipboardData = new DataTransfer();
    document.dispatchEvent(new ClipboardEvent("copy", { bubbles: true, cancelable: true, clipboardData }));
    selection.removeAllRanges();
    return clipboardData.getData("text/plain");
  });
  check("selection copy uses upstream copy-tex rather than duplicate visual/MathML text", selectedCopy, String.raw`概率 $p=\tfrac12$。`);
  const streaming = await page.evaluate(async () => {
    const { renderMarkdownInto } = await import("/04-markdown.js");
    const body = document.querySelector("#math-sample .message-body");
    renderMarkdownInto(body, "$$\n\\frac{1}{", { streaming: true });
    const pending = body.querySelectorAll(".md-math-pending").length === 1 && !body.querySelector(".katex, .md-math-error");
    renderMarkdownInto(body, "$$\n\\frac{1}{2}\n$$\n\n后文", { streaming: true });
    const closed = body.querySelectorAll(".katex").length === 1 && body.textContent.includes("后文");
    const html = body.innerHTML; body.innerHTML = html;
    const restored = body.querySelectorAll(".katex").length === 1;
    renderMarkdownInto(body, String.raw`$\frac{1}{$`);
    const error = body.querySelector(".md-math-error");
    return { pending, closed, restored, errorVisible: !!error && error.title.includes("KaTeX parse error") && error.textContent === String.raw`$\frac{1}{$` };
  });
  for (const [key, value] of Object.entries(streaming)) check(`streaming/history/error: ${key}`, value);
  const preview = await page.evaluate(async () => {
    (await import("/15-views-misc.js")).openRuntimeMarkdown("公式预览", String.raw`\[\frac{1}{2}\]`);
    return document.querySelectorAll("#viewer-body .katex").length;
  });
  check("document viewer shares the same formula renderer", preview, 1);
  check("all local KaTeX assets load successfully", results.failedAssets, []);
  check("no browser runtime errors", results.errors, []);
  console.log(`Math browser PASS: ${results.checks.length} checks`);
} catch (error) {
  results.failure = error.stack;
  await page.screenshot({ path: `${output}/failure.png` });
  throw error;
} finally {
  await writeFile(`${output}/results.json`, JSON.stringify(results, null, 2));
  await browser.close(); await server.close();
}
