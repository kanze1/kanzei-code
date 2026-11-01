use super::*;

const DAY: &str = "2026-11-01";
const START: i64 = 100_000;
fn settings() -> Settings {
    Settings {
        enabled: true,
        idle_minutes: 0,
        review_minutes: 1,
        quiet_start: 0,
        quiet_end: 0,
        ..Settings::default()
    }
}
fn identity(session: &str) -> Identity {
    Identity {
        session_id: session.into(),
        process_id: format!("process:{session}"),
        root: std::path::PathBuf::from(session),
        channel: Channel::QqGroup,
    }
}
fn context(input: &str, sequence: i64) -> Context {
    Context {
        activity: Activity {
            human_input: Some(input.into()),
            message_sequence: sequence,
        },
        async_generation: 11,
    }
}
fn plan(topic: &str) -> Plan {
    Plan {
        kind: TopicKind::Curiosity,
        topic: topic.into(),
        context: "上次聊到了合成器，想知道后来怎么做的".into(),
        not_before: None,
        expires: None,
    }
}
struct Fixture {
    root: kanzei_base::test_support::TestDir,
    store: Store,
    context: Context,
}
impl Fixture {
    fn new() -> Self {
        let root = kanzei_base::test_support::TestDir::new("kz-initiative-");
        let store = Store::open(&root.join("initiative.db")).unwrap();
        store
            .register(identity("group-a"), Some("测试群"), settings(), START)
            .unwrap();
        let context = context("input-a", 1);
        store
            .observe_activity("group-a", context.activity.clone(), START)
            .unwrap();
        Self {
            root,
            store,
            context,
        }
    }
    fn select(&self, id: &str, at: i64) -> Review {
        self.store
            .begin_review("group-a", id, self.context.clone(), at, DAY, 12)
            .unwrap()
            .unwrap();
        self.store
            .decide(
                id,
                Decision::Speak {
                    topic: TopicSelection::New {
                        plan: plan("想接着聊音色"),
                    },
                    reason: "有延续原话题的具体问题".into(),
                },
                &self.context,
                at,
            )
            .unwrap()
    }
    fn send(&self, id: &str, at: i64, outcome: ReviewState) {
        self.select(id, at);
        self.store
            .bind_run(id, &format!("run:{id}"), &self.context)
            .unwrap();
        self.store
            .begin_send(id, &self.context, at, DAY, 12)
            .unwrap();
        self.store
            .record_delivery(id, outcome, &format!("qq:{id}"), None, at)
            .unwrap();
    }
}

#[test]
fn topics_persist_with_their_original_scope_and_optimistic_revision() {
    let f = Fixture::new();
    f.store
        .register(identity("group-b"), Some("另一个群"), settings(), START)
        .unwrap();
    let saved = f
        .store
        .remember(
            "group-a",
            plan("后来选了什么合成器？"),
            "original-run",
            START,
        )
        .unwrap();
    let reopened = Store::open(&f.root.join("initiative.db")).unwrap();
    assert_eq!(
        reopened
            .topic("group-a", &saved.id)
            .unwrap()
            .unwrap()
            .source_run,
        "original-run"
    );
    assert!(reopened.topics("group-b").unwrap().is_empty());
    assert!(reopened
        .revise_topic("group-b", &saved.id, 1, None, "other-run", START)
        .is_err());
    let revised = reopened
        .revise_topic(
            "group-a",
            &saved.id,
            1,
            Some(plan("改问包络设置")),
            "revision-run",
            START + 1,
        )
        .unwrap();
    assert_eq!(revised.revision, 2);
    assert!(reopened
        .revise_topic("group-a", &saved.id, 1, None, "stale-run", START + 2)
        .is_err());
    assert!(
        reopened
            .register(
                identity("group-a"),
                Some("重开"),
                Settings::default(),
                START + 3
            )
            .unwrap()
            .settings
            .enabled
    );
    let mut changed = identity("group-a");
    changed.process_id = "forged-owner".into();
    assert!(reopened
        .register(changed, Some("错误会话"), settings(), START)
        .is_err());
}

