use kanzei_harness::config::{
    Cadence, CommitCadence, FullTestCadence, PushCadence, TargetedTestCadence,
};

/// CLI 与桌面共用的已解析节奏；不再叠加默认规则和覆盖说明。
pub(super) fn effective_verification_policy(c: &Cadence) -> String {
    let full = match c.full_test {
        FullTestCadence::EntryClose => "中/大条目关闭前一次；小条目按改动做定向验证".into(),
        FullTestCadence::EveryCommit => "每次提交前".into(),
        FullTestCadence::EveryNBatches => {
            format!("每 {} 批", c.full_test_batches.unwrap_or(1).max(1))
        }
        FullTestCadence::ReleaseOnly => "仅发布前；正常开发不要求全量".into(),
    };
    let targeted = match c.targeted_test {
        TargetedTestCadence::EveryCommit => "每次提交前，按改动范围选择",
        TargetedTestCadence::Off => "不固定触发，由改动和验收决定",
    };
    let commit = match c.commit {
        CommitCadence::PerBatch => "每批",
        CommitCadence::PerEntry => "每条目",
    };
    let push = match c.push {
        PushCadence::PerCommit => "每次提交后",
        PushCadence::PerEntry => "每条目完成后",
        PushCadence::Periodic => "按项目定期策略",
    };
    format!("<effective-verification-policy>\n来源：合并后的 kanzei.toml [cadence]。\n全量测试：{full}\n定向测试：{targeted}\n提交：{commit}\n推送：{push}（适用于已授权的远端）\n使用本项目真实测试/发布入口；不可用或不适用时说明缺口，不套用其他技术栈命令。发布门禁独立执行。\n</effective-verification-policy>")
}
