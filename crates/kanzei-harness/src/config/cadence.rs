//! 验证与提交节奏域(R-257 B5):Cadence 结构与四个档位枚举、逐键 overlay。
//! 自 config.rs 原样迁出,零行为变更。

use serde::{Deserialize, Serialize};

/// 验证与提交节奏(R-157):把 conventions §1.4 的节奏参数从提示词硬化成可调配置。
/// 每个字段都带 serde default,旧配置没有 `[cadence]` 节时行为与 §1.4 当前默认
/// 逐项一致(conventions §4 向后兼容);层叠合并照既有规矩——项目层只覆盖显式写的键。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Cadence {
    /// 全量测试触发档位。默认 EntryClose:条目关闭前一次(发版前 verify.ps1 是
    /// 独立硬门禁,不受本参数影响,见 A-010)。
    #[serde(default)]
    pub full_test: FullTestCadence,
    /// full_test == EveryNBatches 时的批次间隔 n。
    #[serde(default)]
    pub full_test_batches: Option<u32>,
    /// 定向测试:每次提交前必跑(默认)| off。
    #[serde(default)]
    pub targeted_test: TargetedTestCadence,
    /// 提交粒度:每条目一提交 | 每批一提交(默认,多批大条目按批提交)。
    #[serde(default)]
    pub commit: CommitCadence,
    /// push 频率:条目完成后 push(默认)| 每提交后 push | 定期(与 R-143 并轨)。
    #[serde(default)]
    pub push: PushCadence,
    /// R-144:验收核查节律——自主推进(鞭挞)每关闭 N 条自动插入一轮只读核查
    /// (复用 SubagentBase read/glob/grep,核对已完成条目的验收证据与真实调用方,
    /// 发现问题生成候选缺陷或退回依据,不进入主 conversation/queue)。
    /// 0 = 关闭该机制(默认 3)。
    #[serde(default = "default_verify_every_n")]
    pub verify_every_n: u32,
}

fn default_verify_every_n() -> u32 {
    3
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FullTestCadence {
    /// 条目关闭前一次(§1.4 默认)。
    #[default]
    EntryClose,
    /// 每次提交前全量。
    EveryCommit,
    /// 每 n 批全量一次,间隔见 Cadence::full_test_batches。
    EveryNBatches,
    /// 只发版前跑(verify.ps1 硬门禁,本地开发不跑全量)。
    ReleaseOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetedTestCadence {
    /// 每次提交前必跑(§1.4 默认)。
    #[default]
    EveryCommit,
    /// 关闭定向测试(不推荐;改动面与验证匹配的判断交给模型)。
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitCadence {
    /// 多批大条目每批一提交(§1.4 默认)。
    #[default]
    PerBatch,
    /// 整条目一提交(复杂度小的条目适用)。
    PerEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PushCadence {
    /// 条目完成后 push(§1.4 默认)。
    #[default]
    PerEntry,
    /// 每提交后顺手 push。
    PerCommit,
    /// 定期自动 push(与 R-143 自举循环自动 push 并轨)。
    Periodic,
}

/// D-245:cadence 逐键 overlay。字段非 Option,「没写」与「显式写成默认值」在
/// merge 层不可区分(serde default 把两者都落成默认),所以由调用方把 raw toml
/// `[cadence]` 表里**显式出现的键**传进来,只覆盖这些——与 [limits] 的
/// 「项目层只覆盖显式写的键」同一套层叠语义,避免项目层只调一个 full_test
/// 就把其余字段全部打回默认。
pub(crate) fn overlay_cadence(
    base: &mut Cadence,
    layer: &Cadence,
    written: &std::collections::HashSet<&str>,
) {
    if written.contains("full_test") {
        base.full_test = layer.full_test;
    }
    if written.contains("full_test_batches") {
        base.full_test_batches = layer.full_test_batches;
    }
    if written.contains("targeted_test") {
        base.targeted_test = layer.targeted_test;
    }
    if written.contains("commit") {
        base.commit = layer.commit;
    }
    if written.contains("push") {
        base.push = layer.push;
    }
}

/// kanzei.toml [cadence] 节已知键名单(R-220 单源)。
pub(crate) const CADENCE_KEYS: &[&str] = &[
    "full_test",
    "full_test_batches",
    "targeted_test",
    "commit",
    "push",
    "verify_every_n",
];
