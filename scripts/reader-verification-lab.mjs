// 用阅读器真实源码驱动产品 CLI；实验台账独立，原 R-001、会话与用户数据不改写。
// node scripts/reader-verification-lab.mjs <reader-root> <flutter.bat> [lab-directory]
import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";

const repo = path.resolve(import.meta.dirname, "..");
const source = path.resolve(process.argv[2]);
const flutter = path.resolve(process.argv[3]);
const lab = path.resolve(process.argv[4] || path.join(repo, "output", `reader-lab-${randomUUID().slice(0, 8)}`));
const kz = path.join(repo, "target/debug", process.platform === "win32" ? "kz.exe" : "kz");
await mkdir(path.join(lab, ".kanzei"), { recursive: true });
const transcript = [];
function cli(args, json = false) {
  const before = Date.now();
  const output = execFileSync(kz, args, { cwd: source, env: { ...process.env, KANZEI_PROJECT_ROOT: lab }, encoding: "utf8", windowsHide: true, timeout: 120000 });
  transcript.push({ args, output, duration_ms: Date.now() - before });
  console.log(`${args.slice(0, 2).join(" ")} ${Date.now() - before}ms`);
  return json ? JSON.parse(output) : output;
}
cli(["req", "add", "阅读器后台验证实验", "--complexity", "小", "--tag", "流程", "--priority", "P1",
  "--field", "来源=用户原话：我们将把那个markdown手机端阅读器作为我们的实验项目让你尽情的测试他的的场景",
  "--field", "验收=冻结阅读器源码；后台运行真实测试；独立工作不被验证或缺设备阻塞；证据绑定快照",
  "--field", "批次=0/1"]);
cli(["req", "update", "R-001", "--field", "执行模型=work_units_v1"]);
const unit = (objective, acceptance, dependencies = []) => cli(["work", "create-unit", "--requirement", "R-001", "--objective", objective, "--acceptance", acceptance, "--scope", "lib", "--scope", "test", ...dependencies.flatMap((id) => ["--depends-on", id])], true);
const data = unit("附件、PDF 文字提取与本地搜索回归", "阅读器数据单元测试通过");
const reader = unit("窄屏阅读与搜索交互回归", "阅读器界面测试通过");
const device = unit("真实手机 OCR", "真机图片识别结果正确");
const delivery = unit("组合交付检查", "数据和界面验证均有快照证据", [data.unit_id, reader.unit_id]);
cli(["work", "block", device.unit_id, "--reason", "需要已连接的真实 Android/iOS 设备；此实验只运行桌面 Flutter 测试，其他单元继续"]);

const quote = (v) => `'${v.replaceAll("'", "''")}'`;
const testCommand = (targets) => process.platform === "win32"
  ? `& ${quote(flutter)} pub get --offline; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; ${targets === "test/unit" ? `& ${quote(flutter)} analyze --no-pub; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; ` : ""}& ${quote(flutter)} test --no-pub ${targets} --reporter expanded; exit $LASTEXITCODE`
  : `${quote(flutter)} pub get --offline && ${targets === "test/unit" ? `${quote(flutter)} analyze --no-pub && ` : ""}${quote(flutter)} test --no-pub ${targets} --reporter expanded`;
const start = (u, targets) => {
  cli(["work", "claim", u.unit_id]);
  return cli(["work", "verify-async", u.unit_id, "--command", testCommand(targets), "--criterion", u.acceptance[0], "--environment", "Flutter 3.47.5 / Dart 3.13.4 / desktop test host", "--resource", "flutter-reader-lab", "--timeout-ms", "180000"], true);
};
const first = start(data, "test/unit");
const during = cli(["work", "next", "--detail"], true);
assert.equal(during.selected?.id, reader.unit_id, "后台验证必须释放开发 WIP");
const firstState = JSON.parse(await readFile(path.join(lab, ".kanzei/verification", `${first.verification_job}.json`), "utf8"));
assert(["queued", "running"].includes(firstState.status), "必须在首项验证尚未完成时取得下个单元，不能用顺序执行冒充异步");
assert(during.blocked_items.some((item) => item.id === delivery.unit_id), "依赖仍需等待验证");
const second = start(reader, "test/widget_test.dart test/attachment_widget_test.dart");
await writeFile(path.join(lab, "started.json"), JSON.stringify({ source, lab, first, second, during, device: device.unit_id, delivery: delivery.unit_id }, null, 2));
console.log(JSON.stringify({ lab, first, second, selected_while_verifying: during.selected.id }));
let last = "";
let jobs;
const startTime = Date.now();
while (Date.now() - startTime < 420000) {
  jobs = await Promise.all([first, second].map((item) => readFile(path.join(lab, ".kanzei/verification", `${item.verification_job}.json`), "utf8").then(JSON.parse)));
  const states = jobs.map((j) => `${j.unit_id}:${j.status}`).join(" ");
  if (states !== last) { console.log(states); last = states; }
  if (jobs.every((j) => !["queued", "running"].includes(j.status))) break;
  await new Promise((resolve) => setTimeout(resolve, 1000));
}
const after = cli(["work", "next", "--detail"], true);
const result = { source, lab, during, jobs, after, transcript, timestamp: new Date().toISOString() };
await writeFile(path.join(lab, "result.json"), JSON.stringify(result, null, 2));
console.log(`RESULT ${path.join(lab, "result.json")}`);
if (jobs.every((j) => j.status === "passed")) {
  assert.equal(after.selected?.id, delivery.unit_id, "通过后应解锁依赖单元，真机阻塞不扩散");
} else process.exitCode = 1;
