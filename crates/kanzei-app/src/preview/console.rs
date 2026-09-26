//! 预览面板的控制台:CDP 事件解析 + 环形缓冲(容量 500,seq 单调)。
//!
//! 条目形态即 `kz:preview-console` / `preview_console` 的 IPC 契约:
//! `{seq, ts, level, text, url, line, col}`,level ∈ log | info | debug | warning | error | network。
//! `network` = 资源加载失败(Log.entryAdded source=network 的 4xx/5xx 与 net::ERR_*,以及主文档
//! 加载失败),前端「网络」筛选用它;行列 1 起算(CDP 给 0 起算)。
//!
//! 事件形态取自 B0 实测(web-b0/out/full-results.json):consoleAPICalled 的对象参数只有
//! `preview.properties` 才有内容,只取 description 会塌成「Object」。

use std::collections::VecDeque;

use serde::Serialize;

/// 环形缓冲容量。
pub(crate) const CONSOLE_CAPACITY: usize = 500;
/// 单条文本上限(字符,按字符边界截)。
pub(crate) const MAX_TEXT_CHARS: usize = 2000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ConsoleEntry {
    pub(crate) seq: u64,
    pub(crate) ts: u64,
    pub(crate) level: String,
    pub(crate) text: String,
    pub(crate) url: String,
    pub(crate) line: Option<u64>,
    pub(crate) col: Option<u64>,
}

/// 解析出、尚未编号的条目。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(crate) struct EntryDraft {
    pub(crate) level: String,
    pub(crate) text: String,
    pub(crate) url: String,
    pub(crate) line: Option<u64>,
    pub(crate) col: Option<u64>,
}

/// CDP 事件对面板的意义。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CdpSignal {
    Entry(EntryDraft),
    /// 主 frame 提交了导航;错误页(chrome-error://)时带 unreachable_url。
    /// 错误页的 loader_id 就是那次失败请求的 requestId(Edge 实测),据此取具体错误码。
    MainFrameNavigated {
        frame_id: String,
        loader_id: String,
        url: String,
        unreachable_url: Option<String>,
    },
    /// 同文档导航(pushState / replaceState / hash)。**不分帧**:是不是主 frame 由调用方按
    /// 记下的主 frame id 判。
    SameDocumentNavigated {
        frame_id: String,
        url: String,
    },
    /// 文档请求失败(Network.loadingFailed type=Document,非取消、非 ERR_ABORTED)。
    /// **不分帧**:iframe 被拦(ERR_BLOCKED_BY_CLIENT / X-Frame-Options 的 ERR_BLOCKED_BY_RESPONSE)
    /// 与 iframe 指向死端口同样会来。只有主 frame 随后提交了错误页才算页面失败(见 PaneMeta::apply)。
    DocumentFailed {
        request_id: String,
        error_text: String,
    },
    LoadEventFired,
    /// 批注模式下用户点了一个元素。
    InspectNode {
        backend_node_id: i64,
    },
}

/// 面板订阅的 CDP 事件(先订阅、再 enable、再导航——B0 的顺序)。
pub(crate) const SUBSCRIBED_EVENTS: &[&str] = &[
    "Runtime.consoleAPICalled",
    "Runtime.exceptionThrown",
    "Log.entryAdded",
    "Network.loadingFailed",
    "Page.frameNavigated",
    "Page.navigatedWithinDocument",
    "Page.loadEventFired",
    "Overlay.inspectNodeRequested",
];

