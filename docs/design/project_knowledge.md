# 项目知识、层级记忆与按需上下文

- 身份: live_design
- 日期: 2026-10-06
- 状态: 本次实现基线；验证记录见本文末尾。
- 上游: [memory_system.md](memory_system.md)、[memory_knowledge_graph.md](memory_knowledge_graph.md)、[architecture_diagrams.md](architecture_diagrams.md)

## 1. 产品目标

Kanzei 的记忆服务于项目生产力：定位修改位置、理解职责边界、追踪依赖影响、避免重复踩坑。项目结构组织经验；任务决定加载范围。用户无须手工决定聚类和层级。

架构、文件浏览、文件用途说明和记忆管理共用项目知识投影。可视化图和模型使用的上下文消费同一个区域注册表与关联规则，不能另建一套相互漂移的记忆分类。

层级知识按项目显式启用，默认关闭。新建项目对话框提供未勾选的初始化选项；已有项目可在架构页、记忆页或文件模块知识区开启。启用从当前代码和已有记忆立即生成层级投影，不额外调用模型；用途摘要仍由用户主动生成。关闭停止层级扫描和自动注入，已有记忆与标注继续保留，基础记忆检索仍可用。

开关保存为主项目 `.kanzei/project-knowledge.json`：`{"enabled":true}`。不继承全局或其他项目的开关，工作树共享主项目设置。缺少或损坏的配置按关闭处理。开关写入采用排他锁与原子替换。

## 2. 两种关系必须分开

包含关系组成树：项目 → 子系统/crate → 模块 → 更细模块。文件和符号是可定位的来源，只有任务需要时才展开。目录深度 `tree_depth` 表示这棵树的层级。

依赖组成有向图：使用者 → 被依赖模块。`dependency_depth` 是依赖深度，不是父子层级。环不会成为包含关系；上下游展开只走一跳，防止一个枢纽把全仓经验加载进来。

当前 Cargo 项目从真实清单提取 crate 依赖，Rust 模块从源码目录递归生成；普通项目从代码目录生成层级。普通项目尚未解析语言特定的 import/运行时依赖，界面和模型不将未知依赖补成事实。细粒度的接口契约和运行时边界由有来源的记忆记录补充。

## 3. 持久数据结构

继续以一条 Markdown 文件保存一条记忆；索引、层级和图是派生数据，不建立第二份记忆数据库。既有 id、分类、状态、来源契约保持兼容。

```yaml
id: M-123
scope: project
category: fact
title: 工作树工具必须读取当前代码树
description: 修改文件工具或执行上下文时，核对 cwd 与主项目资产根
status: active
source: run:session/episode
refs: D-267 crates/kanzei-tools/src/files.rs
area: kanzei-tools/files kanzei-core/runner
subject: contract:kanzei-tools/files:code-root
created: 2026-10-06
updated: 2026-10-06
```

正文应说明适用条件、采取的动作、证据和验证方法；记录一次失败的根因、项目约束或可重复的流程，不保存流水账作为经验。

| 字段 | 确定的语义 |
| --- | --- |
| `id` | 稳定身份，文件移动和标题修改不分配新记忆 |
| `area` | 可关联多个模块；显式关联全部保留，不受推断候选数量限制 |
| `refs` / `source` | 来源证据与相关项目资产；不是包含关系 |
| `subject` | 同一主题的演化键；职责用 `responsibility:<area>`，接口用 `contract:<area>:<interface>` |
| `category` | 兼容原存储：fact、sop、habit、preference；不新增重复分类字段 |
| `status` | candidate/shadow 尚待验证；active 可加载；deprecated/invalid 保留墓碑但不进入任务上下文 |

展示类型由现有字段确定：responsibility/contract 由 subject 前缀表示；preference 显示为 constraint；habit 为 environment；sop 为 procedure；带失败指纹的 fact 为 pitfall；其余为 fact。类型不是第二份真源。

项目级职责和约束没有强制区域；模块经验挂到相应区域。同一条跨模块经验只保存一次。合并记忆时引擎保留 area、refs 与失败指纹的并集。

## 4. 运行时投影 v1

`ProjectKnowledge` 由 `kanzei-tools/src/project_knowledge.rs` 生成：

```text
ProjectKnowledge
  version: 1
  enabled: 是否启用；关闭时不扫描层级，投影列表为空
  project_root: 主项目资产根
  code_root: 当前实际代码树根
  areas: KnowledgeArea[]
    id, label, kind, parent
    tree_depth, dependency_depth
    dependencies[], dependents[]
    memory_ids[]
    purposes[{path, text, provenance}]
  memories: KnowledgeMemory[]
    id, title, description, kind, status, source, path, updated
    revision: 源 Markdown 内容指纹
    links[{area, provenance, strength}]
    refs[], missing_areas[]
  unassigned: memory_id[]
  warnings[]
```

