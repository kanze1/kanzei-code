# kanzei Memory 系统设计

状态: 设计基线(2026-08-08 与用户对齐 taste 后沉淀)
需求: R-103(总纲) → R-104/R-105/R-106/R-107(四期)

## 0. 定调(用户拍板的决策记录,后续不再重议)

- **文件优先**:非参数化外部记忆的核心优势就是可编辑与透明。真源永远是 markdown 文件,人可直接改,git 可完整恢复。
- **不要向量库**:向量检索的记忆杂而不精准;需要世界级知识不如联网搜索。给 agent **好的搜索工具**(结构化过滤 + 全文检索)。
- **不要知识图谱**:慢、不一定准,可追溯性不足以抵消成本。
- **不用 Mem0 类外部框架**(实测一般),自建贴合 kanzei 形态的最小机制。
- **子代理管理记忆是合理架构**:最重要的两件事是①记忆写入/演化/删除的**工具集**,②**什么时候触发**记忆管理。
- **agent 既是用户**:agent 的体验与效率本身就是记忆系统最好的校验——验收以自举轨迹的实证为准,不以基准分为准。

## 1. 记忆分级(scope × category)

物理上两个 scope,语义上五个 category,一条记忆 = 一个 markdown 文件 + frontmatter。

**scope(存放位置,决定共享范围)**
- `global` → `~/.kanzei/memory/`:跨项目生效。个人偏好、机器环境、通用习惯。
- `project` → `<项目>/.kanzei/memory/`:仅本项目。项目事实、架构决策、项目 SOP。

**category(frontmatter 字段,决定注入与检索策略)**

| category | 内容 | 典型例子 | 默认 scope |
| --- | --- | --- | --- |
| preference | 用户偏好与定调 | 不带署名;可用即关闭;一轮一个完整条目 | global |
| habit | 命令执行习惯/环境事实 | gh 要走 127.0.0.1:12000 代理;纯 ui/ 改动跑 node 检查不跑 cargo | global |
| fact | 项目事实/根因/坑 | CRLF 是 edit 未命中头号原因;NSIS 装在 AppData 与 cargo bin 是两个通道 | project |
| sop | 流程手册(playbook) | 发版 SOP;更新失败恢复 SOP;缺陷收口 SOP | project |
| episode | 情景摘要(轮次做了什么/学到什么/花费) | 引擎自动生成,人一般只读 | project |

现有 `.kanzei/project/memory.md` 的 M-条目迁移为 fact 文件;conventions.md 保持为用户手写规范不动(它是"宪法",记忆是"判例")。

## 2. 文件格式与引擎强制

```markdown
---
id: M-013
scope: project
category: fact
title: edit 未命中的头号原因是 CRLF 差异
description: 处理 edit 工具替换失败、换行符、\r\n 相关问题时必读
status: active          # active | stale
created: 2026-08-07
updated: 2026-08-08
hits: 14                # 引擎维护:被检索命中次数
source: run:ses_xxx/42  # 可溯源:哪次运行的哪一步写入
refs: R-070 D-200       # R-070 来源引用(可选):空格分隔的引用 ID 或项目内文件路径
---

(正文,自由 markdown)
```

硬门禁(复用 tracker 哲学:结构在写入侧强制,文档永远写不坏):
- ID 引擎分配,scope/category/status 枚举校验,description 必填(它是检索与触发的钩子);
- **refs 来源契约(R-070)**:memory_add/memory_note 的 `refs` 参数代码强制校验——`[RDAMGSF]-<数字>` 必须命中对应 doc 的活跃或归档条目,否则按相对文件路径必须真实存在于项目根;任一非法整体拒绝,不在提示词层面兜底(先例:tracker.rs check_refs)。frontmatter 宽容读,refs 存 `extras` 键,`MemoryEntry::refs()` 还原;
- 每 scope 一份引擎维护的 `INDEX.md`(一行一条:id/category/title/description),人可读,损坏可由文件重建;
- 完整性检测:INDEX ↔ 文件集合一致性、ID 缺号告警(同 D-112 门禁);
- 删除 = 归档:`stale` 后由整理流程移入 `memory-archive/`,带墓碑,绝不静默消失。

SQLite 只存**可重建的派生物**:FTS5 全文索引、hits 统计、episode 轨迹摘要表。真源始终是文件;库删了可全量重建。

## 3. 记忆管理子代理与工具集(核心)

独立子代理 `memory-manager`(fast 模型档,复用现有 SubagentRuntime),持有专用工具:

- `memory_add(scope, category, title, description, content)` — 写入前引擎做近似去重检查(同 category 标题/内容相似即返回候选,要求改为 update 或显式确认新增);
- `memory_update(id, patch)` — 演化:内容修订、description 调整、命中后的强化;
- `memory_merge(ids, merged)` — 合并重复,被并条目自动 stale 并在墓碑里链到新条目;
- `memory_stale(id, reason)` — 失效标记(被推翻/过期),reason 必填;
- `memory_search(query, scope?, category?, status?)` — FTS5 BM25 + 结构化过滤,结果按 相关度×新近度×hits 排序;
- `memory_stats()` — 各 scope/category 计数、体积、低命中候选、stale 候选。

主 agent(dev/dev-pair/research)只拿 `memory_search` + 只读索引注入;**写路径全部走 memory-manager**——主 agent 通过一个轻量 `memory_note(草稿)` 工具投递候选,由管理子代理决定 ADD/UPDATE/MERGE/NOOP。写读分离,避免主 agent 顺手写出垃圾记忆。

