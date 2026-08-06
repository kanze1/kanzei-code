//! 增量 SSE 解析器。字节级缓冲(chunk 边界可能切开 UTF-8 码点,但 0x0A 不会出现在
//! 多字节序列中,按 \n 切行后再做 UTF-8 转换即安全)。

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
}

impl SseParser {
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = self.buffer.drain(..=pos).collect();
            let mut line = String::from_utf8_lossy(&line_bytes).into_owned();
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            self.handle_line(&line, &mut out);
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
}
