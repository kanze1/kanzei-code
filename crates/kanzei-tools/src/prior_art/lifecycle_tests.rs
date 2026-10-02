use std::path::PathBuf;

use kanzei_harness::{Tool, ToolCtx};
use serde_json::json;

use super::{entry_status, EntryPriorArtStatus, PriorArtStatus};
use crate::docstore::{DocStore, Entry, DEFECTS, REQUIREMENTS};
use crate::tracker::TrackerTool;
use crate::work::WorkTool;

struct Fixture {
    root: PathBuf,
    ctx: ToolCtx,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "kz-prior-art-lifecycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::create_dir_all(root.join("docs/design")).unwrap();
        std::fs::write(root.join("docs/design/base.md"), "# 基线\n已有设计\n").unwrap();
        DocStore::open(&root, &DEFECTS).save(&[]).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        Self { root, ctx }
    }

    fn entry(&self) -> Entry {
        DocStore::open(&self.root, &REQUIREMENTS)
            .load()
            .unwrap()
            .remove(0)
    }

    fn state(&self) -> EntryPriorArtStatus {
        entry_status(&self.root, &self.entry()).unwrap()
    }

    async fn add_core(&self) {
        let output = req().execute(json!({
            "action":"add", "title":"收藏库", "priority":"P1", "tag":"核心", "complexity":"小",
            "fields":{"原始描述":"我想先把番剧收藏库需求记下来", "验收":"添加并保存作品"}
        }), &self.ctx).await;
        assert!(!output.is_error, "{}", output.content);
        assert_eq!(self.entry().id, "R-001");
        assert_eq!(self.entry().status, "todo");
    }

    fn complete(&self) -> String {
        let relative = self.state().path.unwrap();
        let topic = relative.split('/').nth(2).unwrap();
        std::fs::write(self.root.join(&relative), format!(
            "---\nkind: prior_art\ntopic: {topic}\nstatus: complete\ntrigger: core_requirement\nentry_refs: R-001\nwebsearch_round_limit: 2\n---\n\n## 外部已有实现\n\n### upstream\n- 出处: https://example.test/upstream\n- 证据等级: V1\n- 差异: 上游只覆盖单机\n- 决策: 采用数据结构\n\n## 仓内既有设计\n\n### baseline\n- 出处: file:docs/design/base.md:2\n- 证据等级: V2\n- 差异: 仓内缺少同步\n- 决策: 保留现有保存机制\n"
        )).unwrap();
        relative
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

fn req() -> TrackerTool {
    TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    }
}

#[tokio::test]
async fn capture_preserves_original_and_pending_artifact_without_implementing() {
    let f = Fixture::new();
    f.add_core().await;
    let entry = f.entry();
    assert!(entry
        .fields
        .iter()
        .any(|(key, value)| key == "原始描述" && value == "我想先把番剧收藏库需求记下来"));
    assert!(!entry.fields.iter().any(|(key, _)| key == "先行调研豁免"));
    let state = f.state();
    assert_eq!(state.status, PriorArtStatus::Pending);
    assert!(f.root.join(state.path.unwrap()).is_file());
    let before = std::fs::read(f.root.join(".kanzei/project/requirements.md")).unwrap();
    let blocked = req()
        .execute(
            json!({"action":"update","id":"R-001","status":"doing"}),
            &f.ctx,
        )
        .await;
    assert!(blocked.is_error, "{}", blocked.content);
    assert_eq!(blocked.code, Some("PRIOR_ART_REQUIRED"));
    assert_eq!(
        before,
        std::fs::read(f.root.join(".kanzei/project/requirements.md")).unwrap()
    );
}

#[tokio::test]
async fn ordinary_claim_waits_for_real_both_sided_evidence_and_revalidates_resume() {
    let f = Fixture::new();
    f.add_core().await;
    let claim = json!({"action":"claim","id":"R-001"});
    let blocked = WorkTool.execute(claim.clone(), &f.ctx).await;
    assert!(blocked.is_error, "{}", blocked.content);
    assert_eq!(blocked.code, Some("PRIOR_ART_REQUIRED"));
    assert_eq!(f.entry().status, "todo");
    let relative = f.complete();
    assert_eq!(f.state().status, PriorArtStatus::Complete);
    let allowed = WorkTool.execute(claim.clone(), &f.ctx).await;
    assert!(!allowed.is_error, "{}", allowed.content);
    assert_eq!(f.entry().status, "doing");
    let text = std::fs::read_to_string(f.root.join(&relative)).unwrap();
    std::fs::write(
        f.root.join(&relative),
        text.replace("- 出处: https://example.test/upstream\n", ""),
    )
    .unwrap();
    assert_eq!(f.state().status, PriorArtStatus::Invalid);
    let resume = WorkTool.execute(claim, &f.ctx).await;
    assert!(resume.is_error, "{}", resume.content);
    assert_eq!(resume.code, Some("PRIOR_ART_REQUIRED"));
}