#[test]
fn eligibility_uses_real_activity_idle_time_quiet_hours_and_user_settings() {
    let f = Fixture::new();
    f.store
        .register(identity("untouched"), Some("没有聊天"), settings(), START)
        .unwrap();
    assert_eq!(
        f.store.eligibility("untouched", START, DAY, 12).unwrap(),
        Eligibility::NoConversation
    );
    let mut cfg = settings();
    cfg.idle_minutes = 15;
    cfg.quiet_start = 23;
    cfg.quiet_end = 8;
    f.store
        .save_settings("group-a", 1, cfg.clone(), START)
        .unwrap();
    assert_eq!(
        f.store
            .eligibility("group-a", START + 14 * 60_000, DAY, 12)
            .unwrap(),
        Eligibility::NotDue
    );
    assert_eq!(
        f.store
            .eligibility("group-a", START + 15 * 60_000, DAY, 23)
            .unwrap(),
        Eligibility::Quiet
    );
    assert_eq!(
        f.store
            .eligibility("group-a", START + 15 * 60_000, DAY, 12)
            .unwrap(),
        Eligibility::Ready
    );
    cfg.enabled = false;
    f.store.save_settings("group-a", 2, cfg, START).unwrap();
    assert_eq!(
        f.store
            .eligibility("group-a", START + 15 * 60_000, DAY, 12)
            .unwrap(),
        Eligibility::Disabled
    );
}

#[test]
fn silence_and_deferral_are_durable_decisions_without_an_outgoing_message() {
    let f = Fixture::new();
    f.store
        .begin_review("group-a", "silent", f.context.clone(), START, DAY, 12)
        .unwrap()
        .unwrap();
    assert_eq!(
        f.store.eligibility("group-a", START, DAY, 12).unwrap(),
        Eligibility::Busy
    );
    let silent = f
        .store
        .decide(
            "silent",
            Decision::Silent {
                reason: "刚聊完，没必要硬找一句话".into(),
            },
            &f.context,
            START,
        )
        .unwrap();
    assert_eq!(silent.state, ReviewState::Silent);
    assert!(silent.topic_id.is_none());
    assert!(f.store.topics("group-a").unwrap().is_empty());
    assert!(f
        .store
        .begin_review(
            "group-a",
            "silent",
            f.context.clone(),
            START + 60_000,
            DAY,
            12
        )
        .unwrap()
        .is_none());
    f.store
        .begin_review(
            "group-a",
            "later",
            f.context.clone(),
            START + 60_000,
            DAY,
            12,
        )
        .unwrap()
        .unwrap();
    f.store
        .decide(
            "later",
            Decision::Defer {
                after_minutes: 90,
                reason: "等他忙完这段再聊".into(),
            },
            &f.context,
            START + 60_000,
        )
        .unwrap();
    assert_eq!(
        f.store
            .eligibility("group-a", START + 90 * 60_000, DAY, 12)
            .unwrap(),
        Eligibility::NotDue
    );
    assert_eq!(
        f.store
            .eligibility("group-a", START + 91 * 60_000, DAY, 12)
            .unwrap(),
        Eligibility::Ready
    );
    assert!(f
        .store
        .recent_reviews("group-a")
        .unwrap()
        .iter()
        .all(|review| review.receipt.is_none()));
}

