//! 事件域(R-155 S4):session_events 的追加/回放/清理。
//! append_event_tx 提 pub(crate):inbox 跨域事务(steer 提升)需要它。
//! 已在事务内不得再调自开 tx 的方法(见 mod.rs unchecked_transaction 注)。

use std::collections::{BTreeMap, HashSet};

use rusqlite::{params, OptionalExtension, Transaction};

use kanzei_llm::Message;
use serde_json::Value;

use super::{now_ms, SessionStore, StoreError, StoredEvent};

/// 段删除的审计事件:记下删了哪个区间、多少条,并占住一个 sequence 防止复用。
pub const SEGMENT_DELETED: &str = "conversation.segment_deleted";

/// 随段删除的内容型事件(typed facts 之外)。凡是承载「这段对话说了什么、做了什么」
/// 的事件都在内:快照、轨迹(预览/报错/artifact 引用)、子代理完整对话、压缩事务四件套
/// (surface_replaced 是整份消息副本)、手机消息、轮次结局。
///
/// 元数据骨架显式保留,不在此列:conversation.reset(段界)、conversation.segment_deleted
/// (审计)、session.*、orchestration.*、prompt.*、experience.fact、task.*、
/// permission.resolved、run.transaction_budget_*、worktree.orphaned。
pub(crate) const SEGMENT_CONTENT_TYPES: [&str; 10] = [
    "conversation.updated",
    "run.trace",
    super::typed::SUBAGENT_TRANSCRIPT,
    "compaction_started",
    "compaction_summary",
    "surface_replaced",
    "compaction_ended",
    "mobile.message",
    "run.completed",
    "run.failed",
];

