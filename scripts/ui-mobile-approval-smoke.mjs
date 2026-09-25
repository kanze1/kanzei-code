#!/usr/bin/env node
import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const pwaRoot = path.join(repoRoot, "crates/kanzei-app/mobile-pwa");
let pending = [
  {
    id: 21,
    kind: "question",
    action: "question",
    question: "选择部署方式",
    options: [
      { label: "采用 A", note: "适用于当前环境" },
      { label: "采用 B", note: "需要额外准备" },
    ],
    default: "采用 A",
    multiple: false,
    session_id: "session-smoke",
    resource: "选择部署方式",
  },
  {
    id: 22,
    kind: "question",
    action: "question",
    question: "请补充说明",
    options: [],
    default: null,
    multiple: false,
    session_id: "session-smoke",
    resource: "请补充说明",
  },
  {
    id: 23,
    kind: "question",
    action: "question",
    question: "选择要保留的颜色,可多选",
    options: [
      { label: "红", note: "暖色" },
      { label: "蓝", note: "冷色" },
    ],
    default: null,
    multiple: true,
    session_id: "session-smoke",
    resource: "选择要保留的颜色",
  },
  {
    id: 24,
    kind: "permission",
    action: "bash",
    resource: "cargo test",
    session_id: "session-smoke",
  },
  {
    id: 26,
    kind: "permission",
    action: "bash",
    resource: "cargo build",
    session_id: "session-smoke",
  },
  {
    id: 27,
    kind: "question",
    action: "question",
    question: "慢速网络下的回答",
    options: [],
    default: null,
    multiple: false,
    session_id: "session-smoke",
    resource: "慢速网络下的回答",
  },
  {
    id: 28,
    kind: "question",
    action: "question",
    question: "发布流程下一步",
    // 字面量 "cancel" 是合法答案(发现 7):单选点它必须作为 reply 投递,不能被当成取消。
    options: [
      { label: "cancel", note: "字面量 cancel 是合法答案" },
      { label: "继续" },
    ],
    default: null,
    multiple: false,
    session_id: "session-smoke",
    resource: "发布流程下一步",
  },
];
// 轮询中途出现的新 question:覆盖「增量追加」与显式取消。
const lateQuestion = {
  id: 25,
  kind: "question",
  action: "question",
  question: "是否继续发布",
  options: [{ label: "继续发布" }],
  default: null,
  multiple: false,
  session_id: "session-smoke",
  resource: "是否继续发布",
};
const answers = [];
const pageErrors = [];
let pendingPolls = 0;
// 指定 id 的 answer 在测试放行前不返回,且 ask 仍留在 pending,模拟 POST 在途期间轮询照常到达。
let heldAnswer = null;
// >0 时 pending 响应按请求到达时的快照、延迟这么久再返回,模拟桥接/网络持续慢于 3s 轮询间隔。
let pendingDelayMs = 0;
// 慢轮询期间才出现的新 ask:响应全部晚于下一轮发起时,它也必须能落到页面上。
const slowQuestion = {
  id: 29,
  kind: "question",
  action: "question",
  question: "慢桥接下的新问题",
  options: [],
  default: null,
  multiple: false,
  session_id: "session-smoke",
  resource: "慢桥接下的新问题",
};

function jsonResponse(response, status, value) {
  response.writeHead(status, { "Content-Type": "application/json; charset=utf-8" });
  response.end(JSON.stringify(value));
}

function requestBody(request) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    request.on("error", reject);
  });
}

async function waitUntil(predicate, message, timeoutMs = 10000) {
  const deadline = Date.now() + timeoutMs;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error(`等待超时: ${message}`);
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
}

const server = http.createServer(async (request, response) => {
  const url = new URL(request.url, "http://127.0.0.1");
  if (url.pathname.startsWith("/v1/")) {
    if (request.headers.authorization !== "Bearer mobile-smoke-token") {
      jsonResponse(response, 401, { error: "unauthorized" });
      return;
    }
    if (request.method === "GET" && url.pathname === "/v1/approval/pending") {
      pendingPolls += 1;
      const snapshot = pending;
      if (pendingDelayMs > 0) await new Promise((resolve) => setTimeout(resolve, pendingDelayMs));
      jsonResponse(response, 200, { pending: snapshot, count: snapshot.length });
      return;
    }
    if (request.method === "POST" && url.pathname === "/v1/approval/answer") {
      const payload = JSON.parse(await requestBody(request));
      answers.push(payload);
      if (heldAnswer && heldAnswer.id === payload.id) {
        heldAnswer.received = true;
        await heldAnswer.released;
      }
      pending = pending.filter((ask) => ask.id !== payload.id);
      jsonResponse(response, 200, { answered: payload.id });
      return;
    }
    jsonResponse(response, 404, { error: "not found" });
    return;
  }

  const relative = decodeURIComponent(url.pathname).replace(/^\/+/, "") || "index.html";
  const filePath = path.resolve(pwaRoot, relative);
  if (filePath !== pwaRoot && !filePath.startsWith(`${pwaRoot}${path.sep}`)) {
    response.writeHead(403);
    response.end("forbidden");
    return;
  }
  fs.readFile(filePath, (error, content) => {
    if (error) {
      response.writeHead(404);
      response.end("not found");
      return;
    }
    const contentType = {
      ".html": "text/html; charset=utf-8",
      ".js": "text/javascript; charset=utf-8",
      ".css": "text/css; charset=utf-8",
      ".json": "application/json; charset=utf-8",
      ".svg": "image/svg+xml",
      ".png": "image/png",
    }[path.extname(filePath)] || "application/octet-stream";
    response.writeHead(200, { "Content-Type": contentType });
    response.end(content);
  });
});

