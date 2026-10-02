---
id: M-003
scope: project
category: fact
title: tracker 状态机只进不退：cannot move backward 时核对状态与当前提交证据
description: 处理 req/defect/goal 的 update 报 cannot move backward，或 defect 关闭缺当前 HEAD 的 verify 证据时必读：不要回退到更早状态；核对允许的单向迁移，并让 verification.json 绑定要关闭的这次提交后重跑 verify。
status: active
created: 2026-08-07
updated: 2026-09-02
source: run(失败信号自动采集) + 人工校正
---

tracker 状态机只进不退。处理 req/defect/goal 的 update 报“cannot move backward”时，不要把状态改回列表中的更早状态；先核对当前状态与允许的单向迁移，改用合法后继状态或相应终态操作。关闭 defect 前，verify 证据必须绑定要关闭的这次提交，而不是仅看仓库最近一次提交；若 dist/verification.json 仍绑定旧提交，提交后以当前目标提交重跑 verify，并用 test_record 记录通过证据。

本次证据：D-738 缺 verify，dist/verification.json 绑定 1b3115ea，而当前 HEAD 为 13154063c90d8960e6957d5f2bd27bc3115ed9d1；应将证据绑定要关闭的这次提交后重跑 verify。

[fp:defect|行动: 的 update 反复报 cannot move backward 时必读:状态只能沿列表顺序前进]
[fp:defect|行动: 处理 的 update 反复报 cannot move backward 时必读：不要把状态改回列表中的更早状态；先核对当前状态与允许的单向迁移，改用合]
