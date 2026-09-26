// UI-0926 #9 弹层技术栈的静态门禁(由 ui-a11y-smoke.mjs 调用;设计见 docs/design/ui_surface_stack.md §7.2)。
//
// 为什么要有它:弹层「加一个坏一个」的根因之一是门禁只认 hex(D-380 旧判据放过了 25 处 rgba),
// 圆角、阴影、层级、新造的 fixed 浮层、拿 <details> 当下拉都没人管。这里把「唯一写法」变成机械判据:
//   C1 字面量色      主题 token 块之外不得出现 hex / 颜色函数(rgba、hsla、oklch…)/ 颜色名
//   T1 token 分层    组件层 --surface-* 只准 surface.css 用、不在亮色块重定义、引用的 token 必须有定义
//   S1 外观归属      弹层外观(底色/边框/圆角/阴影/层级/定位)只写在 surface.css
//   H  页面结构      role=dialog/menu/tooltip 的宿主必须是 <dialog> 或 popover,弹层必带 .k-surface;
//                    <dialog> 里的 data-kz-menu 触发器,弹层必须写在同一个 dialog 内(模态外的节点是惰性的)
//   J  脚本          只有 00-surface.js 能切换弹层的 .hidden(含先取进局部变量再切的写法);不写字面量颜色;
//                    00-surface.js / 00-frame.js 零 import;拖动/调尺寸(setPointerCapture、--kz-frame-*、
//                    data-kz-placed)只准 00-frame.js(22-oc-* 豁免)
//   可调框(UI2-0926 #4,§4.6):data-kz-frame 只准挂在 <dialog class="k-dialog">(命令面板除外)与 .k-card 上,
//                    id 唯一、edges 取值合法;style.css 不得按 [data-kz-placed]/[data-kz-frame] 选择或引用 --kz-frame-*
//   P  预览遮挡(UI2-0926 #8,原生子 webview 永远画在 HTML 之上):surface.css 里常驻浮层宿主 .k-card /
//                    .k-chip-float / .k-toast-region 的 inset 必须引用 --surface-safe-right(面板打开时让开它);
//                    #preview-host 在 index.html 里必须是空元素、style.css 不得给它背景(它只是原生面板的占位框);
//                    #preview-dock 里不得出现常驻浮层(.k-card/.k-chip-float/.k-toast-region/.k-panel/.k-scrim)
// 每条违例都给出「文件:行、原文、改用什么」,报错写全判据,保证门禁可以被满足。
//
// 用法:checkSurfaceRules({ css, surfaceCss, pwaCss, html, sources: [{ name, text }] }) → 违例数组;
//       selfTestSurfaceRules() → 没能命中各自反例的规则名数组(应为空,防判据恒绿)。

export const THEME_END_MARK = "/* ===== 主题 token 块结束";

const HEX = /#[0-9a-fA-F]{3,8}\b/;
// color-mix( 不会命中:它后面跟的是 "-mix(" 而不是 "("。
const COLOR_FN = /\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/i;
const NAMED = /(?<![\w-])(?:white|black|red|green|blue|gray|grey|silver|orange|yellow|purple|pink|brown|navy|teal|maroon|olive|lime|aqua|fuchsia)(?![\w-])/i;

