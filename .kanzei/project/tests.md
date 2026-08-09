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

## T-1786249737 R-153 批0重复测试隔离回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证 update_tests 旧副本禁用后，仅新五个测试模块参与的 kanzei-app 回归。

## T-1786249861 R-153 批0 state 旧副本物理删除回归 [running]
- 命令: cargo test -p kanzei-app
- 摘要: 正在验证继续物理删除 state 旧测试函数后的 kanzei-app 回归。
