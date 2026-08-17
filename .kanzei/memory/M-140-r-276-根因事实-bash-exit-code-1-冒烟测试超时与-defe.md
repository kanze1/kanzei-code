---
id: M-140
scope: project
category: fact
title: 完成 R-276(UI 冒烟测试) 根因与修复 SOP
description: 工具行为：记录 R-276 中的 bash/defect 失败根因与修正方法
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
subject: R-276 测试与缺陷修正失败根因
---

适用场景:bash ×2(ui 运行时失败)+defect×1(状态修正)触发，需要完整的前端检查流程。同时处理文件路径缺失、bash 超时等复合问题。当缺陷已归档(is archived)且出现 cannot open error(os error 2)时

操作步骤:
1. work→memory_search 定位问题相关代码/配置
2. collaboration_status 确认项目协作状态
3. git grep 搜索关键标记或变量名
4. read 检查前端文件内容(memory.rs 等)。路径错误需 \\?\C:\Users\...\path format
5. edit 修正错误或创建缺失源文件
6. insert 添加缺失标记
7. defect status 检查是否已归档(is archived)
8. bash 执行终端命令(预期可能有2次失败模式:超时 kill+exit code 0)
9. ui_dom/ui_console 进行冒烟测试。长报告未补齐最早内容时会 fail
10. frontend_check 验证修复结果
11. frontend_locate 精确定位问题位置
12. test_record 记录测试报告
13. req 请求进一步支持
14. defect fix_terminal id=<id> status=<fixed|wontfix> reason=<why>修正已归档状态

边界与例外:
- bash×2(exit code:)+defect×1(状态修正)是预期中的复合失败模式，不是异常而是工具行为[fp:bash|test bash::tests::timeout_kills_command_and_returns_explicit_error ... ok][fp:defect|is archived]
- ui_dom/console 冒烟失败常见于长报告未补齐最早内容[fp:read|cannot open]
