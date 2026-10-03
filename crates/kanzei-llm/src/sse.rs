//! 增量 SSE 解析器。按 LF、CRLF 或 CR 切行，整行收齐后解码 UTF-8。
//! 跨 chunk 保留 UTF-8 字节和 CRLF 状态，只移除流开头的一个 BOM。

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

#[derive(Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    event: String,
    data: Vec<String>,
    first_line_seen: bool,
    skip_lf: bool,
}

impl SseParser {
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        for &byte in chunk {
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            if byte != b'\r' && byte != b'\n' {
                self.buffer.push(byte);
                continue;
            }
            let line = String::from_utf8_lossy(&self.buffer).into_owned();
            self.buffer.clear();
            let line = if self.first_line_seen {
                line.as_str()
            } else {
                self.first_line_seen = true;
                line.strip_prefix('\u{feff}').unwrap_or(&line)
            };
            self.handle_line(line, &mut out);
            self.skip_lf = byte == b'\r';
        }
        out
    }

    fn handle_line(&mut self, line: &str, out: &mut Vec<SseEvent>) {
        if line.is_empty() {
            if !self.data.is_empty() {
                out.push(SseEvent {
                    event: std::mem::take(&mut self.event),
                    data: self.data.join("\n"),
                });
                self.data.clear();
            } else {
                self.event.clear();
            }
            return;
        }
        if line.starts_with(':') {
            return; // comment / keepalive
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => self.event = value.to_string(),
            "data" => self.data.push(value.to_string()),
            _ => {} // id / retry / 未知字段忽略
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_across_chunk_boundaries() {
        let mut p = SseParser::default();
        let mut events = p.feed(b"event: message_start\ndata: {\"a\":");
        assert!(events.is_empty());
        events.extend(p.feed(b" 1}\n\nevent: ping\ndata: {}\n\n"));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, "message_start");
        assert_eq!(events[0].data, "{\"a\": 1}");
        assert_eq!(events[1].event, "ping");
    }

    #[test]
    fn joins_multiline_data_and_handles_crlf() {
        let mut p = SseParser::default();
        let events = p.feed(b"data: line1\r\ndata: line2\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line1\nline2");
    }

    #[test]
    fn valid_framing_is_independent_of_chunk_boundaries() {
        let wire = "\u{feff}event: delta\rdata: 中文\r\rdata: next\r\ndata: line2\n\n".as_bytes();
        let expected = vec![
            SseEvent {
                event: "delta".into(),
                data: "中文".into(),
            },
            SseEvent {
                event: String::new(),
                data: "next\nline2".into(),
            },
        ];
        for split in 0..=wire.len() {
            let mut parser = SseParser::default();
            let mut events = parser.feed(&wire[..split]);
            events.extend(parser.feed(&wire[split..]));
            assert_eq!(events, expected, "split at {split}");
        }
        let mut parser = SseParser::default();
        let events: Vec<_> = wire
            .chunks(1)
            .flat_map(|chunk| parser.feed(chunk))
            .collect();
        assert_eq!(events, expected);
    }

    #[test]
    fn only_the_leading_bom_is_ignored_and_partial_events_are_not_dispatched() {
        let mut parser = SseParser::default();
        let events = parser.feed("\u{feff}data: first\n\ndata: \u{feff}body\n\n".as_bytes());
        assert_eq!(
            events
                .iter()
                .map(|event| event.data.as_str())
                .collect::<Vec<_>>(),
            ["first", "\u{feff}body"]
        );
        assert!(parser.feed(b"data: incomplete\r").is_empty());
        assert!(parser.feed(b"\n").is_empty());
        assert!(parser.feed(b"").is_empty());
        assert_eq!(parser.feed(b"\r")[0].data, "incomplete");
        assert!(parser.feed(b"\n").is_empty());
    }
}
