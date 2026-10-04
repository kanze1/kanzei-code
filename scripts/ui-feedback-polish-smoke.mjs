/* Production UI with isolated IPC; never writes real project data. */
/* global window, document, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = "output/playwright/feedback-polish";
const checks = [], errors = [];
const check = (condition, label) => { assert(condition, label); checks.push(label); };
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
page.setDefaultTimeout(12000);
page.on("pageerror", error => errors.push(error.stack || error.message));
try {
  await page.goto(`${server.origin}/?scene=chat&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  check(!await page.locator("#log-panel").isVisible(), "Runtime log starts collapsed");
  await page.evaluate(async () => (await import("/03-shell.js")).toastError("关闭失败：证据待补充"));
  check(!await page.locator("#log-panel").isVisible(), "An operation error keeps the log collapsed");
  check(await page.locator("#log-toggle").evaluate(el => el.classList.contains("has-error")), "A collapsed error is indicated on the log entry");
  await page.getByRole("button", { name: "查看日志", exact: true }).click();
  check(await page.locator("#log-panel").isVisible(), "The error notification explicitly opens the log");
  check(await page.locator("#log-toggle").getAttribute("aria-expanded") === "true", "The log toggle reports its open state");
  await page.locator("#log-close").click();
  await page.evaluate(async () => {
    const shell = await import("/03-shell.js"); shell.log("新的工具事件", "warn"); shell.reportPersistentError("第二次失败");
  });
  check(!await page.locator("#log-panel").isVisible(), "New events and errors respect a manually collapsed log");
  await page.locator("#log-toggle").click();
  check(await page.locator("#log-lines").innerText().then(text => text.includes("第二次失败")), "Collapsed logs retain error details");
  await page.locator("#log-toggle").click();

  for (const [theme, lang] of [["light", "zh"], ["dark", "en"]]) {
    await page.goto(`${server.origin}/?scene=chat&theme=${theme}&lang=${lang}`);
    await page.waitForFunction(() => window.__kzPreview?.ready);
    await page.locator("#composer-more").click();
    await page.waitForFunction(() => getComputedStyle(document.querySelector("#composer-more-menu")).opacity === "1");
    check(await page.locator("#composer-more-menu .composer-menu-section").count() === 2, `${theme}: More separates actions and run controls`);
    await page.screenshot({ path: `${output}/composer-${theme}.png` });
    const rows = await page.locator("#composer-more-menu button.composer-menu-action").evaluateAll(elements => elements.filter(el => el.getBoundingClientRect().height).map(el => ({ id: el.id, width: el.getBoundingClientRect().width, x: el.getBoundingClientRect().x, border: getComputedStyle(el).borderWidth })));
    check(rows.length >= 6 && rows.every(row => Math.abs(row.width - rows[0].width) < 1 && Math.abs(row.x - rows[0].x) < 1 && row.border === "0px"), `${theme}: Actions form aligned full-width rows ${JSON.stringify(rows)}`);
    check(await page.locator("#subagent-control").isVisible() && await page.locator("#autorun-bar").isVisible(), `${theme}: Existing run controls stay reachable`);
    check((await page.locator("#composer-more-menu").boundingBox()).height < 620, `${theme}: The menu has no oversized gaps`);
    await page.screenshot({ path: `${output}/composer-${theme}.png` });
    await page.keyboard.press("Escape");
    await page.locator('#workspace-sidebar-footer [data-view="settings"]').click();
    await page.screenshot({ path: `${output}/settings-home-${theme}.png` });
    check(await page.locator(".settings-section").count() === 5, `${theme}: Settings has five functional sections`);
    check(await page.locator("#settings-toc [data-settings-target]").count() === 17, `${theme}: Every top-level settings group is included in the outline`);
    check(await page.locator("#settings-save").isVisible(), `${theme}: Save is visible without scrolling`);
    const outline = await page.locator("#settings-toc").boundingBox();
    await page.locator('#settings-toc [data-settings-target="sg-limits"]').click();
    check(await page.locator("#sg-limits").evaluate(el => el.open), `${theme}: An outline click opens the requested group`);
    check(await page.locator('#settings-toc [data-settings-target="sg-limits"]').getAttribute("aria-current") === "location", `${theme}: The outline highlights the current group`);
    const after = await page.locator("#settings-toc").boundingBox();
    check(Math.abs(outline.y - after.y) < 1, `${theme}: The outline stays fixed while content scrolls`);
    await page.locator("#set-max-tokens").fill("4100");
    await page.locator('#settings-toc [data-settings-target="sg-sound"]').click();
    check(await page.locator("#set-max-tokens").inputValue() === "4100" && await page.locator("#settings-dirty").isVisible(), `${theme}: Section navigation preserves unsaved changes`);
    await page.locator('#settings-toc [data-settings-target="sg-general"]').click();
    await page.screenshot({ path: `${output}/settings-${theme}.png` });
    for (const width of [1000, 390]) {
      await page.setViewportSize({ width, height: 860 });
      await page.locator('#settings-toc [data-settings-target="sg-update"]').click();
      const geometry = await page.evaluate(() => {
        const view = document.querySelector("#view-settings"), scroll = document.querySelector("#settings-scroll"), save = document.querySelector("#settings-save").getBoundingClientRect();
        return { viewOverflow: view.scrollWidth > view.clientWidth + 1, contentOverflow: scroll.scrollWidth > scroll.clientWidth + 1, saveRight: save.right, saveBottom: save.bottom };
      });
      await page.screenshot({ path: `${output}/settings-${theme}-${width}.png` });
      check(!geometry.viewOverflow && !geometry.contentOverflow && geometry.saveRight <= width && geometry.saveBottom <= 860, `${theme} ${width}px: Settings fits and Save remains reachable ${JSON.stringify(geometry)}`);
    }
    await page.setViewportSize({ width: 1440, height: 960 });
  }
  check(errors.length === 0, "No browser runtime errors");
  console.log(`Feedback polish PASS: ${checks.length} checks`);
} finally {
  await writeFile(`${output}/verification.json`, JSON.stringify({ checks, errors }, null, 2));
  await browser.close(); await server.close();
}
