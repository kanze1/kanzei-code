// Production UI in Edge, with isolated IPC and clipboard writes captured at the browser API boundary.
/* global window, document, navigator, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = "output/playwright/chat-code-blocks";
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const result = { checks: [], layouts: [], errors: [] };
const source = 'cd "C:\\Users\\kanzei\\Documents\\轻小说爬取"\nnode crawl.js';
const raw = `语法修好，优化完成。重新启动：\n\n\`\`\`powershell\n${source}\n\`\`\`\n\n改动说明：使用 \`crawl.js\` 保留本地文件。`;
const longSource = `node crawl.js --output "C:\\项目资料\\${"中文目录\\".repeat(24)}结果"`;
const html = '<h1 title="a & b">你好</h1>';
function check(name, actual, expected = true) {
  result.checks.push({ name, actual, expected });
  assert.deepEqual(actual, expected, name);
}

async function open(theme, width, dpr = 1) {
  const page = await browser.newPage({ viewport: { width, height: 1000 }, deviceScaleFactor: dpr });
  page.on("pageerror", (error) => result.errors.push(error.message));
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto(`${server.origin}/?scene=empty&theme=${theme}`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async ({ raw, longSource, html }) => {
    const { activePane } = await import("/01-chat-panes.js");
    const { addMessage, setFollowLatest } = await import("/05-chat-render.js");
    const { renderMarkdownInto } = await import("/04-markdown.js");
    setFollowLatest(false);
    activePane.replaceChildren();
    for (const [id, text] of [["sample", raw], ["long", `长命令：\n\n\`\`\`sh\n${longSource}\n\`\`\``], ["html", `\`\`\`html\n${html}\n\`\`\``]]) {
      const message = addMessage("assistant md", text);
      message.id = `code-${id}`;
      message.dataset.raw = text;
      renderMarkdownInto(message.querySelector(".message-body"), text);
    }
    document.querySelector("#messages").scrollTop = 0;
    window.__copyWrites = [];
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text) => { window.__copyWrites.push(text); } } });
    await document.fonts.ready;
  }, { raw, longSource, html });
  await page.locator("#code-sample .code-copy").waitFor({ state: "visible" });
  return page;
}

function measure() {
  const sample = document.querySelector("#code-sample");
  const block = sample.querySelector(".md-code-block");
  const pre = block.querySelector("pre");
  const code = pre.querySelector("code");
  const long = document.querySelector("#code-long pre");
  const longBlock = long.parentElement;
  const actions = longBlock.querySelector(".md-code-actions");
  const before = actions.getBoundingClientRect().right;
  long.scrollLeft = long.scrollWidth;
  const after = actions.getBoundingClientRect().right;
  long.scrollLeft = 0;
  const inline = sample.querySelector("p code");
  const style = getComputedStyle(code);
  return {
    transparentCode: style.backgroundColor === "rgba(0, 0, 0, 0)",
    zeroCodePadding: style.padding === "0px",
    codeWhiteSpace: style.whiteSpace,
    inlineCodeDistinct: getComputedStyle(inline).backgroundColor !== "rgba(0, 0, 0, 0)",
    singleFrame: getComputedStyle(pre).borderTopWidth === "0px",
    source: code.textContent,
    longSource: long.textContent,
    longScrolls: long.scrollWidth > long.clientWidth,
    localWidth: longBlock.getBoundingClientRect().right <= sample.parentElement.getBoundingClientRect().right + 1,
    noPageOverflow: document.documentElement.scrollWidth <= window.innerWidth + 1,
    toolbarFits: actions.getBoundingClientRect().right <= longBlock.getBoundingClientRect().right,
    toolbarFixed: before === after,
    toolbarAboveCode: block.querySelector(".md-code-toolbar").getBoundingClientRect().bottom <= pre.getBoundingClientRect().top,
    noButtonsInSource: sample.querySelectorAll("pre button").length === 0,
    messageCopyVisible: Number(getComputedStyle(sample.querySelector(".msg-actions")).opacity) > 0,
  };
}

try {
  for (const theme of ["dark", "light"]) {
    for (const [width, dpr] of [[1440, 1], [768, 1.25], [420, 1.5]]) {
      const page = await open(theme, width, dpr);
      const layout = await page.evaluate(measure);
      result.layouts.push({ theme, width, dpr, ...layout });
      const tag = `${theme}/${width}@${dpr}`;
      for (const key of ["transparentCode", "zeroCodePadding", "inlineCodeDistinct", "singleFrame", "longScrolls", "localWidth", "noPageOverflow", "toolbarFits", "toolbarFixed", "toolbarAboveCode", "noButtonsInSource", "messageCopyVisible"]) check(`${tag}: ${key}`, layout[key]);
      check(`${tag}: code preserves whitespace`, layout.codeWhiteSpace, "pre");
      check(`${tag}: code preserves source`, layout.source, source);
      check(`${tag}: long command preserves source`, layout.longSource, longSource);
      if (width === 1440) {
        const box = await page.locator("#code-sample").boundingBox();
        await page.screenshot({ path: `${output}/after-${theme}.png`, clip: { x: box.x, y: box.y, width: box.width, height: box.height + 28 } });
      } else if (width === 420) await page.locator("#chat-area").screenshot({ path: `${output}/after-${theme}-420.png` });
      await page.close();
    }
  }

  const page = await open("dark", 1440);
  const copy = page.locator("#code-sample .code-copy");
  await copy.click();
  await page.waitForFunction(() => window.__copyWrites.length === 1);
  check("Code copy writes only the selected block once", await page.evaluate(() => window.__copyWrites), [source]);
  check("Code copy exposes confirmed success", await copy.getAttribute("data-copied"), "true");
  await page.mouse.move(0, 0);
  await copy.blur();
  check("Leaving a copy button restores its accessible name", await copy.getAttribute("aria-label"), "复制代码");
  await copy.focus();
  await copy.press("Enter");
  await page.waitForFunction(() => window.__copyWrites.length === 2);
  check("Keyboard copy keeps focus on the action", await copy.evaluate((button) => document.activeElement === button));
  await page.locator("#code-sample > .msg-actions .copy-btn").click();
  await page.waitForFunction(() => window.__copyWrites.length === 3);
  check("Message copy retains Markdown without toolbar labels", await page.evaluate(() => window.__copyWrites.at(-1)), raw);

  await page.evaluate(() => {
    document.querySelectorAll(".k-toast").forEach((node) => node.remove());
    navigator.clipboard.writeText = () => new Promise((resolve, reject) => { window.__finishCopy = resolve; window.__rejectCopy = reject; });
  });
  await copy.click();
  check("Pending copy has no success marker", await copy.getAttribute("data-copied"), null);
  check("Pending copy exposes busy state", await copy.getAttribute("aria-busy"), "true");
  check("Pending copy has no success toast", await page.locator('.k-toast[data-kind="ok"]').count(), 0);
  await page.evaluate(() => window.__rejectCopy(new Error("clipboard denied")));
  await page.locator('.k-toast[data-kind="err"]').waitFor();
  check("Rejected copy keeps the action usable", await copy.getAttribute("aria-busy"), null);
  check("Rejected copy cannot display success", await copy.getAttribute("data-copied"), null);

  await copy.click();
  await page.evaluate(() => window.__finishCopy());
  await page.waitForFunction(() => document.querySelector("#code-sample .code-copy").dataset.copied === "true");
  check("Resolved clipboard write displays success", await copy.getAttribute("aria-label"), "已复制");

  await page.evaluate(async () => {
    const { setLanguagePreference } = await import("/02-i18n.js");
    setLanguagePreference("en");
    document.querySelector("#code-sample").outerHTML = document.querySelector("#code-sample").outerHTML;
    navigator.clipboard.writeText = async (text) => { window.__copyWrites.push(text); };
  });
  await copy.hover();
  await page.mouse.move(0, 0);
  check("Cached history resets feedback in the current language", await copy.getAttribute("aria-label"), "Copy code");
  await copy.click();
  await page.waitForFunction(() => window.__copyWrites.length === 4);
  check("Cached history retains delegated copy behavior", await page.evaluate(() => window.__copyWrites.at(-1)), source);
  check("Language changes preserve the copy icon", await copy.locator("svg").count(), 1);

  await page.evaluate(async () => {
    const { renderMarkdownInto } = await import("/04-markdown.js");
    const message = document.querySelector("#code-html");
    renderMarkdownInto(message.querySelector(".message-body"), '```html\n<h1 title="a & b">未完成');
  });
  check("An open streaming fence has one copy action", await page.locator("#code-html .code-copy").count(), 1);
  check("An open streaming fence cannot preview", await page.locator("#code-html .code-preview").count(), 0);
  await page.locator("#code-html .code-copy").click();
  await page.waitForFunction(() => window.__copyWrites.length === 5);
  check("Streaming copy reads the currently rendered source", await page.evaluate(() => window.__copyWrites.at(-1)), '<h1 title="a & b">未完成');
  await page.evaluate(async (html) => {
    const { renderMarkdownInto } = await import("/04-markdown.js");
    const { decorateCodeBlocks } = await import("/24-preview.js");
    const body = document.querySelector("#code-html .message-body");
    renderMarkdownInto(body, `\`\`\`html\n${html}\n\`\`\``);
    decorateCodeBlocks(body);
  }, html);
  check("Closing the fence leaves exactly one toolbar", await page.locator("#code-html .md-code-toolbar").count(), 1);
  check("Closing the fence enables one preview action", await page.locator("#code-html .code-preview").count(), 1);
  await page.locator("#code-html .code-copy").click();
  await page.waitForFunction(() => window.__copyWrites.length === 6);
  check("Closed code copy uses the latest source and decodes HTML entities", await page.evaluate(() => window.__copyWrites.at(-1)), html);
  await page.locator("#code-html .code-preview").click();
  await page.waitForFunction(() => window.__kzPreview.calls.some((call) => call.cmd === "preview_snippet"));
  check("Preview receives only the original HTML", await page.evaluate(() => window.__kzPreview.calls.findLast((call) => call.cmd === "preview_snippet").args.html), html);
  await page.evaluate(async () => (await import("/24-preview.js")).closePreviewDock());

  await page.evaluate(async () => {
    const { renderMarkdownInto } = await import("/04-markdown.js");
    const pane = document.querySelector('.msg-pane[data-active="1"]');
    for (const className of ["reasoning-body", "sv-md"]) {
      const host = document.createElement("div"); host.className = className; pane.append(host);
      renderMarkdownInto(host, '```text\n\t保留缩进 <>&\n\n```');
    }
  });
  check("Reasoning and structured Markdown share the same code behavior", await page.evaluate(() => [...document.querySelectorAll('.msg-pane[data-active="1"] > :is(.reasoning-body, .sv-md)')].every((host) => {
    const code = host.querySelector("pre code");
    return host.querySelectorAll(".code-copy").length === 1 && code.textContent === "\t保留缩进 <>&\n" && getComputedStyle(code).backgroundColor === "rgba(0, 0, 0, 0)";
  })));
  await page.evaluate(async () => {
    const { renderMarkdownInto } = await import("/04-markdown.js");
    const { setDiagramEngine } = await import("/04-diagram.js");
    setDiagramEngine({ parse: async () => true, render: async () => new Promise((resolve) => {
      window.__finishDiagram = () => resolve({ svg: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 60"><rect width="100" height="40"/></svg>' });
    }) });
    const host = document.createElement("div"); host.id = "async-code-diagram";
    document.querySelector('.msg-pane[data-active="1"]').append(host);
    renderMarkdownInto(host, "```mermaid\nflowchart LR\n  A --> B\n```");
  });
  await page.waitForFunction(() => typeof window.__finishDiagram === "function");
  check("An uncached diagram starts with a code toolbar", await page.locator("#async-code-diagram .code-copy").count(), 1);
  await page.evaluate(() => window.__finishDiagram());
  await page.locator("#async-code-diagram .kz-diagram-host").waitFor({ state: "attached" });
  check("Async diagram conversion removes the entire code shell", await page.locator("#async-code-diagram .md-code-block").count(), 0);
  check("No browser runtime errors", result.errors, []);
  await page.close();
  console.log(`Code blocks and clipboard: ${result.checks.length} checks passed across ${result.layouts.length} layouts`);
} finally {
  await writeFile(`${output}/acceptance.json`, JSON.stringify(result, null, 2));
  await browser.close();
  await server.close();
}
