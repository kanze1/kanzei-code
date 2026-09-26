// UI2-0926 #11 输入区(composer)控件几何的浏览器测量(docs/design/ui_surface_stack.md「输入区控件几何」),
// 由 ui-surface-gallery-smoke.mjs §6 调用。假 DOM 冒烟看不到盒子与像素,这里在无头 Edge 里真量:
//   ① #composer 里可见的 .kz-ctl 高 28±0.5,.kz-ctl--round(发送)高 32±0.5;
//   ② 页面里全部 select 的 align-items 计算值为 center(base-select 默认 normal,固定高度时文字贴顶——根因 A);
//   ③ 输入区里可见控件(.kz-ctl、发送、项目名、分支、停机原因)包围盒两两不交,且都在 #composer 内;
//   ④ 模式芯片截图找文字墨迹的纵向范围,中心偏离盒中线 ≤1.5 CSS px(用户截图 13 里「自主推进」贴在上半截);
//   ⑤ 输入区占满列宽(≥760)时工具行是单行(高 ≤32.5)。
// 每次运行都做自检:注入三种回归(select 不居中 / 某控件回到 30px / 模式芯片左移压住邻居),任一没被判红即报
// 「测量判据失效」。page.evaluate 回调在浏览器里执行,浏览器全局在这里声明给 ESLint。
/* global window, document, getComputedStyle, createImageBitmap, OffscreenCanvas, fetch */

export const COMPOSER_MUTATIONS = {
  selectAlign: "select { align-items: normal !important; }",
  ctlHeight: "#model-picker { height: 30px !important; }",
  overlap: "#profile-select { margin-left: -24px !important; }",
};
// 测量期间收起 toast:它浮在输入区上方,会盖住模式芯片、污染墨迹截图(只影响测量,不改应用)。
const QUIET = ".k-toast-region { visibility: hidden !important; }";

function geometryInPage() {
  const out = [];
  const composer = document.querySelector("#composer");
  if (!composer) return { failures: ["找不到 #composer"], info: {} };
  const C = composer.getBoundingClientRect();
  const shown = (el) => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== "hidden";
  };
  const name = (el) => el.id ? `#${el.id}` : `.${String(el.className).split(/\s+/).join(".")}`;
  for (const el of composer.querySelectorAll(".kz-ctl")) {
    if (!shown(el)) continue;
    const h = el.getBoundingClientRect().height;
    if (Math.abs(h - 28) > 0.5) out.push(`① ${name(el)} 高 ${h.toFixed(2)}px,.kz-ctl 应为 28px`);
  }
  for (const el of composer.querySelectorAll(".kz-ctl--round")) {
    if (!shown(el)) continue;
    const h = el.getBoundingClientRect().height;
    if (Math.abs(h - 32) > 0.5) out.push(`① ${name(el)} 高 ${h.toFixed(2)}px,发送键应为 32px`);
  }
  const bent = [...document.querySelectorAll("select")].filter((s) => getComputedStyle(s).alignItems !== "center").map((s) => `${s.id || s.className || "select"}=${getComputedStyle(s).alignItems}`);
  if (bent.length) out.push(`② ${bent.length} 个 select 的 align-items 不是 center(固定高度时文字贴顶):${bent.slice(0, 6).join(", ")}`);
  const items = [...composer.querySelectorAll(".kz-ctl, .kz-ctl--round, #project-label, #ctx-branch, #auto-status")]
    .filter((el) => shown(el) && !el.closest("[popover]"))
    .map((el) => [name(el), el.getBoundingClientRect()]);
  for (let i = 0; i < items.length; i += 1) {
    for (let j = i + 1; j < items.length; j += 1) {
      const [a, A] = items[i];
      const [b, B] = items[j];
      if (A.left < B.right - 0.5 && A.right > B.left + 0.5 && A.top < B.bottom - 0.5 && A.bottom > B.top + 0.5) out.push(`③ ${a} 与 ${b} 叠压`);
    }
  }
  for (const [label, box] of items) {
    if (box.left < C.left - 0.5 || box.right > C.right + 0.5 || box.top < C.top - 0.5 || box.bottom > C.bottom + 0.5) out.push(`③ ${label} 越出输入区`);
  }
  const bar = document.querySelector("#composer-bar").getBoundingClientRect();
  if (C.width >= 760 && bar.height > 32.5) out.push(`⑤ 输入区占满列宽(${C.width.toFixed(0)}px)时工具行应为单行,实际高 ${bar.height.toFixed(1)}px`);
  const project = document.querySelector("#project-label");
  const info = { composer: Math.round(C.width), bar: Math.round(bar.height), controls: items.length };
  if (project && project.textContent.length > 40) {
    info.projectTruncated = project.scrollWidth > project.clientWidth;
    if (!info.projectTruncated) out.push("③ 超长项目名没有截断(应省略号收进 40% 宽)");
  }
  return { failures: out, info };
}

