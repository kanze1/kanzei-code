# CI 与发布证据链(独立验证者 + commit 绑定门禁)

- 身份: validated_design
- 状态: 已验证基线；R-152、R-146、R-156、R-298 已完成
- 日期: 2026-08-09
- 最近核验提交: c0ea88d / 81e6800
- 关联需求: R-152 (done)、R-146 (done)、R-156 (done)、R-298 (done)
- 关联缺陷: 无(D-183/D-198 为同族先例,见下)
- 关联决策: A-009
- 执行模型权威: R-317 的 Outcome/Work Unit/事件投影；发布证据以 VerificationRun/verification.json 和 release gate 为消费面

## 当前交付证据入口

- R-152 已完成 License、CI、`scripts/verify.ps1` 与 `package.ps1` commit 绑定门禁；当前验证入口是 `dist/verification.json`，发布包必须匹配 HEAD 且 `all_pass=true`。
- R-146/R-156 已启用 clippy/fmt 闸门；CI 与 verify.ps1 的门禁清单必须同步。
- R-298 已补齐安装后自校验、安装器 SHA256、版本一致性和 `dist/` 保留策略；这些是既有交付，不在本文再次申报为新实现。
- 实施前输入: 2026-08-09 用户工程评审提出 License 元数据、真 CI、release gate 机械化三项 P0；本文件保留该原始问题与方案演进记录。

## 背景与问题

1. **License 元数据冲突(P0)**:`LICENSE.md` 与 README 是 PolyForm Noncommercial 1.0.0,但 workspace `Cargo.toml` 写 `license = "MIT"`。对任何 license scanner 都是歧义。
2. **无 CI**:仓库没有 `.github/workflows/`。所有验证跑在开发机上,"这个 commit 在干净环境里是否成立"无人回答。自举 agent 的"测试通过"是 self-report,没有独立复核。
3. **发布门禁不闭环**:`package.ps1` 有 D-183 的 `-Ack` 发布范围核对与脏树检查,但**不验证"这个 commit 跑过测试"**。当前正确性依赖"发布者应该先跑过 release.ps1/测试"的约定——与本仓库"规则进代码,不进提示词"的哲学矛盾。实测风险:并发自举提交后直接打包,发布的二进制从未被测试过。

同族先例:D-183(-Ack 数目核对防夹带)、D-198(容器重定向探测防假安装)。本文把同一思路推到"测试证据"环节。

## 目标与非目标

- 目标:①License 元数据统一;②每次 push 有独立环境复跑门禁(GitHub Actions);③发布物与"验证过的 commit"机械绑定——无绑定证据不得打包。
- 非目标:多平台矩阵(仅 windows)、代码签名、SBOM、可重现构建、stable/nightly 通道语义、CI 产物直接发布(构建仍在本机 package.ps1)、自动部署(CD)。这些如日后需要,新开设计文档。

## 最终方案

### ① License 修复

`Cargo.toml` `[workspace.package]`:`license = "MIT"` → `license = "PolyForm-Noncommercial-1.0.0"`(SPDX 合法标识符,scanner 可读)。各 crate 若为 `license.workspace = true` 则自动继承,无需逐个改;改完 `cargo metadata --no-deps | Select-String license` 核对。

### ② GitHub Actions CI(独立验证者)

新增 `.github/workflows/ci.yml`,参考实现(可直接落盘):

```yaml
name: ci
on:
  push:
    branches: [dev, main]
  pull_request:
    branches: [main]
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true
jobs:
  checks:
    runs-on: windows-latest
    timeout-minutes: 90
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      # fmt 闸门由 R-156(全仓 fmt 收敛)交付时取消注释——当前仓库有 435 处 fmt diff,首日启用必红
      # - name: fmt
      #   run: cargo fmt --all -- --check
      # clippy 闸门由 R-146(warning 清零)交付时取消注释
      # - name: clippy
      #   run: cargo clippy --workspace --all-targets -- -D warnings
      - name: test
        run: cargo test --workspace
      - name: ui smoke
        shell: bash
        run: |
          for f in crates/kanzei-app/ui/*.js; do node --check "$f"; done
          node scripts/ui-runtime-smoke.mjs
          node scripts/ui-a11y-smoke.mjs
          node scripts/ui-i18n-smoke.mjs
          node scripts/ui-markdown-smoke.mjs
```

