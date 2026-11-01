//! One projection for structured OneBot messages and their CQ-string equivalent.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ImageSource {
    pub url: Option<String>,
}

fn unescape(value: &str) -> String {
    value
        .replace("&#44;", ",")
        .replace("&#91;", "[")
        .replace("&#93;", "]")
        .replace("&amp;", "&")
}

fn segments(value: &Value) -> Vec<Value> {
    if let Some(parts) = value["message"].as_array() {
        return parts.clone();
    }
    let mut rest = value["message"]
        .as_str()
        .or_else(|| value["raw_message"].as_str())
        .unwrap_or("");
    let mut parts = Vec::new();
    while let Some(start) = rest.find("[CQ:") {
        let Some(end) = rest[start..].find(']').map(|end| start + end) else {
            break;
        };
        parts.push(json!({"type":"text","data":{"text":unescape(&rest[..start])}}));
        let mut fields = rest[start + 4..end].split(',');
        let kind = fields.next().unwrap_or("");
        let data: serde_json::Map<String, Value> = fields
            .filter_map(|field| field.split_once('='))
            .map(|(key, value)| (key.into(), json!(unescape(value))))
            .collect();
        parts.push(json!({"type":kind,"data":data}));
        rest = &rest[end + 1..];
    }
    parts.push(json!({"type":"text","data":{"text":unescape(rest)}}));
    parts
}

pub(super) fn text(value: &Value) -> String {
    segments(value)
        .iter()
        .filter_map(|part| match part["type"].as_str() {
            Some("text") => part["data"]["text"].as_str().map(str::to_owned),
            Some(kind @ ("image" | "face" | "mface" | "file")) => {
                Some(format!("[QQ {kind} {}]", part["data"]))
            }
            Some("reply") => Some(format!("[引用 {}]", part["data"]["id"])),
            Some("at") => Some(format!(
                "@{} ",
                part["data"]["qq"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| part["data"]["qq"].to_string())
            )),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub(super) fn images(value: &Value) -> Vec<ImageSource> {
    segments(value)
        .iter()
        .filter(|part| part["type"] == "image")
        .map(|part| ImageSource {
            url: part["data"]["url"]
                .as_str()
                .filter(|url| !url.is_empty())
                .map(str::to_owned),
        })
        .collect()
}

pub(super) fn mentions_self(value: &Value, account: &str) -> bool {
    segments(value).iter().any(|part| {
        part["type"] == "at" && super::protocol::id(&part["data"]["qq"]).as_deref() == Ok(account)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_and_cq_messages_have_the_same_image_and_mention_projection() {
        let structured = json!({"message":[
            {"type":"text","data":{"text":"看这张 "}},
            {"type":"image","data":{"url":"https://example.com/a,b?x=1&y=2","summary":"[图]"}},
            {"type":"at","data":{"qq":"100"}}
        ]});
        let cq = json!({"message":"看这张 [CQ:image,url=https://example.com/a&#44;b?x=1&amp;y=2,summary=&#91;图&#93;][CQ:at,qq=100]"});
        assert_eq!(text(&structured), text(&cq));
        assert_eq!(images(&structured), images(&cq));
        assert!(mentions_self(&cq, "100"));
        assert_eq!(
            images(&json!({"message":[{"type":"image","data":{"file":"id-only"}}]})),
            vec![ImageSource { url: None }]
        );
        let escaped = json!({"message":"&#91;CQ:image,url=https://example.com/x&#93; &#91;CQ:at,qq=100&#93;"});
        assert!(images(&escaped).is_empty());
        assert!(!mentions_self(&escaped, "100"));
        assert_eq!(
            text(&json!({"message":"literal [CQ:broken"})),
            "literal [CQ:broken"
        );
    }
}
