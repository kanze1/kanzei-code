//! Credentials are transient inputs, never new memory facts or dream artifacts.
use regex::{Captures, Regex};
use std::sync::LazyLock;

pub const REDACTED_CREDENTIAL: &str = "<credential-redacted>";
const CREDENTIAL_LABEL: &str = r"(?:[A-Za-z0-9_.-]*(?:api[ _-]?key|secret[ _-]?access[ _-]?key|private[ _-]?key|password|passphrase|secret|token|credentials?)|authorization|cookies?|set-cookie|密码|密钥|私钥|口令|令牌)";
static LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!("(?i)^{CREDENTIAL_LABEL}$")).expect("credential label is valid")
});

// Every rule preserves groups 1/3 and removes only group 2.
static CREDENTIALS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    let mut patterns:Vec<Regex> = [
    r"(?s)()(-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|\z))()",
    r"()\b((?:sk-[A-Za-z0-9_-]{10,}|gh[pousr]_[A-Za-z0-9_]{16,}|github_pat_[A-Za-z0-9_]{16,}|AKIA[A-Z0-9]{16}|eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}))\b()",
    r"(?im)(\b(?:authorization|cookie|set-cookie)[ \t]*:[ \t]*)([^\r\n]+)()",
    r"(?i)(\b(?:Bearer|Basic)[ \t]+)([A-Za-z0-9+/_.=-]+)()",
    r"(?i)([a-z][a-z0-9+.-]*://)([^/@\s:]+:[^/@\s]+)(@)",
    r"(?i)([?&](?:api[_-]?key|access[_-]?token|refresh[_-]?token|token|password|secret|sig|(?:x-amz-|x-goog-)?(?:signature|credential|security-token))=)([^&\s]+)()",
].into_iter().map(|pattern| Regex::new(pattern).expect("credential pattern is valid")).collect();
    patterns.push(Regex::new(&format!(r#"(?im)((?:["']?{CREDENTIAL_LABEL}["']?)[ \t]*(?:[:=]|为|是)[ \t]*(?:\r?\n[ \t]+)?)("(?:\\.|[^"\\\r\n])+"|'(?:\\.|[^'\\\r\n])+'|[^\s,;&]+)()"#)).expect("credential assignment is valid"));
    patterns
});

pub fn redact_credentials(text: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        let redacted = redact_json(&value);
        return if redacted == value {
            text.to_owned()
        } else {
            serde_json::to_string(&redacted).expect("redacted JSON is serializable")
        };
    }
    redact_scalar(text)
}

