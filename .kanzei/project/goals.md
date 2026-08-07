# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: D-050 已关闭：统一权限路径规范化与 DevProfile hard deny，补充真实 runner→WriteTool→文件系统门禁回归；harness/tools/core 测试通过。D-051 继续 fixing：bash AlwaysAllow 使用完整命令+有效工作目录结构化资源，bash action-aware opaque 匹配避免命令内路径误走文件规范化；旧裸规则在新资源格式下安全降级 Ask；CLI/桌面 AlwaysAllow 持久化失败改为可见错误并返回 Deny，新增 CLI 2 项、桌面 1 项失败/成功回归。仍缺 CLI/桌面真实 AlwaysAllow→bash E2、旧规则提示/正式迁移、并发写入证据。
- 当前判断: 距离"日常主力开发工具"的差距不在功能数量而在可靠性与验收质量——上一轮的验证手段几乎清一色是语法检查,导致带病能力被判定为完成。近期重心应是 R-083(收口 P0 缺陷)、R-084(建立能捕获运行时错误的验收手段)、R-085(完成判定的执行约束),之后再谈新能力。
