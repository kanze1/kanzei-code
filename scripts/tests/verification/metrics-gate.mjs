#!/usr/bin/env node
// R-309 B4:metrics 口径漂移必须在解析 Top-30 前拒绝出数。
import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "../../..");
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
  const baseline = JSON.parse(await readFile(resolve(root, "scripts/metrics-baseline.json"), "utf8"));
  const rows = Object.entries(baseline.files).map(([file, production]) => `${file} ${production} ${production} 0 1 1 0`);
  const runFixture = async lines => {
    await writeFile(fixture, `metrics format: ${baseline.format_version}\n${lines.join("\n")}\n`, "utf8");
    return spawnSync("powershell.exe", ["-NoProfile", "-File", resolve(root, "scripts/metrics-regression-gate.ps1"), "-Root", root, "-MetricsOutputPath", fixture], { encoding: "utf8" });
  };
  const valid = await runFixture(rows);
  assert.equal(valid.status, 0, `${valid.stdout}\n${valid.stderr}`);
  const giant = await runFixture([...rows, "crates/new/src/lib.rs 1201 1201 0 1 1 0"]);
  assert.notEqual(giant.status, 0, "a new giant must fail even if the total growth is only one file");
  assert.match(`${giant.stdout}\n${giant.stderr}`, /new giant/);
  const incomplete = await runFixture(rows.slice(1));
  assert.notEqual(incomplete.status, 0, "a partial inventory cannot pass the all-files gate");
  assert.match(`${incomplete.stdout}\n${incomplete.stderr}`, /omitted/);
  const duplicates = await runFixture([...rows, rows[0]]);
  assert.notEqual(duplicates.status, 0, "duplicates cannot pad the inventory");
  assert.match(`${duplicates.stdout}\n${duplicates.stderr}`, /duplicate metrics row/);
  console.log("R-309 B4 metrics 口径漂移拒绝定向测试通过");
} finally {
  await rm(temp, { recursive: true, force: true });
}
