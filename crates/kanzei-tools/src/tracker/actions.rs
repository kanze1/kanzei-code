//! action 分发域(R-204 从 tracker.rs 拆出):每个 action 一个独立函数,
//! `TrackerTool::execute` 只剩路由。校验方法仍挂在 `TrackerTool`(impl 在
//! tracker.rs),这里经 `tool` 参数调用;调度语义走 `super::scheduling`。

use crate::docstore::{DocStore, Entry};
use kanzei_harness::{ToolCtx, ToolOutput};

use super::scheduling::{deadlock_banner, dependency_states, schedule_entries, structured_entry};
use super::{TrackerInput, TrackerTool};

pub(crate) mod maintenance;

mod action_helpers;
use action_helpers::{
    archived_or_unknown, check_close_acceptance_reconciliation,
    check_close_classification_evidence, check_close_source_ancestry, field_diff_summary,
    render_line, unknown_id, user_visible_fields,
};

pub(crate) fn list(
    tool: &TrackerTool,
    _input: TrackerInput,
    ctx: &ToolCtx,
    _store: &DocStore,
    entries: &mut [Entry],
) -> ToolOutput {
    if entries.is_empty() {
        return ToolOutput::ok(
            serde_json::json!({
                "schema_version": 1,
                "kind": tool.noun,
                "deadlocked": false,
                "entries": [],
            })
            .to_string(),
        );
    }
    let dependency_states = match dependency_states(ctx, tool.kind, entries) {
        Ok(states) => states,
        Err(e) => return ToolOutput::error(format!("cannot read scheduler dependencies: {e}")),
    };
    let scheduled = schedule_entries(entries, &dependency_states);
    let items: Vec<serde_json::Value> = scheduled
        .iter()
        .map(|(entry, reasons)| structured_entry(entry, reasons, false))
        .collect();
    // 饥饿保护:一条可执行都没有是队列的异常状态,不是"没活干"。不加这条横幅时
    // agent 只看到满屏 [blocked:...] 就会判定无可推进项并停住,而阻塞理由多半是
    // 它自己历轮写下的"需先确认方案"(D-163)。
    let deadlocked = scheduled.iter().all(|(_, reasons)| !reasons.is_empty());
    ToolOutput::ok(
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "kind": tool.noun,
            "deadlocked": deadlocked,
            "deadlock_guidance": deadlocked.then(|| deadlock_banner(scheduled.len(), tool.noun)),
            "entries": items,
        }))
        .unwrap(),
    )
}

pub(crate) fn get(
    _tool: &TrackerTool,
    input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    entries: &mut [Entry],
) -> ToolOutput {
    let Some(id) = &input.id else {
        return ToolOutput::error("`id` is required for get");
    };
    match entries.iter().find(|e| &e.id == id) {
        Some(e) => {
            ToolOutput::ok(serde_json::to_string_pretty(&structured_entry(e, &[], false)).unwrap())
        }
        // 已归档条目仍可读:回落到 archive 文件(只读,不可 update)。
        None => match store
            .load_archive()
            .ok()
            .and_then(|arch| arch.into_iter().find(|e| &e.id == id))
        {
            Some(e) => ToolOutput::ok(
                serde_json::to_string_pretty(&structured_entry(&e, &[], true)).unwrap(),
            ),
            None => ToolOutput::error(unknown_id(id, entries)),
        },
    }
}

pub(crate) fn raw_lines(
    tool: &TrackerTool,
    input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    entries: &mut [Entry],
) -> ToolOutput {
    let Some(id) = &input.id else {
        return ToolOutput::error("`id` is required for raw_lines");
    };
    if !entries.iter().any(|e| &e.id == id) {
        return ToolOutput::error(unknown_id(id, entries));
    }
    let raws = store.raw_lines(id);
    if raws.is_empty() {
        return ToolOutput::ok(format!("{id} 没有游离行(条目内每一行都是可寻址字段)"));
    }
    let rendered = raws
        .iter()
        .map(|raw| {
            let body = if raw.text.trim().is_empty() {
                "(空行)".to_string()
            } else {
                raw.text.clone()
            };
            format!("[{:>2}] {}", raw.ordinal, body)
        })
        .collect::<Vec<_>>()
        .join("\n");
    ToolOutput::ok(format!(
        "{id} 的游离行共 {} 条(历史多行写法/手改留下的不可寻址内容,update 删不到)。\
         用 `{tool} raw_delete id={id} ordinal=<n>` 按序号删除单条:\n{rendered}",
        raws.len(),
        tool = tool.tool_name,
    ))
}

