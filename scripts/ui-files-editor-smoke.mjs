// 文件页编辑器的真实浏览器回归(预览夹具里的 scene=files,Monaco 真实加载,IPC 用隔离夹具)。
// 覆盖验收缺陷:焦点切换不得让头部重排(首次点「保存」落空)、切树选「不保存」不得抛 Monaco 取消错误、
// 打开 html/css/ts 文件不得 404 语言服务模块。
/* global window, document, getComputedStyle */
import assert from "node:assert/strict";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const passed = [];
const check = (value, label) => { assert(value, label); passed.push(label); };
try {
  for (const viewport of [{ width: 1333, height: 695 }, { width: 1000, height: 700 }]) {
    const page = await browser.newPage({ viewport });
    page.setDefaultTimeout(10000);
    const errors = [], bad = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
    page.on("response", (response) => { if (response.status() >= 400) bad.push(`HTTP ${response.status()} ${response.url()}`); });
    const settle = () => page.evaluate(() => window.__kzPreview.settle());
    const calls = (cmd) => page.evaluate((cmd) => window.__kzPreview.calls.filter((call) => call.cmd === cmd), cmd);
    const box = (selector) => page.locator(selector).boundingBox();
    const label = `${viewport.width}x${viewport.height}`;
    await page.goto(`${server.origin}/?scene=files&theme=light`);
    await page.waitForFunction(() => window.__kzPreview?.ready);
    await page.waitForFunction(() => document.querySelector("#files-editor .monaco-editor") && document.querySelector("#files-preview-path")?.textContent);
    await settle();

    // 场景自带「registry.rs 已是未保存状态」。先量未聚焦时头部与按钮的位置。
    const idleHead = await box("#files-preview-head"), idleSave = await box("#files-save"), idleDiscard = await box("#files-discard");
    await page.locator(".monaco-editor .view-lines").first().click({ position: { x: 200, y: 60 } });
    await page.keyboard.type("// edit ");
    await page.waitForFunction(() => !document.querySelector("#files-tab-hint").classList.contains("hidden"));
    check(await page.evaluate(() => document.querySelector("#files-tab-hint").parentElement.id === "files-main" && getComputedStyle(document.querySelector("#files-tab-hint")).position === "absolute"),
      `${label}: the Tab hint floats over the editor instead of living in the header flow`);
    const focusedHead = await box("#files-preview-head"), focusedSave = await box("#files-save"), focusedDiscard = await box("#files-discard");
    check(focusedHead.height === idleHead.height && focusedSave.y === idleSave.y && focusedSave.x === idleSave.x && focusedDiscard.y === idleDiscard.y,
      `${label}: focusing the editor never reflows the header or moves the Save / Discard buttons`);
    check(await page.evaluate(() => getComputedStyle(document.querySelector("#files-tab-hint")).pointerEvents === "none"), `${label}: the floating hint never intercepts clicks`);

    // 第一次点击就要生效:按下鼠标时编辑器失焦、提示收起,按钮必须原地不动。
    const before = (await calls("file_write")).length;
    const target = await box("#files-save");
    await page.mouse.move(target.x + target.width / 2, target.y + target.height / 2);
    await page.mouse.down();
    const pressed = await box("#files-save");
    check(pressed.x === target.x && pressed.y === target.y, `${label}: the Save button stays put while the mouse is pressed`);
    await page.mouse.up();
    await settle();
    check((await calls("file_write")).length === before + 1, `${label}: the first click on Save writes the file`);

    // 切树选「不保存」:Monaco 销毁 model 时的取消不能变成未捕获错误(此前必现 pageerror: Canceled)。
    await page.locator(".monaco-editor .view-lines").first().click({ position: { x: 200, y: 60 } });
    await page.keyboard.type("// again ");
    await settle();
    const errorsBefore = errors.length;
    await page.selectOption("#files-tree-select", { index: 1 });
    await page.locator("#confirm-overlay button", { hasText: /不保存/ }).click();
    await page.waitForTimeout(800);
    check(errors.length === errorsBefore, `${label}: switching trees with unsaved edits and choosing "Don't save" raises no uncaught error`);

    // 只带了 JSON 语言服务的 Monaco:html / css / ts / js 的 *Mode 模块用空壳顶住,打开它们不得 404。
    await page.evaluate(async () => {
      const fixtures = await window.__kzPreview.fixtures();
      for (const [path, text] of [["站点/首页.html", "<h1>你好</h1>"], ["站点/a.css", "a{color:red}"], ["站点/b.ts", "const x: number = 1;"], ["站点/c.js", "let y = 2"]]) {
        fixtures.state.fileDisk.disk.set(path, { text, bom: false, note: "", mtime: 3 });
      }
    });
    await page.selectOption("#files-tree-select", { index: 0 });
    await page.waitForTimeout(500);
    await page.click("#files-refresh");
    await page.waitForTimeout(500);
    const languages = [];
    for (const name of ["首页", "a.css", "b.ts", "c.js"]) {
      await page.fill("#files-filter", name);
      await page.waitForTimeout(250);
      await page.keyboard.press("Enter");
      await page.waitForTimeout(700);
      languages.push(await page.evaluate(() => globalThis.monaco.editor.getModels().at(-1)?.getLanguageId()));
    }
    check(languages.join() === "html,css,typescript,javascript", `${label}: html/css/ts/js files open with their own syntax highlighting (${languages.join()})`);
    check(bad.filter((line) => line.includes("/vendor/monaco/")).length === 0, `${label}: opening them never requests a missing Monaco language module (${bad.join("; ")})`);
    check(errors.length === errorsBefore, `${label}: zero console errors on the whole files-editor path (${errors.join("; ")})`);
    await page.close();
  }
  console.log(`Files editor PASS: ${passed.length} checks`);
} finally {
  await browser.close();
  await server.close();
}
