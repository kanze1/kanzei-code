#!/usr/bin/env node
// R-309 B1:验证 ESLint 配置优先实时收集 globals，生成失败时才使用缓存。
import assert from "node:assert/strict";
import { loadUiGlobals } from "../eslint.config.js";

const dynamic = loadUiGlobals({
  collect: () => ["from_source"],
  readCache: () => ["from_cache"],
});
assert.deepEqual(dynamic, ["from_source"], "实时生成结果必须优先于缓存");

const fallback = loadUiGlobals({
  collect: () => {
    throw new Error("synthetic source failure");
  },
  readCache: () => ["cached_global"],
});
assert.deepEqual(fallback, ["cached_global"], "实时生成失败时必须降级到缓存");

assert.throws(
  () =>
    loadUiGlobals({
      collect: () => {
        throw new Error("synthetic source failure");
      },
      readCache: () => {
        throw new Error("synthetic cache failure");
      },
    }),
  /缓存不可用/,
  "实时生成与缓存都失败时必须拒绝启动 lint 配置",
);

const actual = loadUiGlobals();
assert.ok(actual.length > 0, "真实 UI 源码必须产生非空 globals 清单");
assert.ok(actual.includes("activePane"), "真实顶层 UI 标识符必须来自源码收集结果");
console.log(`R-309 B1 globals 配置冒烟通过:${actual.length} 个实时标识符`);