#[tokio::test]
async fn work_unit_claim_keeps_parent_todo_and_unit_unclaimed_until_evidence_complete() {
    let f = Fixture::new();
    f.add_core().await;
    let updated = req()
        .execute(
            json!({"action":"update","id":"R-001","fields":{"执行模型":"work_units_v1"}}),
            &f.ctx,
        )
        .await;
    assert!(!updated.is_error, "{}", updated.content);
    let created = WorkTool.execute(json!({
        "action":"create_unit", "requirement_id":"R-001", "objective":"实现收藏保存",
        "scope":["src"], "acceptance":["保存作品后可读取"], "verification":["收藏保存测试"], "base_revision":"base-head"
    }), &f.ctx).await;
    assert!(!created.is_error, "{}", created.content);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&f.root)).unwrap();
    let before = store.get_work_unit("R-001/W1").unwrap().unwrap();
    let claim = json!({"action":"claim","id":"R-001/W1"});
    let blocked = WorkTool.execute(claim.clone(), &f.ctx).await;
    assert!(blocked.is_error, "{}", blocked.content);
    assert_eq!(blocked.code, Some("PRIOR_ART_REQUIRED"));
    assert_eq!(f.entry().status, "todo");
    let after = store.get_work_unit("R-001/W1").unwrap().unwrap();
    assert_eq!(before.status, after.status);
    assert_eq!(before.claimed_by, after.claimed_by);
    let relative = f.complete();
    let allowed = WorkTool.execute(claim.clone(), &f.ctx).await;
    assert!(!allowed.is_error, "{}", allowed.content);
    assert_eq!(f.entry().status, "doing");
    assert_eq!(
        store.get_work_unit("R-001/W1").unwrap().unwrap().status,
        kanzei_core::WorkUnitStatus::Active
    );
    std::fs::remove_file(f.root.join(relative)).unwrap();
    let resume = WorkTool.execute(claim, &f.ctx).await;
    assert!(resume.is_error, "{}", resume.content);
    assert_eq!(resume.code, Some("PRIOR_ART_REQUIRED"));
}

#[tokio::test]
async fn update_accepts_complete_top_level_artifact_and_rejects_fake_completion() {
    let f = Fixture::new();
    f.add_core().await;
    let relative = f.complete();
    let complete = std::fs::read_to_string(f.root.join(&relative)).unwrap();
    for text in [
        complete
            .split("## 仓内既有设计")
            .next()
            .unwrap()
            .to_string(),
        complete.replace("entry_refs: R-001", "entry_refs: R-999"),
        complete.replace("file:docs/design/base.md:2", "file:docs/design/base.md:999"),
    ] {
        std::fs::write(f.root.join(&relative), text).unwrap();
        assert_eq!(f.state().status, PriorArtStatus::Invalid);
        let refused = req()
            .execute(
                json!({"action":"update","id":"R-001","status":"doing","prior_art":relative}),
                &f.ctx,
            )
            .await;
        assert!(refused.is_error, "{}", refused.content);
        assert_eq!(f.entry().status, "todo");
    }
    std::fs::write(f.root.join(&relative), complete).unwrap();
    let allowed = req()
        .execute(
            json!({"action":"update","id":"R-001","status":"doing","prior_art":relative}),
            &f.ctx,
        )
        .await;
    assert!(!allowed.is_error, "{}", allowed.content);
    assert_eq!(f.entry().status, "doing");
    assert_eq!(f.state().status, PriorArtStatus::Complete);
}

#[tokio::test]
async fn audit_fields_cannot_be_erased_or_replaced_through_free_fields_or_relabeling() {
    let f = Fixture::new();
    f.add_core().await;
    let path = f.state().path;
    for input in [
        json!({"action":"update","id":"R-001","fields":{"先行调研":""}}),
        json!({"action":"update","id":"R-001","fields":{"先行调研豁免":"用户没有作出这个决定却伪造一条理由"}}),
        json!({"action":"update","id":"R-001","prior_art":"../prior-art.md"}),
        json!({"action":"update","id":"R-001","prior_art":".kanzei/research/missing/prior-art.md"}),
        json!({"action":"update","id":"R-001","prior_art":""}),
        json!({"action":"update","id":"R-001","prior_art_waiver":"跳过"}),
    ] {
        let refused = req().execute(input, &f.ctx).await;
        assert!(refused.is_error, "{}", refused.content);
        assert_eq!(f.state().status, PriorArtStatus::Pending);
        assert_eq!(f.state().path, path);
    }
    let relabel = req()
        .execute(
            json!({"action":"update","id":"R-001","tag":"前端","refs":["R-099"]}),
            &f.ctx,
        )
        .await;
    assert!(!relabel.is_error, "{}", relabel.content);
    let refused = req()
        .execute(
            json!({"action":"update","id":"R-001","status":"doing"}),
            &f.ctx,
        )
        .await;
    assert!(refused.is_error, "{}", refused.content);
    assert_eq!(refused.code, Some("PRIOR_ART_REQUIRED"));
}

