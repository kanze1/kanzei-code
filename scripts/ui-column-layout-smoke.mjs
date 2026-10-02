// UI2-0926 #12 对话单列浏览器冒烟(docs/design/chat_presentation_contract.md §4.4),由 ui-lint-smoke.mjs 在弹层样例
// 冒烟之后调用,也可单独运行:node scripts/ui-column-layout-smoke.mjs [--shots <dir>]。
//
// 用户截图 13:正文、运行活动行、输入区三条左边各在一处(style.css 旧规则给 OC 立绘留 222px 右沟,OC 关着也整列
// 左移 100px;列宽又各有一套写法)。静态断言(ui-a11y-smoke「对话单列」)只能锁住写法,这里在无头 Edge 里真量:
//   1. column 场景(空闲:历史 + 一轮已结束 + 发一条消息后的回复)在用户三档缩放 1280@1.5、1600@1.25、2000@1 与 1600@1:
//      以 pane 左右边为基准 L/R,正文首行字、工具组、子代理卡、notice 左缘 = L,子代理卡、用户气泡、输入区右缘 = R,
//      输入区左缘 = L(容差 1px);pane 宽 = min(768, 对话区宽 − 2×沟);#messages 左右内边距 0;
//   2. 工具组:折叠的多行组里失败行可见、成功行收起;单行组不显示组头;⎿ 摘要紧跟参数(间距 0~16px);
//      复制行(强制显形)在块下方、助手靠左/用户靠右,且不压下一块(至少量到一对「消息 → notice」);
//   3. composer 场景(运行中):活动行字形左缘 = L = 输入区左缘;
//   4. empty 场景:空态文案与输入区同一条中线(±1px);
//   5. 自检:注入「222px 右沟」「折叠态藏起失败行」「notice 前距 12px」「复制行回右上角」四种回归,判据必须变红,
//      否则报「判据失效」。
// page.evaluate 回调在浏览器里执行,用到的浏览器全局在这里声明给 ESLint(本文件其余部分是 node 环境)。
/* global window, document, getComputedStyle, NodeFilter */
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

// 用户的三档缩放(1280@1.5、1600@1.25、2000@1 CSS px)+ 1600@1 对照。
const COMBOS = [
  { width: 1280, height: 1000, dpr: 1.5 },
  { width: 1600, height: 1000, dpr: 1.25 },
  { width: 2000, height: 1000, dpr: 1 },
  { width: 1600, height: 1000, dpr: 1 },
];
const TOLERANCE = 1;
const MUTATIONS = {
  // 根因 A 放回去:OC 关着也给 #messages 留 222px 右沟,pane 在不对称内容盒里居中。
  gutter222: "#messages { padding-right: 222px; }",
  // 折叠态把失败行也藏起来(契约 §4.1「错了不该藏起来」)。
  hideFailures: ".tool-group:not([data-expanded='1']) > .tool-group-body > .tool-msg.err { display: none; }",
  // 复核 major:notice 前距压回 12px,每轮最后一条回复的复制行压在「本轮结束」上。
  noticeGap: ".msg.notice { margin-top: 12px !important; }",
  // 复制行回到块右上角(旧版漂在段落右上、压住首行最后几个字)。
  copyTopRight: ".msg-actions { top: 0 !important; left: auto !important; right: 0 !important; }",
};

async function openScene(browser, origin, { scene, theme = "dark", width, height, dpr, mutate = "" }) {
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: dpr, colorScheme: theme });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));
  page.on("console", (message) => { if (message.type() === "error") errors.push(`console.error: ${message.text()}`); });
  await page.goto(`${origin}/?theme=${theme}&scene=${scene}`, { waitUntil: "load" });
  await page.waitForFunction(() => window.__kzPreview?.ready === true, null, { timeout: 30000 });
  if (mutate) await page.addStyleTag({ content: mutate });
  await page.waitForTimeout(250);
  return { context, page, errors };
}