/// 模式芯片墨迹中心相对盒中线的偏移(CSS px)。截图在页面里解码(createImageBitmap + OffscreenCanvas),不引依赖。
async function inkOffset(page) {
  const chip = await page.$("#profile-select");
  if (!chip) return null;
  const box = await chip.boundingBox();
  if (!box || !box.width) return null;
  const png = await chip.screenshot();
  return page.evaluate(async ({ b64, cssHeight }) => {
    const img = await createImageBitmap(await (await fetch(`data:image/png;base64,${b64}`)).blob());
    const canvas = new OffscreenCanvas(img.width, img.height);
    const ctx = canvas.getContext("2d");
    ctx.drawImage(img, 0, 0);
    const data = ctx.getImageData(0, 0, img.width, img.height).data;
    const px = (x, y) => { const i = (y * img.width + x) * 4; return [data[i], data[i + 1], data[i + 2]]; };
    const bg = px(Math.min(3, img.width - 1), Math.floor(img.height / 2));
    let top = -1;
    let bottom = -1;
    for (let y = 0; y < img.height; y += 1) {
      for (let x = 4; x < img.width - 4; x += 1) {
        const p = px(x, y);
        if (Math.abs(p[0] - bg[0]) + Math.abs(p[1] - bg[1]) + Math.abs(p[2] - bg[2]) > 90) {
          if (top < 0) top = y;
          bottom = y;
          break;
        }
      }
    }
    if (top < 0) return null;
    const scale = img.height / cssHeight;
    return ((top + bottom) / 2 - (img.height - 1) / 2) / scale;
  }, { b64: png.toString("base64"), cssHeight: box.height });
}

/// 在已就绪的页面上量一遍;mutate 为注入的回归样式(自检用)。
export async function measureComposer(page, { mutate = "" } = {}) {
  await page.addStyleTag({ content: `${QUIET}\n${mutate}` });
  await page.evaluate(() => document.activeElement?.blur?.());
  await page.waitForTimeout(120);
  const result = await page.evaluate(geometryInPage);
  const offset = await inkOffset(page);
  if (offset === null) result.failures.push("④ 模式芯片截图里找不到文字墨迹");
  else if (Math.abs(offset) > 1.5) result.failures.push(`④ 模式芯片文字偏离中线 ${offset.toFixed(2)}px(应 ≤1.5)`);
  result.info.ink = offset === null ? null : Math.round(offset * 100) / 100;
  return result;
}

const COMBOS = [
  { width: 1100, height: 800, dpr: 1, themes: ["dark"], states: ["running", "idle"] },
  { width: 1280, height: 860, dpr: 1.5, themes: ["dark", "light"], states: ["running"] },
  { width: 1600, height: 900, dpr: 1.25, themes: ["dark", "light"], states: ["running", "idle"], longName: true },
  { width: 2000, height: 1000, dpr: 1, themes: ["dark"], states: ["running"] },
];

async function openApp(browser, origin, { width, height, dpr, theme, state }) {
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: dpr, colorScheme: theme });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  // 运行态 = composer 场景(一轮在跑、自主推进、鞭挞开着);空闲态 = empty 场景(新对话欢迎页,结伴开发)。
  await page.goto(`${origin}/?theme=${theme}&scene=${state === "running" ? "composer" : "empty"}`, { waitUntil: "load" });
  await page.waitForFunction(() => window.__kzPreview?.ready === true, null, { timeout: 25000 });
  await page.waitForTimeout(200);
  return { context, page, errors };
}

/// §6 入口:逐视口/主题/状态量一遍,最后跑自检。返回 { failures, notes }。
export async function runComposerGeometry(browser, origin) {
  const failures = [];
  const notes = [];
  let measured = 0;
  for (const combo of COMBOS) {
    for (const theme of combo.themes) {
      for (const state of combo.states) {
        const tag = `${combo.width}@${combo.dpr} ${theme} ${state}`;
        const { context, page, errors } = await openApp(browser, origin, { ...combo, theme, state });
        const result = await measureComposer(page);
        for (const failure of result.failures) failures.push(`输入区几何 ${tag}:${failure}`);
        for (const error of errors) failures.push(`输入区几何 ${tag}:${error}`);
        measured += 1;
        if (combo.longName && state === "running" && theme === "dark") {
          await page.evaluate(() => { document.querySelector("#project-label").textContent = "一个非常非常长的项目名称用来测试截断效果-Akashic-AgentOS-workspace-二期产品设计与前端开发"; });
          const long = await measureComposer(page);
          for (const failure of long.failures) failures.push(`输入区几何 ${tag} 超长项目名:${failure}`);
          measured += 1;
        }
        await context.close();
      }
    }
  }
  // 自检:三种回归都必须被判红。
  const silent = [];
  for (const [id, css] of Object.entries(COMPOSER_MUTATIONS)) {
    const { context, page } = await openApp(browser, origin, { width: 1600, height: 900, dpr: 1.25, theme: "dark", state: "running" });
    const result = await measureComposer(page, { mutate: css });
    await context.close();
    if (!result.failures.length) silent.push(id);
  }
  if (silent.length) failures.push(`输入区几何测量判据失效:注入回归 ${silent.join(", ")} 后仍全绿`);
  notes.push(`输入区几何 ${measured} 组(控件等高、select 居中、无叠压、墨迹居中、满列单行),3 种注入回归均被判红`);
  return { failures, notes };
}
