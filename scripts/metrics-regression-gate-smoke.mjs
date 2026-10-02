#!/usr/bin/env node
// R-309 B4:metrics 口径漂移必须在解析 Top-30 前拒绝出数。
import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
const temp = await mkdtemp(resolve(tmpdir(), "kz-metrics-gate-"));
const fixture = resolve(temp, "metrics-output.txt");
try {
  await writeFile(fixture, "metrics format: v999\n", "utf8");
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-File",
      resolve(root, "scripts/metrics-regression-gate.ps1"),
      "-Root",
      root,
      "-MetricsOutputPath",
      fixture,
    ],
    { encoding: "utf8" },
  );
  const output = `${result.stdout}\n${result.stderr}`;
  assert.notEqual(result.status, 0, "口径漂移 fixture 不得通过 metrics gate");
  assert.match(output, /metrics format version mismatch/, output);
  console.log("R-309 B4 metrics 口径漂移拒绝定向测试通过");
} finally {
  await rm(temp, { recursive: true, force: true });
}