要点:
- **首日必须全绿**,所以 fmt/clippy 两闸注释着落地,由 R-156/R-146 启用(两者都会产生全仓 diff,必须排在巨石拆解之后,避免行号地图漂移与大搬迁撞车,见 monolith_decomposition.md)。
- ui smoke 步骤用 bash + glob 循环,现在(单 main.js)与 R-154 拆成多文件后**都成立**,不用改。glob 不递归,天然排除 `ui/vendor/`。
- windows-latest 预装 WebView2 与 node,`rusqlite` bundled、`reqwest` rustls,无系统依赖要装。
- 首跑若有环境相关红测(测试隐式依赖本机目录/代理),按缺陷登记修复,**不许 skip/忽略**。

### ③ scripts/verify.ps1(证据生成器)

新脚本。在**干净工作树**上跑当前启用的全套门禁,全绿后写 `dist/verification.json`。参考实现:

```powershell
# kanzei 验证证据:门禁全绿后产出绑定 commit 的 dist/verification.json(R-152/A-009)
# 用法: .\scripts\verify.ps1   # 任何一步失败即中止,不产出证据
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

# 证据只绑定 commit:源码必须干净(口径与 package.ps1 一致,含未跟踪源码)
$dirty = @(git -C $root diff --name-only HEAD -- crates scripts .github Cargo.toml Cargo.lock) | Where-Object { $_ }
$untracked = @(git -C $root ls-files --others --exclude-standard -- crates scripts .github) | Where-Object { $_ }
if ((@($dirty) + @($untracked)).Count -gt 0) {
    throw "工作树不干净,证据无法绑定 commit:`n$(((@($dirty) + @($untracked))) -join "`n")"
}
$full_hash = (git -C $root rev-parse HEAD).Trim()

$checks = [ordered]@{}
function Invoke-Check([string]$name, [scriptblock]$body) {
    Write-Host "==> $name" -ForegroundColor Cyan
    & $body
    if ($LASTEXITCODE -ne 0) { throw "$name 失败(exit=$LASTEXITCODE)" }
    $checks[$name] = "pass"
}

# R-156 交付后启用: Invoke-Check "fmt" { cargo fmt --all --manifest-path "$root\Cargo.toml" -- --check }
# R-146 交付后启用: Invoke-Check "clippy" { cargo clippy --workspace --all-targets --manifest-path "$root\Cargo.toml" -- -D warnings }
Invoke-Check "test" { cargo test --workspace --manifest-path "$root\Cargo.toml" }
Invoke-Check "ui_syntax" {
    Get-ChildItem "$root\crates\kanzei-app\ui\*.js" | ForEach-Object {
        node --check $_.FullName
        if ($LASTEXITCODE -ne 0) { throw "node --check 失败: $($_.Name)" }
    }
}
Invoke-Check "ui_runtime" { node "$root\scripts\ui-runtime-smoke.mjs" }
Invoke-Check "ui_a11y" { node "$root\scripts\ui-a11y-smoke.mjs" }
Invoke-Check "ui_i18n" { node "$root\scripts\ui-i18n-smoke.mjs" }
Invoke-Check "ui_markdown" { node "$root\scripts\ui-markdown-smoke.mjs" }