/// 浏览器内测量:返回 { failures: [...], info }。
function measureColumn({ tolerance, checkGroups }) {
  const out = [];
  const q = (selector, root = document) => root.querySelector(selector);
  const rect = (el) => el?.getBoundingClientRect();
  const visible = (el) => Boolean(el) && getComputedStyle(el).display !== "none" && rect(el).width > 0;
  const pane = q('.msg-pane[data-active="1"]');
  const chatArea = q("#chat-area");
  const messages = q("#messages");
  if (!pane || !chatArea) return { failures: ["找不到活动 pane 或 #chat-area"], info: {} };
  const P = rect(pane);
  const L = P.left;
  const R = P.right;
  const near = (a, b) => Math.abs(a - b) <= tolerance;
  const firstTextLeft = (el) => {
    if (!el) return null;
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if (!node.textContent.trim()) continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      const box = range.getClientRects()[0];
      if (box) return box.left;
    }
    return null;
  };
  const gutter = window.innerWidth <= 1024 ? 12 : 24;
  const expected = Math.min(768, rect(chatArea).width - 2 * gutter);
  if (!near(P.width, expected)) out.push(`pane 宽 ${P.width.toFixed(1)},应为 min(768, 对话区宽 − 2×${gutter}) = ${expected.toFixed(1)}`);
  const pad = getComputedStyle(messages);
  if (pad.paddingLeft !== "0px" || pad.paddingRight !== "0px") out.push(`#messages 左右内边距应为 0,实为 ${pad.paddingLeft} / ${pad.paddingRight}`);
  const lefts = {
    "正文首行字": firstTextLeft(q(".msg.assistant .message-body", pane)),
    "工具组": rect([...pane.children].find((el) => el.classList.contains("tool-group") && visible(el)))?.left,
    "子代理卡": rect(q(".sa-group", pane))?.left,
    "notice 文字": firstTextLeft(q(".msg.notice", pane)),
    "输入区": rect(q("#composer"))?.left,
  };
  for (const [label, value] of Object.entries(lefts)) {
    if (value === null || value === undefined) out.push(`${label}:找不到可测量的节点`);
    else if (!near(value, L)) out.push(`${label}左缘 ${value.toFixed(1)} ≠ 列左缘 ${L.toFixed(1)}`);
  }
  const rights = {
    "子代理卡": rect(q(".sa-group", pane))?.right,
    "用户气泡": rect(q(".msg.user", pane))?.right,
    "输入区": rect(q("#composer"))?.right,
  };
  for (const [label, value] of Object.entries(rights)) {
    if (value === null || value === undefined) out.push(`${label}:找不到可测量的节点`);
    else if (!near(value, R)) out.push(`${label}右缘 ${value.toFixed(1)} ≠ 列右缘 ${R.toFixed(1)}`);
  }
  if (checkGroups) {
    const groups = [...pane.querySelectorAll(".tool-group")];
    const folded = groups.find((group) => group.dataset.expanded !== "1" && Number(group.dataset.count) > 1 && group.querySelector(".tool-msg.err"));
    if (!folded) out.push("没有「折叠且含失败行」的多行工具组可测(场景数据变了)");
    else {
      if (!visible(folded.querySelector(".tool-msg.err"))) out.push("折叠的工具组里失败行被藏起来了(应常驻可见)");
      if (visible(folded.querySelector(".tool-msg.ok"))) out.push("折叠的工具组里成功行没有收起");
      if (!visible(folded.querySelector(".tool-group-head"))) out.push("多行工具组没有组头");
    }
    const single = groups.find((group) => group.dataset.count === "1");
    if (!single) out.push("没有单行工具组可测(场景数据变了)");
    else if (visible(single.querySelector(".tool-group-head"))) out.push("单行工具组仍显示组头");
    let rows = 0;
    for (const row of pane.querySelectorAll(".tool-msg")) {
      const arg = row.querySelector(".tool-msg-arg");
      const result = row.querySelector(".tool-msg-result");
      if (!visible(row) || !visible(arg) || !visible(result) || !result.textContent.trim()) continue;
      rows += 1;
      const gap = rect(result).left - rect(arg).right;
      if (gap < -0.5 || gap > 16) out.push(`⎿ 摘要没有紧跟参数:间距 ${gap.toFixed(1)}px(${row.querySelector(".tool-msg-name")?.textContent})`);
    }
    if (!rows) out.push("没有可见的工具行可测 ⎿ 间距");
  }
  // 复制行(.msg-actions,悬停才显形):强制显形后量。它是绝对定位、不占流内高度,位置全靠下一块的前距让出来——
  // 须在块**下方**(顶边 ≥ 块底边),助手靠左(左缘在块左缘 ±8)、用户靠右(右缘 = 块右缘),且不与下一个兄弟块相交。
  // 最常见的位置是每轮最后一条回复 → 「本轮结束」notice(复核实测 notice 前距 12px 时叠 11px),必须至少量到一对。
  // 错误卡的「重试」是流内常驻(position: static),不在此列。
  const force = document.createElement("style");
  force.textContent = ".msg-actions { opacity: 1 !important; transition: none !important; }";
  document.head.appendChild(force);
  let copyRows = 0;
  let beforeNotice = 0;
  for (const block of pane.children) {
    const actions = [...block.children].find((el) => el.classList.contains("msg-actions"));
    if (!actions || !visible(block) || getComputedStyle(actions).position !== "absolute") continue;
    copyRows += 1;
    const A = rect(actions);
    const B = rect(block);
    const isUser = block.classList.contains("user");
    const tag = `${isUser ? "用户消息" : "正文"}「${(block.textContent || "").trim().slice(0, 12)}」`;
    if (A.top < B.bottom - 0.5) out.push(`${tag}的复制行不在块下方(顶 ${A.top.toFixed(1)} < 块底 ${B.bottom.toFixed(1)})`);
    if (isUser ? Math.abs(A.right - B.right) > tolerance : Math.abs(A.left - B.left) > 8) {
      out.push(`${tag}的复制行没有贴${isUser ? "右" : "左"}(复制行 ${A.left.toFixed(1)}→${A.right.toFixed(1)},块 ${B.left.toFixed(1)}→${B.right.toFixed(1)})`);
    }
    const next = block.nextElementSibling;
    if (!next || !visible(next)) continue;
    if (next.classList.contains("notice")) beforeNotice += 1;
    const N = rect(next);
    const overlap = Math.min(A.bottom, N.bottom) - Math.max(A.top, N.top);
    if (overlap > 0.5 && A.left < N.right && A.right > N.left) {
      out.push(`${tag}的复制行压在下一块(${next.className})上 ${overlap.toFixed(1)}px(下一块前距 ${getComputedStyle(next).marginTop})`);
    }
  }
  force.remove();
  if (!copyRows) out.push("没有带复制行的消息可测(场景数据变了)");
  if (!beforeNotice) out.push("没有「消息 → notice」相邻对可测复制行让位(场景数据变了)");
  return { failures: out, info: { L: Math.round(L * 10) / 10, R: Math.round(R * 10) / 10, width: Math.round(P.width), copyRows } };
}

