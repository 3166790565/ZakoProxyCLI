use super::*;

fn assert_json_marker(value: &str, expected: bool) {
    let mut whole = Observer::new(false, None);
    whole.feed(value.as_bytes());
    whole.finish();
    assert_eq!(whole.cyber, expected, "whole JSON: {value}");

    let mut fragmented = Observer::new(false, None);
    for byte in value.as_bytes() {
        fragmented.feed(std::slice::from_ref(byte));
    }
    fragmented.finish();
    assert_eq!(fragmented.cyber, expected, "fragmented JSON: {value}");
}

fn assert_sse_marker(value: &str, expected: bool) {
    let bytes = value.as_bytes();
    for split in 0..=bytes.len() {
        let mut observer = Observer::new(true, None);
        observer.feed(&bytes[..split]);
        observer.feed(&bytes[split..]);
        observer.finish();
        assert_eq!(observer.cyber, expected, "SSE split at {split}: {value}");
    }

    let mut fragmented = Observer::new(true, None);
    for byte in bytes {
        fragmented.feed(std::slice::from_ref(byte));
    }
    fragmented.finish();
    assert_eq!(fragmented.cyber, expected, "bytewise SSE: {value}");
}

fn echoed_response() -> Value {
    json!({
        "id": "resp_cyber",
        "object": "response",
        "status": "completed",
        "model": "cyber-model",
        "instructions": "The cyber marker in these instructions is not model output.",
        "input": [{"role": "user", "content": "Explain the cyber metadata field."}],
        "metadata": {"cyber": "cyber"},
        "output": [{
            "id": "msg_cyber",
            "type": "message",
            "role": "assistant",
            "status": "completed",
            "content": [{"type": "output_text", "text": "Hello, world!", "annotations": []}]
        }]
    })
}

#[test]
fn cyber_baseline_probe() {
    let mut observer = Observer::new(false, None);
    observer.feed(echoed_response().to_string().as_bytes());
    observer.finish();
    println!("OBSERVED_CYBER={}", observer.cyber);
}

#[test]
fn cyber_json_responses_ignore_echoed_instructions_input_and_metadata() {
    assert_json_marker(&echoed_response().to_string(), false);
}

#[test]
fn cyber_json_responses_detect_output_text_and_refusal() {
    for content in [
        json!({"type": "output_text", "text": "A CyBeR warning", "annotations": []}),
        json!({"type": "refusal", "refusal": "CYBER policy refusal"}),
    ] {
        let response = json!({
            "object": "response",
            "output": [{
                "id": "msg_1", "type": "message", "role": "assistant",
                "status": "completed", "content": [content]
            }]
        });
        assert_json_marker(&response.to_string(), true);
    }
}

#[test]
fn cyber_json_responses_decode_unicode_escapes() {
    assert_json_marker(
        r#"{"object":"response","output":[{"id":"msg_1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"\u0063\u0059\u0062\u0045\u0072","annotations":[]}]}]}"#,
        true,
    );
}

#[test]
fn cyber_json_responses_ignore_tools_and_reasoning() {
    assert_json_marker(
        &json!({
            "object": "response",
            "output": [
                {"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "cyber"}]},
                {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "cyber", "arguments": "{\"query\":\"cyber\"}"},
                {"type": "message", "id": "msg_1", "role": "assistant", "content": [{"type": "output_text", "text": "All done.", "annotations": []}]}
            ]
        })
        .to_string(),
        false,
    );
}

#[test]
fn cyber_json_chat_detects_content_and_refusal() {
    for message in [
        json!({"role": "assistant", "content": "A CYBER warning"}),
        json!({"role": "assistant", "content": null, "refusal": "A cyber refusal"}),
    ] {
        assert_json_marker(
            &json!({"object": "chat.completion", "choices": [{"index": 0, "message": message, "finish_reason": "stop"}]}).to_string(),
            true,
        );
    }
}

#[test]
fn cyber_json_chat_ignores_metadata_reasoning_and_tool_arguments() {
    assert_json_marker(
        &json!({
            "id": "chatcmpl_cyber", "model": "cyber-model", "object": "chat.completion",
            "choices": [{
                "index": 0, "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant", "content": "Hello!", "reasoning_content": "cyber",
                    "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "cyber", "arguments": "{\"query\":\"cyber\"}"}}]
                }
            }]
        }).to_string(),
        false,
    );
}

