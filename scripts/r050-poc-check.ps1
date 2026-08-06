# R-050 只读 POC 验收入口
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
  Write-Host "==> 运行 R-050 只读 POC 与 core 回归" -ForegroundColor Cyan
  cargo test -p kanzei-core
  if ($LASTEXITCODE -ne 0) { throw "kanzei-core 测试失败" }

  Write-Host "==> 运行桌面端回归" -ForegroundColor Cyan
  cargo test -p kanzei-app
  if ($LASTEXITCODE -ne 0) { throw "kanzei-app 测试失败" }

  Write-Host "==> 验证前端语法" -ForegroundColor Cyan
  node --check crates/kanzei-app/ui/main.js
  if ($LASTEXITCODE -ne 0) { throw "前端语法检查失败" }

  Write-Host "R-050 只读 POC 验收通过；未创建 worktree，未执行项目文件或 git 写入。" -ForegroundColor Green
}
finally {
  Pop-Location
}
