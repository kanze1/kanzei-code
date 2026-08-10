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

#[cfg(test)]
mod eval_tests {
    use super::*;
    use crate::store::testutil::store;

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
}
