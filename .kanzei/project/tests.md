# Test Runs

## T-1786248822 R-153 批0d permission 测试迁移回归 [running]
- 命令: cargo test -p kanzei-app permission_tests
- 摘要: 正在验证新增 permission_tests 模块。

## T-1786248951 R-153 批0e state 测试迁移回归 [running]
- 命令: cargo test -p kanzei-app state_tests
- 摘要: 正在验证新增 state_tests 模块。

## T-1786249114 R-153 批0旧测试副本清理回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证批0旧测试副本清理后的 kanzei-app 全量单测。

## T-1786249557 R-153 批0旧测试副本继续清理回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证删除 state/process/conversation/permission 旧测试副本后的 kanzei-app 测试。