fn redact_json(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(text) => Value::String(redact_credentials(text)),
        Value::Array(values) => Value::Array(values.iter().map(redact_json).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| {
                    let sensitive_value = LABEL.is_match(key)
                        && match value {
                            Value::Null | Value::Bool(_) => false,
                            Value::String(text) => !text.is_empty() && text != REDACTED_CREDENTIAL,
                            _ => true,
                        };
                    (
                        redact_credentials(key),
                        if sensitive_value {
                            Value::String(REDACTED_CREDENTIAL.into())
                        } else {
                            redact_json(value)
                        },
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn redact_scalar(text: &str) -> String {
    let mut redacted = text.to_owned();
    for pattern in CREDENTIALS.iter() {
        redacted = pattern
            .replace_all(&redacted, |matched: &Captures<'_>| {
                let value = &matched[2];
                let literal = value.trim_matches(['"', '\'']);
                if literal.is_empty() || literal == REDACTED_CREDENTIAL {
                    return matched[0].to_owned();
                }
                let quote = value.chars().next().filter(|character| {
                    matches!(character, '"' | '\'') && value.ends_with(*character)
                });
                let replacement = quote.map_or_else(
                    || REDACTED_CREDENTIAL.to_owned(),
                    |quote| format!("{quote}{REDACTED_CREDENTIAL}{quote}"),
                );
                format!("{}{}{}", &matched[1], replacement, &matched[3])
            })
            .into_owned();
    }
    redacted
}

pub(super) fn reject_credentials(text: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        redact_credentials(text) == text,
        "记忆不能保存凭据值；请移除密钥、密码或认证令牌后再保存"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_removed_without_dropping_preferences_or_reintroducing_them_on_a_second_pass()
    {
        let text = "喜欢简洁排版。\nOPENAI_API_KEY=sk-proj-0123456789abcdefgh\nAuthorization: Bearer opaque-value\nCookie: sid=cookie-value; csrf=other-value\nhttps://account:password-value@example.test/?token=query-value&mode=normal\n密码是 p@ssword-value\n-----BEGIN PRIVATE KEY-----\nbase64-private-value\n-----END PRIVATE KEY-----\n保留踩过的坑。";
        let safe = redact_credentials(text);
        for secret in [
            "sk-proj-0123456789abcdefgh",
            "opaque-value",
            "cookie-value",
            "other-value",
            "account:password-value",
            "query-value",
            "p@ssword-value",
            "base64-private-value",
        ] {
            assert!(!safe.contains(secret), "credential was retained: {secret}");
        }
        assert!(
            safe.contains("喜欢简洁排版")
                && safe.contains("mode=normal")
                && safe.contains("保留踩过的坑")
        );
        assert_eq!(redact_credentials(&safe), safe);
        assert!(reject_credentials(&safe).is_ok());
        assert!(reject_credentials(text).is_err());
    }

    #[test]
    fn incomplete_keys_are_removed_and_bare_technical_terms_are_preserved() {
        let text = "API key 从环境变量读取；password 字段不应落库。不要把 token 数当采用次数。";
        assert_eq!(redact_credentials(text), text);
        let incomplete = "前文\n-----BEGIN RSA PRIVATE KEY-----\nprivate-tail";
        assert!(!redact_credentials(incomplete).contains("private-tail"));
        assert_eq!(
            redact_credentials("password:\n  multiline-value"),
            format!("password:\n  {REDACTED_CREDENTIAL}")
        );
    }

    #[test]
    fn quoted_values_and_safe_redaction_tokens_have_stable_boundaries() {
        let safe = redact_credentials(
            r#"{"password":"quoted\"credential-tail","title":"prefers compact prose"}"#,
        );
        assert!(!safe.contains("credential-tail"));
        assert!(safe.contains("prefers compact prose"));
        let placeholder = format!(r#"api_key="{REDACTED_CREDENTIAL}""#);
        assert_eq!(redact_credentials(&placeholder), placeholder);
        assert_eq!(redact_credentials("api_key=\"\""), "api_key=\"\"");
        let structured = serde_json::json!({"auth":{"password":"opaque-json-secret"},"cookies":[{"sid":"cookie-json-secret"}],"preference":"compact prose"});
        let safe = redact_credentials(&structured.to_string());
        assert!(!safe.contains("opaque-json-secret") && !safe.contains("cookie-json-secret"));
        assert!(safe.contains("compact prose"));
        assert_eq!(redact_credentials(&safe), safe);
    }

    #[test]
    fn memory_admission_update_inbox_and_rejection_audits_do_not_persist_credential_values() {
        let root = std::env::temp_dir().join(format!(
            "kz-memory-privacy-{}-{}",
            std::process::id(),
            super::super::store::now_ms()
        ));
        let store = super::super::MemoryStore::project(&root);
        let secret = "api_key=sk-proj-abcdefghijklmnop0123456789";
        assert!(store
            .add("fact", "bad", "条件", secret, "user", &[], None, true)
            .is_err());
        assert!(store.append_note("bad", secret, "fact", &[]).is_err());
        assert!(store.load_all().is_empty());
        assert_eq!(store.pending_notes(), 0);
        let entry = match store
            .add(
                "fact",
                "保留真实偏好",
                "条件",
                "喜欢简洁排版",
                "user",
                &[],
                None,
                true,
            )
            .unwrap()
        {
            super::super::AddOutcome::Added(entry) => entry,
            other => panic!("{other:?}"),
        };
        assert!(store
            .update(&entry.id, None, None, Some(secret), None, None, false)
            .is_err());
        assert_eq!(store.load_all()[0].1.body, entry.body);
        let (path, mut metadata) = store.load_all().remove(0);
        metadata
            .extras
            .push(("password".into(), "opaque-extra-secret".into()));
        assert!(store.write_entry(&metadata, Some(&path)).is_err());
        store.record_manager_decision("rejected", secret, secret);
        let db = store.open_db().unwrap();
        let audit: (String, String) = db
            .query_row(
                "SELECT reason,note_head FROM manager_decisions ORDER BY rowid DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(!audit.0.contains("sk-proj-") && !audit.1.contains("sk-proj-"));
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}