function measureActivity({ tolerance }) {
  const pane = document.querySelector('.msg-pane[data-active="1"]');
  const glyph = document.querySelector("#turn-activity-glyph");
  const composer = document.querySelector("#composer");
  const out = [];
  if (!pane || !glyph || document.querySelector("#turn-activity").classList.contains("hidden")) return ["运行中场景没有显示运行活动行"];
  const L = pane.getBoundingClientRect().left;
  const g = glyph.getBoundingClientRect().left;
  const c = composer.getBoundingClientRect().left;
  if (Math.abs(g - L) > tolerance) out.push(`活动行字形左缘 ${g.toFixed(1)} ≠ 列左缘 ${L.toFixed(1)}`);
  if (Math.abs(c - L) > tolerance) out.push(`运行态输入区左缘 ${c.toFixed(1)} ≠ 列左缘 ${L.toFixed(1)}`);
  return out;
}

function measureEmpty({ tolerance }) {
  const copy = document.querySelector('.msg-pane[data-active="1"] .empty-copy');
  const composer = document.querySelector("#composer");
  if (!copy) return ["空态场景找不到 .empty-copy"];
  const a = copy.getBoundingClientRect();
  const b = composer.getBoundingClientRect();
  const ca = a.left + a.width / 2;
  const cb = b.left + b.width / 2;
  const h1 = copy.querySelector("h1")?.getBoundingClientRect();
  const out = [];
  if (Math.abs(ca - cb) > tolerance) out.push(`空态中线 ${ca.toFixed(1)} ≠ 输入区中线 ${cb.toFixed(1)}`);
  if (h1 && Math.abs(h1.left + h1.width / 2 - cb) > 8) out.push(`空态标题没有居中(标题中线 ${(h1.left + h1.width / 2).toFixed(1)},输入区中线 ${cb.toFixed(1)})`);
  return out;
}

