//! 记忆反事实评估聚合(R-166)。
//!
//! F(m) = E[J(e;M) − J(e;M∖{m})] 是「遗忘成本」:把记忆 m 从当前集合 M 里拿掉,
//! 回放决策质量 J 平均下降多少。正值为「m 有正价值」(拿掉它决策变差),
//! 负值/零为「m 无价值甚至有害」。
//!
//! 本模块只做**离线**聚合:六臂回放(ReplayCase × Arm)由 replay.rs 驱动,
//! 落 memory_eval 明细后,这里按 memory_id 把 Current 与 LeaveOneOut 两臂的
//! J 判据(success)配对相减、求均值与 95% 近似置信区间,写入 memory_eval_agg。
//! 在线推理路径不读不写本表——评估永远是周期性的 with/without 回放。

use rusqlite::{OptionalExtension, params};

use super::{now_ms, SessionStore, StoreError};

/// 一条记忆的 F(m) 聚合估计(验收①:每条 active 记忆可查估计与置信区间)。
#[derive(Debug, Clone, PartialEq)]
pub struct EffectEstimate {
    pub memory_id: String,
    /// effect_mean = E[J(e;M) − J(e;M∖{m})] 的样本均值(success 差)。
    /// 正 = 保留 m 提升决策质量;负 = 拿掉 m 反而更好。
    pub effect_mean: f64,
    /// 95% 近似置信区间半宽(1.96·σ/√n);n<2 时为 0(样本不足不作区间判断)。
    pub effect_ci: f64,
    /// 参与聚合的配对数(Q(m) 里同时跑过 Current 与 LeaveOneOut 的 case 数)。
    pub eval_n: usize,
    /// 最近一次离线回放聚合的时间戳(ms)。
    pub last_eval: i64,
}

