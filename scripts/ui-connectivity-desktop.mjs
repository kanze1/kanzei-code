/* global window, document */
// Route registration and visible entry points are different contracts. Exercise
// both against production modules with isolated IPC, rather than infer navigation
// from the number of sidebar buttons.
import assert from "node:assert/strict";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

export async function checkDesktopRoutes(keyPaths) {
  const server = await startPreviewServer({ port: 0 });
  const browser = await chromium.launch({ channel: "msedge", headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.setDefaultTimeout(10000);
  const errors = [], activated = [], entries = [];
  page.on("pageerror", error => errors.push(error.message));
  const settle = () => page.evaluate(() => window.__kzPreview.settle());
  const active = async view => {
    await page.waitForFunction(view => document.body.dataset.view === view && document.getElementById(`view-${view}`)?.classList.contains("active"), view);
    await settle();
    assert.deepEqual(await page.locator('.view.active').evaluateAll(nodes => nodes.map(node => node.id)), [`view-${view}`], `exactly one active container for ${view}`);
  };
  const navigate = async view => {
    await page.evaluate(async view => (await import("/03-shell.js")).navigate_view(view), view);
    await active(view);
  };
  try {
    await page.goto(`${server.origin}/?scene=workspace&theme=light`);
    await page.waitForFunction(() => window.__kzPreview?.ready);
    await page.locator(".workspace-card-open").first().click();
    await settle();
    const project = await page.evaluate(async () => (await import("/12-workbench.js")).workbenchProject());
    assert(project, "a visible workspace card establishes a project browsing identity");
    const structure = await page.evaluate(async () => {
      const { views } = await import("/00-views.js");
      return {
        registered: Object.keys(views),
        containers: [...document.querySelectorAll('[id^="view-"]')].map(node => node.id),
        buttons: [...document.querySelectorAll('button[data-view], button[data-project-view]')].map(node => node.dataset.view || node.dataset.projectView),
      };
    });
    const registered = new Set(structure.registered);
    const deadLinks = structure.buttons.filter(view => !registered.has(view));
    assert.deepEqual(deadLinks, [], "every declared navigation entry has a registered route");
    for (const view of structure.registered) {
      assert(structure.containers.includes(`view-${view}`), `registered route ${view} has a container`);
      await navigate(view);
      activated.push(view);
    }
    for (const entry of keyPaths) {
      assert(await page.evaluate(async project => (await import("/12-workbench.js")).openProjectSpace(project, "project"), project), "restore the project before following its resource entry");
      await settle();
      await navigate(entry.from || "project");
      if (entry.expand && await page.locator(entry.expand).getAttribute("open") === null) await page.locator(`${entry.expand} > summary`).click();
      if (entry.key) await page.keyboard.press(entry.key);
      if (entry.fill) await page.locator(entry.fill.selector).fill(entry.fill.value);
      await page.locator(entry.trigger).first().click();
      await active(entry.view);
      if (entry.assert) await page.locator(entry.assert).first().waitFor({ state: "visible" });
      entries.push({ name: entry.name, trigger: entry.trigger, view: entry.view });
    }
    // Unregistered legacy DOM is not a live route. It must remain inaccessible;
    // passive workspace updates must not eagerly load the retired research page.
    const unregistered = structure.containers.map(id => id.slice(5)).filter(view => !registered.has(view));
    await navigate("project");
    for (const view of unregistered) {
      await page.evaluate(async view => (await import("/03-shell.js")).navigate_view(view), view);
      assert.equal(await page.locator(`#view-${view}`).evaluate(node => node.classList.contains("active")), false, `unregistered ${view} cannot become active`);
    }
    const research = await page.evaluate(async () => {
      const workspaces = await import("/03-workspaces.js");
      const boundary = await import("/03-research-view.js");
      boundary.reset_research_project();
      boundary.sync_research_process_context(null);
      return {
        allowed: workspaces.view_allowed("research"),
        loaded: performance.getEntriesByType("resource").some(entry => /\/19-research(?:\.js|-)/.test(entry.name)),
        loader: typeof boundary.loadResearchView,
      };
    });
    assert.deepEqual(research, { allowed: false, loaded: false, loader: "function" }, "retired research is lazy, denied as a route, and passive changes do not initialize it");
    assert.deepEqual(errors, [], "route and entry transitions have no browser errors");
    return { registered: structure.registered, activated, entries, unregistered, research, deadLinks: [], islands: [], buttons: structure.buttons.length, containers: structure.containers.length };
  } finally {
    await browser.close();
    await server.close();
  }
}
