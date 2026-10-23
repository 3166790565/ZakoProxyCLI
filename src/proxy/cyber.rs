use serde_json::Value;
use std::collections::BTreeMap;

const MAX_CHANNELS: usize = 1024;
const MARKER: &[u8] = b"cyber";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Channel {
    Responses(u64, u64, bool),
    Chat(u64, usize, bool),
    Anthropic(u64),
}

// Track only a matching prefix length per text channel, never model output itself.
#[derive(Default)]
pub(super) struct CyberDetector {
    tails: BTreeMap<Channel, usize>,
}

fn advance(tail: &mut usize, text: &str) -> bool {
    for byte in text.bytes().map(|byte| byte.to_ascii_lowercase()) {
        if byte == MARKER[*tail] {
            *tail += 1;
            if *tail == MARKER.len() {
                *tail = 0;
                return true;
            }
        } else {
            // The marker has no overlapping prefixes except its initial 'c'.
            *tail = usize::from(byte == MARKER[0]);
        }
    }
    false
}

fn contains(text: Option<&Value>) -> bool {
    text.and_then(Value::as_str)
        .is_some_and(|text| advance(&mut 0, text))
}

fn output_part(part: &Value) -> bool {
    match part.get("type").and_then(Value::as_str) {
        Some("output_text") => contains(part.get("text")),
        Some("refusal") => contains(part.get("refusal")),
        _ => false,
    }
}

fn output_item(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str) == Some("message")
        && item.get("role").and_then(Value::as_str) == Some("assistant")
        && item
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|parts| parts.iter().any(output_part))
}

fn response_output(response: &Value) -> bool {
    response
        .get("output")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(output_item))
}

fn chat_content(content: Option<&Value>) -> bool {
    contains(content)
        || content.and_then(Value::as_array).is_some_and(|parts| {
            parts.iter().any(|part| {
                matches!(part.get("type").and_then(Value::as_str), Some("text"))
                    && contains(part.get("text"))
            })
        })
}

fn assistant(message: &Value) -> bool {
    message.get("role").and_then(Value::as_str) == Some("assistant")
}

fn anthropic_message(message: &Value) -> bool {
    assistant(message)
        && message
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|parts| {
                parts.iter().any(|part| {
                    part.get("type").and_then(Value::as_str) == Some("text")
                        && contains(part.get("text"))
                })
            })
}

impl CyberDetector {
    fn delta(&mut self, channel: Channel, text: Option<&Value>) -> bool {
        let Some(text) = text.and_then(Value::as_str).filter(|text| !text.is_empty()) else {
            return false;
        };
        if self.tails.len() >= MAX_CHANNELS && !self.tails.contains_key(&channel) {
            return advance(&mut 0, text);
        }
        advance(self.tails.entry(channel).or_default(), text)
    }

    pub(super) fn observe(&mut self, value: &Value, event_name: &str, sse: bool) -> bool {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(event_name);
        if kind == "error"
            || event_name == "error"
            || value.get("error").is_some_and(|error| !error.is_null())
        {
            return false;
        }

        if !sse {
            return response_output(value)
                || value
                    .get("choices")
                    .and_then(Value::as_array)
                    .is_some_and(|choices| {
                        choices.iter().any(|choice| {
                            choice.get("message").is_some_and(|message| {
                                assistant(message)
                                    && (chat_content(message.get("content"))
                                        || contains(message.get("refusal")))
                            })
                        })
                    })
                || (kind == "message" && anthropic_message(value));
        }

        // Full snapshots are checked independently; never append them to earlier deltas.
        match kind {
            "response.output_text.delta" | "response.refusal.delta" => {
                return self.delta(
                    Channel::Responses(
                        value
                            .get("output_index")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                        value
                            .get("content_index")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                        kind == "response.refusal.delta",
                    ),
                    value.get("delta"),
                );
            }
            "response.output_text.done" => return contains(value.get("text")),
            "response.refusal.done" => return contains(value.get("refusal")),
            "response.content_part.added" | "response.content_part.done" => {
                return value.get("part").is_some_and(output_part);
            }
            "response.output_item.added" | "response.output_item.done" => {
                return value.get("item").is_some_and(output_item);
            }
            "response.completed" | "response.incomplete" => {
                return value.get("response").is_some_and(response_output);
            }
            "message_start" => {
                return value.get("message").is_some_and(anthropic_message);
            }
            "content_block_start" => {
                let index = value.get("index").and_then(Value::as_u64).unwrap_or(0);
                self.tails.remove(&Channel::Anthropic(index));
                return value.get("content_block").is_some_and(|block| {
                    block.get("type").and_then(Value::as_str) == Some("text")
                        && self.delta(Channel::Anthropic(index), block.get("text"))
                });
            }
            "content_block_delta" => {
                return value.get("delta").is_some_and(|delta| {
                    delta.get("type").and_then(Value::as_str) == Some("text_delta")
                        && self.delta(
                            Channel::Anthropic(
                                value.get("index").and_then(Value::as_u64).unwrap_or(0),
                            ),
                            delta.get("text"),
                        )
                });
            }
            "content_block_stop" => {
                self.tails.remove(&Channel::Anthropic(
                    value.get("index").and_then(Value::as_u64).unwrap_or(0),
                ));
                return false;
            }
            _ => {}
        }

        if let Some(choices) = value.get("choices").and_then(Value::as_array) {
            for choice in choices {
                let index = choice.get("index").and_then(Value::as_u64).unwrap_or(0);
                let Some(delta) = choice.get("delta") else {
                    continue;
                };
                if delta
                    .get("role")
                    .and_then(Value::as_str)
                    .is_some_and(|role| role != "assistant")
                {
                    continue;
                }
                if self.delta(Channel::Chat(index, 0, false), delta.get("content"))
                    || self.delta(Channel::Chat(index, 0, true), delta.get("refusal"))
                {
                    return true;
                }
                if let Some(parts) = delta.get("content").and_then(Value::as_array) {
                    for (position, part) in parts.iter().enumerate() {
                        if part.get("type").and_then(Value::as_str) == Some("text")
                            && self.delta(Channel::Chat(index, position, false), part.get("text"))
                        {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}
