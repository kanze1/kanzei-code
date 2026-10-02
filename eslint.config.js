// R-142:前端最低配 ESLint(flat config)。
// 只开 no-undef 类规则(防手误),不引入构建步骤。ui/*.js 以 ESM 显式 import/export 连接。
import globals from "globals";

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
      // 唯一启用的规则:未定义变量。其余规则一律不开(最低配,不引入格式化约束)。
      "no-undef": "error",
      // 注释里的全局用法允许(如 JSDoc @type)
      "no-unused-vars": "off",
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
    // R-292:mobile-pwa(PWA 页面脚本 + service worker)独立覆盖——不在 ui/*.js 的
    // 经典 script 共享作用域内,也不与 scripts/*.mjs 的 node 环境混。app.js 走
    // 浏览器全局;sw.js 额外需要 service worker 宿主(self/caches/clients 等)。
    files: ["crates/kanzei-app/mobile-pwa/*.js"],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "script",
      globals: {
        ...globals.browser,
        // service worker 宿主全局(sw.js);页面脚本 app.js 不需要但声明无害。
        self: "readonly",
        caches: "readonly",
        clients: "readonly",
        skipWaiting: "readonly",
        Response: "readonly",
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
];