#[test]
fn a_review_can_discover_a_new_topic_without_a_preexisting_reminder() {
    let f = Fixture::new();
    assert!(f.store.topics("group-a").unwrap().is_empty());
    let review = f.select("new-topic", START);
    let topic = f
        .store
        .topic("group-a", review.topic_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(topic.plan.kind, TopicKind::Curiosity);
    assert!(topic.plan.not_before.is_none());
    assert_eq!(topic.source_run, "review:new-topic");
    assert_eq!(review.state, ReviewState::Selected);
    f.store
        .bind_run("new-topic", "original-session-run", &f.context)
        .unwrap();
    assert_eq!(
        f.store
            .review_for_run("group-a", "original-session-run")
            .unwrap()
            .unwrap()
            .id,
        "new-topic"
    );
    assert!(f
        .store
        .review_for_run("group-b", "original-session-run")
        .unwrap()
        .is_none());
    assert!(f
        .store
        .bind_run("new-topic", "another-run", &f.context)
        .is_err());
}

#[test]
fn new_input_settings_changes_and_stop_generation_reject_stale_openings() {
    let f = Fixture::new();
    f.select("stale", START);
    f.store.bind_run("stale", "stale-run", &f.context).unwrap();
    let next = context("input-b", 2);
    f.store
        .observe_activity("group-a", next.activity, START + 1)
        .unwrap();
    assert_eq!(
        f.store.review("stale").unwrap().unwrap().state,
        ReviewState::Cancelled
    );
    assert!(f
        .store
        .begin_send("stale", &f.context, START + 2, DAY, 12)
        .is_err());
    let fresh = Fixture::new();
    fresh.select("settings", START);
    fresh
        .store
        .save_settings("group-a", 1, settings(), START + 1)
        .unwrap();
    assert!(fresh
        .store
        .bind_run("settings", "late-run", &fresh.context)
        .is_err());
    let epoch = Fixture::new();
    epoch.select("epoch", START);
    epoch
        .store
        .bind_run("epoch", "epoch-run", &epoch.context)
        .unwrap();
    let mut changed = epoch.context.clone();
    changed.async_generation += 1;
    assert!(epoch
        .store
        .begin_send("epoch", &changed, START, DAY, 12)
        .is_err());
}

#[test]
fn send_checks_current_topic_revision_expiry_and_quiet_hours_again() {
    let f = Fixture::new();
    let selected = f.select("topic", START);
    f.store.bind_run("topic", "topic-run", &f.context).unwrap();
    f.store
        .revise_topic(
            "group-a",
            selected.topic_id.as_deref().unwrap(),
            1,
            Some(plan("新问题")),
            "human-revision",
            START + 1,
        )
        .unwrap();
    assert!(f
        .store
        .begin_send("topic", &f.context, START + 2, DAY, 12)
        .is_err());
    assert_eq!(
        f.store.review("topic").unwrap().unwrap().state,
        ReviewState::Cancelled
    );
    assert!(f.store.active_review("group-a").unwrap().is_none());
    let exp = Fixture::new();
    let mut limited = plan("今天的话题");
    limited.expires = Some(START + 100);
    let topic = exp
        .store
        .remember("group-a", limited, "source", START)
        .unwrap();
    exp.store
        .begin_review("group-a", "expires", exp.context.clone(), START, DAY, 12)
        .unwrap()
        .unwrap();
    exp.store
        .decide(
            "expires",
            Decision::Speak {
                topic: TopicSelection::Existing {
                    id: topic.id,
                    revision: 1,
                },
                reason: "今天有相关内容".into(),
            },
            &exp.context,
            START,
        )
        .unwrap();
    exp.store
        .bind_run("expires", "expires-run", &exp.context)
        .unwrap();
    assert!(exp
        .store
        .begin_send("expires", &exp.context, START + 100, DAY, 12)
        .is_err());
    let quiet = Fixture::new();
    let mut cfg = settings();
    cfg.quiet_start = 23;
    cfg.quiet_end = 8;
    quiet.store.save_settings("group-a", 1, cfg, START).unwrap();
    quiet.select("quiet", START);
    quiet
        .store
        .bind_run("quiet", "quiet-run", &quiet.context)
        .unwrap();
    assert!(quiet
        .store
        .begin_send("quiet", &quiet.context, START, DAY, 23)
        .is_err());
}

#[test]
fn confirmed_and_unknown_sends_count_toward_caps_and_same_millisecond_replies_reset_only_unanswered_budget(
) {
    let mut f = Fixture::new();
    f.send("one", START, ReviewState::Sent);
    f.send("two", START + 60_000, ReviewState::Unknown);
    assert_eq!(
        f.store
            .eligibility("group-a", START + 120_000, DAY, 12)
            .unwrap(),
        Eligibility::UnansweredLimit
    );
    f.context = context("new-human-reply", 3);
    f.store
        .observe_activity("group-a", f.context.activity.clone(), START + 60_000)
        .unwrap();
    assert_eq!(
        f.store
            .eligibility("group-a", START + 120_000, DAY, 12)
            .unwrap(),
        Eligibility::Ready
    );
    f.send("three", START + 120_000, ReviewState::Sent);
    assert_eq!(
        f.store
            .eligibility("group-a", START + 180_000, DAY, 12)
            .unwrap(),
        Eligibility::DailyLimit
    );
    assert_eq!(
        f.store
            .eligibility("group-a", START + 180_000, "2026-11-02", 12)
            .unwrap(),
        Eligibility::Ready
    );
    assert!(f
        .store
        .begin_send("one", &f.context, START + 180_000, DAY, 12)
        .is_err());
}

#[test]
fn restarting_preserves_intentions_but_never_replays_interrupted_or_uncertain_sends() {
    let f = Fixture::new();
    f.select("interrupted", START);
    assert_eq!(f.store.recover(START + 1).unwrap(), 1);
    assert_eq!(
        f.store.review("interrupted").unwrap().unwrap().state,
        ReviewState::Interrupted
    );
    assert!(f
        .store
        .bind_run("interrupted", "replayed-run", &f.context)
        .is_err());
    assert_eq!(
        f.store.topics("group-a").unwrap()[0].state,
        TopicState::Active
    );
    f.select("sending", START + 60_000);
    f.store
        .bind_run("sending", "sending-run", &f.context)
        .unwrap();
    f.store
        .begin_send("sending", &f.context, START + 60_000, DAY, 12)
        .unwrap();
    let reopened = Store::open(&f.root.join("initiative.db")).unwrap();
    assert_eq!(reopened.recover(START + 60_001).unwrap(), 1);
    assert_eq!(
        reopened.review("sending").unwrap().unwrap().state,
        ReviewState::Unknown
    );
    let review = reopened.review("sending").unwrap().unwrap();
    assert_eq!(
        reopened
            .topic("group-a", review.topic_id.as_deref().unwrap())
            .unwrap()
            .unwrap()
            .state,
        TopicState::Uncertain
    );
    assert!(reopened
        .begin_send("sending", &f.context, START + 60_002, DAY, 12)
        .is_err());
    reopened
        .record_delivery(
            "sending",
            ReviewState::Sent,
            "confirmed-channel-receipt",
            None,
            START + 60_003,
        )
        .unwrap();
    assert_eq!(
        reopened.review("sending").unwrap().unwrap().state,
        ReviewState::Sent
    );
    reopened
        .record_delivery(
            "sending",
            ReviewState::Sent,
            "confirmed-channel-receipt",
            None,
            START + 60_003,
        )
        .unwrap();
}

#[test]
fn stopping_and_real_failures_suspend_reviews_until_new_activity_or_user_resumption() {
    let f = Fixture::new();
    f.select("stop", START);
    f.store.pause("group-a", START + 1).unwrap();
    assert_eq!(
        f.store
            .eligibility("group-a", START + 60_000, DAY, 12)
            .unwrap(),
        Eligibility::Stopped
    );
    assert!(!f
        .store
        .observe_activity("group-a", f.context.activity.clone(), START + 60_000)
        .unwrap());
    assert_eq!(
        f.store.target("group-a").unwrap().unwrap().state,
        TargetState::Stopped
    );
    f.store
        .observe_activity("group-a", context("resumed", 2).activity, START + 60_000)
        .unwrap();
    assert_eq!(
        f.store
            .eligibility("group-a", START + 60_000, DAY, 12)
            .unwrap(),
        Eligibility::Ready
    );
    let failure = Fixture::new();
    failure
        .store
        .begin_review(
            "group-a",
            "failure",
            failure.context.clone(),
            START,
            DAY,
            12,
        )
        .unwrap();
    failure
        .store
        .fail_review("failure", "模型配置不可用")
        .unwrap();
    assert_eq!(
        failure
            .store
            .eligibility("group-a", START + 60_000, DAY, 12)
            .unwrap(),
        Eligibility::Faulted
    );
    assert_eq!(
        failure
            .store
            .target("group-a")
            .unwrap()
            .unwrap()
            .error
            .as_deref(),
        Some("模型配置不可用")
    );
    failure
        .store
        .save_settings("group-a", 1, settings(), START + 60_000)
        .unwrap();
    assert_eq!(
        failure
            .store
            .eligibility("group-a", START + 60_000, DAY, 12)
            .unwrap(),
        Eligibility::Ready
    );
}
