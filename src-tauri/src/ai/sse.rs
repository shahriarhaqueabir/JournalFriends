#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    Data(String),
    Done,
    Error(String),
}

pub struct SseParser {
    buffer: String,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let text = String::from_utf8_lossy(chunk);
        self.buffer.push_str(&text);

        let mut events = Vec::new();
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos].trim().to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data == "[DONE]" {
                    events.push(SseEvent::Done);
                } else if !data.is_empty() {
                    events.push(SseEvent::Data(data.to_string()));
                }
            }
        }

        events
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
    }
}
