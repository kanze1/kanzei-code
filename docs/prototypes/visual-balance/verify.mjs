import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { startVisualPrototype, baselineTag } from "./serve.mjs";

const output = new URL("../../../output/playwright/visual-balance-v3-20261020/", import.meta.url);
await mkdir(output, { recursive: true });
const server = await startVisualPrototype({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 1.25 });
const checks = [], errors = [], preservation = [], releaseAssets = new Set();
const check = (value, label) => { assert(value, label); checks.push(label); };
function contrast(a, b) {
  const luminance = color => {
    let hex = color.trim().replace("#", "");
    if (hex.length === 3) hex = [...hex].map(x => x + x).join("");
    const rgb = [0, 2, 4].map(i => parseInt(hex.slice(i, i + 2), 16) / 255).map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4);
    return .2126 * rgb[0] + .7152 * rgb[1] + .0722 * rgb[2];
  };
  const x = luminance(a), y = luminance(b);
  return (Math.max(x, y) + .05) / (Math.min(x, y) + .05);
}
page.setDefaultTimeout(15000);
page.on("pageerror", error => errors.push(error.message));
page.on("response", response => {
  if (response.status() >= 400) errors.push(`${response.status()} ${response.url()}`);
  const url = new URL(response.url());
  if (url.origin === server.origin && !url.pathname.startsWith("/__") && url.pathname !== "/favicon.ico") {
    if (response.headers()["x-kanzei-baseline"] !== baselineTag) errors.push(`Unpinned product asset: ${url.pathname}`);
    releaseAssets.add(url.pathname);
  }
});
const ready = () => page.waitForFunction(() => document.body.dataset.ready === "true");
const app = () => page.frames().find(frame => frame.parentFrame());
const shot = async name => {
  await app().locator("#toast").waitFor({ state: "hidden" });
  await page.locator("#product").screenshot({ path: fileURLToPath(new URL(`${name}.png`, output)), animations: "disabled" });
};
const palette = frame => frame.evaluate(() => {
  const root = getComputedStyle(document.documentElement);
  const tokens = Object.fromEntries([...root].filter(name => name.startsWith("--")).sort().map(name => [name, root.getPropertyValue(name).trim()]));
  const selectors = ["body", "#activitybar", "#sidebar", ".workspace-header", "#composer", "#prompt", "#statusbar", "#send", ".md-code-block", ".msg.user", ".msg.assistant"];
  const surfaces = Object.fromEntries(selectors.map(selector => {
    const css = getComputedStyle(document.querySelector(selector));
    return [selector, { background: css.backgroundColor, image: css.backgroundImage, color: css.color }];
  }));
  return { tokens, surfaces };
});
const activityLayout = frame => frame.locator("#tasks-toggle").evaluate(button => {
  const badge = button.querySelector("#tasks-badge"), icon = button.querySelector("svg").getBoundingClientRect();
  const count = badge.getBoundingClientRect(), target = button.getBoundingClientRect();
  return { gap:count.left - icon.right, contained:count.right <= target.right && count.top >= target.top && count.bottom <= target.bottom, width:target.width, label:button.getAttribute("aria-label"), count:badge.textContent };
});
async function checkActivity(frame, label) {
  check(await frame.locator("#tasks-badge").isVisible(), `${label}: the real activity count is visible`);
  const layout = await activityLayout(frame);
  check(layout.gap >= 4 && layout.contained, `${label}: activity icon and count have separate, contained space`);
  await frame.locator("#tasks-toggle").screenshot({ path:fileURLToPath(new URL(`activity-${label.replaceAll(" ", "-")}.png`, output)), animations:"disabled" });
  const variants = await frame.locator("#tasks-toggle").evaluate(button => {
    const badge = button.querySelector("#tasks-badge"), original = badge.textContent;
    const results = ["1", "12", "120"].map(text => {
      badge.textContent = text;
      const a = button.querySelector("svg").getBoundingClientRect(), b = badge.getBoundingClientRect(), target = button.getBoundingClientRect();
      return { text, gap:b.left - a.right, contained:b.right <= target.right, width:target.width };
    });
    badge.classList.add("hidden");
    const hidden = badge.getBoundingClientRect().width === 0;
    badge.textContent = original; badge.classList.remove("hidden");
    return { results, hidden };
  });
  check(variants.results.every(result => result.gap >= 4 && result.contained), `${label}: one, two and three digit counts do not collide with the icon`);
  check(variants.hidden, `${label}: a hidden count occupies no space`);
}
async function checkTabHover(frame, selector, label) {
  const tab = frame.locator(selector).nth(1);
  const before = await tab.evaluate(element => {
    const box = element.getBoundingClientRect();
    return { x:box.x, y:box.y, width:box.width };
  });
  await tab.hover();
  await frame.waitForFunction(selector => getComputedStyle(document.querySelectorAll(selector)[1], "::before").backgroundColor !== "rgba(0, 0, 0, 0)", selector);
  const hover = await tab.evaluate(element => {
    const box = element.getBoundingClientRect(), css = getComputedStyle(element), patch = getComputedStyle(element, "::before");
    return { background:css.backgroundColor, patchHeight:parseFloat(patch.height), height:box.height, x:box.x, y:box.y, width:box.width };
  });
  check(hover.background === "rgba(0, 0, 0, 0)" && hover.patchHeight < hover.height * .65, `${label}: hover background stays near the label instead of filling the tab`);
  check(hover.height >= 44 && hover.width === before.width && hover.x === before.x && hover.y === before.y, `${label}: compact feedback preserves a large, stable click target`);
}
try {
  await page.goto(`${server.origin}/__visual-balance/`); await ready();
  const baseline = await page.request.get(`${server.origin}/__visual-balance/baseline.json`);
  check((await baseline.json()).commit === "8fb9f71d0b3c0a6f0f42c7c1c24ae26a158721de", "Comparison reads the published commit, including assets and modules");
  check(await app().locator(".md-code-block").count() === 1, "Conversation uses the real Markdown code rendering and copy control");
  const before = await app().locator("#composer").evaluate(el => ({ width: el.getBoundingClientRect().width, border: getComputedStyle(el).borderColor, shadow: getComputedStyle(el).boxShadow }));
  await app().locator("#prompt").focus();
  const focused = await app().locator("#composer").evaluate(el => ({ width: el.getBoundingClientRect().width, border: getComputedStyle(el).borderColor, shadow: getComputedStyle(el).boxShadow }));
  check(before.width === focused.width, "Focusing the composer preserves its geometry");
  check(await app().locator("#send").evaluate(el => getComputedStyle(el).backgroundColor) === "rgb(197, 101, 0)", "Light-theme send retains the release-adapted orange");
  check(await app().locator(".workspace-sidebar-footer .activity-item").evaluate(el => el.clientWidth > 140 && el.clientHeight < 42), "Sidebar footer remains a single horizontal label");
  check(await app().locator("#send").evaluate(el => getComputedStyle(el).borderRadius) === "50%", "Send control retains a circular silhouette");
  await app().locator("#prompt").blur();
  await page.locator("#baseline").click();
  check(await app().evaluate(() => document.getElementById("visual-proposal").disabled), "Original design comparison disables only the proposal stylesheet");
  check(await app().locator(".md-code-block").count() === 1, "Baseline comparison retains the same conversation data");
  await shot("baseline-chat-light");
  await page.locator("#baseline").click();
  await page.locator("#colors").click();
  check(await page.locator("#color-notes").evaluate(el => el.matches(":popover-open")), "Color allocation is available on demand");
  await page.screenshot({ path: fileURLToPath(new URL("color-notes-light.png", output)) });
  await page.keyboard.press("Escape");
  for (const theme of ["light", "dark"]) {
    if (await page.locator("html").getAttribute("data-theme") !== theme) {
      await page.locator("#theme").click();
      await page.waitForFunction(theme => document.querySelector("#product").contentDocument.documentElement.dataset.theme === theme, theme);
    }
    for (const scene of ["chat", "empty", "work", "memory", "skills", "settings"]) {
      await page.locator(`[data-scene="${scene}"]`).click(); await ready();
      const frame = app();
      check(await frame.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), `${scene} ${theme}: no horizontal page overflow`);
      check(await frame.evaluate(() => !document.getElementById("visual-proposal").disabled), `${scene} ${theme}: proposal styles are active`);
      check(await frame.locator("html").getAttribute("data-theme") === theme, `${scene}: theme follows the review control`);
      if (scene === "chat") {
        const proposed = await palette(frame);
        await page.locator("#baseline").click();
        const published = await palette(frame);
        assert.deepEqual(proposed.tokens, published.tokens, `${theme}: every published root color and theme token must remain unchanged`);
        check(true, `${theme}: all ${Object.keys(published.tokens).length} published root tokens remain unchanged`);
        for (const [selector, appearance] of Object.entries(published.surfaces)) {
          assert.deepEqual(proposed.surfaces[selector], appearance, `${theme}: ${selector} release surface, texture and text must remain unchanged`);
          check(true, `${theme}: ${selector} retains its release background, texture and text color`);
        }
        preservation.push({ theme, ...published });
        await shot(`baseline-chat-${theme}`);
        await page.locator("#baseline").click();
        const colors = await frame.evaluate(() => {
          const c = getComputedStyle(document.documentElement);
          return Object.fromEntries(["--bg", "--sidebar-bg", "--fg", "--fg-strong", "--dim", "--ok", "--on-accent", "--accent"].map(key => [key, c.getPropertyValue(key)]));
        });
        for (const fg of ["--fg", "--fg-strong", "--dim", "--ok"]) check(contrast(colors[fg], colors["--bg"]) >= 4.5, `${theme}: ${fg} remains readable on the main surface`);
        check(contrast(colors["--dim"], colors["--sidebar-bg"]) >= 4.5, `${theme}: secondary sidebar text retains contrast`);
        check(contrast(colors["--on-accent"], colors["--accent"]) >= 3, `${theme}: send arrow retains contrast on orange`);
        await checkActivity(frame, `${theme} wide`);
        const label = await frame.locator("#tasks-toggle").getAttribute("aria-label");
        await frame.locator("#tasks-toggle").click();
        check(await frame.locator("#tasks-panel").isVisible(), `${theme}: the activity control still opens its panel`);
        await frame.locator("#tasks-close").click();
        check(await frame.locator("#tasks-toggle").getAttribute("aria-expanded") === "false" && await frame.locator("#tasks-toggle").getAttribute("aria-label") === label, `${theme}: closing activity preserves its accessible count and state`);
        await checkTabHover(frame, ".workspace-header [data-work-surface]", `${theme}: workspace tabs`);
        await page.mouse.move(0, 0);
      }
      if (scene === "work") {
        const accents = await frame.evaluate(() => {
          const root = getComputedStyle(document.documentElement), toRgb = hex => `rgb(${hex.match(/[a-f\d]{2}/gi).slice(0,3).map(part => parseInt(part,16)).join(", ")})`;
          return {
            orange:toRgb(root.getPropertyValue("--accent").trim()), cyan:toRgb(root.getPropertyValue("--ok").trim()),
            workspace:getComputedStyle(document.querySelector('.workspace-header [aria-pressed="true"]'), "::after").backgroundColor,
            content:getComputedStyle(document.querySelector('.management-tabs [aria-current="page"]'), "::after").backgroundColor,
            id:getComputedStyle(document.querySelector(".management-id")).color,
            done:[...document.querySelectorAll('.management-progress i[data-state="done"]')].map(cell => getComputedStyle(cell).backgroundColor),
          };
        });
        check(accents.workspace === accents.orange && accents.content === accents.cyan, `${theme}: orange workspace and cyan content markers provide distinct accents`);
        check(accents.id === accents.cyan && accents.done.length > 0 && accents.done.every(color => color === accents.cyan), `${theme}: short identifiers and completed progress add cyan details`);
        await checkTabHover(frame, ".management-tabs button", `${theme}: management tabs`);
        await frame.locator(".management-tabs").screenshot({ path:fileURLToPath(new URL(`tabs-hover-${theme}.png`, output)), animations:"disabled" });
        const bounds = await page.locator("#product").boundingBox();
        const sidebar = await frame.locator("#sidebar").boundingBox();
        await page.screenshot({ path:fileURLToPath(new URL(`navigation-hover-${theme}.png`, output)), clip:{ x:sidebar.x + sidebar.width, y:bounds.y, width:580, height:170 }, animations:"disabled" });
        await page.mouse.move(0, 0);
      }
      if (scene === "empty") {
        check(await frame.locator(".prototype-auto-hint").isVisible(), `${theme}: automatic mode has one quiet line in the empty welcome`);
        check(!await frame.locator("#project-onboarding").isVisible(), `${theme}: initial guidance does not occupy a persistent button panel`);
        await page.locator("#baseline").click();
        check(!await frame.locator(".prototype-auto-hint").isVisible(), `${theme}: prototype-only welcome copy is absent in release comparison`);
        await page.locator("#baseline").click();
      }
      await shot(`${scene}-${theme}`);
      {
        await page.locator("#narrow").click();
        await frame.waitForFunction(() => innerWidth <= 420);
        check(await frame.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), `${scene} ${theme}: 420px page remains contained`);
        if (scene === "chat" || scene === "empty") check(await frame.locator("#send").isVisible(), `${scene} ${theme}: composer action is visible at 420px`);
        if (scene === "chat") await checkActivity(frame, `${theme} narrow`);
        if (scene === "memory") check(await frame.locator(".memory-manager-toolbar label>span").evaluateAll(labels => labels.every(label => label.clientHeight < 2 * parseFloat(getComputedStyle(label).fontSize))), `${theme}: memory filter labels remain one readable horizontal line at 420px`);
        await shot(`${scene}-${theme}-narrow`);
        await page.locator("#narrow").click();
      }
    }
  }
  await page.setViewportSize({ width: 420, height: 900 });
  await page.locator('[data-scene="chat"]').click(); await ready();
  check(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "Review toolbar itself adapts to a narrow window");
  await page.screenshot({ path: fileURLToPath(new URL("review-narrow.png", output)) });
  check(errors.length === 0, `No browser or resource errors: ${errors.join("; ")}`);
  check(releaseAssets.size > 100, "Product resources consistently come from the published tag");
  await writeFile(new URL("qa.json", output), JSON.stringify({ baseline: server.baseline, releaseAssetCount: releaseAssets.size, checks, preservation, errors }, null, 2));
  console.log(`${checks.length} visual prototype checks passed; captures in ${fileURLToPath(output)}`);
} catch (error) {
  await page.screenshot({ path: fileURLToPath(new URL("failure.png", output)) });
  console.error({ checks, errors }); throw error;
} finally { await browser.close(); await server.close(); }