impl SessionStore {
    /// 按 memory_id 读取 F(m) 聚合。没有评估过返回 None(验收①的查询入口)。
    pub fn memory_effect(&self, memory_id: &str) -> Result<Option<EffectEstimate>, StoreError> {
        let row = self
            .connection
            .query_row(
                "SELECT memory_id, effect_mean, effect_ci, eval_n, last_eval
                 FROM memory_eval_agg WHERE memory_id = ?1",
                params![memory_id],
                |row| {
                    Ok(EffectEstimate {
                        memory_id: row.get(0)?,
                        effect_mean: row.get(1)?,
                        effect_ci: row.get(2)?,
                        eval_n: row.get::<_, i64>(3)? as usize,
                        last_eval: row.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// 覆盖写入一条记忆的 F(m) 聚合(离线回放后调用,幂等)。
    pub fn upsert_memory_effect(
        &self,
        estimate: &EffectEstimate,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO memory_eval_agg(memory_id, effect_mean, effect_ci, eval_n, last_eval)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(memory_id) DO UPDATE SET
                 effect_mean = excluded.effect_mean,
                 effect_ci = excluded.effect_ci,
                 eval_n = excluded.eval_n,
                 last_eval = excluded.last_eval",
            params![
                estimate.memory_id,
                estimate.effect_mean,
                estimate.effect_ci,
                estimate.eval_n as i64,
                estimate.last_eval,
            ],
        )?;
        Ok(())
    }

    /// 全量重算某条记忆的 F(m) 聚合(离线 with/without 回放后的收尾)。
    ///
    /// 数据来源:memory_eval 明细里同一 memory_id、同一 replay_case、同 model/
    /// prompt_version 的 current 与 leave_one_out 两臂(success 为 J 判据)。
    /// 只统计两臂都出现过的 case——配对差才有意义,单臂 case 不参与(样本污染)。
    /// 返回重算结果;没有可用配对该 memory_id 时返回 None(不清空旧值,
    /// 历史估计保留,避免一次空跑把已收敛的估计清零)。
    pub fn recompute_memory_effect(
        &self,
        memory_id: &str,
        model: &str,
        prompt_version: &str,
    ) -> Result<Option<EffectEstimate>, StoreError> {
        // 同一 case 的 current/leave_one_out 配对差。
        let mut statement = self.connection.prepare(
            "SELECT c.success, l.success
             FROM memory_eval c
             JOIN memory_eval l
               ON l.memory_id = c.memory_id
              AND l.replay_case = c.replay_case
              AND l.model = c.model
              AND l.prompt_version = c.prompt_version
             WHERE c.memory_id = ?1
               AND c.model = ?2
               AND c.prompt_version = ?3
               AND c.arm = 'current'
               AND l.arm = 'leave_one_out'",
        )?;
        let rows = statement
            .query_map(params![memory_id, model, prompt_version], |row| {
                Ok((row.get::<_, i64>(0)? != 0, row.get::<_, i64>(1)? != 0))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            return Ok(None);
        }
        let n = rows.len();
        // 配对差:Current − LeaveOneOut(拿掉 m 后变化的反向)。
        let diffs: Vec<f64> = rows
            .iter()
            .map(|(c, l)| (*c as i64 - *l as i64) as f64)
            .collect();
        let mean = diffs.iter().sum::<f64>() / n as f64;
        let variance = if n >= 2 {
            diffs.iter().map(|d| (d - mean) * (d - mean)).sum::<f64>() / (n as f64 - 1.0)
        } else {
            0.0
        };
        let std_err = (variance / n as f64).sqrt();
        // 样本不足(n<2)时置信区间无意义,记 0 表示「不可作区间判断」。
        let ci = if n >= 2 { 1.96 * std_err } else { 0.0 };
        let estimate = EffectEstimate {
            memory_id: memory_id.to_string(),
            effect_mean: mean,
            effect_ci: ci,
            eval_n: n,
            last_eval: now_ms(),
        };
        self.upsert_memory_effect(&estimate)?;
        Ok(Some(estimate))
    }
}

/// Q(m):一条记忆的离线回放案例集(R-166 内容②)。
///
/// 三种来源,三类都是 episode(可转 ReplayCase):
/// - `triggered`:该记忆真的被检索并注入过的历史 episode(positive——验证它有用时该赢)。
/// - `near_miss`:该记忆进了候选但最终没被注入的 episode(边界——差一点就用到它)。
/// - `negative_control`:与该记忆无关的 episode(对照——不该因 m 出现而改变行为)。
///
/// 周期性回放就用这个集合做 with/without:Current 注入 m、LeaveOneOut 不注入,
/// 在 triggered/near_miss 上期望 F(m)>0,在 negative_control 上期望 F(m)≈0——
/// 若 negative_control 上也显著偏离,说明 m 在无关场景造成干扰。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EvalCaseSet {
    /// 注入过该记忆的 episode(recall_events.injected_ids 含 memory_id)。
    pub triggered: Vec<i64>,
    /// 候选但未注入的 episode(candidate_ids 含、injected_ids 不含)。
    pub near_miss: Vec<i64>,
    /// 与该记忆无关的 episode(排除前两类,按最近优先取 negative_limit 条)。
    pub negative_control: Vec<i64>,
}

impl SessionStore {
    /// 组装 Q(m):三类 episode 一次查询返回。
    /// `negative_limit` 控制对照集大小(0 = 不要对照)。episode 必须是 episodes 表
    /// 里真实存在的行,才能被转成 ReplayCase。
    pub fn eval_case_set(
        &self,
        memory_id: &str,
        negative_limit: usize,
    ) -> Result<EvalCaseSet, StoreError> {
        let mut out = EvalCaseSet::default();
        // triggered:injected_ids(JSON 数组)里出现过该记忆的 episode。
        let mut triggered = self.connection.prepare(
            "SELECT DISTINCT r.episode_id
             FROM recall_events r, json_each(r.injected_ids) j
             WHERE j.value = ?1 AND r.episode_id IS NOT NULL
             ORDER BY r.episode_id",
        )?;
        let triggered_rows = triggered.query_map(params![memory_id], |row| row.get::<_, i64>(0))?;
        for id in triggered_rows.flatten() {
            out.triggered.push(id);
        }
        // near_miss:候选里有、注入里没有。
        let mut near = self.connection.prepare(
            "SELECT DISTINCT r.episode_id
             FROM recall_events r, json_each(r.candidate_ids) c
             WHERE c.value = ?1
               AND r.episode_id IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1 FROM json_each(r.injected_ids) j WHERE j.value = ?1
               )
             ORDER BY r.episode_id",
        )?;
        let near_rows = near.query_map(params![memory_id], |row| row.get::<_, i64>(0))?;
        for id in near_rows.flatten() {
            out.near_miss.push(id);
        }
        // negative control:与 m 完全无关的最近 episode。
        if negative_limit > 0 {
            let mut excluded = String::new();
            for (idx, id) in out
                .triggered
                .iter()
                .chain(out.near_miss.iter())
                .collect::<Vec<_>>()
                .iter()
                .enumerate()
            {
                if idx > 0 {
                    excluded.push(',');
                }
                excluded.push_str(&id.to_string());
            }
            let sql = if excluded.is_empty() {
                format!(
                    "SELECT episode_id FROM episodes
                     ORDER BY created_at DESC LIMIT ?1"
                )
            } else {
                format!(
                    "SELECT episode_id FROM episodes
                     WHERE episode_id NOT IN ({excluded})
                     ORDER BY created_at DESC LIMIT ?1"
                )
            };
            let mut stmt = self.connection.prepare(&sql)?;
            let rows = stmt.query_map(params![negative_limit as i64], |row| {
                row.get::<_, i64>(0)
            })?;
            for id in rows.flatten() {
                out.negative_control.push(id);
            }
        }
        Ok(out)
    }

    /// 合并守恒 D(S→m')(R-166 内容④,验收②):合并前后在相同 case 上的
    /// 决策质量差。D = E[J(e;M) − J(e;(M∖S)∪{m'})]。
    ///
    /// 数据来源:memory_eval 里同一 memory_id、同一 case 的 `current` 臂
    /// (合并前)与 `merged` 臂(合并后)的 success 配对差,同 model/prompt_version
    /// 内配对。model/prompt_version 传空串 = 任意版本(merge 工具不知道评估
    /// 当时的版本号,用通配取全部历史评估)。返回 (|D| 均值, 配对 case 数);
    /// 无配对数据返回 None(评估器还没跑过该记忆的合并对照,退化为
    /// fingerprint/用户确认保守闸)。
    pub fn merge_conservation_delta(
        &self,
        memory_id: &str,
        model: &str,
        prompt_version: &str,
    ) -> Result<Option<(f64, usize)>, StoreError> {
        let (sql, version_filtered) = if model.is_empty() && prompt_version.is_empty() {
            (
                "SELECT c.success, m.success
                 FROM memory_eval c
                 JOIN memory_eval m
                   ON m.memory_id = c.memory_id
                  AND m.replay_case = c.replay_case
                  AND m.model = c.model
                  AND m.prompt_version = c.prompt_version
                 WHERE c.memory_id = ?1
                   AND c.arm = 'current'
                   AND m.arm = 'merged'",
                false,
            )
        } else {
            (
                "SELECT c.success, m.success
                 FROM memory_eval c
                 JOIN memory_eval m
                   ON m.memory_id = c.memory_id
                  AND m.replay_case = c.replay_case
                  AND m.model = c.model
                  AND m.prompt_version = c.prompt_version
                 WHERE c.memory_id = ?1
                   AND c.model = ?2
                   AND c.prompt_version = ?3
                   AND c.arm = 'current'
                   AND m.arm = 'merged'",
                true,
            )
        };
        let mut statement = self.connection.prepare(sql)?;
        let rows = if version_filtered {
            statement
                .query_map(params![memory_id, model, prompt_version], |row| {
                    Ok((row.get::<_, i64>(0)? != 0, row.get::<_, i64>(1)? != 0))
                })?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            statement
                .query_map(params![memory_id], |row| {
                    Ok((row.get::<_, i64>(0)? != 0, row.get::<_, i64>(1)? != 0))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        if rows.is_empty() {
            return Ok(None);
        }
        let n = rows.len();
        // D = 合并前 − 合并后;取绝对值均值作为「失真度」。
        let sum_abs: f64 = rows
            .iter()
            .map(|(c, m)| (*c as i64 - *m as i64).unsigned_abs() as f64)
            .sum();
        Ok(Some((sum_abs / n as f64, n)))
    }

    /// deprecate 候选筛选(R-166 内容⑥,验收④):只有 low value + high confidence
    /// 才进候选,age 不作为独立淘汰判据。
    ///
    /// - low value:effect_mean ≤ 0(拿掉该记忆决策质量不下降甚至提升)。
    /// - high confidence:eval_n ≥ 3 且 effect_ci ≤ 0.34(95% CI 上界在 0 附近,
    ///   n=3 时 t≈2.9 的临界半宽 0.34 是「CI 不越过正区」的最小可用宽度)。
    ///
    /// 返回候选 memory_id 列表。候选只是「建议」——真正的 deprecated 由
    /// manager 按 reason 落 memory_stale,本函数不写状态(引擎只筛不判,
    /// 淘汰是人的复核动作)。
    pub fn deprecate_candidates(
        &self,
        min_eval_n: usize,
        max_ci: f64,
    ) -> Result<Vec<String>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT memory_id FROM memory_eval_agg
             WHERE effect_mean <= 0
               AND eval_n >= ?1
               AND effect_ci <= ?2
             ORDER BY effect_mean ASC",
        )?;
        let rows = statement
            .query_map(params![min_eval_n as i64, max_ci], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod eval_tests {
    use super::*;
    use crate::store::testutil::store;
    use crate::store::RecallEvent;

    /// 批1(R-166 验收①):F(m) 聚合可算可查——构造 3 个配对 case,
    /// Current 全成功、LeaveOneOut 全失败 → effect_mean = 1.0,
    /// eval_n = 3;查询可读;无数据的 memory 返回 None。
    #[test]
    fn forgetting_cost_aggregates_and_queries() {
        let store = store();
        // M-1:current 全 1、leave_one_out 全 0 → 差恒为 +1,mean=1,ci=0。
        for case in ["c1", "c2", "c3"] {
            store
                .record_memory_eval("M-1", case, "current", "m", "v1", true, 1, 0, 0, 1, None)
                .unwrap();
            store
                .record_memory_eval("M-1", case, "leave_one_out", "m", "v1", false, 1, 0, 0, 1, None)
                .unwrap();
        }
        let est = store
            .recompute_memory_effect("M-1", "m", "v1")
            .unwrap()
            .expect("有配对数据必须算出估计");
        assert_eq!(est.effect_mean, 1.0);
        assert_eq!(est.eval_n, 3);
        assert_eq!(est.effect_ci, 0.0, "方差为零时 CI 为 0");
        let queried = store
            .memory_effect("M-1")
            .unwrap()
            .expect("落库后可查");
        assert_eq!(queried.effect_mean, 1.0);
        assert_eq!(queried.eval_n, 3);
        // 未评估的记忆:None。
        assert!(store.memory_effect("M-nobody").unwrap().is_none());
    }

    /// 批1(R-166 验收①):单臂 case 不参与配对(样本污染防护),且无配对时
    /// 不清空历史估计。
    #[test]
    fn unpaired_cases_are_excluded_and_stale_estimate_survives() {
        let store = store();
        // M-2:先有一条历史估计。
        store
            .upsert_memory_effect(&EffectEstimate {
                memory_id: "M-2".into(),
                effect_mean: 0.5,
                effect_ci: 0.1,
                eval_n: 4,
                last_eval: 1,
            })
            .unwrap();
        // 新回放只有 current 单臂(无 leave_one_out 配对)。
        store
            .record_memory_eval("M-2", "c9", "current", "m", "v1", true, 1, 0, 0, 1, None)
            .unwrap();
        assert!(
            store
                .recompute_memory_effect("M-2", "m", "v1")
                .unwrap()
                .is_none(),
            "无配对时返回 None"
        );
        let kept = store.memory_effect("M-2").unwrap().unwrap();
        assert_eq!(kept.effect_mean, 0.5, "历史估计不被空跑清零");
        assert_eq!(kept.eval_n, 4);
    }

    /// 批1:不同 model/prompt_version 的配对不串(case 只在同版本内配对)。
    #[test]
    fn paired_cases_are_scoped_to_model_and_prompt_version() {
        let store = store();
        store
            .record_memory_eval("M-3", "c1", "current", "m", "v1", true, 1, 0, 0, 1, None)
            .unwrap();
        store
            .record_memory_eval("M-3", "c1", "leave_one_out", "m", "v1", false, 1, 0, 0, 1, None)
            .unwrap();
        // 另一版本只有 current——不得与 v1 配对。
        store
            .record_memory_eval("M-3", "c1", "current", "m", "v2", false, 1, 0, 0, 1, None)
            .unwrap();
        let est = store
            .recompute_memory_effect("M-3", "m", "v1")
            .unwrap()
            .unwrap();
        assert_eq!(est.eval_n, 1);
        // v2 无 leave_one_out 配对 → None。
        assert!(store.recompute_memory_effect("M-3", "m", "v2").unwrap().is_none());
    }

    // -----------------------------------------------------------------------
    // 批2(R-166 内容②):Q(m) 三类 episode 选择。
    // -----------------------------------------------------------------------

    /// 批2:triggered/near_miss/negative_control 三类正确分类。
    /// - e1: injected_ids 含 M-1 → triggered
    /// - e2: candidate_ids 含 M-1、injected 不含 → near_miss
    /// - e3: 与 M-1 无关 → negative_control(当 negative_limit 足够时)
    #[test]
    fn eval_case_set_splits_three_kinds() {
        let store = store();
        let mut ids = Vec::new();
        for (tag, n) in [("a", 1), ("b", 2), ("c", 3)] {
            let id = store
                .append_episode(&crate::store::EpisodeRecord {
                    session_id: "ses",
                    prompt_head: tag,
                    outcome: "ok",
                    tools_json: "[]",
                    context_json: "{}",
                    metrics_json: "{}",
                    provider: "",
                    model: "",
                    run_id: "r",
                    input_id: "i",
                    overflow_json: "[]",
                    ..Default::default()
                })
                .unwrap();
            ids.push(id);
            let _ = n;
        }
        let (e1, e2, e3) = (ids[0], ids[1], ids[2]);
        store
            .record_recall_event(&RecallEvent {
                recall_id: "r1",
                episode_id: Some(e1),
                step_id: Some(1),
                trigger_type: "tool_failure",
                trigger_payload: "{}",
                policy_action: "lexical",
                query: "q",
                candidate_ids: "[\"M-1\"]",
                retrieved_ids: "[\"M-1\"]",
                injected_ids: "[\"M-1\"]",
                lexical_ms: 1,
                embed_ms: 0,
                vector_ms: 0,
                total_ms: 1,
            })
            .unwrap();
        store
            .record_recall_event(&RecallEvent {
                recall_id: "r2",
                episode_id: Some(e2),
                step_id: Some(1),
                trigger_type: "tool_failure",
                trigger_payload: "{}",
                policy_action: "lexical",
                query: "q",
                candidate_ids: "[\"M-1\"]",
                retrieved_ids: "[\"M-1\"]",
                injected_ids: "[]",
                lexical_ms: 1,
                embed_ms: 0,
                vector_ms: 0,
                total_ms: 1,
            })
            .unwrap();
        let set = store.eval_case_set("M-1", 10).unwrap();
        assert_eq!(set.triggered, vec![e1]);
        assert_eq!(set.near_miss, vec![e2]);
        assert!(set.negative_control.contains(&e3), "e3 与 M-1 无关应进对照");
        assert!(!set.negative_control.contains(&e1));
        assert!(!set.negative_control.contains(&e2));
    }

    /// 批2:negative_limit=0 时不要对照集。
    #[test]
    fn eval_case_set_can_disable_negative_control() {
        let store = store();
        let set = store.eval_case_set("M-x", 0).unwrap();
        assert!(set.triggered.is_empty());
        assert!(set.near_miss.is_empty());
        assert!(set.negative_control.is_empty());
    }

    // -----------------------------------------------------------------------
    // 批4(R-166 内容④):合并守恒 D(S→m')。
    // -----------------------------------------------------------------------

    /// 批4(验收②):合并前后行为等价时 D≈0;失真时 D 显著 >0。
    #[test]
    fn merge_conservation_delta_measures_distortion() {
        let store = store();
        // 等价合并:3 个 case 上 current 与 merged 全相同 → D = 0。
        for (case, ok) in [("c1", true), ("c2", false), ("c3", true)] {
            store
                .record_memory_eval("M-4", case, "current", "m", "v1", ok, 1, 0, 0, 1, None)
                .unwrap();
            store
                .record_memory_eval("M-4", case, "merged", "m", "v1", ok, 1, 0, 0, 1, None)
                .unwrap();
        }
        let (delta, n) = store
            .merge_conservation_delta("M-4", "m", "v1")
            .unwrap()
            .expect("有配对数据");
        assert_eq!(delta, 0.0, "行为等价合并 D 必须为 0");
        assert_eq!(n, 3);
        // 失真合并:current 全成功、merged 全失败 → D = 1.0。
        for case in ["c4", "c5"] {
            store
                .record_memory_eval("M-4", case, "current", "m", "v1", true, 1, 0, 0, 1, None)
                .unwrap();
            store
                .record_memory_eval("M-4", case, "merged", "m", "v1", false, 1, 0, 0, 1, None)
                .unwrap();
        }
        let (delta2, n2) = store
            .merge_conservation_delta("M-4", "m", "v1")
            .unwrap()
            .unwrap();
        assert!((delta2 - 0.4).abs() < 1e-9, "合并把成功变失败:前 3 配对差 0、后 2 配对差 1 → 均值 0.4,实得 {delta2}");
        assert_eq!(n2, 5);
        // 无配对数据 → None(退化为保守闸)。
        assert!(store.merge_conservation_delta("M-nobody", "m", "v1").unwrap().is_none());
    }

    // -----------------------------------------------------------------------
    // 批5(R-166 内容⑥,验收④):deprecate 候选 = low value + high confidence。
    // -----------------------------------------------------------------------

    /// 批5:只有 effect_mean≤0 且 eval_n/CI 达标才进候选;高价值、样本不足、
    /// CI 过宽都不进。age(created 新旧)不参与判定。
    #[test]
    fn deprecate_candidates_require_low_value_and_high_confidence() {
        let store = store();
        // low value + high confidence:mean=-1, n=4, ci=0 → 进候选。
        store
            .upsert_memory_effect(&EffectEstimate {
                memory_id: "M-low".into(),
                effect_mean: -1.0,
                effect_ci: 0.0,
                eval_n: 4,
                last_eval: 1,
            })
            .unwrap();
        // 高价值(mean>0)→ 不进。
        store
            .upsert_memory_effect(&EffectEstimate {
                memory_id: "M-high".into(),
                effect_mean: 1.0,
                effect_ci: 0.0,
                eval_n: 4,
                last_eval: 1,
            })
            .unwrap();
        // 样本不足(n=1)→ 不进。
        store
            .upsert_memory_effect(&EffectEstimate {
                memory_id: "M-weak".into(),
                effect_mean: -1.0,
                effect_ci: 0.0,
                eval_n: 1,
                last_eval: 1,
            })
            .unwrap();
        // CI 过宽(不 confidence)→ 不进。
        store
            .upsert_memory_effect(&EffectEstimate {
                memory_id: "M-wide".into(),
                effect_mean: -1.0,
                effect_ci: 0.8,
                eval_n: 4,
                last_eval: 1,
            })
            .unwrap();
        let candidates = store.deprecate_candidates(3, 0.34).unwrap();
        assert_eq!(candidates, vec!["M-low".to_string()], "只有 low+high confidence 进候选");
    }
}
