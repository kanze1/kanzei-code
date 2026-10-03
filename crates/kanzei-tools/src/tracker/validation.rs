//! Tracker 需求登记与生命周期校验(R-313/R-315)。
//!
//! 该模块只承载旧格式验收开放度、草稿完整性、语义确认和限定词一致性校验；
//! action 路由通过 `TrackerTool` 复用这些门禁，保持校验顺序与错误文案不变。

use crate::docstore::Entry;

use super::TrackerTool;

impl TrackerTool {
    /// R-315 B1/B2:验收条款开放度与复杂度必须一致。这里故意采用保守的
    /// 文本模式而不是自然语言推断:命中「全量/所有/全库/逐一核对并修复」等
    /// 开放式审计词即视为开放条款,小/中条目在 add 与 update/close 修订路径拒绝。
    pub(super) fn acceptance_scope_markers(value: &str) -> Vec<&'static str> {
        const MARKERS: &[&str] = &[
            "逐一核对并修复",
            "逐项核对并修复",
            "审计全库",
            "全库审计",
            "全量",
            "所有",
            "全部",
            "全库",
        ];
        let compact: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
        MARKERS
            .iter()
            .copied()
            .filter(|marker| compact.contains(marker))
            .collect()
    }

    pub(super) fn check_acceptance_scope<'a, I>(&self, fields: I) -> Result<(), String>
    where
        I: Iterator<Item = (&'a String, &'a String)> + Clone,
    {
        if self.kind.prefix != "R" {
            return Ok(());
        }
        if Self::field_value(fields.clone(), &["需求格式"]) == Some("2") {
            return Ok(());
        }
        let Some(acceptance) = Self::field_value(fields.clone(), &["验收", "acceptance"]) else {
            return Ok(());
        };
        let markers = Self::acceptance_scope_markers(acceptance);
        if markers.is_empty() {
            return Ok(());
        }
        let complexity = Self::field_value(fields, &["复杂度", "complexity"]);
        match complexity {
            Some("大") => Ok(()),
            Some(value @ ("小" | "中")) => Err(format!(
                "条款过开放:复杂度 `{value}` 的验收含开放式全量审计词 `{}`；请拆分为独立条目或提升复杂度",
                markers.join("、")
            )),
            Some(other) => Err(format!(
                "验收开放度无法判定:复杂度 `{other}` 非法；请先填写 小 | 中 | 大"
            )),
            None => Err(format!(
                "验收条款含开放式全量审计词 `{}` 但缺少复杂度；请先填写复杂度，或拆分为独立条目",
                markers.join("、")
            )),
        }
    }

    /// R-315 B4:供 audit_acceptance_scope 读取活动与归档需求,只报告需要处置的
    /// 小/中开放条款或缺复杂度条款;大条目是允许范围,不进入不匹配清单。
    pub(super) fn acceptance_scope_finding(entry: &Entry) -> Option<String> {
        let acceptance = Self::field_value(
            entry.fields.iter().map(|(key, value)| (key, value)),
            &["验收", "acceptance"],
        )?;
        let markers = Self::acceptance_scope_markers(acceptance);
        if markers.is_empty() {
            return None;
        }
        let complexity = Self::field_value(
            entry.fields.iter().map(|(key, value)| (key, value)),
            &["复杂度", "complexity"],
        );
        match complexity {
            Some("大") => None,
            Some(value) => Some(format!(
                "复杂度 `{value}` 命中开放式全量审计词: {}",
                markers.join("、")
            )),
            None => Some(format!(
                "缺少复杂度但命中开放式全量审计词: {}",
                markers.join("、")
            )),
        }
    }

    /// Draft capture is allowed; execution requires a complete specification.
    pub(crate) fn check_requirement_start(entry: &Entry) -> Result<(), String> {
        if entry.status == "draft" {
            return Err(
                "需求仍是草稿；补齐正文、验收和来源后用 req update <id> todo 纳入执行".into(),
            );
        }
        if let Some(spec) =
            kanzei_memory::docstore::requirement::RequirementSpec::from_entry(entry)?
        {
            if !spec.gaps().is_empty() {
                return Err(format!("需求尚有未决内容: {}", spec.gaps().join("；")));
            }
        }
        Self::check_semantic_confirmation(entry.fields.iter().map(|(key, value)| (key, value)))?;
        Self::check_qualifier_consistency(entry.fields.iter().map(|(key, value)| (key, value)))
    }

    fn field_value<'a, I>(fields: I, names: &[&str]) -> Option<&'a str>
    where
        I: Iterator<Item = (&'a String, &'a String)>,
    {
        fields
            .filter(|(key, _)| names.iter().any(|name| key.eq_ignore_ascii_case(name)))
            .map(|(_, value)| value.trim())
            .find(|value| !value.is_empty())
    }

    fn quoted_user_text(source: &str) -> String {
        let mut quoted = String::new();
        for (open, close) in [('「', '」'), ('“', '”'), ('‘', '’')] {
            let mut rest = source;
            while let Some(start) = rest.find(open) {
                let after = &rest[start + open.len_utf8()..];
                let Some(end) = after.find(close) else { break };
                quoted.push_str(&after[..end]);
                rest = &after[end + close.len_utf8()..];
            }
        }
        let mut ascii = source.split('"');
        while let (Some(_), Some(value)) = (ascii.next(), ascii.next()) {
            quoted.push_str(value);
            let _ = ascii.next();
        }
        quoted
    }

    fn check_semantic_confirmation<'a, I>(fields: I) -> Result<(), String>
    where
        I: Iterator<Item = (&'a String, &'a String)> + Clone,
    {
        let pending = ["待确认", "ambiguities", "歧义"]
            .iter()
            .filter_map(|name| Self::field_value(fields.clone(), &[*name]))
            .collect::<Vec<_>>()
            .join(" ");
        let core_pending = (pending.contains("核心语义")
            || pending.to_ascii_lowercase().contains("core semantic"))
            && ["待确认", "未确认", "未决", "pending", "unresolved"]
                .iter()
                .any(|marker| {
                    pending
                        .to_ascii_lowercase()
                        .contains(&marker.to_ascii_lowercase())
                });
        if !core_pending {
            return Ok(());
        }
        let evidence = ["确认记录", "用户豁免", "豁免", "question", "confirmation"]
            .iter()
            .filter_map(|name| Self::field_value(fields.clone(), &[*name]))
            .collect::<Vec<_>>()
            .join(" ");
        let lower = evidence.to_ascii_lowercase();
        if lower.contains("question")
            || evidence.contains("用户明确豁免")
            || evidence.contains("用户豁免")
        {
            Ok(())
        } else {
            Err(
                "检测到未决核心语义，不能进入 doing/设计冻结；请先调用 `question` 并把结果写入 `确认记录`，或写入用户明确豁免及原话。"
                    .into(),
            )
        }
    }

    pub(super) fn check_qualifier_consistency<'a, I>(fields: I) -> Result<(), String>
    where
        I: Iterator<Item = (&'a String, &'a String)> + Clone,
    {
        let Some(raw) = Self::field_value(fields.clone(), &["限定词", "qualifiers"]) else {
            return Ok(());
        };
        let source = Self::field_value(fields.clone(), &["来源", "source"]).unwrap_or_default();
        let quoted = Self::quoted_user_text(source);
        let assumptions =
            Self::field_value(fields.clone(), &["假设", "assumptions"]).unwrap_or_default();
        for qualifier in raw
            .split(|ch: char| [',', '，', '、', ';', '；'].contains(&ch))
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "无")
        {
            if quoted.contains(qualifier) {
                continue;
            }
            let assumption = assumptions.contains(qualifier)
                && (assumptions.to_ascii_lowercase().contains("assumption")
                    || assumptions.contains("假设"));
            if !assumption {
                return Err(format!(
                    "未确认解释:限定词 `{qualifier}` 不在来源的用户原话中；请确认、标 `assumption`、或移除限定词。"
                ));
            }
        }
        Ok(())
    }
}
