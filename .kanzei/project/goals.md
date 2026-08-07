# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: D-050 已关闭；D-054 已关闭并归档。D-051 继续 fixing：CLI 真实 AlwaysAllow→本地 SSE→bash→marker E2，桌面真实 UI E2 因无前端 harness 暂缓。D-055 继续 fixing：UI ask 按 session 分队列、pending_asks_get 重建、后台 done/error/stopped 触发 process_list 刷新；node --check/cargo test 通过，真实 UI E2 暂缓。D-056 继续 fixing：工作区项目切换补运行态复位和目标进程刷新，真实切项目运行中 E2 待补。D-060 继续 fixing：DocStore 模板化保留未知行；已补 archive_terminal 模板转移及真实 req archive 回归，cargo test -p kanzei-tools 21 项通过；update/close/reorder/并发仍待覆盖。D-059 已 fixed：webfetch/websearch 共享 HTML 解析改为 ASCII 大小写折叠，İ/ẞ script/style 回归通过，cargo test -p kanzei-tools 23 项通过。D-061 因第三方 OAuth 共享凭证并发写入需用户确认方案，保持 open 并已记录阻塞。D-064 继续 fixing：append_event 使用 BEGIN IMMEDIATE 串行化 sequence，4 连接 80 事件并发回归通过；run_task 收尾落库错误已隔离为可见告警，不再伪装模型结果，app/core 测试通过；注入收尾失败 E2 仍待补。
- 当前判断: 距离"日常主力开发工具"的差距不在功能数量而在可靠性与验收质量——上一轮的验证手段几乎清一色是语法检查,导致带病能力被判定为完成。近期重心应是 R-083(收口 P0 缺陷)、R-084(建立能捕获运行时错误的验收手段)、R-085(完成判定的执行约束),之后再谈新能力。
