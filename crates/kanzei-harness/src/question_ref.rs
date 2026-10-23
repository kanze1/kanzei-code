//! D-764③:已作答 question 的可引用凭证 `question:{run_id}:{call_id}`。
//!
//! runner 在拿到用户回答时把凭证写进工具输出,tracker 的语义确认门禁要求
//! `确认记录` 逐字带上它。凭证不证明调用真实存在——作答记录落在易失的
//! `.kanzei/runtime/questions/` 里,按 R-354 不能当条目账本证据——它的作用是让
//! 诚实路径变成一次复制粘贴,同时把模型自己的决定(`dec-…`)和散文式「已通过
//! question 确认」挡在用户确认之外:后者正是 D-764 里被冒写的那种留痕。

/// 凭证前缀。生产方(runner)与消费方(tracker 门禁)共用,别各写一份字面量。
pub const QUESTION_REF_PREFIX: &str = "question:";

/// 凭证主体允许的字符:run_id / call_id 都是引擎生成的标识,不含空白与冒号。
fn is_ref_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/')
}

pub fn question_ref(run_id: &str, call_id: &str) -> String {
    format!("{QUESTION_REF_PREFIX}{run_id}:{call_id}")
}

/// 待答问题的投递标识，不是作答凭证。取消通知也只能引用此标识。
pub fn pending_question_ref(run_id: &str, call_id: &str) -> String {
    format!("pending-question:{run_id}:{call_id}")
}

/// 取文本中第一个形态合法的凭证(两段都非空、段内无空白与额外冒号)。
pub fn find_question_ref(text: &str) -> Option<&str> {
    let mut cursor = 0usize;
    while let Some(offset) = text[cursor..].find(QUESTION_REF_PREFIX) {
        let start = cursor + offset;
        let body_start = start + QUESTION_REF_PREFIX.len();
        // 凭证必须是独立 token；不能从 pending-question 或畸形长 token 中截出子串。
        if text[..start]
            .chars()
            .next_back()
            .is_some_and(|ch| is_ref_char(ch) || ch == ':')
        {
            cursor = body_start;
            continue;
        }
        let body = &text[body_start..];
        let end = body
            .find(|ch: char| !(is_ref_char(ch) || ch == ':'))
            .unwrap_or(body.len());
        // 「引用 question:run:call: 用户确认…」这种行文里,冒号是标点不是第三段。
        let body_end = body[..end].trim_end_matches(':').len();
        let segments: Vec<&str> = body[..body_end].split(':').collect();
        if segments.len() == 2 && segments.iter().all(|segment| !segment.is_empty()) {
            return Some(&text[start..body_start + body_end]);
        }
        cursor = body_start;
    }
    None
}

/// 文本是否引用了至少一个作答凭证形态；不反查作答记录的存在性。
pub fn cites_question_ref(text: &str) -> bool {
    find_question_ref(text).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 凭证按引擎产出的形态互相识别() {
        let minted = question_ref("run-7f3a", "call_b2");
        assert_eq!(minted, "question:run-7f3a:call_b2");
        assert_eq!(
            find_question_ref(&format!("作答凭证：{minted}\n回答：A")),
            Some(minted.as_str())
        );
        assert!(cites_question_ref(&format!("更新 R-001 前先引用 {minted}")));
    }

    #[test]
    fn 散文与畸形引用不放行() {
        for text in [
            "已通过 question 确认：先支持知乎站内收藏",
            "question: 用户确认先支持知乎站内收藏",
            "question:run-7f3a",
            "question::call_b2",
            "question:run-7f3a:call_b2:extra",
            "question:run 7f3a:call_b2",
            "pending-question:run-7f3a:call_b2",
            "unanswered-question:run-7f3a:call_b2",
            "my_question:run-7f3a:call_b2",
            "question:run-7f3a:call_b2:question:run-9:call-1",
            "dec-1ec3ded4ec2dc3c9",
        ] {
            assert!(!cites_question_ref(text), "{text} 被误判为凭证");
        }
    }

    #[test]
    fn 同一文本里跳过畸形取到真凭证() {
        assert_eq!(
            find_question_ref("question: 旧写法，随后 question:run-1:call-9 才是凭证"),
            Some("question:run-1:call-9")
        );
        assert_eq!(
            find_question_ref("question:run-1:call-9: 用户确认先支持知乎站内收藏"),
            Some("question:run-1:call-9")
        );
    }
}
