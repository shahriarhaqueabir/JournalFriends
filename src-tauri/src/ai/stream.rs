#![allow(dead_code)]

use crate::ai::sse::{SseEvent, SseParser};

/// Parse llama.cpp OpenAI-compatible SSE server output.
///
/// Delegates all chunk parsing to [`SseParser`] and maps each parsed
/// event into a caller-friendly [`StreamEvent`].
pub struct StreamParser {
    sse: SseParser,
}

impl StreamParser {
    pub fn new() -> Self {
        Self {
            sse: SseParser::new(),
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent> {
        self.sse
            .feed(chunk)
            .into_iter()
            .map(|event| match event {
                SseEvent::Data(json) => StreamEvent::Token(json),
                SseEvent::Done => StreamEvent::Done,
                SseEvent::Error(e) => StreamEvent::Error(e),
            })
            .collect()
    }

    pub fn reset(&mut self) {
        self.sse.reset();
    }
}

pub enum StreamEvent {
    Token(String),
    Done,
    Error(String),
}