/// 事件 → 信号。未识别、不相关(子 frame 导航、取消的请求、verbose 日志)返回 None。
pub(crate) fn parse_event(name: &str, params: &serde_json::Value) -> Option<CdpSignal> {
    match name {
        "Runtime.consoleAPICalled" => {
            let level = match params["type"].as_str().unwrap_or("log") {
                "warning" => "warning",
                "error" | "assert" => "error",
                "info" => "info",
                "debug" => "debug",
                _ => "log",
            };
            let text = params["args"]
                .as_array()
                .map(|args| args.iter().map(render_remote_object).collect::<Vec<_>>())
                .unwrap_or_default()
                .join(" ");
            let frame = &params["stackTrace"]["callFrames"][0];
            Some(CdpSignal::Entry(EntryDraft {
                level: level.into(),
                text,
                url: frame["url"].as_str().unwrap_or("").to_string(),
                line: one_based(&frame["lineNumber"]),
                col: one_based(&frame["columnNumber"]),
            }))
        }
        "Runtime.exceptionThrown" => {
            let details = &params["exceptionDetails"];
            let description = details["exception"]["description"]
                .as_str()
                .or_else(|| details["exception"]["value"].as_str())
                .map(str::to_string);
            let prefix = details["text"].as_str().unwrap_or("Uncaught");
            let text = match description {
                Some(description) if description.starts_with(prefix) => description,
                Some(description) => format!("{prefix} {description}"),
                None => prefix.to_string(),
            };
            Some(CdpSignal::Entry(EntryDraft {
                level: "error".into(),
                text,
                url: details["url"].as_str().unwrap_or("").to_string(),
                line: one_based(&details["lineNumber"]),
                col: one_based(&details["columnNumber"]),
            }))
        }
        "Log.entryAdded" => {
            let entry = &params["entry"];
            let source = entry["source"].as_str().unwrap_or("");
            let level = match (source, entry["level"].as_str().unwrap_or("")) {
                ("network", _) => "network",
                (_, "verbose") => return None,
                (_, "warning") => "warning",
                (_, "error") => "error",
                _ => "info",
            };
            Some(CdpSignal::Entry(EntryDraft {
                level: level.into(),
                text: entry["text"].as_str().unwrap_or("").to_string(),
                url: entry["url"].as_str().unwrap_or("").to_string(),
                line: one_based(&entry["lineNumber"]),
                col: None,
            }))
        }
        "Network.loadingFailed" => {
            if params["type"].as_str() != Some("Document") || params["canceled"] == true {
                return None;
            }
            let error_text = params["errorText"].as_str().unwrap_or("").to_string();
            if error_text.contains("ERR_ABORTED") {
                return None;
            }
            Some(CdpSignal::DocumentFailed {
                request_id: params["requestId"].as_str().unwrap_or("").to_string(),
                error_text,
            })
        }
        "Page.frameNavigated" => {
            let frame = &params["frame"];
            if frame.get("parentId").is_some_and(|p| !p.is_null()) {
                return None;
            }
            Some(CdpSignal::MainFrameNavigated {
                frame_id: frame["id"].as_str().unwrap_or("").to_string(),
                loader_id: frame["loaderId"].as_str().unwrap_or("").to_string(),
                url: frame["url"].as_str().unwrap_or("").to_string(),
                unreachable_url: frame["unreachableUrl"].as_str().map(str::to_string),
            })
        }
        "Page.navigatedWithinDocument" => Some(CdpSignal::SameDocumentNavigated {
            frame_id: params["frameId"].as_str().unwrap_or("").to_string(),
            url: params["url"].as_str().unwrap_or("").to_string(),
        }),
        "Page.loadEventFired" => Some(CdpSignal::LoadEventFired),
        "Overlay.inspectNodeRequested" => params["backendNodeId"]
            .as_i64()
            .map(|backend_node_id| CdpSignal::InspectNode { backend_node_id }),
        _ => None,
    }
}

fn one_based(value: &serde_json::Value) -> Option<u64> {
    value.as_u64().map(|n| n + 1)
}

/// RemoteObject → 可读文本。对象/数组走 preview.properties(B0:否则只剩「Object」)。
pub(crate) fn render_remote_object(object: &serde_json::Value) -> String {
    let kind = object["type"].as_str().unwrap_or("");
    let subtype = object["subtype"].as_str().unwrap_or("");
    if kind == "string" {
        return object["value"].as_str().unwrap_or("").to_string();
    }
    if kind == "undefined" {
        return "undefined".into();
    }
    if subtype == "null" {
        return "null".into();
    }
    if let Some(preview) = object.get("preview").filter(|p| p.is_object()) {
        if subtype != "error" && kind == "object" {
            return render_preview(preview);
        }
    }
    if let Some(value) = object.get("value").filter(|v| !v.is_null()) {
        return match value {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        };
    }
    if let Some(unserializable) = object["unserializableValue"].as_str() {
        return unserializable.to_string();
    }
    object["description"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| kind.to_string())
}

fn render_preview(preview: &serde_json::Value) -> String {
    let properties = preview["properties"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let overflow = if preview["overflow"] == true {
        ", …"
    } else {
        ""
    };
    let value_of = |property: &serde_json::Value| -> String {
        let value = property["value"].as_str().unwrap_or("");
        match property["type"].as_str() {
            Some("string") => format!("'{value}'"),
            Some("object") if value.is_empty() => "{…}".into(),
            _ => value.to_string(),
        }
    };
    if preview["subtype"].as_str() == Some("array") {
        let items: Vec<String> = properties.iter().map(value_of).collect();
        return format!("[{}{overflow}]", items.join(", "));
    }
    let items: Vec<String> = properties
        .iter()
        .map(|property| {
            format!(
                "{}: {}",
                property["name"].as_str().unwrap_or("?"),
                value_of(property)
            )
        })
        .collect();
    let class = preview["description"].as_str().unwrap_or("Object");
    let prefix = if class == "Object" {
        String::new()
    } else {
        format!("{class} ")
    };
    format!("{prefix}{{{}{overflow}}}", items.join(", "))
}

fn truncate_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((index, _)) => format!("{}…", &text[..index]),
        None => text.to_string(),
    }
}

