# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: D-050 已关闭；D-054 已关闭并归档。D-051 继续 fixing：CLI 真实 AlwaysAllow→本地 SSE→bash→marker E2，桌面真实 UI E2 因无前端 harness 暂缓。D-055 继续 fixing：UI kz:ask 按 session 分队列且不被活动 session 过滤丢弃；新增 pending_asks_get Tauri command 与切回重建；后台 done/error/stopped 控制事件触发 process_list 刷新，node --check/cargo test 通过。D-056 继续 fixing：工作区项目切换补 setRunning(false)+refreshProcesses，目标态来自后端 process_list；真实切项目运行中 E2 待补。
- 当前判断: 距离"日常主力开发工具"的差距不在功能数量而在可靠性与验收质量——上一轮的验证手段几乎清一色是语法检查,导致带病能力被判定为完成。近期重心应是 R-083(收口 P0 缺陷)、R-084(建立能捕获运行时错误的验收手段)、R-085(完成判定的执行约束),之后再谈新能力。