区域身份继续使用已有规范 id，兼容已有 area 字段。区域移动后旧关联进入 `missing_areas`，经验保留并待复核；不按相似名字静默迁移约束。

关联依据的优先级沿用 field → path → tool → via → keyword。field/path 为强关联，其余为推断。弱关联用于探索，不能把工具名或关键词猜测当成确认的模块职责。正文、来源、候选状态、关联依据在投影中保留。

## 5. 上下文加载契约

1. 启用的项目开跑只常驻项目第一层结构、有效经验计数、依赖方向和查询方式。无区域的有效项目约束另行常驻，遵守预算。关闭的项目不注入层级概览，显式请求 `architecture context` 返回开启入口提示。
2. `architecture {action:"context"}` 返回项目概览；传 `area`，可使用区域 id、文件路径或 Rust 路径。
3. 模块请求加载祖先区域、目标子树和祖先/目标的一跳上下游。不展开祖先的其他子模块，不递归走全图。上下游的细分模块只补职责、接口和约束，内部事实不全量展开；无区域的项目职责/接口/约束也在模块请求中加载。
4. 本模块强关联先于影响邻域；只加载 active 且不存在失效显式区域的记忆。用途说明先验指纹，正文先验 revision；读取期间被改动的旧快照不会注入新内容。
5. 预算折叠必须显式报数，过长记忆给可读取的文件指向；不把半段正文作为完整约束。结构和用途摘要仅使用部分预算，给经验正文留空间。
6. `files` 输出物理目录，同时补相关区域上下文；没有区域时退到项目概览。`memory_search` 保留按任务检索；任务提示独立提供召回钩子，不再指向已退役的平铺常驻索引。
7. `memory_note` 新增可选 `area`，在当前代码树校验、归一后写入草稿。管理器默认将所有项目经验写入 project，保留跨模块关系与出处。事实可引用相关的真实 tracker 或源码/设计文档；不必为了记录职责虚构一个缺陷，来源主题核验与候选验证仍适用。

主项目 `.kanzei` 提供记忆；工作树的 Cargo 清单、源码和文件内容提供结构与指纹。两根分别传递，避免在工作树修改代码却加载主树的过时用途说明。

## 6. 旧标注和文件浏览

`.kanzei/file-annotations.json` 继续作为可重建的用途摘要来源，接入项目知识，无须复制进正式记忆。文件摘要以正文指纹验证；目录摘要新增 `dir_hashes`，其直属内容和成员变化使摘要失效。旧目录摘要没有指纹时保留原记录，但不展示为当前知识。

生成只处理变化的文件和失效目录。生成目录摘要前后再次比较指纹；文件阶段结束后重新扫描，并要求所有可标注来源完整有效；文件生成结束后也重读核对正文。在途修改产生的旧结果不能绑定到新内容。停止请求同样作用于目录阶段。

三处 UI 共用 `24-project-knowledge.js`：记忆页按模块展开经验；架构页同时显示层级、用途和上下游；文件浏览显示当前文件所属模块的职责和经验。节点展开时才创建下一层 DOM。点击经验打开原始记忆详情，候选显示待复核。原目录树和编辑器仍承担定位与编辑。

## 7. 退役清单与迁移

- 退役平铺事实 INDEX 的常驻注入与 resident_index 实现。INDEX.md 文件仍供磁盘完整性与人工查阅，不删除历史文件。
- 退役记忆概览中的 scope×category 卡片及其专用样式，改为项目知识层级。分类仍可作为检索筛选条件。
- 移除 architecture_snapshot 的旧 graph 兼容字段和单独包装函数；依赖来自共享区域注册表，架构图继续用 crates/mermaid。
- 修正管理器 preference/habit 默认写入普通召回已停用的 global 库；项目约束、环境经验和流程默认全部进 project。显式全局历史库仍可查看，既有跨项目流程的显式写入保持兼容。
- 退役目录摘要永久有效的行为；用来源指纹决定是否展示。

现有 Markdown 不批量改写、不清空全局历史、不删除原始标注。迁移在读取投影时完成：旧路径/refs 可自动聚类，无法确定的内容保留未归类，缺失区域进入待复核。回滚源码不要求回滚 SQLite；新增 JSON 字段采用 serde default，旧版本可忽略。

## 8. 验证

定向测试覆盖层级与依赖区别、局部加载与兄弟模块排除、候选/孤立区域不注入、旧快照修订检查、预算折叠、工作树代码与主项目记忆分根、文件/目录摘要指纹和记忆合并的跨模块关系。

项目开关覆盖默认关闭、配置持久化、启用初始化、关闭后重启保留记忆；浏览器验证新建项目默认选项、草稿保留与三处页面的开关同步。

IPC 形状、既有 Rust workspace 回归和三处页面的浏览器交互验证在实现后执行；实际结果记录在 [本次验证报告](../reports/2026-10-06-project-knowledge.md)。浏览器预览使用隔离 IPC 夹具，不代表已安装桌面版本升级。
