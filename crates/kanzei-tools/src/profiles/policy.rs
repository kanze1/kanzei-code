use kanzei_harness::config::{
    Cadence, CommitCadence, FullTestCadence, PushCadence, TargetedTestCadence,
};
use kanzei_harness::ProfileKind;

/// 已合并配置的模型可见偏好，不调度测试，也不授予提交或推送权限。
pub(super) fn effective_verification_policy(profile: ProfileKind, c: &Cadence) -> Option<String> {
    let mode = match profile {
        ProfileKind::DevAuto => "Auto 自主划分工作批次，按下列节奏安排已授权的验证与交付。",
        ProfileKind::DevPair => {
            "Pair 按当前请求选择验证范围；提交、推送节奏仅在任务包含这些操作时适用。"
        }
        _ => return None,
    };
    let full = match c.full_test {
        FullTestCadence::EntryClose => "条目关闭前".into(),
        FullTestCadence::EveryCommit => "每次提交前".into(),
        FullTestCadence::EveryNBatches => match c.full_test_batches {
            Some(0) => "按批次（间隔 0 无效，full_test_batches 应为正整数）".into(),
            Some(interval) => format!("每 {interval} 个实际工作批次"),
            None => "按批次（full_test_batches 尚未设置）".into(),
        },
        FullTestCadence::ReleaseOnly => "仅发布前".into(),
    };
    let targeted = match c.targeted_test {
        TargetedTestCadence::EveryCommit => "每次提交前，按改动范围选择",
        TargetedTestCadence::Off => "按改动和验收自主决定",
    };
    let commit = match c.commit {
        CommitCadence::PerBatch => "按实际工作批次",
        CommitCadence::PerEntry => "按条目",
    };
    let push = match c.push {
        PushCadence::PerCommit => "每次提交后",
        PushCadence::PerEntry => "每条目完成后",
        PushCadence::Periodic => "按项目约定安排",
    };
    Some(format!("<effective-verification-policy>\n来源：合并后的 kanzei.toml [cadence]。{mode}\n全量测试：{full}\n定向测试：{targeted}\n提交：{commit}\n推送：{push}\n按实际改动选择验证方法，使用项目入口并复用仍适用的结果。提交、推送遵守当前任务和已有授权；发布门禁按项目规范执行。\n</effective-verification-policy>"))
}
