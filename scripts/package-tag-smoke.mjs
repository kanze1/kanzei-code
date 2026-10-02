#!/usr/bin/env node
// D-656:package.ps1 的 build 标签同步与发布范围前置核对必须针对真实脚本执行。
import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
const packageScript = resolve(root, "scripts", "package.ps1");
const temp = await mkdtemp(join(tmpdir(), "kz-package-tag-smoke-"));
const fakeBin = join(temp, "bin");
const fakeGit = join(fakeBin, "git.cmd");
await mkdir(fakeBin, { recursive: true });

const fakeGitScript = `@echo off
set "args=%*"

 echo(%args%| findstr /c:"ls-remote" >nul
if not errorlevel 1 (
  if "%KZ_TAG_SMOKE_MODE%"=="missing-local" (
    echo 2222222222222222222222222222222222222222\trefs/tags/build-remote
  ) else (
    echo 2222222222222222222222222222222222222222\trefs/tags/build-same
  )
  exit /b 0
)

echo(%args%| findstr /c:"tag --list" >nul
if not errorlevel 1 (
  if "%KZ_TAG_SMOKE_MODE%"=="missing-local" (
    echo build-local
  ) else (
    echo build-same
  )
  exit /b 0
)

echo(%args%| findstr /c:"rev-parse --short HEAD" >nul
if not errorlevel 1 (
  echo abc1234
  exit /b 0
)

echo(%args%| findstr /c:"rev-parse HEAD" >nul
if not errorlevel 1 (
  echo 1111111111111111111111111111111111111111
  exit /b 0
)

rem The target comparison asks for the local tag ref.
echo(%args%| findstr /c:"refs/tags/build-same" >nul
if not errorlevel 1 (
  if "%KZ_TAG_SMOKE_MODE%"=="target-mismatch" (
    echo 1111111111111111111111111111111111111111
  ) else (
    echo 2222222222222222222222222222222222222222
  )
  exit /b 0
)
exit /b 0
`;

const runPackage = (mode) =>
  spawnSync(
    "powershell.exe",
    ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", packageScript, "-Ack", "0"],
    {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, KZ_TAG_SMOKE_MODE: mode, PATH: `${fakeBin};${process.env.PATH}` },
    },
  );

try {
  await writeFile(fakeGit, fakeGitScript, "ascii");
  await chmod(fakeGit, 0o755);

  const missing = runPackage("missing-local");
  const missingOutput = `${missing.stdout}\n${missing.stderr}`;
  assert.notEqual(missing.status, 0, "远端多出 build 标签时 package.ps1 必须中止");
  assert.match(missingOutput, /build .*\[build-remote\]/);
  assert.match(missingOutput, /build .*\[build-local\]/);

  const targetMismatch = runPackage("target-mismatch");
  const targetOutput = `${targetMismatch.stdout}\n${targetMismatch.stderr}`;
  assert.notEqual(targetMismatch.status, 0, "最新 build 标签指向不一致时 package.ps1 必须中止");
  assert.match(targetOutput, /build .*build-same/);

  const source = await readFile(packageScript, "utf8");
  assert.match(
    source,
    /gh release create[\s\S]*?git -C \$root fetch origin "tag" \$tag/,
    "发布成功后必须 fetch 同名 build 标签",
  );
  assert.match(
    source,
    /if \(\$LASTEXITCODE -ne 0\) \{\s*Write-Warning "远端 release 已发布,但本地标签/,
    "标签 fetch 失败只能告警，不能把已成功的远端发布改报失败",
  );

  console.log("D-656 package.ps1 build 标签同步 smoke 通过");
} finally {
  await rm(temp, { recursive: true, force: true });
}