impl SessionStore {
    pub fn append_event(
        &self,
        session_id: &str,
        event_type: &str,
        payload: &Value,
    ) -> Result<StoredEvent, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let event = append_event_tx(&tx, session_id, event_type, payload)?;
        tx.commit()?;
        Ok(event)
    }

    /// R-243 批1：以单个 SQLite 事务追加完整 compaction 事务。
    /// 原始 typed/session 事实不在此处修改；surface 只是追加的投影结果。
    pub fn append_compaction_transaction(
        &self,
        session_id: &str,
        transaction_id: &str,
        summary: &Value,
        surface: &Value,
    ) -> Result<Vec<StoredEvent>, StoreError> {
        if transaction_id.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "compaction transaction_id 不能为空".into(),
            ));
        }
        if !surface.is_array() {
            return Err(StoreError::InvalidInput(
                "compaction surface 必须是消息数组".into(),
            ));
        }
        let tx = self.connection.unchecked_transaction()?;
        let events = [
            (
                "compaction_started",
                serde_json::json!({
                    "transaction_id": transaction_id,
                }),
            ),
            (
                "compaction_summary",
                serde_json::json!({
                    "transaction_id": transaction_id,
                    "summary": summary,
                }),
            ),
            (
                "surface_replaced",
                serde_json::json!({
                    "transaction_id": transaction_id,
                    "surface": surface,
                }),
            ),
            (
                "compaction_ended",
                serde_json::json!({
                    "transaction_id": transaction_id,
                }),
            ),
        ];
        let mut appended = Vec::with_capacity(events.len());
        for (event_type, payload) in events {
            appended.push(append_event_tx(&tx, session_id, event_type, &payload)?);
        }
        tx.commit()?;
        Ok(appended)
    }

    /// 返回未完成 compaction 的可见诊断；这些事务没有 `compaction_ended`，
    /// recovery 必须忽略其 surface_replaced，保留旧 surface。
    pub fn incomplete_compaction_diagnostics(
        &self,
        session_id: &str,
    ) -> Result<Vec<String>, StoreError> {
        let events = self.list_events(session_id, 0)?;
        let mut started = BTreeMap::<String, i64>::new();
        let mut ended = HashSet::new();
        for event in events {
            let transaction_id = event.payload["transaction_id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if transaction_id.is_empty() {
                continue;
            }
            match event.event_type.as_str() {
                "compaction_started" => {
                    started.insert(transaction_id, event.sequence);
                }
                "compaction_ended" => {
                    ended.insert(transaction_id);
                }
                _ => {}
            }
        }
        Ok(started
            .into_iter()
            .filter(|(transaction_id, _)| !ended.contains(transaction_id))
            .map(|(transaction_id, sequence)| {
                format!(
                    "compaction transaction {transaction_id} incomplete at sequence {sequence}; surface_replaced ignored"
                )
            })
            .collect())
    }

    /// 返回最新一笔已完整结束的 compaction surface 及其结束序号。
    /// 未见 `compaction_ended` 的 surface 永远不会成为恢复后的模型上下文。
    pub fn latest_completed_compaction_surface(
        &self,
        session_id: &str,
        after_sequence: i64,
    ) -> Result<Option<(i64, Vec<Message>)>, StoreError> {
        let events = self.list_events(session_id, after_sequence)?;
        let mut surfaces = BTreeMap::<String, Vec<Message>>::new();
        let mut latest: Option<(i64, Vec<Message>)> = None;
        for event in events {
            let transaction_id = event.payload["transaction_id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if transaction_id.is_empty() {
                continue;
            }
            match event.event_type.as_str() {
                "surface_replaced" => {
                    let surface = serde_json::from_value(
                        event
                            .payload
                            .get("surface")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!([])),
                    )?;
                    surfaces.insert(transaction_id, surface);
                }
                "compaction_ended" => {
                    if let Some(surface) = surfaces.remove(&transaction_id) {
                        latest = Some((event.sequence, surface));
                    }
                }
                _ => {}
            }
        }
        Ok(latest)
    }

    pub fn list_events(
        &self,
        session_id: &str,
        after_sequence: i64,
    ) -> Result<Vec<StoredEvent>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT event_id, session_id, sequence, event_type, payload_json, created_at
                 FROM session_events WHERE session_id = ?1 AND sequence > ?2 ORDER BY sequence",
        )?;
        let rows = statement.query_map(params![session_id, after_sequence], event_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按类型下推过滤的事件列表(D-297 验收①):event_type 进 WHERE 与复合索引
    /// `(session_id, event_type, sequence)`,只解析所需类型的行——conversation_list /
    /// conversation_trace_get / 按序号恢复不再为取一种事件而全表 serde 解析。
    pub fn list_events_by_type(
        &self,
        session_id: &str,
        after_sequence: i64,
        event_type: &str,
    ) -> Result<Vec<StoredEvent>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT event_id, session_id, sequence, event_type, payload_json, created_at
                 FROM session_events
                 WHERE session_id = ?1 AND sequence > ?2 AND event_type = ?3
                 ORDER BY sequence",
        )?;
        let rows = statement.query_map(
            params![session_id, after_sequence, event_type],
            event_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按类型取最小 sequence 的单个事件(D-297 验收②:按序号恢复改单行查询)。
    pub fn event_by_sequence_and_type(
        &self,
        session_id: &str,
        sequence: i64,
        event_type: &str,
    ) -> Result<Option<StoredEvent>, StoreError> {
        self.connection
            .query_row(
                "SELECT event_id, session_id, sequence, event_type, payload_json, created_at
                     FROM session_events
                     WHERE session_id = ?1 AND sequence = ?2 AND event_type = ?3",
                params![session_id, sequence, event_type],
                event_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// 按 sequence 删除指定类型的事件(类型限定防止误删调度事件)。返回删除数。
    /// 用途:历史对话管理——快照删除不影响 prompt/session 生命周期事件。
    pub fn delete_events_by_sequence(
        &self,
        session_id: &str,
        event_type: &str,
        sequences: &[i64],
    ) -> Result<usize, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let mut deleted = 0usize;
        {
            let mut statement = tx.prepare(
                "DELETE FROM session_events
                     WHERE session_id = ?1 AND event_type = ?2 AND sequence = ?3",
            )?;
            for sequence in sequences {
                deleted += statement.execute(params![session_id, event_type, sequence])?;
            }
        }
        tx.commit()?;
        Ok(deleted)
    }

    /// 按 sequence 查事件(任意类型)。D-421:投影模式下列表返回 typed fact 的
    /// sequence,删除前需判断该 sequence 指向快照还是 typed fact。
    pub fn event_by_sequence(
        &self,
        session_id: &str,
        sequence: i64,
    ) -> Result<Option<StoredEvent>, StoreError> {
        self.connection
            .query_row(
                "SELECT event_id, session_id, sequence, event_type, payload_json, created_at
                     FROM session_events
                     WHERE session_id = ?1 AND sequence = ?2",
                params![session_id, sequence],
                event_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// 删除 (start, end] 范围内的一段历史对话(「历史对话」勾选删除的落库入口)。
    ///
    /// 删除集合 = typed facts([`FACT_TYPES`](super::typed::FACT_TYPES))∪
    /// [`SEGMENT_CONTENT_TYPES`]。D-421 只删前者与快照,同段的压缩摘要、轨迹、子代理
    /// 对话都留着:删掉最新段后压缩 surface 顶上来成为 prior(被删对话以压缩版复活),
    /// 重新载入时轨迹又画回活动面板,轨迹里的 artifact 引用让安全整理永远释放不掉原文。
    ///
    /// 同一事务内依次:收集段内 `prompt.admitted` 指向的输入 → 清空其中**已结束**
    /// (completed/failed/cancelled)输入的原文(行、状态与统计口径保留;未结束的输入
    /// 还要执行,不动)→ 先追加一条 `conversation.segment_deleted` 审计事件 → 再 DELETE。
    /// 审计事件在 DELETE 之前拿到旧 MAX(sequence)+1:删掉尾部后 MAX 不会回退,被删的
    /// sequence 与 event_id 永不复用。`end == i64::MAX`(删的是当前段)时审计事件的
    /// `end` 记为 null——[`conversation_floor`](Self::conversation_floor) 据此把它当作
    /// 当前对话的新地板。区间内既无可删事件也无输入时什么都不写,返回零。
    pub fn delete_conversation_segment(
        &self,
        session_id: &str,
        start: i64,
        end: i64,
    ) -> Result<super::SegmentDeletion, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let input_ids: Vec<String> = {
            let mut statement = tx.prepare(
                "SELECT json_extract(payload_json, '$.input_id') FROM session_events
                     WHERE session_id = ?1 AND sequence > ?2 AND sequence <= ?3
                       AND event_type = 'prompt.admitted'",
            )?;
            let rows = statement.query_map(params![session_id, start, end], |row| {
                row.get::<_, Option<String>>(0)
            })?;
            rows.filter_map(|row| row.transpose())
                .collect::<Result<_, _>>()?
        };
        // rusqlite 无数组参数,删除集合展开为 IN 占位符链(?4 起)。
        let content_types: Vec<&str> = super::typed::FACT_TYPES
            .iter()
            .chain(SEGMENT_CONTENT_TYPES.iter())
            .copied()
            .collect();
        let placeholders = (0..content_types.len())
            .map(|index| format!("?{}", index + 4))
            .collect::<Vec<_>>()
            .join(",");
        let mut range_params: Vec<&dyn rusqlite::types::ToSql> = vec![&session_id, &start, &end];
        for event_type in &content_types {
            range_params.push(event_type);
        }
        let count: i64 = tx.query_row(
            &format!(
                "SELECT COUNT(*) FROM session_events
                     WHERE session_id = ?1 AND sequence > ?2 AND sequence <= ?3
                       AND event_type IN ({placeholders})"
            ),
            rusqlite::params_from_iter(range_params.iter()),
            |row| row.get(0),
        )?;
        if count == 0 && input_ids.is_empty() {
            tx.commit()?;
            return Ok(super::SegmentDeletion::default());
        }
        let mut redacted_inputs = 0usize;
        {
            let mut statement = tx.prepare(
                "UPDATE session_inputs SET prompt = ''
                     WHERE session_id = ?1 AND input_id = ?2
                       AND status IN ('completed', 'failed', 'cancelled')",
            )?;
            for input_id in &input_ids {
                redacted_inputs += statement.execute(params![session_id, input_id])?;
            }
        }
        append_event_tx(
            &tx,
            session_id,
            SEGMENT_DELETED,
            &serde_json::json!({
                "start": start,
                "end": (end != i64::MAX).then_some(end),
                "events": count,
                "redacted_inputs": redacted_inputs,
            }),
        )?;
        let events = tx.execute(
            &format!(
                "DELETE FROM session_events
                     WHERE session_id = ?1 AND sequence > ?2 AND sequence <= ?3
                       AND event_type IN ({placeholders})"
            ),
            rusqlite::params_from_iter(range_params.iter()),
        )?;
        tx.commit()?;
        Ok(super::SegmentDeletion {
            events,
            redacted_inputs,
        })
    }

    /// 当前对话的「地板」:最后一个 `conversation.reset`,与最后一次删掉当前段的
    /// `conversation.segment_deleted`(payload.end 为 null)两者中较大的 sequence。
    ///
    /// 地板之前的 legacy 快照不属于当前对话:不得被播种进当前段,也不得被 legacy 回退
    /// 读回 prior(D-427 同一语义)。只删旧段的审计事件(end 为数值)不抬地板——当前段
    /// 里尚未播种的旧快照仍是当前对话的一部分。
    pub fn conversation_floor(&self, session_id: &str) -> Result<Option<i64>, StoreError> {
        self.connection
            .query_row(
                "SELECT MAX(sequence) FROM session_events
                     WHERE session_id = ?1
                       AND (event_type = 'conversation.reset'
                            OR (event_type = ?2 AND json_extract(payload_json, '$.end') IS NULL))",
                params![session_id, SEGMENT_DELETED],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// 遗留/测试用:硬删会话的全部 `conversation.updated` 快照。CLI 不再调用——
    /// `kz run --new` 与桌面主线共用同一 session,它曾把桌面端全部 legacy 历史静默抹掉;
    /// 现在 `--new` 只追加 `conversation.reset`。
    pub fn clear_conversation(&self, session_id: &str) -> Result<usize, StoreError> {
        self.connection
                .execute(
                    "DELETE FROM session_events WHERE session_id = ?1 AND event_type = 'conversation.updated'",
                    params![session_id],
                )
                .map_err(Into::into)
    }

    pub fn latest_event(
        &self,
        session_id: &str,
        event_type: &str,
    ) -> Result<Option<StoredEvent>, StoreError> {
        self.connection
            .query_row(
                "SELECT event_id, session_id, sequence, event_type, payload_json, created_at
                     FROM session_events
                     WHERE session_id = ?1 AND event_type = ?2
                     ORDER BY sequence DESC LIMIT 1",
                params![session_id, event_type],
                event_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// 回放评估台原料(R-163 批1):最近 N 条 `run.trace` 的 payload_json(新→旧)。
    /// 只取该类型——run.trace 是引擎机械写入的轨迹画像(工具名/输入摘要/ok/error),
    /// 不掺对话快照等其它事件;回放案例由 [`crate::replay::parse_trace_payload`] 解析。
    pub fn list_trace_payloads(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT event_id, payload_json FROM session_events
                 WHERE session_id = ?1 AND event_type = 'run.trace'
                 ORDER BY sequence DESC LIMIT ?2",
        )?;
        let rows = statement
            .query_map(params![session_id, limit as i64], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// D-297 验收③:run.trace 保留策略——每会话只保留最近 `keep_rounds` 轮的轨迹行。
    /// 增量事件与整包补写共用顶层 `run_id` 字段(payload_json 内),按 run_id 分组、
    /// 保留 sequence 最新的一组,更早的整组删除。返回删除的行数。
    ///
    /// 只清理 `run.trace` 类型,conversation.updated / orchestration.* 等审计事件不动。
    /// SQLite 的 JSON1 在 bundled 构建里默认可用,json_extract 在这里提取 run_id。
    pub fn prune_trace_rounds(
        &self,
        session_id: &str,
        keep_rounds: usize,
    ) -> Result<usize, StoreError> {
        let changed = self.connection.execute(
            "DELETE FROM session_events
                 WHERE session_id = ?1 AND event_type = 'run.trace'
                   AND json_extract(payload_json, '$.run_id') NOT IN (
                       SELECT run_id FROM (
                           SELECT DISTINCT json_extract(payload_json, '$.run_id') AS run_id
                           FROM session_events
                           WHERE session_id = ?1 AND event_type = 'run.trace'
                           ORDER BY sequence DESC
                           LIMIT ?2
                       )
                   )",
            params![session_id, keep_rounds as i64],
        )?;
        Ok(changed)
    }
}

/// 事件行解析。
pub(crate) fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvent> {
    let payload_json: String = row.get(4)?;
    Ok(StoredEvent {
        event_id: row.get(0)?,
        session_id: row.get(1)?,
        sequence: row.get(2)?,
        event_type: row.get(3)?,
        payload: serde_json::from_str(&payload_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        created_at: row.get(5)?,
    })
}

/// 在同一事务内追加事件并刷新会话 updated_at(S4 提 pub(crate))。
pub(crate) fn append_event_tx(
    tx: &Transaction<'_>,
    session_id: &str,
    event_type: &str,
    payload: &Value,
) -> Result<StoredEvent, StoreError> {
    let sequence: i64 = tx.query_row(
        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM session_events WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )?;
    let created_at = now_ms();
    let event_id = format!("evt_{}_{}", session_id, sequence);
    tx.execute(
            "INSERT INTO session_events(event_id, session_id, sequence, event_type, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![event_id, session_id, sequence, event_type, serde_json::to_string(payload)?, created_at],
        )?;
    tx.execute(
        "UPDATE sessions SET updated_at = ?1 WHERE session_id = ?2",
        params![created_at, session_id],
    )?;
    Ok(StoredEvent {
        event_id,
        session_id: session_id.to_string(),
        sequence,
        event_type: event_type.to_string(),
        payload: payload.clone(),
        created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::store;

    #[test]
    fn prune_trace_rounds_只保留最近n轮的run_trace() {
        let store = store();
        // 三轮运行:每轮一条整包 run.trace(run_id 顶层),另混入一条 conversation.updated。
        for round in 1..=3 {
            store
                .append_event(
                    "ses_test",
                    "run.trace",
                    &serde_json::json!({"run_id": format!("run_{round}"), "events": [{"kind": "tool.started"}]}),
                )
                .unwrap();
        }
        store
            .append_event(
                "ses_test",
                "conversation.updated",
                &serde_json::json!({"messages": []}),
            )
            .unwrap();
        let removed = store.prune_trace_rounds("ses_test", 2).unwrap();
        assert_eq!(removed, 1, "只应删掉最旧一轮(run_1)的轨迹行");
        let traces = store.list_trace_payloads("ses_test", 10).unwrap();
        assert_eq!(traces.len(), 2, "保留最近两轮");
        assert!(traces.iter().any(|(_, p)| p.contains("run_2")));
        assert!(traces.iter().any(|(_, p)| p.contains("run_3")));
        assert!(!traces.iter().any(|(_, p)| p.contains("run_1")));
        // conversation.updated 不受影响。
        assert!(
            store
                .list_events_by_type("ses_test", 0, "conversation.updated")
                .unwrap()
                .len()
                == 1
        );
        // 保留轮数不小于现有轮数时不删除。
        assert_eq!(store.prune_trace_rounds("ses_test", 99).unwrap(), 0);
    }

    /// D-297 验收④:主会话规模(数千事件,run.trace 占绝大多数)下,按类型下推
    /// 过滤的解析字节量比全表解析低一个数量级。解析成本正比于 payload 大小,
    /// 用序列化总长近似(同一库、同一行的 payload 固定)。
    #[test]
    fn 主会话规模下类型下推解析字节量降一个数量级() {
        let store = store();
        // 模拟主会话实测分布:4333 条中 run.trace 占 95%+。这里 4000 条:3800 条
        // run.trace(带实际大小的轨迹 payload),200 条 conversation.updated(对话快照)。
        for index in 0..3800 {
            store
                .append_event(
                    "ses_test",
                    "run.trace",
                    &serde_json::json!({
                        "run_id": format!("run_{}", index / 20),
                        "events": [
                            {"kind": "tool.started", "id": format!("t{index}"), "name": "read", "summary": "s".repeat(120)},
                            {"kind": "tool.completed", "id": format!("t{index}"), "ok": true, "durationMs": 12}
                        ]
                    }),
                )
                .unwrap();
        }
        for index in 0..200 {
            store
                .append_event(
                    "ses_test",
                    "conversation.updated",
                    &serde_json::json!({"messages": [{"role": "user", "parts": [{"type": "text", "text": format!("消息 {index}")}]}]}),
                )
                .unwrap();
        }
        let all = store.list_events("ses_test", 0).unwrap();
        let conversations = store
            .list_events_by_type("ses_test", 0, "conversation.updated")
            .unwrap();
        let bytes_all: usize = all
            .iter()
            .map(|e| serde_json::to_string(&e.payload).map_or(0, |s| s.len()))
            .sum();
        let bytes_conv: usize = conversations
            .iter()
            .map(|e| serde_json::to_string(&e.payload).map_or(0, |s| s.len()))
            .sum();
        assert_eq!(all.len(), 4000);
        assert_eq!(conversations.len(), 200);
        assert!(
            bytes_all >= bytes_conv * 10,
            "类型下推解析字节量应比全表低一个数量级:全表 {bytes_all} vs 下推 {bytes_conv}"
        );
    }

    #[test]
    fn list_events_by_type_只返回指定类型并按下推过滤() {
        let store = store();
        store
            .append_event(
                "ses_test",
                "conversation.updated",
                &serde_json::json!({"messages": []}),
            )
            .unwrap();
        store
            .append_event(
                "ses_test",
                "run.trace",
                &serde_json::json!({"events": [{"kind": "tool.started"}]}),
            )
            .unwrap();
        store
            .append_event(
                "ses_test",
                "conversation.updated",
                &serde_json::json!({"messages": [{"role": "user"}]}),
            )
            .unwrap();
        let convs = store
            .list_events_by_type("ses_test", 0, "conversation.updated")
            .unwrap();
        assert_eq!(convs.len(), 2, "只应返回 conversation.updated");
        assert!(convs.iter().all(|e| e.event_type == "conversation.updated"));
        let traces = store
            .list_events_by_type("ses_test", 0, "run.trace")
            .unwrap();
        assert_eq!(traces.len(), 1, "只应返回 run.trace");
        assert_eq!(traces[0].payload["events"][0]["kind"], "tool.started");
        // after_sequence 与类型同时生效。
        let after = store
            .list_events_by_type("ses_test", 1, "conversation.updated")
            .unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].sequence, 3);
    }

    #[test]
    fn event_by_sequence_and_type_单行查询命中或为空() {
        let store = store();
        store
            .append_event(
                "ses_test",
                "conversation.updated",
                &serde_json::json!({"v": 1}),
            )
            .unwrap();
        store
            .append_event("ses_test", "run.trace", &serde_json::json!({"events": []}))
            .unwrap();
        let hit = store
            .event_by_sequence_and_type("ses_test", 1, "conversation.updated")
            .unwrap();
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().payload["v"], 1);
        // 同序号但类型不符:查不到,不误配。
        assert!(store
            .event_by_sequence_and_type("ses_test", 1, "run.trace")
            .unwrap()
            .is_none());
        assert!(store
            .event_by_sequence_and_type("ses_test", 99, "conversation.updated")
            .unwrap()
            .is_none());
    }

    #[test]
    fn 事件序列按会话递增并可回放() {
        let store = store();
        let first = store
            .append_event(
                "ses_test",
                "session.created",
                &serde_json::json!({"ok": true}),
            )
            .unwrap();
        let second = store
            .append_event("ses_test", "turn.started", &serde_json::json!({"step": 1}))
            .unwrap();
        assert_eq!((first.sequence, second.sequence), (1, 2));
        assert_eq!(store.list_events("ses_test", 1).unwrap().len(), 1);
    }

    #[test]
    fn latest_event_按类型返回最新事件() {
        let store = store();
        store
            .append_event(
                "ses_test",
                "conversation.updated",
                &serde_json::json!({"v": 1}),
            )
            .unwrap();
        store
            .append_event("ses_test", "run.completed", &serde_json::json!({}))
            .unwrap();
        store
            .append_event(
                "ses_test",
                "conversation.updated",
                &serde_json::json!({"v": 2}),
            )
            .unwrap();
        let latest = store
            .latest_event("ses_test", "conversation.updated")
            .unwrap()
            .unwrap();
        assert_eq!(latest.payload["v"], 2);
        assert!(store.latest_event("ses_test", "missing").unwrap().is_none());
    }

    #[test]
    fn compaction_transaction_is_atomic_ordered_and_preserves_raw_events() {
        let store = store();
        let original = store
            .append_event(
                "ses_test",
                "turn.started",
                &serde_json::json!({"raw": true}),
            )
            .unwrap();
        let surface = serde_json::json!([
            {"role": "user", "parts": [{"type": "text", "text": "surface"}]}
        ]);
        let events = store
            .append_compaction_transaction(
                "ses_test",
                "cmp-1",
                &serde_json::json!({"digest": "summary"}),
                &surface,
            )
            .unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            vec![
                "compaction_started",
                "compaction_summary",
                "surface_replaced",
                "compaction_ended"
            ]
        );
        assert_eq!(events[0].sequence + 1, events[1].sequence);
        assert_eq!(events[1].sequence + 1, events[2].sequence);
        assert_eq!(events[2].sequence + 1, events[3].sequence);
        let raw = store
            .event_by_sequence("ses_test", original.sequence)
            .unwrap()
            .unwrap();
        assert_eq!(raw.event_type, "turn.started");
        assert_eq!(raw.payload, serde_json::json!({"raw": true}));
        assert_eq!(events[2].payload["surface"], surface);
        let (ended_sequence, recovered_surface) = store
            .latest_completed_compaction_surface("ses_test", 0)
            .unwrap()
            .expect("已结束事务必须成为可恢复 surface");
        assert_eq!(ended_sequence, events[3].sequence);
        assert_eq!(
            recovered_surface,
            vec![kanzei_llm::Message::user_text("surface")]
        );
    }

    #[test]
    fn invalid_compaction_transaction_writes_no_partial_events() {
        let store = store();
        let before = store.list_events("ses_test", 0).unwrap().len();
        assert!(store
            .append_compaction_transaction(
                "ses_test",
                "",
                &serde_json::json!({}),
                &serde_json::json!([]),
            )
            .is_err());
        assert_eq!(store.list_events("ses_test", 0).unwrap().len(), before);
        assert!(store
            .append_compaction_transaction(
                "ses_test",
                "cmp-invalid-surface",
                &serde_json::json!({}),
                &serde_json::json!({"not": "messages"}),
            )
            .is_err());
        assert_eq!(store.list_events("ses_test", 0).unwrap().len(), before);
    }

    #[test]
    fn incomplete_compaction_is_diagnosed_without_becoming_surface() {
        let store = store();
        store
            .append_event(
                "ses_test",
                "compaction_started",
                &serde_json::json!({"transaction_id": "cmp-crash"}),
            )
            .unwrap();
        store
            .append_event(
                "ses_test",
                "compaction_summary",
                &serde_json::json!({"transaction_id": "cmp-crash", "summary": {}}),
            )
            .unwrap();
        store
            .append_event(
                "ses_test",
                "surface_replaced",
                &serde_json::json!({"transaction_id": "cmp-crash", "surface": []}),
            )
            .unwrap();
        let diagnostics = store.incomplete_compaction_diagnostics("ses_test").unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("cmp-crash"));
        assert!(diagnostics[0].contains("surface_replaced ignored"));

        store
            .append_event(
                "ses_test",
                "compaction_ended",
                &serde_json::json!({"transaction_id": "cmp-crash"}),
            )
            .unwrap();
        assert!(store
            .incomplete_compaction_diagnostics("ses_test")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn list_trace_payloads_只返回run_trace并按新到旧() {
        // R-163 批1:回放评估台原料接口——只取 run.trace 类型,其它事件不混入,
        // 顺序按 sequence 倒序(最新轨迹优先)。
        let store = store();
        store
            .create_session("ses_replay", "C:/replay", None)
            .unwrap();
        store
            .append_event(
                "ses_replay",
                "run.completed",
                &serde_json::json!({"outcome": "ok"}),
            )
            .unwrap();
        store
            .append_event(
                "ses_replay",
                "run.trace",
                &serde_json::json!({"events": [{"id": "a", "kind": "tool.started", "name": "read"}], "outcome": "failed"}),
            )
            .unwrap();
        store
            .append_event(
                "ses_replay",
                "run.trace",
                &serde_json::json!({"events": [], "outcome": "completed"}),
            )
            .unwrap();
        let traces = store.list_trace_payloads("ses_replay", 10).unwrap();
        assert_eq!(traces.len(), 2, "run.completed 不得混入回放原料");
        assert!(
            traces[0].1.contains("completed"),
            "最新 run.trace 在前: {}",
            traces[0].1
        );
        // 限量:limit=1 只取最新一条。
        let one = store.list_trace_payloads("ses_replay", 1).unwrap();
        assert_eq!(one.len(), 1);
        assert!(one[0].1.contains("completed"));
        // 缺 session 时为空,不报错。
        assert!(store.list_trace_payloads("missing", 10).unwrap().is_empty());
    }

    #[test]
    fn clear_conversation_只删除对话快照() {
        let store = store();
        store
            .append_event(
                "ses_test",
                "conversation.updated",
                &serde_json::json!({"v": 1}),
            )
            .unwrap();
        store
            .append_event(
                "ses_test",
                "session.status_changed",
                &serde_json::json!({"status": "idle"}),
            )
            .unwrap();
        assert_eq!(store.clear_conversation("ses_test").unwrap(), 1);
        assert!(store
            .latest_event("ses_test", "conversation.updated")
            .unwrap()
            .is_none());
        assert!(store
            .latest_event("ses_test", "session.status_changed")
            .unwrap()
            .is_some());
    }

    /// UI-0926 #1:段删除连带删除内容型事件(压缩摘要、轨迹、子代理对话……会让被删
    /// 对话「复活」或永远占着 artifact),清空已结束输入的原文,保留元数据骨架;
    /// 审计事件先于 DELETE 拿序号,被删的 sequence 永不复用。
    #[test]
    fn 删除对话段连带删除内容型事件并保留骨架() {
        let store = store();
        use serde_json::json;
        let reset = store
            .append_event("ses_test", "conversation.reset", &json!({"cleared": true}))
            .unwrap();
        store
            .admit_input(
                "ses_test",
                "in-done",
                "已结束输入的原文",
                super::super::Delivery::Queue,
            )
            .unwrap();
        store
            .admit_input(
                "ses_test",
                "in-pending",
                "还没执行的输入",
                super::super::Delivery::Queue,
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE session_inputs SET status = 'completed' WHERE input_id = 'in-done'",
                [],
            )
            .unwrap();
        for input_id in ["in-done", "in-pending"] {
            store
                .append_event(
                    "ses_test",
                    "prompt.admitted",
                    &json!({"input_id": input_id, "delivery": "queue"}),
                )
                .unwrap();
        }
        let skeleton = [
            "session.status_changed",
            "orchestration.writer.acquired",
            "experience.fact",
            "permission.resolved",
        ];
        for event_type in skeleton {
            store
                .append_event("ses_test", event_type, &json!({}))
                .unwrap();
        }
        for event_type in super::super::typed::FACT_TYPES {
            store
                .append_event("ses_test", event_type, &json!({}))
                .unwrap();
        }
        for event_type in SEGMENT_CONTENT_TYPES {
            store
                .append_event(
                    "ses_test",
                    event_type,
                    &json!({"transaction_id": "cmp", "artifact": {"artifact_id": "tool-x"}}),
                )
                .unwrap();
        }
        let max_before = store
            .list_events("ses_test", 0)
            .unwrap()
            .last()
            .unwrap()
            .sequence;
        let content_count = super::super::typed::FACT_TYPES.len() + SEGMENT_CONTENT_TYPES.len();

        let deletion = store
            .delete_conversation_segment("ses_test", reset.sequence, i64::MAX)
            .unwrap();
        assert_eq!(deletion.events, content_count);
        assert_eq!(deletion.redacted_inputs, 1, "只清空已结束输入的原文");

        let remaining = store.list_events("ses_test", 0).unwrap();
        let kinds: Vec<&str> = remaining.iter().map(|e| e.event_type.as_str()).collect();
        for event_type in super::super::typed::FACT_TYPES
            .iter()
            .chain(SEGMENT_CONTENT_TYPES.iter())
        {
            assert!(!kinds.contains(event_type), "{event_type} 应随段删除");
        }
        for event_type in skeleton
            .iter()
            .chain(["conversation.reset", "prompt.admitted"].iter())
        {
            assert!(kinds.contains(event_type), "元数据骨架 {event_type} 应保留");
        }
        let prompt = |input_id: &str| -> String {
            store
                .connection
                .query_row(
                    "SELECT prompt FROM session_inputs WHERE input_id = ?1",
                    [input_id],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(prompt("in-done"), "", "已结束输入的原文应就地清空");
        assert_eq!(
            prompt("in-pending"),
            "还没执行的输入",
            "未结束输入还要执行,不得清空"
        );
        assert_eq!(
            store.input_status("in-done").unwrap().as_deref(),
            Some("completed"),
            "输入行与状态保留(任务统计口径)"
        );

        let audits = store
            .list_events_by_type("ses_test", 0, SEGMENT_DELETED)
            .unwrap();
        assert_eq!(audits.len(), 1, "恰好一条审计事件");
        assert!(
            audits[0].sequence > max_before,
            "审计事件在删除前占住新序号"
        );
        assert_eq!(audits[0].payload["start"], reset.sequence);
        assert!(
            audits[0].payload["end"].is_null(),
            "删当前段时 end 记为 null"
        );
        assert_eq!(audits[0].payload["events"], content_count as i64);
        assert_eq!(
            store.conversation_floor("ses_test").unwrap(),
            Some(audits[0].sequence),
            "删掉当前段抬高当前对话地板"
        );
        let next = store
            .append_event("ses_test", "session.status_changed", &json!({}))
            .unwrap();
        assert!(
            next.sequence > audits[0].sequence,
            "被删的 sequence 不得复用"
        );
    }

    #[test]
    fn 空区间删除不写审计事件() {
        let store = store();
        store
            .append_event("ses_test", "session.status_changed", &serde_json::json!({}))
            .unwrap();
        let before = store.list_events("ses_test", 0).unwrap().len();
        let deletion = store
            .delete_conversation_segment("ses_test", 0, i64::MAX)
            .unwrap();
        assert_eq!(deletion, crate::store::SegmentDeletion::default());
        assert_eq!(store.list_events("ses_test", 0).unwrap().len(), before);
        assert!(store.conversation_floor("ses_test").unwrap().is_none());
    }

    /// 只删旧段(end 为数值)不抬当前对话地板;地板取 reset 与「删当前段」中较大者。
    #[test]
    fn 删旧段不抬当前对话地板() {
        let store = store();
        use serde_json::json;
        store
            .append_event("ses_test", "run.trace", &json!({"run_id": "old"}))
            .unwrap();
        let reset = store
            .append_event("ses_test", "conversation.reset", &json!({"cleared": true}))
            .unwrap();
        store
            .append_event("ses_test", "run.trace", &json!({"run_id": "cur"}))
            .unwrap();
        let deletion = store
            .delete_conversation_segment("ses_test", 0, reset.sequence)
            .unwrap();
        assert_eq!(deletion.events, 1);
        let audit = store
            .latest_event("ses_test", SEGMENT_DELETED)
            .unwrap()
            .unwrap();
        assert_eq!(audit.payload["end"], reset.sequence);
        assert_eq!(
            store.conversation_floor("ses_test").unwrap(),
            Some(reset.sequence),
            "只删旧段时地板仍是最后一个 reset"
        );
    }

    #[test]
    fn 不同会话的事件_id_保持唯一() {
        let store = store();
        store.create_session("ses_other", "C:/other", None).unwrap();
        let first = store
            .append_event("ses_test", "turn.started", &serde_json::json!({}))
            .unwrap();
        let second = store
            .append_event("ses_other", "turn.started", &serde_json::json!({}))
            .unwrap();
        assert_ne!(first.event_id, second.event_id);
    }

    #[test]
    fn r050_poc_不同会话事件回放互不串线() {
        let store = store();
        store.create_session("ses_other", "C:/other", None).unwrap();
        store
            .append_event(
                "ses_test",
                "conversation.updated",
                &serde_json::json!({"thread": "a"}),
            )
            .unwrap();
        store
            .append_event(
                "ses_other",
                "conversation.updated",
                &serde_json::json!({"thread": "b"}),
            )
            .unwrap();

        let a = store.list_events("ses_test", 0).unwrap();
        let b = store.list_events("ses_other", 0).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].payload["thread"], "a");
        assert_eq!(b[0].payload["thread"], "b");
        assert_eq!(a[0].sequence, 1);
        assert_eq!(b[0].sequence, 1);
    }

    #[test]
    fn orchestration_写租约事件闭环可审计() {
        // R-171 批5 验收⑦:queued→acquired→released 三事件可顺序回放,
        // 审计不丢持有者身份——会话事件流是写仲裁的可审计轨迹。
        //
        // R-173 批5:改由 OrchestrationEvent 自己给出类型名与 payload。原版在这里
        // 手抄了一遍字符串字面量和 payload 形状,于是**测试本身也是漂移面的一部分**:
        // 枚举那边改了名,测试照样绿,谁都发现不了。现在测试和生产代码走同一个出口。
        use kanzei_harness::orchestration::OrchestrationEvent;
        let store = store();
        store.create_session("ses_lease", "C:/proj", None).unwrap();
        let root = std::path::PathBuf::from("C:/proj");
        let timeline = [
            OrchestrationEvent::WriterQueued {
                project_root: root.clone(),
                run_id: "run_1".into(),
                process_id: "proc_1".into(),
                reason: "session writer run".into(),
            },
            OrchestrationEvent::WriterAcquired {
                project_root: root.clone(),
                run_id: "run_1".into(),
                process_id: "proc_1".into(),
            },
            OrchestrationEvent::WriterReleased {
                project_root: root,
                run_id: "run_1".into(),
                process_id: "proc_1".into(),
            },
        ];
        for (seq, event) in (1i64..).zip(timeline.iter()) {
            let stored = store
                .append_event("ses_lease", event.event_type(), &event.payload())
                .unwrap();
            assert_eq!(stored.sequence, seq);
        }
        let events = store.list_events("ses_lease", 0).unwrap();
        let kinds: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "orchestration.writer.queued",
                "orchestration.writer.acquired",
                "orchestration.writer.released"
            ],
            "落库类型名必须与 R-171 已写进 state.db 的历史行一致,否则旧轨迹回放不出来"
        );
        for ev in &events {
            assert_eq!(ev.payload["run_id"], "run_1");
            assert_eq!(ev.payload["process_id"], "proc_1");
            assert_eq!(ev.payload["project_root"], "C:/proj");
        }
    }

    #[test]
    fn 并发追加事件的_sequence_连续且唯一() {
        let path = std::env::temp_dir().join(format!(
            "kz-store-concurrency-{}-{}.db",
            std::process::id(),
            now_ms()
        ));
        let initializer = SessionStore::open(&path).unwrap();
        initializer
            .create_session("ses_concurrent", "C:/project", None)
            .unwrap();
        drop(initializer);

        let stores = (0..4)
            .map(|_| SessionStore::open(&path).unwrap())
            .collect::<Vec<_>>();
        let handles = stores
            .into_iter()
            .enumerate()
            .map(|(worker, store)| {
                std::thread::spawn(move || {
                    (0..20)
                        .map(|index| {
                            store
                                .append_event(
                                    "ses_concurrent",
                                    "test.concurrent",
                                    &serde_json::json!({"worker": worker, "index": index}),
                                )
                                .unwrap()
                                .sequence
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let mut sequences = handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        assert_eq!(sequences, (1..=80).collect::<Vec<_>>());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }
}