/// 控制台环形缓冲。seq 从 1 起单调递增,清空后也不回退(增量拉取靠它)。
#[derive(Debug, Default)]
pub(crate) struct ConsoleRing {
    entries: VecDeque<ConsoleEntry>,
    next_seq: u64,
}

impl ConsoleRing {
    pub(crate) fn push(&mut self, draft: EntryDraft, ts: u64) -> u64 {
        self.next_seq += 1;
        let seq = self.next_seq;
        self.entries.push_back(ConsoleEntry {
            seq,
            ts,
            level: draft.level,
            text: truncate_chars(&draft.text, MAX_TEXT_CHARS),
            url: draft.url,
            line: draft.line,
            col: draft.col,
        });
        while self.entries.len() > CONSOLE_CAPACITY {
            self.entries.pop_front();
        }
        seq
    }

    /// seq 大于 `since` 的条目(按顺序)。
    pub(crate) fn since(&self, since: u64) -> Vec<ConsoleEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.seq > since)
            .cloned()
            .collect()
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    /// 主 frame 导航:未勾「保留日志」就清空(seq 不回退)。
    pub(crate) fn on_main_frame_navigated(&mut self, preserve_log: bool) {
        if !preserve_log {
            self.clear();
        }
    }

    #[cfg(test)]
    pub(crate) fn last_seq(&self) -> u64 {
        self.next_seq
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// `kz:preview-console` / `preview_console` 的载荷。
pub(crate) fn console_payload(entries: &[ConsoleEntry]) -> serde_json::Value {
    serde_json::json!({ "entries": entries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(signal: Option<CdpSignal>) -> EntryDraft {
        match signal {
            Some(CdpSignal::Entry(draft)) => draft,
            other => panic!("期望条目,实得 {other:?}"),
        }
    }

    /// 样例取自 B0 实测的 check3_raw_console_args_first_event。
    #[test]
    fn console_api多参数按预览渲染而不是塌成object() {
        let params = json!({
            "type": "log",
            "args": [
                {"type": "string", "value": "spike-log"},
                {"className": "Object", "description": "Object", "objectId": "1.2.1",
                 "preview": {"description": "Object", "overflow": false,
                             "properties": [{"name": "a", "type": "number", "value": "1"}],
                             "type": "object"},
                 "type": "object"},
                {"description": "42", "type": "number", "value": 42},
                {"type": "object", "subtype": "array", "description": "Array(2)",
                 "preview": {"subtype": "array", "overflow": false, "description": "Array(2)",
                             "properties": [{"name": "0", "type": "string", "value": "x"},
                                            {"name": "1", "type": "number", "value": "2"}]}},
                {"type": "undefined"},
                {"type": "object", "subtype": "null", "value": null}
            ],
            "stackTrace": {"callFrames": [{"url": "http://127.0.0.1:9960/index.html", "lineNumber": 18, "columnNumber": 10}]}
        });
        let draft = entry(parse_event("Runtime.consoleAPICalled", &params));
        assert_eq!(draft.level, "log");
        assert_eq!(draft.text, "spike-log {a: 1} 42 ['x', 2] undefined null");
        assert_eq!(draft.url, "http://127.0.0.1:9960/index.html");
        assert_eq!((draft.line, draft.col), (Some(19), Some(11)), "行列 1 起算");
        let warn = entry(parse_event(
            "Runtime.consoleAPICalled",
            &json!({"type": "warning", "args": [{"type": "string", "value": "w"}]}),
        ));
        assert_eq!(warn.level, "warning");
    }

    #[test]
    fn 未捕获异常带描述与位置() {
        let params = json!({"exceptionDetails": {
            "text": "Uncaught", "lineNumber": 24, "columnNumber": 27,
            "url": "http://127.0.0.1:9960/index.html",
            "exception": {"type": "object", "subtype": "error",
                          "description": "Error: spike-boom\n    at http://127.0.0.1:9960/index.html:25:28"}
        }});
        let draft = entry(parse_event("Runtime.exceptionThrown", &params));
        assert_eq!(draft.level, "error");
        assert!(
            draft.text.starts_with("Uncaught Error: spike-boom"),
            "{}",
            draft.text
        );
        assert_eq!(draft.line, Some(25));
    }

    #[test]
    fn log条目的网络失败归到network级别_verbose跳过() {
        let params = json!({"entry": {"source": "network", "level": "error",
            "text": "Failed to load resource: the server responded with a status of 404 (Not Found)",
            "url": "http://127.0.0.1:9960/missing.png"}});
        let draft = entry(parse_event("Log.entryAdded", &params));
        assert_eq!(draft.level, "network");
        assert!(draft.text.contains("404"));
        assert_eq!(draft.url, "http://127.0.0.1:9960/missing.png");
        let violation = json!({"entry": {"source": "violation", "level": "verbose", "text": "[Violation] slow"}});
        assert_eq!(parse_event("Log.entryAdded", &violation), None);
        let intervention =
            json!({"entry": {"source": "intervention", "level": "warning", "text": "w"}});
        assert_eq!(
            entry(parse_event("Log.entryAdded", &intervention)).level,
            "warning"
        );
    }

    /// B0:服务没在跑时先来 loadingFailed(Document),再来导航到 chrome-error 的 frameNavigated。
    /// 复核修复:两者都带关联键(requestId / frame.loaderId),子帧的 frameNavigated 仍然不算。
    #[test]
    fn 主文档失败与错误页导航_取消与子帧忽略() {
        assert_eq!(
            parse_event(
                "Network.loadingFailed",
                &json!({"requestId": "R1", "type": "Document", "errorText": "net::ERR_CONNECTION_REFUSED", "canceled": false})
            ),
            Some(CdpSignal::DocumentFailed {
                request_id: "R1".into(),
                error_text: "net::ERR_CONNECTION_REFUSED".into()
            })
        );
        for ignored in [
            json!({"type": "Fetch", "errorText": "net::ERR_UNSAFE_PORT", "canceled": false}),
            json!({"type": "Document", "errorText": "net::ERR_ABORTED", "canceled": false}),
            json!({"type": "Document", "errorText": "net::ERR_FAILED", "canceled": true}),
        ] {
            assert_eq!(
                parse_event("Network.loadingFailed", &ignored),
                None,
                "{ignored}"
            );
        }
        assert_eq!(
            parse_event(
                "Page.frameNavigated",
                &json!({"frame": {"id": "F", "loaderId": "R1", "url": "chrome-error://chromewebdata/",
                                  "unreachableUrl": "http://127.0.0.1:11685/"}, "type": "Navigation"})
            ),
            Some(CdpSignal::MainFrameNavigated {
                frame_id: "F".into(),
                loader_id: "R1".into(),
                url: "chrome-error://chromewebdata/".into(),
                unreachable_url: Some("http://127.0.0.1:11685/".into()),
            })
        );
        assert_eq!(
            parse_event(
                "Page.navigatedWithinDocument",
                &json!({"frameId": "F", "url": "http://127.0.0.1:4296/spa/route-2", "navigationType": "historyApi"})
            ),
            Some(CdpSignal::SameDocumentNavigated {
                frame_id: "F".into(),
                url: "http://127.0.0.1:4296/spa/route-2".into(),
            }),
            "SPA 的 pushState(Edge 实测形态)"
        );
        assert_eq!(
            parse_event(
                "Page.frameNavigated",
                &json!({"frame": {"id": "C", "parentId": "F", "url": "http://x/"}})
            ),
            None,
            "子 frame 导航不算"
        );
        assert_eq!(
            parse_event(
                "Overlay.inspectNodeRequested",
                &json!({"backendNodeId": 42})
            ),
            Some(CdpSignal::InspectNode {
                backend_node_id: 42
            })
        );
        assert_eq!(parse_event("Network.responseReceived", &json!({})), None);
    }

    #[test]
    fn 环形缓冲容量_seq单调_导航清空与保留日志_字符边界截断() {
        let mut ring = ConsoleRing::default();
        for index in 0..(CONSOLE_CAPACITY + 20) {
            ring.push(
                EntryDraft {
                    level: "log".into(),
                    text: format!("m{index}"),
                    ..EntryDraft::default()
                },
                7,
            );
        }
        assert_eq!(ring.len(), CONSOLE_CAPACITY, "容量上限");
        let all = ring.since(0);
        assert_eq!(all.first().unwrap().seq, 21, "丢头留尾");
        assert!(
            all.windows(2).all(|w| w[1].seq == w[0].seq + 1),
            "seq 单调连续"
        );
        assert_eq!(ring.since(ring.last_seq() - 2).len(), 2);

        ring.on_main_frame_navigated(true);
        assert_eq!(ring.len(), CONSOLE_CAPACITY, "保留日志时不清");
        let before = ring.last_seq();
        ring.on_main_frame_navigated(false);
        assert_eq!(ring.len(), 0);
        let seq = ring.push(EntryDraft::default(), 1);
        assert_eq!(seq, before + 1, "清空后 seq 不回退");

        let long = "中".repeat(MAX_TEXT_CHARS + 5);
        ring.push(
            EntryDraft {
                text: long,
                ..EntryDraft::default()
            },
            1,
        );
        let tail = ring.since(seq);
        let text = &tail.last().unwrap().text;
        assert_eq!(text.chars().count(), MAX_TEXT_CHARS + 1, "截断 + 省略号");
    }
}