export async function runColumnLayoutSmoke({ channel = "msedge", shotsDir = null } = {}) {
  const failures = [];
  const notes = [];
  const { origin, close } = await startPreviewServer({ port: 0 });
  const browser = await chromium.launch({ channel, headless: true });
  try {
    if (shotsDir) await mkdir(shotsDir, { recursive: true });
    for (const combo of COMBOS) {
      const tag = `${combo.width}@${combo.dpr}`;
      const { context, page, errors } = await openScene(browser, origin, { scene: "column", ...combo });
      const result = await page.evaluate(measureColumn, { tolerance: TOLERANCE, checkGroups: true });
      for (const failure of result.failures) failures.push(`column ${tag}:${failure}`);
      for (const error of errors) failures.push(`column ${tag}:${error}`);
      notes.push(`${tag} 列 ${result.info.L}→${result.info.R}`);
      if (shotsDir) await page.screenshot({ path: path.join(shotsDir, `column-${tag}.png`) });
      await context.close();
      const running = await openScene(browser, origin, { scene: "composer", ...combo });
      for (const failure of await running.page.evaluate(measureActivity, { tolerance: TOLERANCE })) failures.push(`composer ${tag}:${failure}`);
      for (const error of running.errors) failures.push(`composer ${tag}:${error}`);
      await running.context.close();
    }
    const empty = await openScene(browser, origin, { scene: "empty", width: 1600, height: 1000, dpr: 1 });
    for (const failure of await empty.page.evaluate(measureEmpty, { tolerance: TOLERANCE })) failures.push(`empty 1600@1:${failure}`);
    await empty.context.close();
    // 自检:每种回归都必须被判红。
    for (const [id, css] of Object.entries(MUTATIONS)) {
      const probe = await openScene(browser, origin, { scene: "column", width: 1600, height: 1000, dpr: 1, mutate: css });
      const result = await probe.page.evaluate(measureColumn, { tolerance: TOLERANCE, checkGroups: true });
      await probe.context.close();
      if (!result.failures.length) failures.push(`对话单列判据失效:注入回归 ${id} 后仍全绿`);
    }
  } finally {
    await browser.close();
    await close();
  }
  return { failures, notes };
}

const entry = process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;
if (entry) {
  const index = process.argv.indexOf("--shots");
  const shotsDir = index > 0 ? path.resolve(process.argv[index + 1]) : null;
  const { failures, notes } = await runColumnLayoutSmoke({ shotsDir });
  if (failures.length) {
    console.error(`对话单列浏览器冒烟失败(${failures.length} 处):`);
    for (const failure of failures) console.error(` - ${failure}`);
    process.exit(1);
  }
  console.log(`对话单列浏览器冒烟通过:${notes.join(";")};工具组折叠/单行/⎿ 间距、复制行让位、活动行、空态同轴;${Object.keys(MUTATIONS).length} 种注入回归均被判红`);
}