pub(crate) fn repair_reused_id(
    tool: &TrackerTool,
    input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    _entries: &mut Vec<Entry>,
) -> ToolOutput {
    let Some(id) = &input.id else {
        return ToolOutput::error("`id` is required for repair_reused_id");
    };
    match store.repair_reused_archived_id(id) {
        Ok(new_id) => ToolOutput::ok(format!(
            "repaired reused ID: archived {id} → {new_id}; active {id} kept unchanged. Commit `{}` and its archive file together.",
            tool.kind.rel_path,
        )),
        Err(e) => ToolOutput::error(format!("repair_reused_id failed: {e}")),
    }
}

// 补回从 git 历史里捞回来的条目:只允许补真空洞,并插回原编号位置。
pub(crate) fn repair_missing_id(
    tool: &TrackerTool,
    input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    _entries: &mut Vec<Entry>,
) -> ToolOutput {
    let Some(id) = &input.id else {
        return ToolOutput::error("`id` is required for repair_missing_id");
    };
    let Some(title) = input.title.as_deref().filter(|t| !t.trim().is_empty()) else {
        return ToolOutput::error(
            "`title` is required for repair_missing_id — restore the entry's real \
             title from git history, do not invent one",
        );
    };
    if let Some(title_err) = tool.check_title(title) {
        return ToolOutput::error(title_err);
    }
    if let Some(sev_err) = tool.check_severity(&input.severity) {
        return ToolOutput::error(sev_err);
    }
    if let Some(priority_err) = tool.check_priority(&input.priority) {
        return ToolOutput::error(priority_err);
    }
    if let Some(tag_err) = tool.check_tag(&input.fields) {
        return ToolOutput::error(tag_err);
    }
    if let Some(complexity_err) = tool.check_complexity(&input.fields) {
        return ToolOutput::error(complexity_err);
    }
    let mut fields: Vec<(String, String)> = input.fields.into_iter().collect();
    if !input.refs.is_empty() {
        fields.push(("refs".into(), input.refs.join(" ")));
    }
    // D-330:priority 参数与 fields 里「优先级」键去重——调用方可能同时传两者,
    // 直接 push 会双写同名字段(值相同冗余、值不同语义歧义)。语义与 update 分支
    // 一致:已存在(中文键或大小写不敏感的 priority)则覆盖,否则追加。
    if let Some(priority) = input.priority {
        match fields
            .iter_mut()
            .find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority"))
        {
            Some((_, value)) => *value = priority,
            None => fields.push(("优先级".into(), priority)),
        }
    }
    let status = input
        .status
        .clone()
        .unwrap_or_else(|| tool.kind.statuses[0].to_string());
    if !tool.kind.statuses.contains(&status.as_str()) {
        return ToolOutput::error(format!(
            "unknown status `{status}`; valid: {}",
            tool.kind.statuses.join(" | ")
        ));
    }
    let entry = Entry {
        id: id.clone(),
        title: title.trim().to_string(),
        status,
        severity: if tool.kind.severities.is_some() {
            input.severity
        } else {
            None
        },
        fields,
    };
    match store.restore_entry(entry) {
        Ok(()) => ToolOutput::ok(format!(
            "restored {id} into {} at its original position. Verify it against the \
             git-history version you recovered before committing.",
            tool.kind.rel_path,
        )),
        Err(e) => ToolOutput::error(format!("repair_missing_id failed: {e}")),
    }
}

