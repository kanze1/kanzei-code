//! Tracker normalize action: activity/archive ledger reconciliation.
//!
//! This action is kept separate from the routing module so the action family remains
//! below the metrics growth guard. The behavior and output contract are unchanged.

use crate::docstore::{DocStore, Entry};
use kanzei_harness::{ToolCtx, ToolOutput};

use super::super::{TrackerInput, TrackerTool};

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
        // ② duplicate fields(key 大小写不敏感,如「优先级」vs「priority」)。
        // 保留字段「状态」暂不参加通用去重:它们必须全部被下面的状态对账逻辑
        // 消费,否则第二个旧状态会在历史证据生成前被静默丢掉。
        let mut seen: Vec<String> = Vec::new();
        let mut dropped = 0usize;
        entry.fields.retain(|(key, _)| {
            let reserved_status = key.eq_ignore_ascii_case("status") || key == "状态";
            if reserved_status {
                return true;
            }
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
        // D-713:生命周期状态的唯一真源是标题 header 的 Entry.status。旧式 tracker
        // 曾把状态再写进正文 `状态`/`status` 字段,导致调度与页面可能各读一份。无论
        // 值是否与 header 相同,apply 都移除正文副本;不一致时把旧值写进进展，保留
        // 可审计历史而不再留下会被解析成当前状态的字段。
        let mut reserved_status_values = Vec::new();
        entry.fields.retain(|(key, value)| {
            let reserved = key.eq_ignore_ascii_case("status") || key == "状态";
            if reserved {
                let value = value.trim();
                if !value.is_empty() {
                    reserved_status_values.push(value.to_string());
                }
                false
            } else {
                true
            }
        });
        if !reserved_status_values.is_empty() {
            let old = reserved_status_values.join("、");
            let differs = reserved_status_values
                .iter()
                .any(|value| value != entry.status.trim());
            let finding = if differs {
                format!(
                    "{region} {entry_id}: reserved status field(s) `{old}` conflict with header `{}`; apply 将移除并写入进展",
                    entry.status
                )
            } else {
                format!(
                    "{region} {entry_id}: redundant reserved status field(s) `{old}`; apply 将移除并写入进展"
                )
            };
            findings.push(finding);
            if apply {
                let note = if differs {
                    format!(
                        "状态对账: 正文旧字段 `{old}` 与权威标题状态 `{}` 冲突;已移除正文副本。",
                        entry.status
                    )
                } else {
                    format!(
                        "状态对账: 正文旧字段 `{old}` 与权威标题状态 `{}` 重复;已移除正文副本。",
                        entry.status
                    )
                };
                match entry.fields.iter_mut().find(|(key, _)| key == "进展") {
                    Some((_, progress)) => {
                        if !progress.is_empty() {
                            progress.push('；');
                        }
                        progress.push_str(&note);
                    }
                    None => entry.fields.push(("进展".into(), note)),
                }
                touched = true;
                fixed.push(format!(
                    "{region} {entry_id}: removed {} reserved status field(s) and recorded history",
                    reserved_status_values.len()
                ));
            }
        }
        // 空的保留字段也由上面的统一逻辑移除;空值没有可恢复语义,不写入进展。
        // 机制产物字段:引擎每轮重算的东西被腌进了条目。`取活依据` 是典型——
        // structured_entry 会把条目的**全部** fields 序列化进控制状态,于是这行必然
        // 过期的快照跟着条目进每一次 work next 与文档快照,而调度器本来就每轮重算。
        // 不写进展:它没有可审计价值,留一句"曾经写过"只是把噪音换个地方放。
        let engine_fields: Vec<String> = entry
            .fields
            .iter()
            .filter(|(key, _)| {
                crate::docstore::DocStore::ENGINE_DERIVED_FIELDS.contains(&key.trim())
            })
            .map(|(key, _)| key.clone())
            .collect();
        if !engine_fields.is_empty() {
            findings.push(format!(
                "{region} {entry_id}: engine-derived field(s) `{}` — 引擎每轮重算,apply 将移除",
                engine_fields.join("、")
            ));
            if apply {
                entry.fields.retain(|(key, _)| {
                    !crate::docstore::DocStore::ENGINE_DERIVED_FIELDS.contains(&key.trim())
                });
                touched = true;
                fixed.push(format!(
                    "{region} {entry_id}: removed {} engine-derived field(s)",
                    engine_fields.len()
                ));
            }
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
        let reserved_status_values: Vec<&str> = entry
            .fields
            .iter()
            .filter(|(key, value)| {
                (key.eq_ignore_ascii_case("status") || key == "状态") && !value.trim().is_empty()
            })
            .map(|(_, value)| value.trim())
            .collect();
        if !reserved_status_values.is_empty() {
            let values = reserved_status_values.join("、");
            let differs = reserved_status_values
                .iter()
                .any(|value| *value != entry.status.trim());
            findings.push(if differs {
                format!(
                    "archived {}: reserved status field(s) `{values}` conflict with authoritative header `{}`; apply 将移除并写入进展",
                    entry.id, entry.status
                )
            } else {
                format!(
                    "archived {}: redundant reserved status field(s) `{values}`; apply 将移除并写入进展",
                    entry.id
                )
            });
        }
        let archived_engine_fields: Vec<&str> = entry
            .fields
            .iter()
            .filter(|(key, _)| {
                crate::docstore::DocStore::ENGINE_DERIVED_FIELDS.contains(&key.trim())
            })
            .map(|(key, _)| key.as_str())
            .collect();
        if !archived_engine_fields.is_empty() {
            findings.push(format!(
                "archived {}: engine-derived field(s) `{}` — apply 将移除",
                entry.id,
                archived_engine_fields.join("、")
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
            match store.drop_archived_engine_fields(&entry.id) {
                Ok((true, removed)) => fixed.push(format!(
                    "archived {}: removed {removed} engine-derived field(s)",
                    entry.id
                )),
                Ok((false, _)) => {}
                Err(e) => {
                    return ToolOutput::error(format!(
                        "cannot drop archived engine fields {}: {e}",
                        entry.id
                    ))
                }
            }
            match store.reconcile_archived_status_fields(&entry.id) {
                Ok((true, removed)) => fixed.push(format!(
                    "archived {}: removed {removed} reserved status field(s) and recorded history",
                    entry.id
                )),
                Ok((false, _)) => {}
                Err(e) => {
                    return ToolOutput::error(format!(
                        "cannot reconcile archived status {}: {e}",
                        entry.id
                    ))
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