await new Promise((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const address = server.address();
const baseUrl = `http://127.0.0.1:${address.port}`;
const browser = await chromium.launch({ channel: "msedge", headless: true });
const context = await browser.newContext({
  serviceWorkers: "block",
  viewport: { width: 375, height: 812 },
  locale: "zh-CN",
});

try {
  // 不桩掉生产轮询(D-751 跟进):3s 定时刷新照常运行,交互状态必须经得起重绘。
  await context.addInitScript(() => {
    localStorage.setItem("kanzei_device", JSON.stringify({
      device_id: "mobile-smoke-device",
      token: "mobile-smoke-token",
    }));
  });
  const page = await context.newPage();
  page.on("pageerror", (error) => pageErrors.push(String(error)));
  await page.goto(`${baseUrl}/`, { waitUntil: "networkidle" });
  await page.locator('#approval-list .question[data-ask-id="21"]').waitFor();
  const cardCount = (id) => page.locator(`#approval-list [data-ask-id="${id}"]`).count();

  const single = page.locator('#approval-list .question[data-ask-id="21"]');
  assert.equal(await single.locator(".approval-desc").textContent(), "选择部署方式");
  assert.equal(await single.locator(".question-option").count(), 2);
  assert.equal(await single.locator(".question-option-note").first().textContent(), "适用于当前环境");
  assert.equal(await single.locator(".question-answer").inputValue(), "采用 A");
  assert.equal(await single.locator(".approve, .reject").count(), 0, "question 卡片不可出现 permission 按钮");
  const singleResponse = page.waitForResponse((res) => res.url().endsWith("/v1/approval/answer"));
  await single.locator(".question-option").first().click();
  await singleResponse;
  assert.deepEqual(answers[0], { id: 21, reply: "采用 A" });

  // 先填自由文本、勾选多选并停留在输入框,再让轮询至少重绘一轮。
  const freeText = page.locator('#approval-list .question[data-ask-id="22"]');
  assert.equal(await freeText.locator(".question-option").count(), 0, "空选项 question 应提供纯文本回答");
  await freeText.locator(".question-answer").fill("用户的自由文本答案");
  const multiple = page.locator('#approval-list .question[data-ask-id="23"]');
  await multiple.locator(".question-option").nth(0).click();
  await multiple.locator(".question-option").nth(1).click();
  await multiple.locator(".question-answer").fill("另加颜色说明");
  const interactedAt = Date.now();
  const pollsBeforeWait = pendingPolls;
  // 服务端同时变更 pending:新增 25、移除 26(已在桌面端回答)。
  pending = [...pending.filter((ask) => ask.id !== 26), lateQuestion];
  await page.locator('#approval-list .question[data-ask-id="25"]').waitFor({ timeout: 8000 });
  await page.locator('#approval-list [data-ask-id="26"]').waitFor({ state: "detached", timeout: 8000 });
  await page.waitForTimeout(Math.max(0, 3500 - (Date.now() - interactedAt)));
  assert(pendingPolls > pollsBeforeWait, "交互后必须至少经历一次真实 pending 轮询");
  assert.equal(await freeText.locator(".question-answer").inputValue(), "用户的自由文本答案", "轮询重绘不得清空自由文本");
  assert.equal(await multiple.locator(".question-answer").inputValue(), "另加颜色说明", "轮询重绘不得清空多选补充说明");
  assert.equal(await multiple.locator(".question-option").nth(0).getAttribute("aria-pressed"), "true", "轮询重绘不得丢失多选状态");
  assert.equal(await multiple.locator(".question-option").nth(1).getAttribute("aria-pressed"), "true", "轮询重绘不得丢失多选状态");
  assert.equal(
    await page.evaluate(() => {
      const active = document.activeElement;
      return active?.classList.contains("question-answer") ? active.closest("[data-ask-id]")?.dataset.askId : null;
    }),
    "23",
    "轮询重绘不得抢走输入框焦点",
  );
  for (const id of [22, 23, 24, 25, 27, 28]) assert.equal(await cardCount(id), 1, `ask ${id} 只能有一张卡片`);

  const freeTextResponse = page.waitForResponse((res) => res.url().endsWith("/v1/approval/answer"));
  await freeText.locator(".question-submit").click();
  await freeTextResponse;
  assert.deepEqual(answers[1], { id: 22, reply: "用户的自由文本答案" });

  const multipleResponse = page.waitForResponse((res) => res.url().endsWith("/v1/approval/answer"));
  await multiple.locator(".question-submit").click();
  await multipleResponse;
  assert.deepEqual(answers[2], { id: 23, reply: "红\n蓝\n另加颜色说明" });

  // 显式取消:提交 {cancel: true},不再发带内字面量 "cancel"。
  const late = page.locator('#approval-list .question[data-ask-id="25"]');
  const cancelResponse = page.waitForResponse((res) => res.url().endsWith("/v1/approval/answer"));
  await late.locator(".question-cancel").click();
  await cancelResponse;
  assert.deepEqual(answers[3], { id: 25, cancel: true });
  await late.getByText("问题已取消").waitFor();

  // 字面量 "cancel" 选项是合法答案:单选点它投递 {id, reply:"cancel"},不能变成取消。
  const literal = page.locator('#approval-list .question[data-ask-id="28"]');
  const literalOption = literal.locator(".question-option").first();
  assert.equal(await literalOption.locator(".question-option-label").textContent(), "cancel");
  const literalResponse = page.waitForResponse((res) => res.url().endsWith("/v1/approval/answer"));
  await literalOption.click();
  await literalResponse;
  assert.deepEqual(answers.at(-1), { id: 28, reply: "cancel" }, "字面量 cancel 选项必须作为答案投递");
  await literal.getByText("答案已提交").waitFor();

  // POST 在途期间轮询照常到达:卡片不被重建成可点的新卡片,也不产生第二次 answer。
  let releaseHeld = () => {};
  heldAnswer = { id: 27, received: false, released: new Promise((resolve) => { releaseHeld = resolve; }) };
  const held = page.locator('#approval-list .question[data-ask-id="27"]');
  await held.locator(".question-answer").fill("慢速提交");
  await held.locator(".question-submit").click();
  await waitUntil(() => heldAnswer.received, "ask 27 的 answer 请求到达");
  const nextPoll = page.waitForResponse((res) => res.url().endsWith("/v1/approval/pending"), { timeout: 8000 });
  await (await nextPoll).finished();
  await page.waitForTimeout(300); // 让这一轮 pending 响应在页面内完成对账。
  assert.equal(await cardCount(27), 1, "提交在途时轮询不得重复渲染卡片");
  assert.equal(await held.locator(".question-submit").isDisabled(), true, "提交在途时提交按钮保持禁用");
  assert.equal(await held.locator(".question-cancel").isDisabled(), true, "提交在途时取消按钮保持禁用");
  releaseHeld();
  await held.getByText("答案已提交").waitFor();
  assert.deepEqual(answers.filter((answer) => answer.id === 27), [{ id: 27, reply: "慢速提交" }], "不得二次提交");
  heldAnswer = null;

  const permission = page.locator('#approval-list .card.approval[data-ask-id="24"]');
  assert.equal(await permission.locator(".approve").count(), 1, "permission 卡片保留批准按钮");
  assert.equal(await permission.locator(".reject").count(), 1, "permission 卡片保留拒绝按钮");
  const permissionResponse = page.waitForResponse((res) => res.url().endsWith("/v1/approval/answer"));
  await permission.locator(".approve").click();
  await permissionResponse;
  assert.deepEqual(answers.at(-1), { id: 24, reply: "allow" }, "permission 仍走 reply=allow 协议");

  // pending 清空后,空列表提示出现在独立状态行,不覆盖回执卡片。
  await page.locator("#approval-list > .approval-status", { hasText: "当前无待批准请求" }).waitFor({ timeout: 8000 });

  // 慢桥接(F4 复审):pending 响应持续 3.5s(>3s 轮询间隔)时,每个响应落地前都已发出更新一轮。
  // 只丢比「已采纳」更旧的响应——按「最后发起的一轮」取舍会把它们全部丢掉,新 ask 永远不出现。
  const pollsBeforeSlow = pendingPolls;
  pendingDelayMs = 3500;
  pending = [slowQuestion];
  await page.locator('#approval-list .question[data-ask-id="29"]').waitFor({ timeout: 15000 });
  assert(pendingPolls - pollsBeforeSlow >= 1, "慢轮询用例必须经过至少一次延迟的 pending 响应");
  assert.equal(await cardCount(29), 1, "慢响应落地后 ask 29 只能有一张卡片");
  pendingDelayMs = 0;

  // question 的答案不走 permission 协议;字面量 "cancel" 是合法答案(ask 28),
  // 显式取消由 answers[3] 精确约束为 {id:25, cancel:true}、不带 reply。
  assert(answers.filter((answer) => [21, 22, 23, 25, 27, 28].includes(answer.id))
    .every((answer) => !["allow", "deny"].includes(answer.reply)), "question 不能提交 allow/deny");
  assert.deepEqual(pageErrors, [], `PWA page errors: ${pageErrors.join(" | ")}`);
  console.log("PASS: question options, free text, multi-select survive polling, explicit cancel, literal cancel answer, no double submit, permission buttons, slow polls still apply");
} finally {
  await context.close();
  await browser.close();
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
}
