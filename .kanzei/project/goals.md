# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮完成 R-087：首次 run_once_with_parts 请求前统一清洗 prior，修复未触发上下文压缩时孤儿 tool_result 进入 provider 的缺口；既有 docstore 自由文本保留、压缩重试配对、权限拒绝占位结果回归均核验。cargo test -p kanzei-core、cargo test -p kanzei-tools、cargo test --workspace 全部通过。D-148 已 fixed，提交 252e955/40729ad。R-085 仍因托管 conventions.md 缺少可用专用写入口未闭环。
- 当前判断: 距离"日常主力开发工具"的差距不在功能数量而在可靠性与验收质量——上一轮的验证手段几乎清一色是语法检查,导致带病能力被判定为完成。近期重心应是 R-083(收口 P0 缺陷)、R-084(建立能捕获运行时错误的验收手段)、R-085(完成判定的执行约束),之后再谈新能力。
