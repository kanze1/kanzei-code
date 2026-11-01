//! Conversation-scoped intentions and review leases. Channel stores own sends.
use super::model::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::path::Path;

pub(crate) struct Store {
    db: Connection,
}
impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(10))?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS initiative_targets(id TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS initiative_topics(id TEXT PRIMARY KEY, session TEXT NOT NULL, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS initiative_reviews(id TEXT PRIMARY KEY, session TEXT NOT NULL, data TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS initiative_topic_owner ON initiative_topics(session);
             CREATE INDEX IF NOT EXISTS initiative_review_owner ON initiative_reviews(session,json_extract(data,'$.created_at'));
             CREATE UNIQUE INDEX IF NOT EXISTS initiative_live_review ON initiative_reviews(session)
               WHERE json_extract(data,'$.state') IN ('reviewing','selected','running','sending');
             CREATE UNIQUE INDEX IF NOT EXISTS initiative_run ON initiative_reviews(json_extract(data,'$.run_id'))
               WHERE json_extract(data,'$.run_id') IS NOT NULL;",
        )?;
        Ok(Self { db })
    }
    fn transaction(&self) -> anyhow::Result<Transaction<'_>> {
        Ok(Transaction::new_unchecked(
            &self.db,
            TransactionBehavior::Immediate,
        )?)
    }
    pub fn target(&self, session: &str) -> anyhow::Result<Option<Target>> {
        let data: Option<String> = self
            .db
            .query_row(
                "SELECT data FROM initiative_targets WHERE id=?1",
                [session],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }
    fn require_target(&self, session: &str) -> anyhow::Result<Target> {
        self.target(session)?
            .ok_or_else(|| anyhow::anyhow!("主动话题缺少原会话"))
    }
    fn put_target(&self, target: &Target) -> anyhow::Result<()> {
        self.db.execute(
            "INSERT INTO initiative_targets(id,data) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![target.identity.session_id, serde_json::to_string(target)?],
        )?;
        Ok(())
    }
    pub fn register(
        &self,
        identity: Identity,
        title: Option<&str>,
        settings: Settings,
        now: i64,
    ) -> anyhow::Result<Target> {
        settings.validate()?;
        anyhow::ensure!(
            !identity.session_id.is_empty() && !identity.process_id.is_empty(),
            "会话身份不能为空"
        );
        let tx = self.transaction()?;
        let target = if let Some(mut target) = self.target(&identity.session_id)? {
            anyhow::ensure!(target.identity == identity, "主动话题会话身份发生变化");
            if let Some(title) = title {
                target.title = title.into();
            }
            target
        } else {
            Target {
                title: title
                    .unwrap_or(match identity.channel {
                        Channel::Desktop => "大肥鱼",
                        Channel::QqFriend => "QQ 好友",
                        Channel::QqGroup => "QQ 群聊",
                        Channel::Bilibili => "B站浏览",
                    })
                    .into(),
                identity,
                settings,
                revision: 1,
                state: TargetState::Ready,
                activity: Activity::default(),
                last_activity: None,
                last_review: None,
                next_review: now,
                error: None,
            }
        };
        self.put_target(&target)?;
        tx.commit()?;
        Ok(target)
    }
    pub fn save_settings(
        &self,
        session: &str,
        expected: u64,
        settings: Settings,
        now: i64,
    ) -> anyhow::Result<Target> {
        settings.validate()?;
        let tx = self.transaction()?;
        let mut target = self.require_target(session)?;
        anyhow::ensure!(
            target.revision == expected,
            "主动搭话设置已变化，请刷新后再保存"
        );
        target.settings = settings;
        target.revision = target
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("设置版本超出范围"))?;
        target.state = TargetState::Ready;
        target.error = None;
        target.next_review = now;
        self.cancel_open(session, "主动搭话设置已变化", now)?;
        self.put_target(&target)?;
        tx.commit()?;
        Ok(target)
    }
    pub fn observe_activity(
        &self,
        session: &str,
        activity: Activity,
        now: i64,
    ) -> anyhow::Result<bool> {
        let tx = self.transaction()?;
        let mut target = self.require_target(session)?;
        if activity.is_empty() || target.activity == activity {
            return Ok(false);
        }
        self.cancel_open(session, "原会话有新消息，旧主动话题不再发送", now)?;
        target.activity = activity;
        target.last_activity = Some(now);
        target.state = TargetState::Ready;
        target.error = None;
        let idle = after_minutes(now, target.settings.idle_minutes)?;
        target.next_review = match target.last_review {
            Some(at) => idle.max(after_minutes(at, target.settings.review_minutes)?),
            None => idle,
        };
        self.put_target(&target)?;
        tx.commit()?;
        Ok(true)
    }
    pub fn pause(&self, session: &str, now: i64) -> anyhow::Result<()> {
        let tx = self.transaction()?;
        let mut target = self.require_target(session)?;
        target.state = TargetState::Stopped;
        target.error = None;
        self.cancel_open(session, "用户已停止原会话", now)?;
        self.put_target(&target)?;
        tx.commit()?;
        Ok(())
    }
    pub fn due_targets(&self, now: i64) -> anyhow::Result<Vec<Target>> {
        let mut query = self.db.prepare(
            "SELECT data FROM initiative_targets WHERE json_extract(data,'$.settings.enabled')=1
               AND json_extract(data,'$.state')='ready' AND json_extract(data,'$.next_review')<=?1
             ORDER BY json_extract(data,'$.next_review'),id",
        )?;
        let rows = query
            .query_map([now], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|data| serde_json::from_str(&data).map_err(Into::into))
            .collect()
    }
    pub fn targets(&self) -> anyhow::Result<Vec<Target>> {
        let mut query = self
            .db
            .prepare("SELECT data FROM initiative_targets ORDER BY id")?;
        let rows = query
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|data| serde_json::from_str(&data).map_err(Into::into))
            .collect()
    }
    pub fn active_review(&self, session: &str) -> anyhow::Result<Option<Review>> {
        let data:Option<String>=self.db.query_row("SELECT data FROM initiative_reviews WHERE session=?1 AND json_extract(data,'$.state') IN ('reviewing','selected','running','sending')",[session],|row|row.get(0)).optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }
    pub fn fault_target(&self, session: &str, error: &str) -> anyhow::Result<()> {
        let tx = self.transaction()?;
        let mut target = self.require_target(session)?;
        target.state = TargetState::Faulted;
        target.error = Some(error.into());
        self.put_target(&target)?;
        tx.commit()?;
        Ok(())
    }
    pub fn cancel_review(&self, id: &str, reason: &str) -> anyhow::Result<()> {
        let tx = self.transaction()?;
        let mut review = self
            .review(id)?
            .ok_or_else(|| anyhow::anyhow!("主动评估不存在"))?;
        anyhow::ensure!(
            review.state != ReviewState::Sending,
            "发送中的消息必须结算真实渠道结果"
        );
        if review.state.active() {
            review.state = ReviewState::Cancelled;
            review.error = Some(reason.into());
            self.put_review(&review)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn finish_without_message(&self, id: &str, reason: &str) -> anyhow::Result<()> {
        let tx = self.transaction()?;
        let mut review = self
            .review(id)?
            .ok_or_else(|| anyhow::anyhow!("主动评估不存在"))?;
        if matches!(review.state, ReviewState::Running | ReviewState::Selected) {
            review.state = ReviewState::Silent;
            review.error = Some(reason.into());
            self.put_review(&review)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn eligibility(
        &self,
        session: &str,
        now: i64,
        day: &str,
        hour: u32,
    ) -> anyhow::Result<Eligibility> {
        anyhow::ensure!(hour < 24, "当前小时无效");
        chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")?;
        let target = self.require_target(session)?;
        if !target.settings.enabled {
            return Ok(Eligibility::Disabled);
        }
        match target.state {
            TargetState::Stopped => return Ok(Eligibility::Stopped),
            TargetState::Faulted => return Ok(Eligibility::Faulted),
            TargetState::Ready => {}
        }
        let Some(activity_at) = target.last_activity else {
            return Ok(Eligibility::NoConversation);
        };
        let active: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM initiative_reviews WHERE session=?1 AND json_extract(data,'$.state') IN ('reviewing','selected','running','sending'))",
            [session], |row| row.get(0),
        )?;
        if active {
            return Ok(Eligibility::Busy);
        }
        if now < target.next_review
            || now < after_minutes(activity_at, target.settings.idle_minutes)?
        {
            return Ok(Eligibility::NotDue);
        }
        if target.settings.quiet(hour) {
            return Ok(Eligibility::Quiet);
        }
        self.message_budget(&target, day)
    }
    fn message_budget(&self, target: &Target, day: &str) -> anyhow::Result<Eligibility> {
        chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")?;
        let session = &target.identity.session_id;
        let daily: u32 = self.db.query_row(
            "SELECT COUNT(*) FROM initiative_reviews WHERE session=?1 AND json_extract(data,'$.day')=?2 AND json_extract(data,'$.state') IN ('sent','unknown')",
            params![session, day], |row| row.get(0),
        )?;
        if daily >= target.settings.daily_messages {
            return Ok(Eligibility::DailyLimit);
        }
        let unanswered: u32 = self.db.query_row(
            "SELECT COUNT(*) FROM initiative_reviews WHERE session=?1 AND json_extract(data,'$.context.activity')=?2 AND json_extract(data,'$.state') IN ('sent','unknown')",
            params![session, serde_json::to_string(&target.activity)?], |row| row.get(0),
        )?;
        if unanswered >= target.settings.unanswered_messages {
            return Ok(Eligibility::UnansweredLimit);
        }
        Ok(Eligibility::Ready)
    }
    pub fn topic(&self, session: &str, id: &str) -> anyhow::Result<Option<Topic>> {
        let data: Option<String> = self
            .db
            .query_row(
                "SELECT data FROM initiative_topics WHERE session=?1 AND id=?2",
                params![session, id],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }
    pub fn topics(&self, session: &str) -> anyhow::Result<Vec<Topic>> {
        let mut query = self.db.prepare("SELECT data FROM initiative_topics WHERE session=?1 ORDER BY json_extract(data,'$.updated_at') DESC,id")?;
        let rows = query
            .query_map([session], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|data| serde_json::from_str(&data).map_err(Into::into))
            .collect()
    }
    fn put_topic(&self, topic: &Topic) -> anyhow::Result<()> {
        self.db.execute(
            "INSERT INTO initiative_topics(id,session,data) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![topic.id,topic.session_id,serde_json::to_string(topic)?],
        )?;
        Ok(())
    }
    fn new_topic(&self, session: &str, plan: Plan, run: &str, now: i64) -> anyhow::Result<Topic> {
        plan.validate(now)?;
        let topic = Topic {
            id: random_id("topic")?,
            session_id: session.into(),
            revision: 1,
            plan,
            state: TopicState::Active,
            source_run: run.into(),
            created_at: now,
            updated_at: now,
        };
        self.put_topic(&topic)?;
        Ok(topic)
    }
    pub fn remember(
        &self,
        session: &str,
        plan: Plan,
        run: &str,
        now: i64,
    ) -> anyhow::Result<Topic> {
        let tx = self.transaction()?;
        self.require_target(session)?;
        anyhow::ensure!(!run.is_empty(), "话题缺少来源运行");
        let topic = self.new_topic(session, plan, run, now)?;
        tx.commit()?;
        Ok(topic)
    }
    pub fn revise_topic(
        &self,
        session: &str,
        id: &str,
        expected: u64,
        plan: Option<Plan>,
        run: &str,
        now: i64,
    ) -> anyhow::Result<Topic> {
        let tx = self.transaction()?;
        let mut topic = self
            .topic(session, id)?
            .ok_or_else(|| anyhow::anyhow!("当前会话没有这条话题"))?;
        anyhow::ensure!(topic.revision == expected, "话题已变化，请重新读取");
        if let Some(mut review) = self.active_review(session)? {
            if review.topic_id.as_deref() == Some(id) {
                anyhow::ensure!(
                    review.state != ReviewState::Sending,
                    "话题正在发送，请等待发送回执后修改"
                );
                review.state = ReviewState::Cancelled;
                review.error = Some("选中的话题已修改或关闭，本次不再发送".into());
                self.put_review(&review)?;
            }
        }
        topic.revision = topic
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("话题版本超出范围"))?;
        topic.updated_at = now;
        topic.source_run = run.into();
        match plan {
            Some(plan) => {
                plan.validate(now)?;
                topic.plan = plan;
                topic.state = TopicState::Active;
            }
            None => topic.state = TopicState::Closed,
        }
        self.put_topic(&topic)?;
        tx.commit()?;
        Ok(topic)
    }
    pub fn review(&self, id: &str) -> anyhow::Result<Option<Review>> {
        let data: Option<String> = self
            .db
            .query_row(
                "SELECT data FROM initiative_reviews WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }
    pub fn review_for_run(&self, session: &str, run: &str) -> anyhow::Result<Option<Review>> {
        let data: Option<String> = self.db.query_row(
            "SELECT data FROM initiative_reviews WHERE session=?1 AND json_extract(data,'$.run_id')=?2",params![session,run],|row| row.get(0),
        ).optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }
    pub fn recent_reviews(&self, session: &str) -> anyhow::Result<Vec<Review>> {
        let mut query = self.db.prepare("SELECT data FROM initiative_reviews WHERE session=?1 ORDER BY json_extract(data,'$.created_at') DESC,id DESC LIMIT 20")?;
        let rows = query
            .query_map([session], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|data| serde_json::from_str(&data).map_err(Into::into))
            .collect()
    }
    fn put_review(&self, review: &Review) -> anyhow::Result<()> {
        self.db.execute(
            "INSERT INTO initiative_reviews(id,session,data) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![review.id,review.session_id,serde_json::to_string(review)?],
        )?;
        Ok(())
    }
    pub fn begin_review(
        &self,
        session: &str,
        id: &str,
        context: Context,
        now: i64,
        day: &str,
        hour: u32,
    ) -> anyhow::Result<Option<Review>> {
        let tx = self.transaction()?;
        if let Some(review) = self.review(id)? {
            anyhow::ensure!(
                review.session_id == session && review.context == context,
                "评估身份载荷发生变化"
            );
            return Ok(None);
        }
        if self.eligibility(session, now, day, hour)? != Eligibility::Ready {
            return Ok(None);
        }
        let mut target = self.require_target(session)?;
        if target.activity != context.activity {
            return Ok(None);
        }
        let review = Review {
            id: id.into(),
            session_id: session.into(),
            target_revision: target.revision,
            context,
            day: day.into(),
            created_at: now,
            state: ReviewState::Reviewing,
            decision: None,
            topic_id: None,
            topic_revision: None,
            run_id: None,
            receipt: None,
            error: None,
        };
        self.put_review(&review)?;
        target.last_review = Some(now);
        target.next_review = after_minutes(now, target.settings.review_minutes)?;
        self.put_target(&target)?;
        tx.commit()?;
        Ok(Some(review))
    }
    fn fresh(&self, review: &Review, context: &Context) -> anyhow::Result<Target> {
        let target = self.require_target(&review.session_id)?;
        anyhow::ensure!(
            target.settings.enabled && target.state == TargetState::Ready,
            "原会话已停止主动搭话"
        );
        anyhow::ensure!(
            target.revision == review.target_revision,
            "主动搭话设置已变化"
        );
        anyhow::ensure!(
            review.context == *context && target.activity == context.activity,
            "原会话或执行代际已变化，旧话题不再发送"
        );
        Ok(target)
    }
    pub fn decide(
        &self,
        id: &str,
        decision: Decision,
        context: &Context,
        now: i64,
    ) -> anyhow::Result<Review> {
        decision.validate()?;
        let tx = self.transaction()?;
        let mut review = self
            .review(id)?
            .ok_or_else(|| anyhow::anyhow!("主动评估不存在"))?;
        anyhow::ensure!(review.state == ReviewState::Reviewing, "这次主动评估已结束");
        let mut target = self.fresh(&review, context)?;
        review.state = match &decision {
            Decision::Speak { topic, .. } => {
                let topic = match topic {
                    TopicSelection::Existing { id, revision } => {
                        let topic = self
                            .topic(&review.session_id, id)?
                            .ok_or_else(|| anyhow::anyhow!("选择的话题不属于原会话"))?;
                        anyhow::ensure!(
                            topic.revision == *revision
                                && topic.state == TopicState::Active
                                && topic.plan.available(now),
                            "选择的话题已变化、关闭或暂不可用"
                        );
                        topic
                    }
                    TopicSelection::New { plan } => self.new_topic(
                        &review.session_id,
                        plan.clone(),
                        &format!("review:{id}"),
                        now,
                    )?,
                };
                review.topic_revision = Some(topic.revision);
                review.topic_id = Some(topic.id);
                ReviewState::Selected
            }
            Decision::Defer {
                after_minutes: minutes,
                ..
            } => {
                target.next_review = after_minutes(now, *minutes)?;
                ReviewState::Deferred
            }
            Decision::Silent { .. } => ReviewState::Silent,
        };
        review.decision = Some(decision);
        self.put_review(&review)?;
        self.put_target(&target)?;
        tx.commit()?;
        Ok(review)
    }
    pub fn bind_run(&self, id: &str, run: &str, context: &Context) -> anyhow::Result<()> {
        let tx = self.transaction()?;
        let mut review = self
            .review(id)?
            .ok_or_else(|| anyhow::anyhow!("主动评估不存在"))?;
        self.fresh(&review, context)?;
        if review.state == ReviewState::Running && review.run_id.as_deref() == Some(run) {
            return Ok(());
        }
        anyhow::ensure!(
            review.state == ReviewState::Selected && review.run_id.is_none(),
            "主动话题已执行或取消，不能重放"
        );
        anyhow::ensure!(!run.is_empty(), "主动搭话缺少真实运行编号");
        review.run_id = Some(run.into());
        review.state = ReviewState::Running;
        self.put_review(&review)?;
        tx.commit()?;
        Ok(())
    }
    pub fn begin_send(
        &self,
        id: &str,
        context: &Context,
        now: i64,
        day: &str,
        hour: u32,
    ) -> anyhow::Result<()> {
        let tx = self.transaction()?;
        let mut review = self
            .review(id)?
            .ok_or_else(|| anyhow::anyhow!("主动评估不存在"))?;
        let target = self.fresh(&review, context)?;
        anyhow::ensure!(
            hour < 24 && !target.settings.quiet(hour),
            "已进入主动聊天安静时段"
        );
        anyhow::ensure!(
            self.message_budget(&target, day)? == Eligibility::Ready,
            "主动消息已达到每日或未回复上限"
        );
        anyhow::ensure!(
            review.state == ReviewState::Running,
            "本轮主动消息已经处理，不能重复发送"
        );
        let topic = self
            .topic(
                &review.session_id,
                review
                    .topic_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("主动搭话缺少话题"))?,
            )?
            .ok_or_else(|| anyhow::anyhow!("主动话题不存在"))?;
        anyhow::ensure!(
            Some(topic.revision) == review.topic_revision
                && topic.state == TopicState::Active
                && topic.plan.available(now),
            "话题已变化、关闭或过期"
        );
        review.day = day.into();
        review.state = ReviewState::Sending;
        self.put_review(&review)?;
        tx.commit()?;
        Ok(())
    }
    pub fn record_delivery(
        &self,
        id: &str,
        outcome: ReviewState,
        receipt: &str,
        error: Option<String>,
        now: i64,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(
                outcome,
                ReviewState::Sent | ReviewState::Unknown | ReviewState::Failed
            ),
            "主动消息结局无效"
        );
        anyhow::ensure!(!receipt.is_empty(), "主动消息缺少渠道回执引用");
        let tx = self.transaction()?;
        let mut review = self
            .review(id)?
            .ok_or_else(|| anyhow::anyhow!("主动评估不存在"))?;
        if review.state == outcome
            && review.receipt.as_deref() == Some(receipt)
            && review.error == error
        {
            return Ok(());
        }
        anyhow::ensure!(
            matches!(review.state, ReviewState::Sending | ReviewState::Unknown),
            "没有待结算的主动发送"
        );
        review.state = outcome;
        review.receipt = Some(receipt.into());
        review.error = error.clone();
        if let Some(topic_id) = &review.topic_id {
            let mut topic = self
                .topic(&review.session_id, topic_id)?
                .ok_or_else(|| anyhow::anyhow!("主动话题不存在"))?;
            topic.state = match outcome {
                ReviewState::Sent => TopicState::Discussed,
                ReviewState::Unknown => TopicState::Uncertain,
                ReviewState::Failed => TopicState::Active,
                _ => unreachable!("delivery outcome validated"),
            };
            topic.updated_at = now;
            topic.revision = topic
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("话题版本超出范围"))?;
            self.put_topic(&topic)?;
        }
        self.put_review(&review)?;
        if outcome == ReviewState::Failed {
            let mut target = self.require_target(&review.session_id)?;
            target.state = TargetState::Faulted;
            target.error = error;
            self.put_target(&target)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn fail_review(&self, id: &str, error: &str) -> anyhow::Result<()> {
        let tx = self.transaction()?;
        let mut review = self
            .review(id)?
            .ok_or_else(|| anyhow::anyhow!("主动评估不存在"))?;
        if !review.state.active() {
            return Ok(());
        }
        anyhow::ensure!(
            review.state != ReviewState::Sending,
            "发送中断需要保留未知送达结局"
        );
        review.state = ReviewState::Failed;
        review.error = Some(error.into());
        let mut target = self.require_target(&review.session_id)?;
        target.state = TargetState::Faulted;
        target.error = Some(error.into());
        self.put_review(&review)?;
        self.put_target(&target)?;
        tx.commit()?;
        Ok(())
    }
    fn cancel_open(&self, session: &str, reason: &str, now: i64) -> anyhow::Result<()> {
        let mut query = self.db.prepare("SELECT data FROM initiative_reviews WHERE session=?1 AND json_extract(data,'$.state') IN ('reviewing','selected','running','sending')")?;
        let rows = query
            .query_map([session], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for row in rows {
            let mut review: Review = serde_json::from_str(&row)?;
            review.state = if review.state == ReviewState::Sending {
                ReviewState::Unknown
            } else {
                ReviewState::Cancelled
            };
            review.error = Some(reason.into());
            if review.state == ReviewState::Unknown {
                if let Some(id) = &review.topic_id {
                    let mut topic = self
                        .topic(session, id)?
                        .ok_or_else(|| anyhow::anyhow!("发送中的主动话题不存在"))?;
                    topic.state = TopicState::Uncertain;
                    topic.updated_at = now;
                    topic.revision = topic
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("话题版本超出范围"))?;
                    self.put_topic(&topic)?;
                }
            }
            self.put_review(&review)?;
        }
        Ok(())
    }
    pub fn recover(&self, now: i64) -> anyhow::Result<usize> {
        let tx = self.transaction()?;
        let mut query = self.db.prepare("SELECT data FROM initiative_reviews WHERE json_extract(data,'$.state') IN ('reviewing','selected','running','sending')")?;
        let rows = query
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let count = rows.len();
        for row in rows {
            let mut review: Review = serde_json::from_str(&row)?;
            review.state = if review.state == ReviewState::Sending {
                ReviewState::Unknown
            } else {
                ReviewState::Interrupted
            };
            review.error = Some("上次执行已退出；重新读取原聊天后再评估，不重放旧消息".into());
            if review.state == ReviewState::Unknown {
                if let Some(id) = &review.topic_id {
                    let mut topic = self
                        .topic(&review.session_id, id)?
                        .ok_or_else(|| anyhow::anyhow!("发送中的主动话题不存在"))?;
                    topic.state = TopicState::Uncertain;
                    topic.updated_at = now;
                    topic.revision = topic
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("话题版本超出范围"))?;
                    self.put_topic(&topic)?;
                }
            }
            self.put_review(&review)?;
        }
        tx.commit()?;
        Ok(count)
    }
}

pub(crate) fn random_id(prefix: &str) -> anyhow::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|error| anyhow::anyhow!("生成主动话题身份失败：{error}"))?;
    Ok(format!(
        "{prefix}:{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

#[cfg(test)]
mod tests;