New-Item -ItemType Directory -Force "$root\dist" | Out-Null
[ordered]@{
    commit = $full_hash
    verified_at_utc = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    checks = $checks
    all_pass = $true
} | ConvertTo-Json | Set-Content "$root\dist\verification.json" -Encoding UTF8
Write-Host "==> 证据已写入 dist\verification.json(commit $full_hash)" -ForegroundColor Green
```

### ④ package.ps1 证据门禁

在现有 D-183 区间核对与脏树检查**之后**、`cargo build` **之前**插入(新增 param `[string]$VerificationPath`):

```powershell
# ---- 验证证据门禁(R-152/A-009):无绑定 HEAD 的全绿证据不打包 ----
$verification = if ($VerificationPath) { $VerificationPath } else { Join-Path $root "dist\verification.json" }
if (-not (Test-Path $verification)) {
    throw "缺验证证据 $verification:先跑 scripts\verify.ps1。发布树打包时可用 -VerificationPath 指向 dev 树产出的证据(ff 合并后两树 HEAD 相同)"
}
$evidence = Get-Content $verification -Raw | ConvertFrom-Json
if ($evidence.commit -ne $full_hash) {
    throw "验证证据绑定 $($evidence.commit),HEAD 是 $full_hash:commit 变了就要重新 verify——这正是本门禁存在的原因"
}
if (-not $evidence.all_pass) { throw "验证证据未全绿,不得打包" }
Write-Host "==> 验证证据核对通过($($evidence.verified_at_utc))" -ForegroundColor Green
```

### 发布树流程的配合

conventions §9.1:发布从 `C:\Users\kanzei\Documents\kanzei-release`(main worktree)执行,main 只接 dev 的 ff 合并 → **合并后两树 HEAD 是同一 commit**,dev 树 verify 产出的证据在发布树直接可用:`package.ps1 -Ack N -Publish -VerificationPath <dev树>\dist\verification.json`。commit 全 SHA 绑定是完整性锚点,证据文件路径无所谓。

### release.ps1(开发通道)不动

它自带 `cargo test --workspace` 且只装本机,不产生对外发布物,维持现状。
**开发通道最低门禁(R-298 留档)**:`cargo test --workspace` 全绿(显式 `-SkipTests` 除外)。
允许 `-SkipTests` 是刻意的逃逸阀(用户明确要求快速装),但默认路径必须全量测试后才落本机。
发布通道(package.ps1)门槛更高:verify.ps1 十步全绿证据绑定 HEAD(A-009)。

## 技术选型与取舍

| 选择 | 备选 | 理由 |
| --- | --- | --- |
| windows-latest 单平台 | 三平台矩阵 | 产品只发 Windows;ubuntu 要装 webkit2gtk 一堆系统依赖,纯烧额度 |
| CI 只跑 test+冒烟(暂无 fmt/clippy) | 首日全闸 | 仓库现存 435 处 fmt diff、23 条 clippy warning,首日全闸=首日全红;闸门随 R-156/R-146 启用 |
| 证据文件落 dist/(不入库) | 入库/CI artifact | 产物卫生规则 dist 不入库;证据靠 commit SHA 绑定,不需要历史 |
| 本机构建+发布不变,CI 只做独立复核 | CI 构建产物直接发布 | 变更最小;tauri NSIS 打包上 CI 是大工程,且 -Ack 人工核对环节本来就要人在场 |
| verify.ps1 与 ci.yml 双份门禁清单 | 单一来源 | 两处清单必须逐项同步(fmt/clippy 启用时两处一起开),在两个文件里互相注明;接受此重复换取"本地无 gh 依赖" |

## 实施边界与调用方

- 触碰文件:`Cargo.toml`(1 行)、`.github/workflows/ci.yml`(新)、`scripts/verify.ps1`(新)、`scripts/package.ps1`(param + 一段门禁,约 15 行)。**零业务代码变更。**
- 调用方:发布者(人或 agent)在打包前跑 verify.ps1;GitHub 在每次 push 自动跑 ci.yml。
- push 需代理(`$env:HTTPS_PROXY = "http://127.0.0.1:12000"`),CI 侧不需要。

## 变更记录

- 2026-08-09 初版(用户评审定调 → 方案落盘,交自举执行 R-152)。

## 验证证据

- R-152：`Cargo.toml` workspace license 已统一为 PolyForm-Noncommercial-1.0.0；CI 首跑及后续连续 runs 全绿；`scripts/verify.ps1` 已实测脏树拒跑与全绿产出 `dist/verification.json`；`package.ps1` 已实测无证据、commit 漂移和未全绿三类拦截，并在 `-VerificationPath` 证据齐全时放行至构建。
- R-146/R-156：clippy/fmt 闸门已从设计注释进入当前提交门禁，证据入口为 CI/verify.ps1 的同步清单。
- R-298：安装后自校验、SHA256 release notes、Cargo/Tauri 版本一致性和 dist 保留策略已有真实调用方与验证记录（`T-1786922726461`、`T-1786922726462`）。
- 以上结论以 R-152/R-298 tracker 关闭证据和当前发布脚本为准，不再把历史 TODO 清单当作现行验证状态。

## 后续边界与历史输入

- 实施前输入：fmt/clippy 闸门曾因 R-156/R-146 排期而暂缓启用；当前两条闸门已完成并由 CI/verify.ps1 同步维护。
- 实施前输入：CI 首跑可能暴露测试隐式依赖本机环境；当前若再出现同类问题按缺陷登记，不得 skip。
- stable/nightly 通道仍是后续可选范围；R-298 已交付安装器 SHA256、安装后校验、版本一致性和 dist 保留策略。