#[tokio::test]
async fn explicit_waiver_is_audited_and_replaces_artifact_without_inventing_reason() {
    let f = Fixture::new();
    f.add_core().await;
    let reason = "用户明确要求此次复用已经调研的方案直接实施";
    let allowed = req()
        .execute(
            json!({"action":"update","id":"R-001","status":"doing","prior_art_waiver":reason}),
            &f.ctx,
        )
        .await;
    assert!(!allowed.is_error, "{}", allowed.content);
    assert_eq!(f.entry().status, "doing");
    assert_eq!(f.state().status, PriorArtStatus::Waived);
    assert_eq!(f.state().path, None);
    assert!(f
        .entry()
        .fields
        .iter()
        .any(|(key, value)| key == "先行调研豁免" && value == reason));
    assert!(!f.entry().fields.iter().any(|(key, _)| key == "先行调研"));
    let claimed = WorkTool
        .execute(json!({"action":"claim","id":"R-001"}), &f.ctx)
        .await;
    assert!(!claimed.is_error, "{}", claimed.content);
}

#[tokio::test]
async fn invalid_artifact_or_short_waiver_on_add_never_creates_a_requirement() {
    let f = Fixture::new();
    for additional in [
        json!({"prior_art":"../pretend.md"}),
        json!({"prior_art_waiver":"跳过"}),
        json!({"fields":{"先行调研豁免":"伪造看似明确的长理由"}}),
    ] {
        let mut input = json!({"action":"add","title":"新的方向","priority":"P1","tag":"核心","complexity":"小"});
        for (key, value) in additional.as_object().unwrap() {
            input[key] = value.clone();
        }
        let refused = req().execute(input, &f.ctx).await;
        assert!(refused.is_error, "{}", refused.content);
        assert!(DocStore::open(&f.root, &REQUIREMENTS)
            .load()
            .unwrap()
            .is_empty());
    }
}

#[tokio::test]
async fn legacy_requirements_and_normal_new_requirements_keep_existing_lifecycle() {
    let f = Fixture::new();
    let old = Entry {
        id: "R-001".into(),
        title: "既有核心需求".into(),
        status: "todo".into(),
        severity: None,
        fields: vec![
            ("标签".into(), "核心".into()),
            ("复杂度".into(), "小".into()),
        ],
    };
    assert!(entry_status(&f.root, &old).is_none());
    DocStore::open(&f.root, &REQUIREMENTS).save(&[old]).unwrap();
    let allowed = req()
        .execute(
            json!({"action":"update","id":"R-001","status":"doing"}),
            &f.ctx,
        )
        .await;
    assert!(!allowed.is_error, "{}", allowed.content);
    let ordinary = req().execute(json!({"action":"add","title":"普通保存按钮","priority":"P2","tag":"前端","complexity":"小"}), &f.ctx).await;
    assert!(!ordinary.is_error, "{}", ordinary.content);
    let entries = DocStore::open(&f.root, &REQUIREMENTS).load().unwrap();
    assert!(entry_status(&f.root, &entries[1]).is_none());
    let mut audited_old = entries[1].clone();
    audited_old.fields.push((
        "先行调研豁免".into(),
        "用户明确决定复用原有方案直接实施".into(),
    ));
    assert_eq!(
        entry_status(&f.root, &audited_old).unwrap().status,
        PriorArtStatus::Waived
    );
    super::check_start(&f.root, &audited_old).unwrap();
}

#[tokio::test]
async fn existing_complete_artifacts_validate_and_duplicate_audits_are_invalid() {
    let f = Fixture::new();
    f.add_core().await;
    f.complete();
    let mut old = f.entry();
    assert_eq!(
        entry_status(&f.root, &old).unwrap().status,
        PriorArtStatus::Complete
    );
    super::check_start(&f.root, &old).unwrap();
    old.fields.push((
        "先行调研豁免".into(),
        "用户明确决定复用原有方案直接实施".into(),
    ));
    assert_eq!(
        entry_status(&f.root, &old).unwrap().status,
        PriorArtStatus::Invalid
    );
    assert!(super::check_start(&f.root, &old).is_err());
}
