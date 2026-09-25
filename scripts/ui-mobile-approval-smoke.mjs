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
];
const answers = [];
const pageErrors = [];

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

const server = http.createServer(async (request, response) => {
  const url = new URL(request.url, "http://127.0.0.1");
  if (url.pathname.startsWith("/v1/")) {
    if (request.headers.authorization !== "Bearer mobile-smoke-token") {
      jsonResponse(response, 401, { error: "unauthorized" });
      return;
    }
    if (request.method === "GET" && url.pathname === "/v1/approval/pending") {
      jsonResponse(response, 200, { pending, count: pending.length });
      return;
    }
    if (request.method === "POST" && url.pathname === "/v1/approval/answer") {
      const payload = JSON.parse(await requestBody(request));
      answers.push(payload);
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
});

try {
  await context.addInitScript(() => {
    localStorage.setItem("kanzei_device", JSON.stringify({
      device_id: "mobile-smoke-device",
      token: "mobile-smoke-token",
    }));
    // 本测试只覆盖首次 pending 响应,禁用定时刷新避免它覆盖正在交互的 fixture。
    globalThis.setInterval = () => 0;
  });
  const page = await context.newPage();
  page.on("pageerror", (error) => pageErrors.push(String(error)));
  await page.goto(`${baseUrl}/`, { waitUntil: "networkidle" });
  await page.locator('#approval-list .question[data-ask-id="21"]').waitFor();

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

  const freeText = page.locator('#approval-list .question[data-ask-id="22"]');
  assert.equal(await freeText.locator(".question-option").count(), 0, "空选项 question 应提供纯文本回答");
  await freeText.locator(".question-answer").fill("用户的自由文本答案");
  const freeTextResponse = page.waitForResponse((res) => res.url().endsWith("/v1/approval/answer"));
  await freeText.locator(".question-submit").click();
  await freeTextResponse;
  assert.deepEqual(answers[1], { id: 22, reply: "用户的自由文本答案" });

  const multiple = page.locator('#approval-list .question[data-ask-id="23"]');
  await multiple.locator(".question-option").nth(0).click();
  await multiple.locator(".question-option").nth(1).click();
  await multiple.locator(".question-answer").fill("另加颜色说明");
  const multipleResponse = page.waitForResponse((res) => res.url().endsWith("/v1/approval/answer"));
  await multiple.locator(".question-submit").click();
  await multipleResponse;
  assert.deepEqual(answers[2], { id: 23, reply: "红\n蓝\n另加颜色说明" });

  const permission = page.locator('#approval-list .card.approval:not(.question)');
  assert.equal(await permission.locator(".approve").count(), 1, "permission 卡片保留批准按钮");
  assert.equal(await permission.locator(".reject").count(), 1, "permission 卡片保留拒绝按钮");
  assert(answers.filter((answer) => [21, 22, 23].includes(answer.id))
    .every((answer) => !["allow", "deny"].includes(answer.reply)), "question 不能提交 allow/deny");
  assert.deepEqual(pageErrors, [], `PWA page errors: ${pageErrors.join(" | ")}`);
  console.log("PASS: question options, free text, multi-select, exact answer payloads, and permission buttons");
} finally {
  await context.close();
  await browser.close();
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
}