pub(crate) fn add(
    tool: &TrackerTool,
    input: TrackerInput,
    ctx: &ToolCtx,
    store: &DocStore,
    entries: &mut Vec<Entry>,
) -> ToolOutput {
    let Some(title) = input.title.as_deref().filter(|t| !t.trim().is_empty()) else {
        return ToolOutput::error("`title` is required for add");
    };
    if let Some(title_err) = tool.check_title(title) {
        return ToolOutput::error(title_err);
    }
    // R-191:登记硬约束(缺必填字段即拒,提示补什么)。
    if let Some(required_err) = tool.check_add_required(&input) {
        return ToolOutput::error(required_err);
    }
    if let Some(sev_err) = tool.check_severity(&input.severity) {
        return ToolOutput::error(sev_err);
    }
    if let Some(priority_err) = tool.check_priority(&input.priority) {
        return ToolOutput::error(priority_err);
    }
    if let Some(tag_err) = tool.check_tag(&input.fields) {
        return ToolOutput::error(tag_err);
    }
    if let Some(complexity_err) = tool.check_complexity(&input.fields) {
        return ToolOutput::error(complexity_err);
    }
    // 新建没有既有批次值:严格按上限约束。
    if let Some(batch_err) = tool.check_batches(&input.fields, None) {
        return ToolOutput::error(batch_err);
    }
    if let Err(e) = tool.check_refs(ctx, &input.refs, true, input.topic.as_deref()) {
        return ToolOutput::error(e);
    }
    let id = store.next_id(entries);
    let mut fields: Vec<(String, String)> = input.fields.into_iter().collect();
    if !input.refs.is_empty() {
        fields.push(("refs".into(), input.refs.join(" ")));
    }
    // D-330:priority 参数与 fields 里「优先级」键去重——调用方可能同时传两者,
    // 直接 push 会双写同名字段(值相同冗余、值不同语义歧义)。语义与 update 分支
    // 一致:已存在(中文键或大小写不敏感的 priority)则覆盖,否则追加。
    if let Some(priority) = input.priority {
        match fields
            .iter_mut()
            .find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority"))
        {
            Some((_, value)) => *value = priority,
            None => fields.push(("优先级".into(), priority)),
        }
    }
    let severity = input
        .severity
        .or_else(|| tool.kind.severities.map(|s| s[s.len() / 2].to_string()));
    entries.push(Entry {
        id: id.clone(),
        title: title.trim().to_string(),
        status: tool.kind.statuses[0].to_string(),
        severity: if tool.kind.severities.is_some() {
            severity
        } else {
            None
        },
        fields,
    });
    if let Err(e) = store.save(entries) {
        return ToolOutput::error(format!("cannot write {}: {e}", store.path.display()));
    }
    ToolOutput::ok(format!("added {id} [{}] {title}", tool.kind.statuses[0]))
}

