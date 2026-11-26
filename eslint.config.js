import globals from "globals";
import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default [
  {
    ignores: [
      "crates/kanzei-app/dist-web/**",
      "crates/kanzei-app/web/public/**",
      "crates/kanzei-app/web/src/ipc/generated.ts",
      "node_modules/**",
      "target/**",
      "dist/**",
    ],
  },
  ...tseslint.configs.recommended.map((cfg) => ({
    ...cfg,
    files: ["crates/kanzei-app/web/src/**/*.{ts,tsx}"],
  })),
  {
    files: ["crates/kanzei-app/web/src/**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "module",
      globals: {
        ...globals.browser,
      },
    },
    rules: {
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_", caughtErrorsIgnorePattern: "^_" },
      ],
      "no-restricted-syntax": [
        "error",
        {
          selector: "CallExpression[callee.name=/^(alert|confirm|prompt)$/]",
          message: "WebView2 下禁止原生对话框，请使用 store/uiStore 中的 confirmDialog / promptDialog / pushToast",
        },
        {
          selector: "CallExpression[callee.object.name=/^(window|globalThis)$/][callee.property.name=/^(alert|confirm|prompt)$/]",
          message: "WebView2 下禁止原生对话框，请使用 confirmDialog / promptDialog / pushToast",
        },
      ],
    },
  },
  {
    files: ["crates/kanzei-app/mobile-pwa/*.js"],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "script",
      globals: {
        ...globals.browser,
      },
    },
    rules: {
      ...js.configs.recommended.rules,
      "no-undef": "error",
      "no-unused-vars": "off",
    },
  },
  {
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
