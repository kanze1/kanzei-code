//! inbox 域(R-155 S5):输入入队/提升/取消与跨域事务(finalize_interrupt)。
//! Delivery::as_str 已提 pub(super)(在 mod.rs)。
//! 已在事务内不得再调自开 tx 的方法(见 mod.rs unchecked_transaction 注)。

use rusqlite::{params, OptionalExtension};

use super::events::append_event_tx;
use super::{now_ms, AdmittedInput, Delivery, SessionStore, StoreError};

impl SessionStore {
        pub fn admit_input(
            &self,
            session_id: &str,
            input_id: &str,
            prompt: &str,
            delivery: Delivery,
        ) -> Result<AdmittedInput, StoreError> {
            let now = now_ms();
            self.connection.execute(
                "INSERT INTO session_inputs(input_id, session_id, prompt, delivery, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'pending', ?5)
                 ON CONFLICT(input_id) DO NOTHING",
                params![input_id, session_id, prompt, delivery.as_str(), now],
            )?;
            self.connection
                .query_row(
                    "SELECT input_id, session_id, prompt, delivery, created_at
                     FROM session_inputs WHERE input_id = ?1",
                    params![input_id],
                    input_from_row,
                )
                .map_err(Into::into)
        }
    
        pub fn has_pending(&self, session_id: &str, delivery: Delivery) -> Result<bool, StoreError> {
            let count: i64 = self.connection.query_row(
                "SELECT COUNT(*) FROM session_inputs WHERE session_id = ?1 AND delivery = ?2 AND status = 'pending'",
                params![session_id, delivery.as_str()],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        }
    
        /// 取消尚未提升的输入。已提升或已取消的输入不会被回收，避免篡改调度事实。
        /// 返回尚未提升的输入,按 admission 顺序展示给前端队列面板。
        pub fn list_pending_inputs(&self, session_id: &str) -> Result<Vec<AdmittedInput>, StoreError> {
            let mut statement = self.connection.prepare(
                "SELECT input_id, session_id, prompt, delivery, created_at
                 FROM session_inputs
                 WHERE session_id = ?1 AND status = 'pending'
                 ORDER BY created_at, rowid",
            )?;
            let rows = statement.query_map(params![session_id], input_from_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    
        pub fn cancel_input(&self, session_id: &str, input_id: &str) -> Result<bool, StoreError> {
            let changed = self.connection.execute(
                "UPDATE session_inputs SET status = 'cancelled'
                 WHERE session_id = ?1 AND input_id = ?2 AND status = 'pending'",
                params![session_id, input_id],
            )?;
            Ok(changed > 0)
        }
    
        /// Ctrl+C/停止的统一收尾(D-085):恢复 idle、记录 stopped_by_user、
        /// 取消未完成输入,三步在同一事务内原子提交——中断路径不能再留下
        /// 永久 running 的幽灵会话,也不能只做一半。返回被取消的输入数。
        pub fn finalize_interrupt(&self, session_id: &str) -> Result<usize, StoreError> {
            let tx = self.connection.unchecked_transaction()?;
            let changed = tx.execute(
                "UPDATE sessions SET status = 'idle', updated_at = ?1 WHERE session_id = ?2",
                params![now_ms(), session_id],
            )?;
            if changed == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows.into());
            }
            append_event_tx(
                &tx,
                session_id,
                "session.status_changed",
                &serde_json::json!({ "status": "idle", "reason": "stopped_by_user" }),
            )?;
            // 只回收**还没有结局**的输入。completed/failed 是终态,任何一次停止都
            // 不得回头改写它们——否则历史上早已跑完的输入会被追认为 cancelled。
            let cancelled = tx.execute(
                "UPDATE session_inputs SET status = 'cancelled', finished_at = ?1
                 WHERE session_id = ?2 AND status IN ('pending', 'promoted', 'running')",
                params![now_ms(), session_id],
            )?;
            tx.commit()?;
            Ok(cancelled)
        }
    
        /// 取消会话中尚无结局的输入，供停止运行时清理 queue。
        pub fn cancel_unfinished_inputs(&self, session_id: &str) -> Result<usize, StoreError> {
            let changed = self.connection.execute(
                "UPDATE session_inputs SET status = 'cancelled', finished_at = ?1
                 WHERE session_id = ?2 AND status IN ('pending', 'promoted', 'running')",
                params![now_ms(), session_id],
            )?;
            Ok(changed)
        }
    
        /// promoted → running:输入真的开始执行了。
        pub fn start_input(&self, input_id: &str) -> Result<bool, StoreError> {
            let changed = self.connection.execute(
                "UPDATE session_inputs SET status = 'running'
                 WHERE input_id = ?1 AND status = 'promoted'",
                params![input_id],
            )?;
            Ok(changed > 0)
        }
    
        /// running → completed | failed:给输入一个结局,此后任何停止都不再改写它。
        pub fn finish_input(&self, input_id: &str, ok: bool) -> Result<bool, StoreError> {
            let changed = self.connection.execute(
                "UPDATE session_inputs SET status = ?1, finished_at = ?2
                 WHERE input_id = ?3 AND status IN ('promoted', 'running')",
                params![if ok { "completed" } else { "failed" }, now_ms(), input_id],
            )?;
            Ok(changed > 0)
        }
    
        /// 输入的当前状态(审计与测试用)。
        pub fn input_status(&self, input_id: &str) -> Result<Option<String>, StoreError> {
            self.connection
                .query_row(
                    "SELECT status FROM session_inputs WHERE input_id = ?1",
                    params![input_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
        }
        pub fn cancel_pending_inputs(&self, session_id: &str) -> Result<usize, StoreError> {
            let changed = self.connection.execute(
                "UPDATE session_inputs SET status = 'cancelled'
                 WHERE session_id = ?1 AND status = 'pending'",
                params![session_id],
            )?;
            Ok(changed)
        }
    
        pub fn promote_steers(&self, session_id: &str) -> Result<Vec<AdmittedInput>, StoreError> {
            self.promote_where(session_id, "steer", false)
        }
    
        pub fn promote_next_queue(
            &self,
            session_id: &str,
        ) -> Result<Option<AdmittedInput>, StoreError> {
            Ok(self
                .promote_where(session_id, "queue", true)?
                .into_iter()
                .next())
        }
    
        fn promote_next_steer(
            &self,
            session_id: &str,
        ) -> Result<Option<AdmittedInput>, StoreError> {
            Ok(self
                .promote_where(session_id, "steer", true)?
                .into_iter()
                .next())
        }
    
        /// 优先提升一条 steer；没有 steer 时再提升一条 queue，供运行边界 drain 使用。
        pub fn promote_next_input(
            &self,
            session_id: &str,
        ) -> Result<Option<AdmittedInput>, StoreError> {
            if let Some(input) = self.promote_next_steer(session_id)? {
                return Ok(Some(input));
            }
            self.promote_next_queue(session_id)
        }
    
        fn promote_where(
            &self,
            session_id: &str,
            delivery: &str,
            one: bool,
        ) -> Result<Vec<AdmittedInput>, StoreError> {
            let tx = self.connection.unchecked_transaction()?;
            let limit = if one { " LIMIT 1" } else { "" };
            let sql = format!(
                "SELECT input_id, session_id, prompt, delivery, created_at
                 FROM session_inputs WHERE session_id = ?1 AND delivery = ?2 AND status = 'pending'
                 ORDER BY created_at, rowid{limit}"
            );
            let inputs = {
                let mut statement = tx.prepare(&sql)?;
                let rows = statement.query_map(params![session_id, delivery], input_from_row)?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            for input in &inputs {
                tx.execute(
                    "UPDATE session_inputs SET status = 'promoted', promoted_at = ?1
                     WHERE input_id = ?2 AND status = 'pending'",
                    params![now_ms(), input.input_id],
                )?;
            }
            tx.commit()?;
            Ok(inputs)
        }
    
}

/// 输入行解析。
fn input_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdmittedInput> {
        let delivery: String = row.get(3)?;
        let delivery = match delivery.as_str() {
            "steer" => Delivery::Steer,
            "queue" => Delivery::Queue,
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        Ok(AdmittedInput {
            input_id: row.get(0)?,
            session_id: row.get(1)?,
            prompt: row.get(2)?,
            delivery,
            created_at: row.get(4)?,
        })
    }
    

