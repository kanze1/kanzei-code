// Real Markdown/message rendering in Edge; inspect clipboard writes at the browser API boundary.
/* global window, document, navigator, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = "output/playwright/chat-quote-blocks";
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const result = { checks: [], layouts: [], errors: [] };
const reply = "a) Remote: Mon–Fri, 8 hours per day (~40 hours/week). If I have any exception (e.g. need to go to school for a presentation), I'll let you know in advance — it's rare.\nb) In person: 3 days per week works for me. Just note I can't be there very early in the morning due to commuting — I'll let you know when I'll arrive.";
const alternative = "Remote: 8 hours on weekdays (Mon–Fri), so about 40 hours a week. Occasionally I'll need time to run to school for a presentation, but I'll flag it ahead of time — it's rare. In person: I can do 3 days per week, though I can't be there super early in the morning because of commute. I'll tell you once I arrive.";
const raw = `你可以这样回：\n\n${reply.split("\n").map(line => `> ${line}`).join("\n")}\n\n语气稍微自然一点的话也可以用：\n\n> ${alternative}\n\n两种都行，你看哪个更顺口就能直接发。`;
function check(name, actual, expected = true) {
  result.checks.push({ name, actual, expected });
  assert.deepEqual(actual, expected, name);
}

async function open(theme, width, dpr = 1) {
  const page = await browser.newPage({ viewport: { width, height: 1000 }, deviceScaleFactor: dpr });
  page.on("pageerror", error => result.errors.push(error.message));
  await page.goto(`${server.origin}/?scene=empty&theme=${theme}`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async raw => {
    const { activePane } = await import("/01-chat-panes.js");
    const { addMessage, setFollowLatest } = await import("/05-chat-render.js");
    const { renderMarkdownInto } = await import("/04-markdown.js");
    setFollowLatest(false);
    activePane.replaceChildren();
    const message = addMessage("assistant md", raw);
    message.id = "quote-sample";
    message.dataset.raw = raw;
    renderMarkdownInto(message.querySelector(".message-body"), raw);
    document.querySelector("#messages").scrollTop = 0;
    window.__quoteWrites = [];
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async text => { window.__quoteWrites.push(text); } } });
    await document.fonts.ready;
  }, raw);
  return page;
}

try {
  for (const theme of ["light", "dark"]) {
    for (const [width, dpr] of [[1440, 1], [420, 1.5]]) {
      const page = await open(theme, width, dpr);
      const layout = await page.evaluate(() => {
        const body = document.querySelector("#quote-sample .message-body");
        const quote = body.querySelector(".md-quote");
        const content = quote.querySelector(".md-quote-content");
        const actions = quote.querySelector(".md-quote-actions");
        return {
          copies: body.querySelectorAll(".quote-copy").length,
          readableColor: getComputedStyle(quote).color === getComputedStyle(body).color,
          lineBreaks: content && getComputedStyle(content.querySelector("p")).whiteSpace,
          toolbarAboveText: actions && actions.getBoundingClientRect().bottom <= content.getBoundingClientRect().top,
          toolbarFits: actions && actions.getBoundingClientRect().right <= quote.getBoundingClientRect().right,
          noPageOverflow: document.documentElement.scrollWidth <= window.innerWidth + 1,
        };
      });
      result.layouts.push({ theme, width, dpr, ...layout });
      await page.locator("#chat-area").screenshot({ path: `${output}/${theme}-${width}.png` });
      check(`${theme}/${width}: each reply has its own copy action`, layout.copies, 2);
      for (const key of ["readableColor", "toolbarAboveText", "toolbarFits", "noPageOverflow"]) check(`${theme}/${width}: ${key}`, layout[key]);
      check(`${theme}/${width}: original reply line breaks stay visible`, layout.lineBreaks, "pre-wrap");
      await page.close();
    }
  }

  const page = await open("light", 1440);
  const copy = page.locator("#quote-sample .quote-copy").first();
  await copy.click();
  check("First reply copies only its own two lines", await page.evaluate(() => window.__quoteWrites), [reply]);
  await page.locator("#quote-sample .quote-copy").nth(1).click();
  check("Alternative reply copies independently", await page.evaluate(() => window.__quoteWrites.at(-1)), alternative);
  await page.locator("#quote-sample > .msg-actions > .copy-btn").click();
  check("Whole-message copy retains the original Markdown", await page.evaluate(() => window.__quoteWrites.at(-1)), raw);

  const rich = '> ## Reply\n>\n> **Hello** & `welcome` [here](https://example.com).\n>\n> 1. Remote\n>    - Weekdays\n> 2. In person\n>\n> > Nested reply\n>\n> ```text\n> \tkeep <>&\n>\n> line two\n> ```\n>\n> | day | hours |\n> | --- | --- |\n> | Mon | 8 |';
  await page.evaluate(async raw => {
    (await import("/04-markdown.js")).renderMarkdownInto(document.querySelector("#quote-sample .message-body"), raw);
  }, rich);
  check("Quoted Markdown retains its structural blocks", await page.locator("#quote-sample h2, #quote-sample ol, #quote-sample ul, #quote-sample pre.code, #quote-sample table").count(), 5);
  await copy.click();
  check("Rendered quote copy preserves structure without Markdown or toolbar labels", await page.evaluate(() => window.__quoteWrites.at(-1)), "Reply\n\nHello & welcome here.\n\n1. Remote\n  - Weekdays\n2. In person\n\nNested reply\n\n\tkeep <>&\n\nline two\n\nday\thours\nMon\t8");
  await page.locator("#quote-sample .md-quote .md-quote .quote-copy").click();
  check("Nested quote has an independent copy scope", await page.evaluate(() => window.__quoteWrites.at(-1)), "Nested reply");

  await page.evaluate(async () => {
    const { renderMarkdownInto } = await import("/04-markdown.js");
    const body = document.querySelector("#quote-sample .message-body");
    renderMarkdownInto(body, "> First\n>\n> Second unfinished", { streaming: true });
    renderMarkdownInto(body, "> First\n>\n> Second completed", { streaming: true });
  });
  check("Streaming rerender retains a single copy action", await page.locator("#quote-sample .quote-copy").count(), 1);
  await copy.click();
  check("Streaming quote copies the latest paragraphs", await page.evaluate(() => window.__quoteWrites.at(-1)), "First\n\nSecond completed");
  await page.evaluate(async () => {
    (await import("/02-i18n.js")).setLanguagePreference("en");
    document.querySelector("#quote-sample").outerHTML = document.querySelector("#quote-sample").outerHTML;
  });
  await copy.hover();
  await page.mouse.move(0, 0);
  await copy.blur();
  check("Cached history resets feedback in the current language", await copy.getAttribute("aria-label"), "Copy quote");
  await copy.focus();
  await page.keyboard.press("Enter");
  check("Cached history supports keyboard copy", await page.evaluate(() => window.__quoteWrites.at(-1)), "First\n\nSecond completed");

  await page.evaluate(() => { navigator.clipboard.writeText = async () => { throw new Error("clipboard denied"); }; });
  await copy.click();
  await page.waitForFunction(() => !document.querySelector(".quote-copy[aria-busy]"));
  check("Failed copy leaves no success state or disabled action", [await copy.getAttribute("data-copied"), await copy.getAttribute("aria-disabled")], [null, null]);
  check("Clipboard failure stays visible", await page.locator("#toast").innerText().then(text => text.includes("clipboard denied")));
  check("No browser runtime errors", result.errors, []);
  await page.close();
  console.log(`Quote blocks and clipboard: ${result.checks.length} checks passed across ${result.layouts.length} layouts`);
} finally {
  await writeFile(`${output}/acceptance.json`, JSON.stringify(result, null, 2));
  await browser.close();
  await server.close();
}
