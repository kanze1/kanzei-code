//! episode 域(R-155 S2):轮次情景摘要的落库与回放。
//! 方法都在 impl SessionStore 上,connection/path 字段经 pub(crate) 访问(super)。

use rusqlite::{params, OptionalExtension};

pub type EpisodeListRow = (i64, String, String, u32, String);

use super::{now_ms, EpisodeRecord, SessionStore, StoreError};

impl SessionStore {
    /// 轮次情景摘要(R-106):机械生成的轨迹画像,R-099 度量与记忆系统共用。
    pub fn append_episode(&self, episode: &EpisodeRecord<'_>) -> Result<i64, StoreError> {
        self.connection.execute(
                "INSERT INTO episodes(session_id, created_at, prompt_head, outcome, steps,
                                      input_tokens, output_tokens, tools_json, context_json, metrics_json,
                                      provider, model, run_id, input_id, duration_ms, overflow_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    episode.session_id,
                    now_ms(),
                    episode.prompt_head.chars().take(200).collect::<String>(),
                    episode.outcome,
                    episode.steps,
                    episode.input_tokens as i64,
                    episode.output_tokens as i64,
                    episode.tools_json,
                    episode.context_json,
                    episode.metrics_json,
                    episode.provider,
                    episode.model,
                    episode.run_id,
                    episode.input_id,
                    episode.duration_ms as i64,
                    episode.overflow_json,
                ],
            )?;
        Ok(self.connection.last_insert_rowid())
    }

    /// 最近若干轮的运行归属(D-173):provider/model/run_id/input_id/duration_ms。
    /// 与 `recent_episodes` 分开取,免得那个本已臃肿的元组再长五格。
    #[allow(clippy::type_complexity)]
    pub fn recent_episode_identities(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<(i64, String, String, String, String, u64)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT created_at, provider, model, run_id, input_id, duration_ms
                 FROM episodes WHERE session_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![session_id, limit as i64], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get::<_, i64>(5)? as u64,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 最近若干轮的完整画像(R-099/R-127):按时间倒序。
    /// 返回原始 JSON 字符串,解析交给调用方——存储层不该关心画像的字段构成。
    #[allow(clippy::type_complexity)]
    pub fn recent_episodes(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<(i64, String, String, u32, u64, u64, String, String, String)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT created_at, prompt_head, outcome, steps, input_tokens, output_tokens,
                        tools_json, context_json, metrics_json
                 FROM episodes WHERE session_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![session_id, limit as i64], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, i64>(3)? as u32,
                row.get::<_, i64>(4)? as u64,
                row.get::<_, i64>(5)? as u64,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        })?;
        Ok(rows.flatten().collect())
    }

    /// 最近一轮的上下文账单(context_json 原文),无 episode 时 None。
    pub fn latest_episode_context(&self, session_id: &str) -> Result<Option<String>, StoreError> {
        self.connection
            .query_row(
                "SELECT context_json FROM episodes WHERE session_id = ?1
                     ORDER BY created_at DESC LIMIT 1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// 最近 N 条 episode(新→旧):(created_at, prompt_head, outcome, steps, tools_json)。
    pub fn list_episodes(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<EpisodeListRow>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT created_at, prompt_head, outcome, steps, tools_json
                 FROM episodes WHERE session_id = ?1
                 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![session_id, limit as i64], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 最近含溢出轨迹的 episode(R-106):(created_at, overflow_json)。只返回
    /// 真正发生过上下文压缩的轮次,空的 `[]` 不占行——溢出路径有迹可查。
    pub fn recent_overflow_traces(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<(i64, String)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT created_at, overflow_json FROM episodes
                 WHERE session_id = ?1 AND overflow_json != '' AND overflow_json != '[]'
                 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![session_id, limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use crate::store::testutil::store;
    use crate::store::*;

    #[test]
    fn episode_落库并按时间倒序回放() {
        let store = store();
        store
            .append_episode(&EpisodeRecord {
                session_id: "ses_test",
                prompt_head: "修复 D-068 限流分类",
                outcome: "completed",
                steps: 12,
                input_tokens: 50_000,
                output_tokens: 3_000,
                tools_json: r#"{"bash":5,"edit":3}"#,
                context_json: r#"[["agent/system",1200],["dev/memory",800]]"#,
                metrics_json: r#"{"terminal_calls":5,"edit_calls":3,"edit_misses":1}"#,
                provider: "deepseek",
                model: "deepseek-v4-flash",
                run_id: "run_a",
                input_id: "input_a",
                duration_ms: 708_000,
                overflow_json: r#"[{"dropped_messages":3,"tools":{"bash":2},"failures":[],"preview":"旧任务"}]"#,
            })
            .unwrap();
        store
            .append_episode(&EpisodeRecord {
                session_id: "ses_test",
                prompt_head: "第二轮",
                outcome: "halted",
                steps: 3,
                tools_json: "{}",
                context_json: "[]",
                metrics_json: "{}",
                ..EpisodeRecord::default()
            })
            .unwrap();
        let episodes = store.list_episodes("ses_test", 10).unwrap();
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[0].1, "第二轮");
        assert_eq!(episodes[1].3, 12);
        assert!(episodes[1].4.contains("bash"));
        assert!(store.list_episodes("missing", 10).unwrap().is_empty());

        // R-099:调用画像随轮次落库,并能按时间倒序取回。空对象要与"度量为零"区分开。
        let recent = store.recent_episodes("ses_test", 10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].1, "第二轮");
        assert_eq!(recent[0].8, "{}", "未度量的轮次应保持空对象");
        assert!(
            recent[1].8.contains("edit_misses"),
            "画像未随轮次落库: {}",
            recent[1].8
        );

        // D-173:轮次归属必须落库。之前只能从"当前配置"反推这一轮跑的哪个模型,
        // 而配置随时会变——复盘时连最基本的事实都无法证伪。
        let identities = store.recent_episode_identities("ses_test", 10).unwrap();
        assert_eq!(identities.len(), 2);
        assert_eq!(identities[1].1, "deepseek");
        assert_eq!(identities[1].2, "deepseek-v4-flash");
        assert_eq!(identities[1].3, "run_a");
        assert_eq!(identities[1].4, "input_a");
        assert_eq!(identities[1].5, 708_000);

        // R-106:上下文压缩时被丢弃的轨迹随 episode 落库,并可查询回放。
        let traces = store.recent_overflow_traces("ses_test", 10).unwrap();
        assert_eq!(traces.len(), 1, "只有第一条 episode 带溢出轨迹");
        assert!(traces[0].1.contains("dropped_messages"));
        assert!(
            traces[0].1.contains("\"bash\":2"),
            "工具画像应随轨迹沉淀: {}",
            traces[0].1
        );
        assert!(store
            .recent_overflow_traces("missing", 10)
            .unwrap()
            .is_empty());
    }
}