#[test]
fn cyber_json_anthropic_detects_only_text_blocks() {
    assert_json_marker(
        &json!({"id": "msg_1", "type": "message", "role": "assistant", "content": [{"type": "text", "text": "A CyBeR warning"}], "stop_reason": "end_turn"}).to_string(),
        true,
    );
    assert_json_marker(
        &json!({
            "id": "msg_cyber", "type": "message", "role": "assistant", "model": "cyber-model",
            "content": [
                {"type": "thinking", "thinking": "cyber", "signature": "cyber"},
                {"type": "tool_use", "id": "toolu_1", "name": "cyber", "input": {"query": "cyber"}},
                {"type": "text", "text": "Hello!"}
            ], "stop_reason": "end_turn"
        })
        .to_string(),
        false,
    );
}

#[test]
fn cyber_json_never_concatenates_separate_output_blocks_or_choices() {
    for response in [
        json!({"object": "response", "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "cy", "annotations": []}, {"type": "output_text", "text": "ber", "annotations": []}]}]}),
        json!({"object": "chat.completion", "choices": [{"index": 0, "message": {"role": "assistant", "content": "cy"}}, {"index": 1, "message": {"role": "assistant", "content": "ber"}}]}),
        json!({"type": "message", "role": "assistant", "content": [{"type": "text", "text": "cy"}, {"type": "text", "text": "ber"}]}),
    ] {
        assert_json_marker(&response.to_string(), false);
    }
}

#[test]
fn cyber_sse_responses_ignore_created_and_completed_echoed_fields() {
    let response = echoed_response();
    let stream = format!(
        "event: response.created\ndata: {}\n\nevent: response.completed\ndata: {}\n\n",
        json!({"type": "response.created", "response": {"id": "resp_cyber", "model": "cyber-model", "instructions": "cyber", "metadata": {"cyber": "cyber"}, "input": "cyber", "output": []}}),
        json!({"type": "response.completed", "response": response})
    );
    assert_sse_marker(&stream, false);
}

#[test]
fn cyber_sse_responses_detect_text_across_deltas_and_network_chunks() {
    assert_sse_marker(
        concat!(
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Cy\"}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"BeR\"}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n"
        ),
        true,
    );
}

#[test]
fn cyber_sse_responses_detect_refusal_across_deltas() {
    assert_sse_marker(
        concat!(
            "data: {\"type\":\"response.refusal.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"cy\"}\n\n",
            "data: {\"type\":\"response.refusal.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"ber\"}\n\n"
        ),
        true,
    );
}

#[test]
fn cyber_sse_chat_detects_content_and_refusal_across_deltas() {
    for field in ["content", "refusal"] {
        let mut stream = String::new();
        for text in ["cy", "BER"] {
            stream.push_str(&format!(
                "data: {}\n\n",
                json!({"id": "chatcmpl_1", "object": "chat.completion.chunk", "choices": [{"index": 0, "delta": {(field): text}, "finish_reason": null}]})
            ));
        }
        stream.push_str("data: [DONE]\n\n");
        assert_sse_marker(&stream, true);
    }
}

#[test]
fn cyber_sse_anthropic_detects_text_across_deltas() {
    assert_sse_marker(
        concat!(
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"cY\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"BeR\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        ),
        true,
    );
}

#[test]
fn cyber_sse_ignores_reasoning_tools_errors_event_names_and_comments() {
    for stream in [
        concat!(
            ": cyber keepalive\n\nevent: cyber\ndata: {\"metadata\":{\"note\":\"cyber\"}}\n\n",
            "data: {\"type\":\"response.reasoning_text.delta\",\"item_id\":\"rs_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"cyber\"}\n\n",
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":0,\"delta\":\"cyber\"}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":1,\"delta\":\"{\\\"query\\\":\\\"cyber\\\"}\"}\n\n",
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"cyber_error\",\"message\":\"cyber\"}}\n\n"
        ),
        concat!(
            ": cyber keepalive\n\n",
            "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"cyber\",\"tool_calls\":[{\"index\":0,\"id\":\"call_cyber\",\"type\":\"function\",\"function\":{\"name\":\"cyber\",\"arguments\":\"cyber\"}}]}}]}\n\n",
            "data: {\"error\":{\"code\":\"cyber_error\",\"message\":\"cyber\"}}\n\n"
        ),
        concat!(
            ": cyber keepalive\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"cyber\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\\\"cyber\\\"}\"}}\n\n",
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"cyber_error\",\"message\":\"cyber\"}}\n\n"
        ),
    ] {
        assert_sse_marker(stream, false);
    }
}

#[test]
fn cyber_sse_responses_keep_output_items_and_content_parts_separate() {
    for other_output in [0, 1] {
        let other_content = u64::from(other_output == 0);
        let stream = format!(
            "data: {}\n\ndata: {}\n\n",
            json!({"type": "response.output_text.delta", "item_id": "msg_1", "output_index": 0, "content_index": 0, "delta": "cy"}),
            json!({"type": "response.output_text.delta", "item_id": if other_output == 0 {"msg_1"} else {"msg_2"}, "output_index": other_output, "content_index": other_content, "delta": "ber"})
        );
        assert_sse_marker(&stream, false);
    }

    assert_sse_marker(
        concat!(
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"cy\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_2\",\"output_index\":1,\"content_index\":0,\"delta\":\"unrelated\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"ber\"}\n\n"
        ),
        true,
    );
}

#[test]
fn cyber_sse_chat_keeps_interleaved_choices_separate() {
    let separated = concat!(
        "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"cy\"}}]}\n\n",
        "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":1,\"delta\":{\"content\":\"ber\"}}]}\n\n"
    );
    assert_sse_marker(separated, false);
    let interleaved = format!(
        "{separated}data: {{\"object\":\"chat.completion.chunk\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"ber\"}}}}]}}\n\n"
    );
    assert_sse_marker(&interleaved, true);
}

#[test]
fn cyber_sse_anthropic_keeps_interleaved_content_blocks_separate() {
    let separated = concat!(
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"cy\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"ber\"}}\n\n"
    );
    assert_sse_marker(separated, false);
    let interleaved = format!(
        "{separated}event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"ber\"}}}}\n\n"
    );
    assert_sse_marker(&interleaved, true);
}

#[test]
fn cyber_sse_detects_final_only_responses_snapshots() {
    let part = json!({"type": "output_text", "text": "A CYBER warning", "annotations": []});
    let item = json!({"id": "msg_1", "type": "message", "role": "assistant", "status": "completed", "content": [part.clone()]});
    for event in [
        json!({"type": "response.output_text.done", "item_id": "msg_1", "output_index": 0, "content_index": 0, "text": "A CYBER warning"}),
        json!({"type": "response.refusal.done", "item_id": "msg_1", "output_index": 0, "content_index": 0, "refusal": "A CYBER refusal"}),
        json!({"type": "response.content_part.done", "item_id": "msg_1", "output_index": 0, "content_index": 0, "part": part}),
        json!({"type": "response.output_item.done", "output_index": 0, "item": item.clone()}),
        json!({"type": "response.completed", "response": {"id": "resp_1", "status": "completed", "output": [item]}}),
    ] {
        assert_sse_marker(
            &format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            ),
            true,
        );
    }
}

#[test]
fn cyber_http_errors_skip_even_output_shaped_payloads() {
    for sse in [false, true] {
        let mut response = echoed_response();
        response["output"][0]["content"][0]["text"] = json!("cyber");
        let payload = if sse {
            format!(
                "data: {}\n\n",
                json!({"type": "response.completed", "response": response})
            )
        } else {
            response.to_string()
        };
        let mut observer = Observer::new(sse, None);
        observer.http_error = true;
        for chunk in payload.as_bytes().chunks(3) {
            observer.feed(chunk);
        }
        observer.finish();
        assert!(!observer.cyber, "HTTP-error payload with SSE={sse}");
    }
}

#[test]
fn cyber_channel_limit_does_not_merge_overflow_channels_or_evict_existing_tails() {
    let delta = |index, text| {
        format!(
            "data: {}\n\n",
            json!({"type": "response.output_text.delta", "output_index": index, "content_index": 0, "delta": text})
        )
    };
    for (last_index, last_text) in [(0, "ber"), (1026, "CyBeR")] {
        let mut observer = Observer::new(true, None);
        for index in 0..1024 {
            observer.feed(delta(index, "cy").as_bytes());
        }
        observer.feed(delta(1024, "cy").as_bytes());
        observer.feed(delta(1025, "ber").as_bytes());
        assert!(!observer.cyber);
        observer.feed(delta(last_index, last_text).as_bytes());
        observer.finish();
        assert!(observer.cyber, "channel {last_index}");
    }
}

struct CyberUpstream(tokio::task::JoinHandle<()>);

impl Drop for CyberUpstream {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn assert_forwarded_marker(payload: String, sse: bool, expected: bool) {
    use axum::routing::post;
    use tower::ServiceExt;

    let expected_bytes = payload.as_bytes().to_vec();
    let upstream = axum::Router::new().route(
        "/v1/responses",
        post(move |headers: HeaderMap, Json(request): Json<Value>| {
            let payload = payload.clone();
            async move {
                assert_eq!(headers["authorization"], "Bearer upstream-test-secret");
                assert_eq!(request["instructions"], "cyber is input metadata");
                let chunks: Vec<_> = payload
                    .as_bytes()
                    .chunks(7)
                    .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
                    .collect();
                (
                    [
                        (
                            "content-type",
                            if sse {
                                "text/event-stream"
                            } else {
                                "application/json"
                            },
                        ),
                        ("x-request-id", "cyber-regression-upstream"),
                    ],
                    Body::from_stream(futures_util::stream::iter(chunks)),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let _upstream = CyberUpstream(tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    }));

    let dir = tempfile::tempdir().unwrap();
    let state = crate::new_app(dir.path(), dir.path().into()).unwrap();
    let upstream_secret = state.store.seal("upstream-test-secret").unwrap();
    state
        .store
        .update(|config| {
            config.groups.push(Group {
                id: "cyber-group".into(),
                ..Default::default()
            });
            config.keys.push(ApiKey {
                id: "cyber-key".into(),
                group_id: "cyber-group".into(),
                hash: store::digest("cyber-client-key"),
                enabled: true,
                ..Default::default()
            });
            config.accounts.push(Account {
                id: "cyber-account".into(),
                endpoint,
                api_key: upstream_secret,
                group_ids: vec!["cyber-group".into()],
                models: vec!["chosen".into()],
                protocols: vec!["responses".into()],
                enabled: true,
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();

    let response = crate::app(state.clone())
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .header("authorization", "Bearer cyber-client-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "model": "chosen", "input": "Hello",
                        "instructions": "cyber is input metadata", "stream": sse
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["x-request-id"],
        "cyber-regression-upstream"
    );
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    assert_eq!(bytes.as_ref(), expected_bytes.as_slice());

    let logs = state.store.logs(0, store::now() + 10).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, 200);
    assert_eq!(logs[0].request_path, "/v1/responses");
    assert_eq!(logs[0].attempts, 1);
    assert_eq!(logs[0].cyber, expected, "SSE={sse}");
    assert_eq!(state.runtime.lock().unwrap()["cyber-account"].active, 0);
}

#[tokio::test]
async fn cyber_proxy_preserves_json_and_sse_bytes_and_persists_correct_marker() {
    for expected in [false, true] {
        let mut response = echoed_response();
        if expected {
            response["output"][0]["content"][0]["text"] = json!("A CyBeR warning");
        }
        assert_forwarded_marker(response.to_string(), false, expected).await;

        let mut stream = format!(
            "event: response.created\ndata: {}\n\n",
            json!({"type": "response.created", "response": {"id": "resp_cyber", "instructions": "cyber", "metadata": {"cyber": "cyber"}, "output": []}})
        );
        for delta in if expected {
            ["Cy", "BeR warning"]
        } else {
            ["Hello", ", world!"]
        } {
            stream.push_str(&format!(
                "event: response.output_text.delta\ndata: {}\n\n",
                json!({"type": "response.output_text.delta", "item_id": "msg_cyber", "output_index": 0, "content_index": 0, "delta": delta})
            ));
        }
        stream.push_str(&format!(
            "event: response.completed\ndata: {}\n\n",
            json!({"type": "response.completed", "response": response})
        ));
        assert_forwarded_marker(stream, true, expected).await;
    }
}