pub(crate) fn update_close(
    tool: &TrackerTool,
    input: TrackerInput,
    ctx: &ToolCtx,
    store: &DocStore,
    entries: &mut [Entry],
) -> ToolOutput {
    let Some(id) = &input.id else {
        return ToolOutput::error("`id` is required");
    };
    let Some(pos) = entries.iter().position(|e| &e.id == id) else {
        return ToolOutput::error(archived_or_unknown(id, entries, store, tool.tool_name));
    };
    if let Some(sev_err) = tool.check_severity(&input.severity) {
        return ToolOutput::error(sev_err);
    }
    if let Some(priority_err) = tool.check_priority(&input.priority) {
        return ToolOutput::error(priority_err);
    }
    if let Some(tag_err) = tool.check_tag(&input.fields) {
        return ToolOutput::error(tag_err);
    }
    if let Some(complexity_err) = tool.check_complexity(&input.fields) {
        return ToolOutput::error(complexity_err);
    }
    // 以条目现有的批次总数为基准:存量 >10 的条目照常逐批推进,只拦抬高。
    if let Some(batch_err) = tool.check_batches(&input.fields, Some(&entries[pos])) {
        return ToolOutput::error(batch_err);
    }
    if let Err(e) = tool.check_refs(ctx, &input.refs, false, input.topic.as_deref()) {
        return ToolOutput::error(e);
    }
    let updates_progress = input.fields.contains_key("进展");
    // R-232:幂等化需要"变更前"快照。锚点字段(recorded_at/observed_head/
    // observed_worktree_hash)是引擎维护的仓库指纹,不属于用户可见变更,
    // 同值 update 连锚点都不刷新(文件零写入,验收①)。
    let before = entries[pos].clone();
    // R-232 验收③:close 幂等重入——已终态(done/fixed)条目再次 close
    // 不是新关闭,不重跑关闭门禁(前端冒烟/分类断言/批次/测试记录校验),
    // 目标仍是当前终态;字段合并照常,让"补字段的重入"可写。
    let already_terminal = tool.kind.terminal.contains(&entries[pos].status.as_str());
    // R-228 关闭门禁:带「前端」标签的条目关闭前必须已有前端冒烟 passed
    // 测试记录(verify.ps1 六步前端 smoke 的一部分)。cargo test 全绿不等于
    // 全量——前端标签任务可能改了 ui/*.js,只跑 Rust 测试发现不了 i18n 缺
    // key、smoke 断言过时(D-320 根因)。非前端标签条目不受影响(验收③)。
    // R-232:终态重入跳过——已关闭条目的再次 close 不是新关闭,不再要求冒烟。
    let action = input.action.clone();
    if action == "close" && !already_terminal {
        if let Some(ancestry_err) = check_close_source_ancestry(&entries[pos], &ctx.cwd) {
            return ToolOutput::error(format!("{id} {ancestry_err}"));
        }
    }
    if action == "close" && !already_terminal {
        let tag = entries[pos]
            .fields
            .iter()
            .find(|(k, _)| k == "标签")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        if tag.contains("前端")
            && crate::test_record::frontend_smoke_passed(&ctx.project_root).is_none()
        {
            return ToolOutput::error(format!(
                "{id} 带「前端」标签,但没有任何前端冒烟 passed 测试记录,不能关闭。\
                 前端标签任务关闭前必须跑过 ui smoke(node scripts/ui-runtime-smoke.mjs / \
                 ui-i18n-smoke.mjs / ui-lint-smoke.mjs 等)并用 test_record 记 passed;\
                 cargo test --workspace 全绿不等于前端全量。"
            ));
        }
    }
    let target_status = if action == "close" {
        // R-232 验收③:close 幂等重入——已终态条目再次 close 不是新关闭,
        // 跳过批次/断言/测试记录等关闭校验,目标仍是当前终态。
        // 字段合并照常(重入可补字段),无变更时下方 no-op 判定会零写入返回。
        if already_terminal {
            Some(entries[pos].status.clone())
        } else {
            // 批次没走完不能关:格子是给人看进度的,关闭时还剩空格,要么是漏了批次,
            // 要么是总数当初估多了——两种都要说清楚,不能默默把空格留在那儿。
            let merged = {
                let mut probe = entries[pos].clone();
                for (key, value) in &input.fields {
                    match probe.fields.iter_mut().find(|(k, _)| k == key) {
                        Some((_, slot)) => *slot = value.clone(),
                        None => probe.fields.push((key.clone(), value.clone())),
                    }
                }
                probe
            };
            // R-229 关闭门禁:出现「剩余/其余 N 处」式分类断言时,关闭文本必须
            // 逐处带 file:line 引证,引证数不足断言声称的总处数即拒关闭。
            // (根因:R-199 关闭证据把完整否决误归为「非续跑否决」且无人核对,
            // 产出 D-320/D-323;无分类断言的关闭不受影响。)
            if let Some(evidence_err) = check_close_classification_evidence(&merged) {
                return ToolOutput::error(format!("{id} {evidence_err}"));
            }
            // 2026-08-16 审计门禁:验收条款对账——带圈条款号必须在进展中逐条覆盖
            // 并带证据锚,沉默降级即拒(详见函数注释;真伪由波次审计另查)。
            if let Some(reconcile_err) = check_close_acceptance_reconciliation(&merged) {
                return ToolOutput::error(format!("{id} {reconcile_err}"));
            }
            let derived_done = crate::git_batches::completed_batches(&ctx.project_root, id)
                .ok()
                .filter(|done| *done > 0);
            if let (Some((declared_done, declared_total)), Some(derived_done)) = (
                crate::docstore::declared_batch_progress(&merged),
                derived_done,
            ) {
                if declared_done != derived_done {
                    return ToolOutput::error(format!(
                        "{id} 的手写批次是 {declared_done}/{declared_total},但 Git 提交历史标记数为 {derived_done};请先核对并更新批次字段后再关闭。"
                    ));
                }
            }
            let (done, total) =
                crate::docstore::batch_progress_with_derived_done(&merged, derived_done);
            if total > 1 && done < total {
                return ToolOutput::error(format!(
                    "{id} 批次未走完({done}/{total}),不能关闭。真做完了就把总数改成实际批数\
                     (`批次: {done}/{done}`——当初估多了是正常的,改它比留着空格诚实);\
                     还有批次没做就先做完再关。"
                ));
            }
            // 关闭前必须先收尾该条目名下的测试记录:一条挂着的 running 与"根本没跑"
            // 无法区分,带着它关闭等于把未验证当成已验证入账。
            let unclosed = crate::test_record::unclosed_running_for(&ctx.project_root, id);
            if !unclosed.is_empty() {
                return ToolOutput::error(format!(
                    "{id} 名下还有 {} 条 running 测试记录没收尾,不能关闭:\n{}\n\
                     跑完就用 test_record 带上对应 id 记终态(passed/failed/skipped);\
                     确实不跑了记 skipped 并写明原因。",
                    unclosed.len(),
                    unclosed
                        .iter()
                        .map(|(rid, title)| format!("  - {rid} {title}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
            let status = input
                .status
                .clone()
                .unwrap_or_else(|| tool.kind.terminal[0].to_string());
            if !tool.kind.terminal.contains(&status.as_str()) {
                return ToolOutput::error(format!(
                    "close target must be terminal: {}",
                    tool.kind.terminal.join(" | ")
                ));
            }
            Some(status)
        }
    } else {
        input.status.clone()
    };
    let entry = &mut entries[pos];
    if let Some(status) = target_status {
        // R-252 验收④:想法转 split 的 refs 硬门禁——refs 必须非空且每个 ID 在
        // requirements/defects 活跃或归档真实存在。refs 可能走顶层参数或 fields
        // 键,两处合并后一并校验;非想法线(prefix != I)不触发。
        if status == "split" {
            let mut refs = input.refs.clone();
            if let Some(raw) = input.fields.get("refs") {
                refs.extend(
                    raw.split([' ', ','])
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                );
            }
            if let Err(e) = tool.check_idea_split_gate(ctx, &refs) {
                return ToolOutput::error(e);
            }
        }
        if let Err(e) = store.transition_allowed(&entry.status, &status) {
            return ToolOutput::error(e);
        }
        entry.status = status;
    }
    if let Some(title) = input.title.filter(|t| !t.trim().is_empty()) {
        if let Some(title_err) = tool.check_title(&title) {
            return ToolOutput::error(title_err);
        }
        entry.title = title.trim().to_string();
    }
    if input.severity.is_some() && tool.kind.severities.is_some() {
        entry.severity = input.severity;
    }
    if let Some(priority) = input.priority {
        match entry
            .fields
            .iter_mut()
            .find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority"))
        {
            Some((_, value)) => *value = priority,
            None => entry.fields.push(("优先级".into(), priority)),
        }
    }
    for (key, value) in input.fields {
        match entry.fields.iter_mut().find(|(k, _)| *k == key) {
            Some((_, slot)) => *slot = value,
            None => entry.fields.push((key, value)),
        }
    }
    // 每次落「进展」都同时保存仓库锚点。后续取活会机械比较 HEAD /
    // worktree 指纹，把历史叙事标为 current/stale/future/unanchored，
    // 不再让一段未对齐当前代码的文字冒充事实。
    if updates_progress {
        for (key, value) in crate::work::progress_anchor_fields(&ctx.cwd) {
            match entry
                .fields
                .iter_mut()
                .find(|(candidate, _)| *candidate == key)
            {
                Some((_, slot)) => *slot = value,
                None => entry.fields.push((key, value)),
            }
        }
    }
    if !input.refs.is_empty() {
        let joined = input.refs.join(" ");
        match entry.fields.iter_mut().find(|(k, _)| k == "refs") {
            Some((_, slot)) => *slot = joined,
            None => entry.fields.push(("refs".into(), joined)),
        }
    }
    // R-232 幂等判定:用户可见变更(忽略引擎锚点)不存在 → no-op,零写入。
    // 锚点字段(recorded_at/observed_head/observed_worktree_hash)是仓库指纹,
    // 由「进展」落笔时引擎维护;同值 update 不刷新它们,文件字节保持原样。
    let before_visible = user_visible_fields(&before);
    let after_visible = user_visible_fields(&entries[pos]);
    if before_visible == after_visible {
        return ToolOutput::ok(format!("no-op: {id} 字段已是该值,未写入(旧→新无差异)。"));
    }
    // 有变更:返回 旧→新 摘要,再落盘。
    let diff_summary = field_diff_summary(&before_visible, &after_visible);
    let line = render_line(&entries[pos]);
    if let Err(e) = store.save(entries) {
        return ToolOutput::error(format!("cannot write {}: {e}", store.path.display()));
    }
    // D-276 修复方向③:update 后自检游离段落并告警。push_field(D-294)
    // 保证本次写入不新增游离段落,但历史多行/手改残留仍在字段体系外、
    // update 触及不到——返回里点名并指路 raw_lines/raw_delete,否则
    // 残留段落会一直藏到有人用 git 手工翻。
    let raws = store.raw_lines(id);
    if raws.is_empty() {
        ToolOutput::ok(format!("updated: {line}\n变更: {diff_summary}"))
    } else {
        ToolOutput::ok(format!(
            "updated: {line}\n变更: {diff_summary}\n⚠ {id} 仍携带 {} 条不可寻址的游离段落(历史多行写法/手改残留,本次 update 不新增也不清除)。\
             用 `{tool} raw_lines id={id}` 查看、`{tool} raw_delete id={id} ordinal=<n>` 按序号清理。",
            raws.len(),
            tool = tool.tool_name
        ))
    }
}

// R-054:整表重排(文件顺序 = 开发顺序)。要求 order 是现有条目的完整置换,
// 缺一多一都拒绝——引擎整读整写,天然与并发的状态更新互斥。
pub(crate) fn reorder(
    tool: &TrackerTool,
    input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    entries: &mut Vec<Entry>,
) -> ToolOutput {
    if input.order.is_empty() {
        return ToolOutput::error("`order` (complete id list) is required for reorder");
    }
    let mut seen = std::collections::HashSet::new();
    for id in &input.order {
        if !seen.insert(id.as_str()) {
            return ToolOutput::error(format!("duplicate id `{id}` in order"));
        }
        if !entries.iter().any(|e| &e.id == id) {
            return ToolOutput::error(unknown_id(id, entries));
        }
    }
    if input.order.len() != entries.len() {
        let missing: Vec<&str> = entries
            .iter()
            .filter(|e| !input.order.iter().any(|id| id == &e.id))
            .map(|e| e.id.as_str())
            .collect();
        return ToolOutput::error(format!(
            "order must cover ALL {} entries; missing: {}",
            entries.len(),
            missing.join(", ")
        ));
    }
    let mut reordered = Vec::with_capacity(entries.len());
    for id in &input.order {
        if let Some(pos) = entries.iter().position(|e| &e.id == id) {
            reordered.push(entries.remove(pos));
        }
    }
    if let Err(e) = store.save(&reordered) {
        return ToolOutput::error(format!("cannot write {}: {e}", store.path.display()));
    }
    ToolOutput::ok(format!(
        "reordered {} {}s: {}",
        reordered.len(),
        tool.noun,
        input.order.join(" → ")
    ))
}

// D-332 验收②:统一 repair surface——把散落在 fix_terminal / 手改 markdown /
// raw_delete 之间的修复动作收敛成一个机械、幂等、dry-run-first 的入口。
// 扫描活动 + 归档区,报告/修复:
//   ① invalid lifecycle(非空但不在合法枚举)——报告,apply 不自动猜(缺语义);
//   ② duplicate fields(同 key 多次出现,key 大小写不敏感)——apply 保留首条;
//   ③ 标题状态标记污染(title_status_marker 命中)——apply 剥离;
//   ④ 活动区出现终态 / 归档区出现非终态——报告,提示用 close/archive/reopen。
// dry-run 默认:只报告不写入;apply=true 才落盘。幂等:重复 apply 无新变化。
pub(crate) fn normalize(
    tool: &TrackerTool,
    input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    entries: &mut [Entry],
) -> ToolOutput {
    let apply = input.apply;
    let mut findings: Vec<String> = Vec::new();
    let mut fixed: Vec<String> = Vec::new();
    let mut touched = false;

    let mut normalize_entry = |entry: &mut Entry, region: &str| {
        let entry_id = entry.id.clone();
        // ④ 活动区终态:应归档(只报告,close 后由 archive 动作执行)
        if region == "active" && tool.kind.terminal.contains(&entry.status.as_str()) {
            findings.push(format!(
                "{region} {entry_id}: terminal lifecycle `{}` — close 后应 archive",
                entry.status
            ));
            return;
        }
        // ① invalid lifecycle:dry-run 报告;apply 且调用方显式传 status=合法值时
        // 机械落盘(不猜语义——非法状态该变什么只有人知道,normalize 只做写入)。
        if !entry.status.is_empty() && !tool.kind.statuses.contains(&entry.status.as_str()) {
            if apply {
                if let Some(target) = input
                    .status
                    .as_deref()
                    .filter(|s| tool.kind.statuses.contains(s))
                {
                    let old = entry.status.clone();
                    findings.push(format!(
                        "{region} {entry_id}: invalid lifecycle `{old}` → fixed to `{target}`"
                    ));
                    entry.status = target.to_string();
                    touched = true;
                    fixed.push(format!(
                        "{region} {entry_id}: lifecycle `{old}` → `{target}`"
                    ));
                } else {
                    findings.push(format!(
                        "{region} {entry_id}: invalid lifecycle `{status}`; valid: {valid} \
                         (apply 需要 status=合法值才写入)",
                        status = entry.status,
                        valid = tool.kind.statuses.join(" | "),
                    ));
                }
            } else {
                findings.push(format!(
                    "{region} {entry_id}: invalid lifecycle `{status}`; valid: {valid} \
                     (apply 需要 status=合法值才写入)",
                    status = entry.status,
                    valid = tool.kind.statuses.join(" | "),
                ));
            }
        }
        // ② duplicate fields(key 大小写不敏感,如「优先级」vs「priority」)
        let mut seen: Vec<String> = Vec::new();
        let mut dropped = 0usize;
        entry.fields.retain(|(key, _)| {
            let norm = key.trim().to_ascii_lowercase();
            if seen.contains(&norm) {
                if dropped == 0 {
                    findings.push(format!(
                        "{region} {entry_id}: duplicate field `{key}` (kept first, dropped rest)"
                    ));
                }
                dropped += 1;
                false
            } else {
                seen.push(norm);
                true
            }
        });
        if dropped > 0 {
            touched = true;
            fixed.push(format!(
                "{region} {entry_id}: deduplicated {dropped} field(s)"
            ));
        }
        // ③ 标题状态标记污染(D-331 口径:状态的家是 header,不是标题)
        if let Some(marker) = crate::docstore::title_status_marker(&entry.title) {
            let stripped = crate::docstore::strip_status_markers(&entry.title);
            findings.push(format!(
                "{region} {entry_id}: title status marker `[{marker}]` → will strip to `{stripped}`"
            ));
            if apply {
                entry.title = stripped;
                touched = true;
                fixed.push(format!(
                    "{region} {entry_id}: stripped title status marker [{marker}]",
                ));
            }
        }
    };

    for entry in entries.iter_mut() {
        normalize_entry(entry, "active");
    }

    // 归档区:非终态 = mismatch(终态是归档的合法形态,不动)
    // 归档区只**报告**②③(重复字段/标题标记)——归档写通道不公开整表
    // 保存(走 archive_terminal/fix_terminal),apply 不动归档,避免制造
    // 第二套写路径;findings 里给出可执行的处置。
    let archived = match store.load_archive() {
        Ok(a) => a,
        Err(e) => return ToolOutput::error(format!("cannot read archive: {e}")),
    };
    for entry in &archived {
        if !tool.kind.terminal.contains(&entry.status.as_str()) {
            findings.push(format!(
                "archived {}: non-terminal lifecycle `{}` — 应终态(reopen/fix_terminal)",
                entry.id, entry.status
            ));
        }
        if !entry.status.is_empty() && !tool.kind.statuses.contains(&entry.status.as_str()) {
            findings.push(format!(
                "archived {}: invalid lifecycle `{}` — fix_terminal 纠错",
                entry.id, entry.status
            ));
        }
        if let Some(marker) = crate::docstore::title_status_marker(&entry.title) {
            findings.push(format!(
                "archived {}: title status marker `[{marker}]` — fix_terminal 纠错通道处理",
                entry.id
            ));
        }
        let mut seen: Vec<String> = Vec::new();
        for (key, _) in &entry.fields {
            let norm = key.trim().to_ascii_lowercase();
            if seen.contains(&norm) {
                // D-358:这句原本写「需手动整理归档」——那是 apply 还不会去重时留下的
                // 文案,能力补上后没跟着改。上一轮就是照这句话把 D-333 验收③判成
                // 不可修、挂了个「解除人=用户」的阻塞,而实际上一条 apply 就修完了。
                // 「进展」按内容合并不丢字,其余字段只保首个非空——两条内容不同的
                // 同名字段(如 D-180 的两条「验证」)会丢掉后一条,故一并写明。
                findings.push(format!(
                    "archived {}: duplicate field `{key}` — apply 可自动收敛\
                     (进展合并内容,其余保留首个非空:同名字段内容不同则后者丢弃)",
                    entry.id
                ));
                break;
            }
            seen.push(norm);
        }
    }

    // D-358:写盘必须发生在拼输出**之前**。原来这一段在 content 拼好之后才跑,
    // 归档去重 push 进 fixed 的条目一条也进不了输出——实测修了 6 条却报「0 fix(es)」、
    // 连「已修复」段都没有。工具少报自己的工作,在证据驱动的流程里等于说谎。
    if apply {
        // 活动区写回(若活动区有改动)
        if touched {
            if let Err(e) = store.save(entries) {
                return ToolOutput::error(format!("cannot write {}: {e}", store.path.display()));
            }
        }
        // 归档区修复:D-333 验收③——重复字段经 dedupe_archived_fields
        // 收敛(进展合并内容,其余保留首条),与 correct_archived_terminal
        // 共用锁与写路径,不制造第二套整表写 API。
        for entry in &archived {
            match store.dedupe_archived_fields(&entry.id) {
                Ok((true, removed)) => fixed.push(format!(
                    "archived {}: deduplicated {removed} field(s)",
                    entry.id
                )),
                Ok((false, _)) => {}
                Err(e) => {
                    return ToolOutput::error(format!("cannot dedupe archived {}: {e}", entry.id))
                }
            }
        }
    }

    let header = format!(
        "normalize {} ({}): {} finding(s), {} fix(es)",
        tool.kind.rel_path,
        if apply { "apply" } else { "dry-run" },
        findings.len(),
        fixed.len()
    );
    let body = if findings.is_empty() {
        "  无待修项(clean)".to_string()
    } else {
        findings
            .iter()
            .map(|f| format!("  - {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut content = format!("{header}\n{body}");
    if !fixed.is_empty() {
        content.push_str(&format!(
            "\n  已修复:\n{}",
            fixed
                .iter()
                .map(|f| format!("    - {f}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if !apply {
        content.push_str("\n  本次为 dry-run,未写盘。加 apply=true 执行修复。");
    }
    ToolOutput::ok(content)
}
