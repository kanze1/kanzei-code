// Native ESM correctness checks. No bundler or formatting dependency is needed.
import globals from "globals";
import js from "@eslint/js";

export default [
  {
    files: ["crates/kanzei-app/ui/*.js"],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "module",
      globals: {
        // Tauri 注入的宿主对象
        window: "readonly",
        document: "readonly",
        navigator: "readonly",
        location: "readonly",
        history: "readonly",
        localStorage: "readonly",
        sessionStorage: "readonly",
        console: "readonly",
        alert: "readonly",
        fetch: "readonly",
        setTimeout: "readonly",
        clearTimeout: "readonly",
        setInterval: "readonly",
        clearInterval: "readonly",
        requestAnimationFrame: "readonly",
        cancelAnimationFrame: "readonly",
        EventSource: "readonly",
        MutationObserver: "readonly",
        NodeFilter: "readonly",
        Blob: "readonly",
        URL: "readonly",
        FileReader: "readonly",
        Image: "readonly",
        TextEncoder: "readonly",
        TextDecoder: "readonly",
        AudioContext: "readonly",
        CustomEvent: "readonly",
        KeyboardEvent: "readonly",
        MouseEvent: "readonly",
        Event: "readonly",
        performance: "readonly",
        ...globals.browser,
        // Tauri IPC(由 WebView 注入,非标准 browser 全局)
        invoke: "readonly",
        listen: "readonly",
        __TAURI_INTERNALS__: "readonly",
        // vendor/monaco/loader.js 在运行时挂上的全局(03-shell.js:513 使用)。
        // 它是**宿主全局**,不是 ui/*.js 的顶层声明,所以声明在这里而不是进
        // 生成清单。收紧生成器(只认列 0 声明)后它是唯一暴露出来的真实缺口。
        monaco: "readonly",
      },
    },
    rules: {
      ...js.configs.recommended.rules,
      "no-unused-vars": ["error", { argsIgnorePattern: "^_", caughtErrorsIgnorePattern: "^_" }],
    },
  },
  {
    // UI-0926 #9 弹层技术栈(docs/design/ui_surface_stack.md §7.1):弹层只有一种写法。
    // 模态/菜单/浮层/卡片/toast/tooltip 一律经 00-surface.js 的原语开关,Esc 只有它一个入口;
    // 写错时报错信息直接给出应该改用的写法。00-surface.js 本身、样例页与 oc-studio 不在此列。
    files: ["crates/kanzei-app/ui/*.js"],
    ignores: [
      "crates/kanzei-app/ui/00-surface.js",
      "crates/kanzei-app/ui/gallery.js",
      "crates/kanzei-app/ui/oc-studio.js",
    ],
    rules: {
      "no-restricted-syntax": [
        "error",
        { selector: "CallExpression[callee.name=/^(alert|confirm|prompt)$/]", message: "用 01-core.js 的 confirmDialog/inputDialog 或 03-shell.js 的 toast(WebView2 下原生对话框不可用)" },
        { selector: "CallExpression[callee.object.name=/^(window|globalThis)$/][callee.property.name=/^(alert|confirm|prompt)$/]", message: "用 01-core.js 的 confirmDialog/inputDialog 或 03-shell.js 的 toast" },
        { selector: "CallExpression[callee.property.name='createElement'][arguments.0.value='dialog']", message: "模态只在 index.html 里声明 <dialog class=\"k-surface k-dialog\">,由 00-surface.js 的 openDialog 打开" },
        { selector: "CallExpression[callee.property.name=/^(showModal|showPopover|hidePopover|togglePopover)$/]", message: "只有 00-surface.js 能开关顶层弹层:用 openDialog/closeSurface、openPopover/openMenu、showCard/hideCard" },
        { selector: "CallExpression[callee.property.name='setAttribute'][arguments.0.value=/^(popover|popovertarget)$/]", message: "弹层用 00-surface.js 的 openMenu/openPopover,或在 index.html 写 data-kz-menu 触发器 + popover 弹层" },
        { selector: "AssignmentExpression[left.property.name=/^(popover|popoverTargetElement)$/]", message: "弹层用 00-surface.js 的 openMenu/openPopover,或在 index.html 写 data-kz-menu 触发器 + popover 弹层" },
        { selector: "AssignmentExpression[left.object.property.name='style'][left.property.name='position'][right.value='fixed']", message: "不要新造 position:fixed 浮层:用 00-surface.js 的 openPopover/openMenu/showCard(顶层元素 + CSS 锚点定位)" },
        { selector: "CallExpression[callee.object.name=/^(document|window)$/][callee.property.name='addEventListener'][arguments.0.value='keydown'] BinaryExpression[right.value='Escape']", message: "全局 Esc 归 00-surface.js 的弹层栈(捕获阶段只关栈顶):弹层传 onEscape;局部输入框的 Esc 挂在元素自己身上" },
      ],
    },
  },
  {
    // PWA 页面脚本使用浏览器宿主；与桌面 ESM 和 Node 冒烟脚本分别检查。
    // 旧 service worker 已删除，不向页面声明 worker 专属全局。
    files: ["crates/kanzei-app/mobile-pwa/*.js"],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "script",
      globals: {
        ...globals.browser,
      },
    },
    rules: {
      "no-undef": "error",
      "no-unused-vars": "off",
    },
  },
  {
    // scripts/*.mjs 冒烟脚本自身:node 环境 + ESM
    files: ["scripts/*.mjs"],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "module",
      globals: { ...globals.node, document: "readonly" },
    },
    rules: {
      "no-undef": "error",
      "no-unused-vars": "off",
    },
  },
  {
    // Playwright callbacks execute in the actual browser page, not Node.
    files: ["scripts/ui-design-audit-repairs-smoke.mjs", "scripts/ui-state-admission-smoke.mjs", "scripts/ui-project-resource-events-smoke.mjs", "scripts/ui-file-link-menu-smoke.mjs", "scripts/resource-native-browser.mjs"],
    languageOptions: { globals: { ...globals.browser } },
  },
];
