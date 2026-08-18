---
id: M-173
scope: project
category: fact
title: bash AssertionError 静态文案未同步的根因与对策
description: bash 报错"HTML 静态文案未进入资源表"时的必读：工具契约失效识别与修复路径
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

[fp:bash|AssertionError [ERR_ASSERTION]: HTML 静态文案未进入资源表: 记忆控制面]

**错误特征**: exit code 1 + HTML 静态文案声称同步但实际未进入资源表（如 ui-lint-globals.json 720个标识符与源码不同步）
**适用场景**: D-480(ui-lint生成流程)涉及HTML UI代码的UI运行时冒烟测试；bash执行node/ESLint验证步骤失败；资源表同步工具链调用

**根因分析**: HTML 静态文案（static markup）未正确注入到记忆控制面的资源表中，导致运行时解析时找不到预期内容。可能原因:
1. 前端构建流程遗漏"静态HTML→资源表"转换步骤
2. 渲染顺序错误（先渲染后同步=读取失败）
3. UI组件未声明依赖资源表路径

**处置步骤**:
1. 停止bash执行（防止错误累积）
2. 定位ui-lint-globals.json生成脚本的HTML处理模块
3. 验证静态文案注入时机：必须在node模块初始化前完成同步
4. 检查memory控制面板配置:是否启用"静态→资源表"桥接
5. 重新执行bash+ESLint冒烟测试

**例外与边界**: 
- 若UI代码使用Dynamic HTML Generation，跳过此流程
- 运行时24个ui/*.js按序执行已通过但依赖加载失败（需单独排查）