const SURFACE_SEL = /(?:^|[\s,>+~(])(?:dialog(?![\w-])|::backdrop|::picker\(|:popover-open|\[popover|\.k-[a-z][\w-]*|option(?![\w-]))/;
const LEGACY = /^(?:#(?:ask-overlay|ask-dialog|ask-reopen|viewer-overlay|viewer-dialog|confirm-overlay|confirm-dialog|input-overlay|input-dialog|palette|toast|sop-picker-panel|context-detail|file-suggestions|composer-more-menu|task-options-menu|autorun-menu|voice-settings-panel|kz-tip|kz-surface-root)|\.(?:palette|palette-box|composer-menu-panel|task-options-panel|autorun-menu|context-detail|file-suggestions|sop-picker-panel|voice-settings-panel))(?![\w-])/;
const CHROME = /(?:^|[;{\s])(?:background(?:-color)?|box-shadow|border(?:-radius|-color)?|backdrop-filter|z-index|position|inset)\s*:/;
const PANEL = /^#tasks-panel(?![\w-])/;
const PANEL_CHROME = /(?:^|[;{\s])(?:background(?:-color)?|box-shadow|border(?:-radius|-color)?)\s*:/;
const FIXED = /position:\s*fixed/;
const HIGH_Z = /z-index:\s*var\(--z-(?:float|overlay|dialog|toast)\)/;
const BIG_SHADOW = /box-shadow:\s*(?!none\b|inset\b|var\()[^;]*?(?<![\d.])(?:1[6-9]|[2-9]\d|\d{3,})px/;
const BIG_SHADOW_OK = new Set(["#composer", "#composer:focus-within", "#sidebar:not(.collapsed)"]);
const CHROME_IMPORTANT = /(?:background|box-shadow|border(?:-radius|-color)?)\s*:[^;]*!important/;
const DETAILS_POSITION = /position:\s*(?:absolute|fixed)/;

const SURFACE_IDS = "ask-overlay|ask-reopen|viewer-overlay|confirm-overlay|input-overlay|palette|toast|sop-picker-panel|context-detail|file-suggestions";
const J1 = new RegExp(`\\$\\(\\s*["'](?:${SURFACE_IDS})["']\\s*\\)\\.classList\\.(?:add|remove|toggle)\\(\\s*["']hidden["']`);
// J1 的局部变量形态:`const detail = $("context-detail"); … detail.classList.remove("hidden")`。
// 只在同一个顶层函数体里配对(取值行到下一个行首 `}` 为止),同名变量在别的函数里指向别的元素不算。
const J1_BIND = new RegExp(`\\b(?:const|let|var)\\s+([A-Za-z_$][\\w$]*)\\s*=\\s*(?:\\$|document\\.getElementById)\\(\\s*["'](?:${SURFACE_IDS})["']\\s*\\)`, "g");
const J2 = /\.style\.(?:background|backgroundColor|color|borderColor|boxShadow|outlineColor)\s*=\s*["'`](?!var\(|transparent|currentColor|inherit|["'`])/;
const J3 = /^\s*import\b/m;
const ZERO_IMPORT = new Set(["00-surface.js", "00-frame.js"]);
// J4:几何手势只有一个入口。22-oc-* 是角色工作室(自带画布拖动),不在弹层体系里。
const J4 = /setPointerCapture\(|--kz-frame-|data-kz-placed/;
// 可调框:落位只在 surface.css §10;运行时写入的 6 个变量不要求在 CSS 里有定义(T1 豁免)。
const FRAME_SEL = /\[data-kz-(?:placed|frame)/;
const FRAME_VAR = /var\(\s*--kz-frame-/;
const FRAME_RUNTIME_TOKEN = /^--kz-frame-[lrtbwh]$/;
const FRAME_EDGES = new Set(["n", "e", "s", "w", "ne", "se", "sw", "nw"]);

// 注释换成等长空白(保留换行),行号不漂移。
function stripComments(text) {
  return String(text).replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "));
}
function lineAt(text, index) {
  let line = 1;
  for (let i = 0; i < index && i < text.length; i += 1) if (text.charCodeAt(i) === 10) line += 1;
  return line;
}
function lineText(text, line) {
  return (String(text).split("\n")[line - 1] ?? "").trim();
}
// 逗号分组:括号里的逗号(:where(a, b)、:not(a, b))不算分组。
function splitSelector(selector) {
  const parts = [];
  let depth = 0;
  let current = "";
  for (const ch of selector) {
    if (ch === "(") depth += 1;
    else if (ch === ")") depth = Math.max(0, depth - 1);
    if (ch === "," && depth === 0) {
      parts.push(current);
      current = "";
    } else current += ch;
  }
  if (current.trim()) parts.push(current);
  return parts.map((part) => part.trim()).filter(Boolean);
}
// 每个逗号分支的「主体」= 最后一个复合选择器(括号内的空格不拆)。
function subjectOf(part) {
  const tokens = [];
  let depth = 0;
  let current = "";
  for (const ch of part) {
    if (ch === "(") depth += 1;
    else if (ch === ")") depth = Math.max(0, depth - 1);
    if (depth === 0 && /[\s>+~]/.test(ch)) {
      if (current) tokens.push(current);
      current = "";
    } else current += ch;
  }
  if (current) tokens.push(current);
  return tokens.at(-1) ?? "";
}
// 最内层规则:{ selector, body, index(规则起点), bodyIndex }。
function innerRules(cssNoComments) {
  const rules = [];
  const re = /([^{}]+)\{([^{}]*)\}/g;
  for (let m = re.exec(cssNoComments); m; m = re.exec(cssNoComments)) {
    const selector = m[1].trim();
    if (!selector || selector.startsWith("@")) continue;
    rules.push({ selector, body: m[2], index: m.index + m[0].indexOf(m[1].trim()), bodyIndex: m.index + m[1].length + 1 });
  }
  return rules;
}
function declarations(rule) {
  const out = [];
  const re = /([\w-]+)\s*:\s*([^;]+)/g;
  for (let m = re.exec(rule.body); m; m = re.exec(rule.body)) {
    out.push({ prop: m[1].toLowerCase(), value: m[2], index: rule.bodyIndex + m.index });
  }
  return out;
}

function checkLiteralColors(file, text, startIndex, violations) {
  const clean = stripComments(text);
  for (const rule of innerRules(clean)) {
    if (rule.index < startIndex) continue;
    for (const decl of declarations(rule)) {
      if (decl.prop.endsWith("mask-image")) continue; // 遮罩只取 alpha,不是主题的一部分
      const value = decl.value.replace(/"[^"]*"|'[^']*'/g, "");
      const hit = HEX.test(value) ? "hex" : COLOR_FN.test(value) ? "颜色函数" : NAMED.test(value) ? "颜色名" : "";
      if (!hit) continue;
      const line = lineAt(clean, decl.index);
      violations.push({
        rule: "C1",
        file,
        line,
        text: lineText(text, line),
        fix: `主题 token 块之外不得出现字面量颜色(${hit})。改法:引用语义 token var(--x);半透明写 color-mix(in srgb, var(--token) N%, transparent);确需新颜色时在 :root 与 [data-theme="light"] 两组各加一个 token。判据:hex /#[0-9a-f]{3,8}/、rgba/hsla/hwb/lab/lch/oklab/oklch/color(、CSS 颜色名;mask-image 豁免。`,
      });
    }
  }
}

function componentTokens(css) {
  const root = css.match(/:root\s*\{([\s\S]*?)\n\}/)?.[1] ?? "";
  const start = root.indexOf("/* 组件层");
  if (start < 0) return null;
  const afterComment = root.indexOf("*/", start) + 2;
  const next = root.indexOf("/*", afterComment);
  const block = root.slice(afterComment, next < 0 ? undefined : next);
  return new Set([...block.matchAll(/(--surface-[a-z0-9-]+)\s*:/g)].map((m) => m[1]));
}

function checkTokens(css, surfaceCss, violations) {
  const components = componentTokens(css);
  if (!components || !components.size) {
    violations.push({ rule: "T1", file: "style.css", line: 1, text: ":root", fix: "找不到 :root 里「/* 组件层」注释开头的组件 token 块(--surface-*),判据无法定位。组件层写在语义层之后,以「/* 组件层」注释开头。" });
    return;
  }
  const themeEnd = css.indexOf(THEME_END_MARK);
  const cleanCss = stripComments(css);
  for (const m of cleanCss.slice(themeEnd).matchAll(/var\((--surface-[a-z0-9-]+)\)/g)) {
    if (!components.has(m[1])) continue;
    const line = lineAt(cleanCss, themeEnd + m.index);
    violations.push({ rule: "T1", file: "style.css", line, text: lineText(css, line), fix: `组件 token ${m[1]} 只准 surface.css 使用。视图要弹层外观就给元素加 .k-surface 等类,不要在 style.css 里引用组件层。` });
  }
  const light = cleanCss.match(/\[data-theme="light"\]\s*\{([^}]*)\}/);
  if (light) {
    for (const m of light[1].matchAll(/(--surface-[a-z0-9-]+)\s*:/g)) {
      if (!components.has(m[1])) continue;
      const line = lineAt(cleanCss, light.index + light[0].indexOf(light[1]) + m.index);
      violations.push({ rule: "T1", file: "style.css", line, text: lineText(css, line), fix: `组件 token ${m[1]} 不得在 [data-theme="light"] 里重定义:主题切换只发生在语义层,改它引用的语义 token。` });
    }
  }
  const defined = (name) => new RegExp(`${name.replace(/[-]/g, "\\-")}\\s*:`).test(cleanCss) || new RegExp(`${name.replace(/[-]/g, "\\-")}\\s*:`).test(stripComments(surfaceCss));
  const cleanSurface = stripComments(surfaceCss);
  for (const m of cleanSurface.matchAll(/var\((--[a-z0-9-]+)/g)) {
    if (defined(m[1]) || FRAME_RUNTIME_TOKEN.test(m[1])) continue;
    const line = lineAt(cleanSurface, m.index);
    violations.push({ rule: "T1", file: "surface.css", line, text: lineText(surfaceCss, line), fix: `surface.css 引用了未定义的 token ${m[1]}(style.css 与 surface.css 都没有 "${m[1]}:" 定义),会静默取到 initial。` });
  }
  for (const m of cleanCss.matchAll(/(--c-[a-z0-9-]+)\s*:/g)) {
    const line = lineAt(cleanCss, m.index);
    violations.push({ rule: "T1", file: "style.css", line, text: lineText(css, line), fix: "本批不引入原始层 --c-*:hex 只写在两块主题 token 里(语义层),组件层引用语义层。" });
  }
}

function detailsHooks(html) {
  const hooks = new Set();
  for (const m of String(html).matchAll(/<details\b([^>]*)>/g)) {
    const id = m[1].match(/\bid="([^"]+)"/)?.[1];
    if (id) hooks.add(`#${id}`);
    for (const cls of (m[1].match(/\bclass="([^"]+)"/)?.[1] ?? "").split(/\s+/).filter(Boolean)) hooks.add(`.${cls}`);
  }
  return hooks;
}

function checkOwnership(css, html, violations) {
  const themeEnd = css.indexOf(THEME_END_MARK);
  const clean = stripComments(css);
  const details = detailsHooks(html);
  for (const rule of innerRules(clean)) {
    if (rule.index < themeEnd) continue;
    const line = lineAt(clean, rule.index);
    const where = { file: "style.css", line, text: `${rule.selector.replace(/\s+/g, " ").slice(0, 120)} { … }` };
    const decls = rule.body;
    if (SURFACE_SEL.test(rule.selector)) {
      violations.push({ rule: "S1", ...where, fix: "弹层外观(dialog/::backdrop/::picker(select)/:popover-open/[popover]/.k-*/option)只写在 surface.css;视图 CSS 只写尺寸与内容排版。" });
    }
    if (FRAME_SEL.test(rule.selector) || FRAME_VAR.test(decls)) {
      violations.push({ rule: "S1", ...where, fix: "可调框的落位只写在 surface.css §10:00-frame.js 写 --kz-frame-* 变量与 data-kz-placed 令牌;style.css 不得按 [data-kz-placed]/[data-kz-frame] 选择,也不得引用 --kz-frame-*(视图 CSS 只写默认尺寸)。" });
    }
    const subjects = splitSelector(rule.selector).map(subjectOf);
    for (const subject of subjects) {
      if (LEGACY.test(subject) && CHROME.test(decls)) {
        violations.push({ rule: "S1", ...where, fix: `${subject} 是弹层宿主:底色/边框/圆角/阴影/backdrop-filter/z-index/position/inset 归 surface.css(.k-surface/.k-dialog/.k-menu/.k-popover/.k-card),这里只留宽度与内容排版。` });
      }
      if (PANEL.test(subject) && PANEL_CHROME.test(decls)) {
        violations.push({ rule: "S1", ...where, fix: `${subject} 的外观走 .k-surface.k-panel(surface.css);style.css 只保留 position/尺寸/z-index。` });
      }
      if (FIXED.test(decls) && !/^\.resize-handle(?![\w-])/.test(subject)) {
        violations.push({ rule: "S1", ...where, fix: "不要新造 position:fixed 浮层:菜单/浮层用 openMenu/openPopover 或 data-kz-menu,卡片用 showCard,模态用 <dialog>(顶层元素不需要 fixed + z-index)。白名单只有 .resize-handle。" });
      }
      if (BIG_SHADOW.test(decls) && !BIG_SHADOW_OK.has(subject)) {
        violations.push({ rule: "S1", ...where, fix: "大阴影(模糊 ≥16px)只属于弹层:用 surface.css 的 --surface-shadow-*;白名单 #composer、#composer:focus-within、#sidebar:not(.collapsed)。" });
      }
      const bare = subject.replace(/\[open\]/g, "").replace(/::?[\w-]+(?:\([^)]*\))?/g, "");
      if (details.has(bare) && DETAILS_POSITION.test(decls)) {
        violations.push({ rule: "S1", ...where, fix: `${bare} 是 <details>:不要拿 details 做浮出的下拉/菜单(position:absolute/fixed)。改成 <button data-kz-menu="…"> + popover 弹层菜单。` });
      }
    }
    if (HIGH_Z.test(decls)) {
      violations.push({ rule: "S1", ...where, fix: "--z-float/--z-overlay/--z-dialog/--z-toast 已随弹层进顶层而删除:顶层元素不需要 z-index。" });
    }
    if (CHROME_IMPORTANT.test(decls)) {
      violations.push({ rule: "S1", ...where, fix: "外观属性(background/box-shadow/border*)上不得写 !important:!important 会反转层序,弹层外观就压不住了。" });
    }
  }
}

function checkHtml(html, violations) {
  const text = String(html);
  const at = (index) => {
    const line = lineAt(text, index);
    return { file: "index.html", line, text: lineText(text, line).slice(0, 160) };
  };
  for (const m of text.matchAll(/<(\w+)\b([^>]*)\brole="(dialog|alertdialog|menu|tooltip)"([^>]*)>/g)) {
    const attrs = `${m[2]} ${m[4]}`;
    const id = attrs.match(/\bid="([^"]+)"/)?.[1];
    if (m[1].toLowerCase() === "dialog" || /\spopover(?:[\s=>]|$)/.test(` ${attrs}`)) continue;
    violations.push({ rule: "H", ...at(m.index), fix: `role="${m[3]}" 的宿主必须是 <dialog>(模态)或带 popover 属性的弹层(由 00-surface 开关);常驻侧栏 #tasks-panel 是 <aside> 地标(${id ? `#${id}` : "此处"}不写 role=dialog)。` });
  }
  for (const m of text.matchAll(/<dialog\b[^>]*>|<[a-z][\w-]*\s[^>]*\bpopover\b[^>]*>/g)) {
    if (/class="[^"]*\bk-surface\b/.test(m[0])) continue;
    violations.push({ rule: "H", ...at(m.index), fix: "每个 <dialog> 与 [popover] 都必须带 class=\"k-surface …\"(外观唯一真源在 surface.css)。" });
  }
  for (const m of text.matchAll(/<details\b[^>]*\bid="(?:composer-more|task-options|autorun-more)"|<details\b[^>]*class="[^"]*\bvoice-settings\b/g)) {
    violations.push({ rule: "H", ...at(m.index), fix: "输入区的菜单不得再用 <details>:改成 <button data-kz-menu=\"x-menu\"> + <div id=\"x-menu\" popover class=\"k-surface k-menu\">。" });
  }
  // 模态开着时 dialog 子树之外的一切都是惰性的(含之后才弹出的顶层 popover):dialog 里的菜单触发器,
  // 它的弹层必须写在同一个 dialog 里,否则菜单弹得出来却点不动、拿不到焦点。
  for (const m of text.matchAll(/<dialog\b[\s\S]*?<\/dialog>/g)) {
    for (const trigger of m[0].matchAll(/\bdata-kz-menu="([^"]+)"/g)) {
      const id = trigger[1].replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      if (new RegExp(`\\bid="${id}"`).test(m[0])) continue;
      violations.push({ rule: "H", ...at(m.index + trigger.index), fix: `<dialog> 里的触发器 data-kz-menu="${trigger[1]}" 对应的弹层必须写在同一个 <dialog> 内:模态开着时 dialog 之外的节点是惰性的,菜单点不动。JS 菜单用 openMenu(自动挂进锚点所在的 dialog)。` });
    }
  }
  // 可调框(00-frame.js):宿主只能是模态弹窗(命令面板除外)与停靠卡片;菜单/浮层/提示/toast/芯片跟着锚点走,不是框。
  const frameIds = new Set();
  for (const m of text.matchAll(/<(\w+)\b([^>]*\bdata-kz-frame[\w-]*=[^>]*)>/g)) {
    const attrs = m[2];
    const id = attrs.match(/\bdata-kz-frame="([^"]*)"/)?.[1];
    const cls = attrs.match(/\bclass="([^"]*)"/)?.[1] ?? "";
    const where = at(m.index);
    if (id === undefined) {
      violations.push({ rule: "H", ...where, fix: "有 data-kz-frame-* 属性就必须有 data-kz-frame=\"<id>\"(持久化键与接线都靠它)。" });
      continue;
    }
    const isDialog = m[1].toLowerCase() === "dialog" && /\bk-dialog\b/.test(cls) && !/\bdata-size="palette"/.test(attrs);
    const isCard = /\bk-card\b/.test(cls);
    if (!isDialog && !isCard) {
      violations.push({ rule: "H", ...where, fix: `data-kz-frame="${id}" 只准挂在 <dialog class="k-surface k-dialog">(命令面板除外)或 .k-card 上;菜单/浮层/提示/toast/芯片按锚点定位,不做成可拖的框。` });
    }
    if (!id || frameIds.has(id)) {
      violations.push({ rule: "H", ...where, fix: `data-kz-frame 的 id 必须非空且唯一(重复的 "${id}" 会共用同一份几何偏好)。` });
    }
    frameIds.add(id);
    const edges = attrs.match(/\bdata-kz-frame-edges="([^"]*)"/)?.[1];
    if (edges !== undefined && edges !== "all" && !edges.trim().split(/\s+/).every((edge) => FRAME_EDGES.has(edge))) {
      violations.push({ rule: "H", ...where, fix: `data-kz-frame-edges="${edges}" 取值非法:只能是 all 或空格分隔的 n e s w ne se sw nw。` });
    }
  }
  for (const m of text.matchAll(/<select\b[^>]*\b(?:multiple|size=)/g)) {
    violations.push({ rule: "H", ...at(m.index), fix: "select 不带 multiple/size(列表框模式的外观规则不同,base-select 不覆盖);要多选就换成勾选框组。" });
  }
  for (const m of text.matchAll(/\sstyle="[^"]*(?:background|color|box-shadow|border)[^"]*"/g)) {
    violations.push({ rule: "H", ...at(m.index), fix: "不写内联颜色/边框样式:用类名 + token。" });
  }
}

function checkScripts(sources, violations) {
  for (const { name, text } of sources) {
    const lines = String(text).split(/\r?\n/);
    const isSurface = name === "00-surface.js";
    const geometryOk = name === "00-frame.js" || /^22-oc-/.test(name) || !/^\d/.test(name);
    lines.forEach((raw, index) => {
      const line = raw.replace(/\/\/.*$/, "");
      if (!isSurface && J1.test(line)) {
        violations.push({ rule: "J1", file: name, line: index + 1, text: raw.trim(), fix: "除 00-surface.js 外不得直接切换弹层的 .hidden:用 openDialog/closeSurface、openPopover、showCard/hideCard、toast(模块会镜像 .hidden)。" });
      }
      if (/^\d/.test(name) && !/^22-(?:neural-flow|oc-)/.test(name) && J2.test(line)) {
        violations.push({ rule: "J2", file: name, line: index + 1, text: raw.trim(), fix: "脚本里不写字面量颜色:写 var(--token) 或切换类名。" });
      }
      if (!geometryOk && J4.test(line)) {
        violations.push({ rule: "J4", file: name, line: index + 1, text: raw.trim(), fix: "拖动/调尺寸只有一个入口:弹窗与卡片在 index.html 写 data-kz-frame*,布局两栏用 00-frame.js 的 installSplit;不要自己 setPointerCapture 或写 --kz-frame-*/data-kz-placed。" });
      }
    });
    if (!isSurface) {
      const body = lines.map((raw) => raw.replace(/\/\/.*$/, ""));
      for (let start = 0; start < body.length; start += 1) {
        for (const m of body[start].matchAll(J1_BIND)) {
          const toggle = new RegExp(`(?<![\\w$.])${m[1].replace(/\$/g, "\\$")}\\.classList\\.(?:add|remove|toggle)\\(\\s*["']hidden["']`);
          for (let i = start; i < body.length; i += 1) {
            if (i > start && /^\}/.test(body[i])) break;
            if (!toggle.test(body[i])) continue;
            violations.push({ rule: "J1", file: name, line: i + 1, text: lines[i].trim(), fix: `${m[1]} 取的是弹层宿主(第 ${start + 1} 行):除 00-surface.js 外不得直接切换弹层的 .hidden,用 openDialog/closeSurface、openPopover、showCard/hideCard、toast(模块会镜像 .hidden)。` });
          }
        }
      }
    }
    if (ZERO_IMPORT.has(name) && J3.test(text)) {
      const line = lines.findIndex((l) => /^\s*import\b/.test(l)) + 1;
      violations.push({ rule: "J3", file: name, line, text: lines[line - 1]?.trim() ?? "", fix: `${name} 必须零 import(样例页与假 DOM 冒烟要能单独加载;翻译经 setSurfaceTranslator 注入,几何存储经 setFrameStore 注入)。` });
    }
  }
}

// ── 分区:网页预览前端 ──
// P 组:原生子 webview(网页预览)永远画在 HTML 之上,浮层只有两条活路——让开它(--surface-safe-right),
// 或者压上去时由 24-preview.js 冻结(截图替身 + 隐藏原生面板)。这里把「让开」与「占位框干净」变成机械判据。
const SAFE_RIGHT_HOSTS = [".k-card", ".k-chip-float", ".k-toast-region"];
const DOCK_PERSISTENT = /\bk-(?:card|chip-float|toast-region|panel|scrim)\b/;
function checkPreviewOcclusion(css, surfaceCss, html, violations) {
  if (surfaceCss) {
    const clean = stripComments(surfaceCss);
    for (const host of SAFE_RIGHT_HOSTS) {
      const rules = innerRules(clean).filter((rule) => splitSelector(rule.selector).includes(host));
      const insets = rules.flatMap((rule) => declarations(rule).filter((d) => d.prop === "inset").map((d) => ({ rule, d })));
      const ok = insets.length > 0 && insets.every(({ d }) => /var\(\s*--surface-safe-right\b/.test(d.value));
      if (ok) continue;
      const at = insets.find(({ d }) => !/var\(\s*--surface-safe-right\b/.test(d.value))?.d ?? null;
      const line = at ? lineAt(clean, at.index) : (rules[0] ? lineAt(clean, rules[0].index) : 1);
      violations.push({
        rule: "P",
        file: "surface.css",
        line,
        text: lineText(surfaceCss, line) || host,
        fix: `${host} 是常驻浮层宿主:它的 inset 右值必须加上 var(--surface-safe-right, 0px)(卡片/芯片写 calc(22px + max(var(--kz-dock-right, 0px), var(--surface-safe-right, 0px))),toast 区域右值写 var(--surface-safe-right, 0px))。网页预览面板是原生子窗口、永远画在 HTML 之上,不让开就被它盖住。判据:surface.css 里选择器分支等于 ${host} 的规则至少一条声明 inset,且每条 inset 都引用 --surface-safe-right。`,
      });
    }
  }
  const text = String(html);
  const host = text.match(/<(\w+)\b[^>]*\bid="preview-host"[^>]*>/);
  if (host) {
    const after = text.slice(host.index + host[0].length);
    const closing = `</${host[1]}>`;
    const selfClosed = /\/>$/.test(host[0]);
    if (!selfClosed && !after.startsWith(closing)) {
      const line = lineAt(text, host.index);
      violations.push({ rule: "P", file: "index.html", line, text: lineText(text, line).slice(0, 160), fix: `#preview-host 必须是空元素(开标签后紧跟 ${closing}):它只是原生预览面板的占位框,里面的任何内容都被原生窗口盖住;冻结截图、空态、错误页写成它的兄弟节点(#preview-freeze / #preview-empty / #preview-error)。` });
    }
    const dock = text.match(/<section\b[^>]*\bid="preview-dock"[^>]*>[\s\S]*?<\/section>/);
    if (dock) {
      for (const m of dock[0].matchAll(/\bclass="([^"]*)"/g)) {
        if (!DOCK_PERSISTENT.test(m[1])) continue;
        const line = lineAt(text, dock.index + m.index);
        violations.push({ rule: "P", file: "index.html", line, text: lineText(text, line).slice(0, 160), fix: "#preview-dock 里不得放常驻浮层(.k-card/.k-chip-float/.k-toast-region/.k-panel/.k-scrim):它们压在原生预览面板上会被盖住。面板里的菜单用 openMenu 现造(打开时面板自动冻结),提示走 toast。" });
      }
    }
  }
  const clean = stripComments(css);
  for (const rule of innerRules(clean)) {
    if (!splitSelector(rule.selector).some((branch) => /#preview-host(?![\w-])/.test(subjectOf(branch)))) continue;
    for (const decl of declarations(rule)) {
      if (!/^background(?:-color|-image)?$/.test(decl.prop) || /^(?:none|transparent)\s*$/.test(decl.value.trim())) continue;
      const line = lineAt(clean, decl.index);
      violations.push({ rule: "P", file: "style.css", line, text: lineText(css, line), fix: "#preview-host 不得有背景:它被原生预览面板完全盖住,背景只会在面板改尺寸/冻结的一帧里闪出来。舞台底色写在 #preview-stage 上。" });
    }
  }
}
// ── 分区:网页预览前端(完) ──

export function checkSurfaceRules({ css = "", surfaceCss = "", pwaCss = "", html = "", sources = [] } = {}) {
  const violations = [];
  const themeEnd = css.indexOf(THEME_END_MARK);
  if (themeEnd < 0) {
    violations.push({ rule: "C1", file: "style.css", line: 1, text: "", fix: `找不到主题 token 块结束标记「${THEME_END_MARK}」,判据无法定位。` });
  } else {
    checkLiteralColors("style.css", css, themeEnd, violations);
  }
  if (surfaceCss) checkLiteralColors("surface.css", surfaceCss, 0, violations);
  // 移动端 PWA 本版配色不改(用户待决策):只在它有了自己的 token 块之后才查块外。
  const pwaEnd = pwaCss.indexOf(THEME_END_MARK);
  if (pwaEnd >= 0) checkLiteralColors("mobile-pwa/style.css", pwaCss, pwaEnd, violations);
  checkTokens(css, surfaceCss, violations);
  checkOwnership(css, html, violations);
  checkHtml(html, violations);
  checkScripts(sources, violations);
  checkPreviewOcclusion(css, surfaceCss, html, violations);
  return violations;
}

// 同一条改法只说一遍,下面列出全部命中位置。
export function formatViolations(violations) {
  const groups = new Map();
  for (const v of violations) {
    const key = `[${v.rule}] ${v.fix}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(`  ${v.file}:${v.line}  ${v.text}`);
  }
  return [...groups].map(([fix, where]) => `${fix}\n${where.join("\n")}`).join("\n");
}

// 反例自测:每条规则喂一条必须命中的样本。某条规则没命中 = 判据已恒绿,门禁比没有还危险。
export function selfTestSurfaceRules() {
  const root = `:root {\n  --bg: #111; --panel: #222; --fg: #eee;\n  /* 组件层:弹层外观 */\n  --surface-bg: var(--panel);\n  /* 动效 */\n}\n[data-theme="light"] { --bg: #fff; --surface-bg: #fff; }\n${THEME_END_MARK} ===== */\n`;
  const samples = {
    "C1 rgba": { css: `${root}.x { background: rgba(0,0,0,.5); }` },
    "C1 hex": { css: `${root}.x { color: #abc; }` },
    "C1 颜色名": { css: `${root}.x { border-color: white; }` },
    "T1 视图引用组件层": { css: `${root}.x { background: var(--surface-bg); }` },
    "T1 亮色块重定义": { css: root },
    "T1 未定义 token": { css: root, surfaceCss: ".k-surface { color: var(--nope); }" },
    "S1 dialog 选择器": { css: `${root}dialog { padding: 0; }` },
    "S1 宿主外观": { css: `${root}#confirm-overlay { background: var(--panel); }` },
    "S1 侧栏外观": { css: `${root}#tasks-panel[data-dock="side"] { box-shadow: var(--elev-3); }` },
    "S1 fixed 浮层": { css: `${root}.floaty { position: fixed; }` },
    "S1 details 下拉": { css: `${root}.dd { position: absolute; }`, html: '<details class="dd"></details>' },
    "H popover 缺 k-surface": { html: '<div id="m" popover class="menu"></div>' },
    "H role=dialog 宿主": { html: '<div id="d" role="dialog"></div>' },
    "H dialog 内菜单写在外面": { html: '<dialog class="k-surface k-dialog"><button data-kz-menu="m">⋯</button></dialog><div id="m" popover class="k-surface k-menu"></div>' },
    "J1 直接切 hidden": { sources: [{ name: "07-events.js", text: '$("toast").classList.add("hidden");' }] },
    "J1 局部变量切 hidden": { sources: [{ name: "07-events.js", text: 'function f() {\n  const detail = $("context-detail");\n  detail.classList.remove("hidden");\n}' }] },
    "J3 surface import": { sources: [{ name: "00-surface.js", text: 'import { x } from "./01-core.js";' }] },
    "J3 frame import": { sources: [{ name: "00-frame.js", text: 'import { x } from "./01-core.js";' }] },
    "J4 自写拖动": { sources: [{ name: "07-events.js", text: "handle.setPointerCapture(event.pointerId);" }] },
    "J4 自写框变量": { sources: [{ name: "06-agent-panel.js", text: 'el.style.setProperty("--kz-frame-w", "400px");' }] },
    "S1 按摆放令牌选择": { css: `${root}#viewer-overlay[data-kz-placed~="w"] { width: 900px; }` },
    "S1 引用框变量": { css: `${root}.x { width: var(--kz-frame-w); }` },
    "H 框挂在菜单上": { html: '<div id="m" popover class="k-surface k-menu" data-kz-frame="m"></div>' },
    "H 框挂在命令面板上": { html: '<dialog class="k-surface k-dialog" data-size="palette" data-kz-frame="p"></dialog>' },
    "H 框 id 重复": { html: '<dialog class="k-surface k-dialog" data-kz-frame="a"></dialog><div class="k-surface k-card" popover data-kz-frame="a"></div>' },
    "H 框 edges 非法": { html: '<dialog class="k-surface k-dialog" data-kz-frame="a" data-kz-frame-edges="left"></dialog>' },
    "H 框缺 id": { html: '<dialog class="k-surface k-dialog" data-kz-frame-edges="all"></dialog>' },
    // ── 分区:网页预览前端 ──
    "P toast 不让开预览": { surfaceCss: ".k-card { inset: auto calc(22px + var(--surface-safe-right, 0px)) 18px auto; }\n.k-chip-float { inset: auto var(--surface-safe-right, 0px) 18px auto; }\n.k-toast-region { inset: auto 0 40px 0; }" },
    "P 占位框有子元素": { html: '<section id="preview-dock"><div id="preview-host"><span>x</span></div></section>' },
    "P 占位框有背景": { css: `${root}#preview-host { background: var(--panel); }` },
    "P 面板里有常驻浮层": { html: '<section id="preview-dock"><div id="preview-host"></div><div class="k-surface k-card" popover></div></section>' },
  };
  const expectRule = (label) => label.split(" ")[0];
  const silent = [];
  for (const [label, sample] of Object.entries(samples)) {
    const found = checkSurfaceRules({ css: root, ...sample }).filter((v) => v.rule === expectRule(label));
    // T1 亮色块样本本身就在 root 里;其余样本要求「比空白基线多出至少一条」。
    const baseline = checkSurfaceRules({ css: root }).filter((v) => v.rule === expectRule(label)).length;
    if (label === "T1 亮色块重定义" ? found.length === 0 : found.length <= baseline) silent.push(label);
  }
  return silent;
}
