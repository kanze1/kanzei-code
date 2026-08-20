// R-142:前端最低配 ESLint(flat config)。
// 只开 no-undef 类规则(防手误),不引入构建步骤。ui/*.js 是经典 script 按序加载,
// 顶层声明的全局标识符清单由 scripts/gen-ui-lint-globals.mjs 生成(冒烟会校验同步)。
import globals from "globals";
import { collectUiGlobals, readCachedUiGlobals } from "./scripts/gen-ui-lint-globals.mjs";

export function loadUiGlobals({ collect = collectUiGlobals, readCache = readCachedUiGlobals } = {}) {
  try {
    return collect();
  } catch (error) {
    try {
      const cached = readCache();
      console.warn(`实时 UI globals 生成失败，降级使用缓存清单: ${error.message}`);
      return cached;
    } catch (cacheError) {
      throw new Error(`实时 UI globals 生成失败且缓存不可用: ${cacheError.message}`, { cause: error });
    }
  }
}

const uiGlobals = loadUiGlobals();

export default [
  {
    files: ["crates/kanzei-app/ui/*.js"],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "script",
      globals: {
        // ui/*.js 跨文件全局(经典 script 顶层作用域共享)。放前面:同名宿主全局
        // 由后面的 Tauri/browser 只读块覆盖,保持 readonly 语义。
        ...Object.fromEntries(uiGlobals.map((name) => [name, "writable"])),
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