## 4. 触发策略(什么时候管理记忆)

| 触发点 | 动作 | 成本 |
| --- | --- | --- |
| 轮末收尾 | memory-manager 复盘本轮轨迹:生成 episode 摘要;对 note 草稿与轨迹中的"新知"做 ADD/UPDATE/NOOP | 每轮一次 fast 调用 |
| 条目关闭(defect fixed / req done) | 根因→fact 候选;重复出现的操作序列→sop 候选 | 随轮末合并 |
| 用户显式 | 「记住这个」指令 / UI 按钮 → 直接投递 note | 零 |
| 空闲整理(sleep-time) | 桌面端空闲时:合并重复、低命中降级提示、stale 归档、INDEX 重建校验 | 后台,可关 |
| 上下文溢出 | 先把被裁剪段压成 episode 再重置(与 D-088 联动),轨迹不再无声蒸发 | 溢出时一次 |
| 体积阈值 | 某 scope 条目数/体积超限 → 强制一轮整理 | 罕见 |

## 5. 注入与检索(上下文管理精准化)

- **常驻**:各 scope 的 INDEX 摘要(id+title+description 一行一条)进系统上下文,预算封顶、超限折叠计数——正文一律不常驻;
- **按需**:主 agent 用 memory_search 拉正文;sop 类按 description 与当前任务的匹配提示加载(「做发版相关任务时必读 M-021」);
- **上下文账单**:harness 逐 source 记录注入 token 数,落库可查——"本轮上下文里有什么、各占多少"成为数据,供 UI 展示与膨胀归因。

## 6. UI:独立 Memory 页(与设置同级)

- **动态架构图**:scope × category 网格卡片,实时显示条数/体积/最近写入/本轮注入 token;点击进入条目列表;
- **条目视图**:正文、来源轨迹链接(source 字段可跳转)、hits、stale 开关、直接编辑(写盘即生效);
- **上下文账单面板**:当前会话每个注入源的 token 占用,一眼定位膨胀源;
- **全局检索框**:memory_search 同一入口。

## 7. 分期与验收(agent 即用户:验收全部取自举实证)

- **R-104(M1 存储与检索)**:文件格式+两级 scope+引擎门禁+INDEX+FTS5+memory_search/stats;迁移现有 M-条目。验收:检索命中率在真实轨迹中可观测;INDEX 完整性门禁有回归;全部内容可 git 恢复。
- **R-105(M2 管理子代理与触发)**:memory-manager 子代理+全套写工具+note 投递+轮末/关闭/显式三个触发点。验收:连续自举轮次中出现"轮末写入→后续轮检索命中→避免重复探索"的完整闭环实证(轨迹为证);去重门禁拦下重复写入的用例。
- **R-106(M3 注入改造与账单)**:索引常驻+正文按需+sop 触发提示+逐 source token 计量落库。验收:同类任务每轮注入 token 较基线下降且无信息缺失导致的返工;账单数据可查询。
- **R-107(M4 UI 与空闲整理)**:Memory 页(架构图/条目/账单/检索)+sleep-time 整理+溢出压缩联动。验收:800/1024/1280 三档可用;整理动作全部有墓碑与日志,无静默删除。

## 8. 明确不做(决策记录)

向量嵌入检索、知识图谱、外部记忆框架(Mem0/Zep/Letta 直接接入)、参数化记忆。若未来重议,须新开设计文档引用本节说明变更理由。

## 8.5 判据层扩展(2026-08-09):决策充分性

写入/遗忘/合并/检索四个操作的判据从「语义显著度」升级为「决策价值」(反事实写入闸、subject 状态语义、复发检测、召回→采纳率排序),设计与边界拍板见 [memory_decision_sufficiency.md](memory_decision_sufficiency.md)。本文 §0 品味决策与存储形态不变。

## 9. 具体工程决策(2026-08-08 用户逐条拍板)

1. **代码落位**:`kanzei_tools::memory` 模块(mod/store/tools),kanzei-tools 加 rusqlite(bundled,锁内已有);不开新 crate。
2. **ID 与文件**:global 前缀 `U-`、project 沿用 `M-`,两序列独立;文件名 `<id>-<创建时slug>.md`,slug 终身不改;frontmatter 手写平铺 `key: value` 解析器(不引 serde_yaml),解析宽容、写入侧强制。
3. **episode 落位**:state.db 表(机器生成、量大、按会话关联查询,本质是日志);不做 md 文件。
4. **分词**:FTS5 unicode61 起步,靠自举轨迹实证决定是否升级 tokenizer;排序 = bm25 取 topN 后按 新近度×log(1+hits) 在 Rust 侧重排。
5. **安全模型**:不做重安全规则——个人开发者场景,可视化 + source 溯源 + 墓碑可逆就是安全模型;global 域写入不设确认门(避免踩 Claude Code 的易用性痛点)。
6. **并发**:不做跨进程文件锁;tmp+rename 原子替换 + 索引可重建 + 完整性门禁检测,竞争产生的冲突留给 agent 事后解决。
7. **工具面(M1)**:主 agent 挂 `memory_search`/`memory_note`(inbox 草稿投递)/`memory_stats`;写路径(add/update/merge/stale)留给 M2 的 memory-manager;去重门禁在引擎 add 内(FTS 近似命中即拒绝并返回候选,可 force)。
8. **注入(M1)**:INDEX 行(id+category+title+description)预算内常驻,替换现有 dev/memory source;"SOP 触发"不做魔法匹配,description 质量即触发器;M3 再加开跑时按 prompt 预检索的提示行。
