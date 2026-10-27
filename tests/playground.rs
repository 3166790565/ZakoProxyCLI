use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tower::ServiceExt;
use zakoai_proxy::{
    Shared, app, new_app,
    store::{Account, ApiKey, Group},
};

fn fixture() -> (tempfile::TempDir, Shared) {
    let dir = tempfile::tempdir().unwrap();
    let state = new_app(dir.path(), dir.path().join("web")).unwrap();
    state.store.session("playground-console-session").unwrap();
    let secret = "sk-zako-playground-test";
    let sealed = state.store.seal(secret).unwrap();
    state
        .store
        .update(|config| {
            config.groups.push(Group {
                id: "playground-group".into(),
                name: "Playground".into(),
                ..Default::default()
            });
            config.keys.push(ApiKey {
                id: "playground-key".into(),
                name: "Playground".into(),
                group_id: "playground-group".into(),
                secret: sealed,
                hash: zakoai_proxy::store::digest(secret),
                enabled: true,
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    (dir, state)
}

fn sample_pdf() -> Vec<u8> {
    let stream = b"BT /F1 12 Tf 72 720 Td (Hello PDF attachment) Tj ET";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", stream.len(), String::from_utf8_lossy(stream)),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in offsets.into_iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

async fn request(
    state: &Shared,
    method: &str,
    path: &str,
    body: Value,
    signed_in: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("x-console-path", "/");
    if signed_in {
        builder = builder.header("cookie", "zako_session=playground-console-session");
    }
    let response = app(state.clone())
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn playground_requires_console_auth_and_enabled_key() {
    let (_dir, state) = fixture();
    let payload = json!({"key_id":"playground-key","model":"demo","messages":[{"role":"user","content":"Hi"}]});
    assert_eq!(
        request(
            &state,
            "POST",
            "/api/playground/chat",
            payload.clone(),
            false
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &state,
            "GET",
            "/api/playground/models?key_id=playground-key",
            Value::Null,
            false
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &state,
            "POST",
            "/api/playground/chat",
            json!({"key_id":"unknown","model":"demo","messages":[{"role":"user","content":"Hi"}]}),
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    state
        .store
        .update(|config| {
            config.keys[0].enabled = false;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        request(&state, "POST", "/api/playground/chat", payload, true)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn playground_lists_chat_models_and_forwards_without_exposing_key() {
    let (_dir, state) = fixture();
    let router = Router::new().route(
        "/v1/chat/completions",
        post(|Json(body): Json<Value>| async move {
            Json(json!({"choices":[{"index":0,"message":{"role":"assistant","content":"Hello"},"finish_reason":"stop"}],"echo":body}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let sealed_upstream = state.store.seal("upstream-secret").unwrap();
    state
        .store
        .update(|config| {
            config.accounts.push(Account {
                id: "chat-account".into(),
                name: "Chat".into(),
                endpoint,
                api_key: sealed_upstream,
                models: vec!["actual-model".into()],
                model_mappings: BTreeMap::from([("display-model".into(), "actual-model".into())]),
                group_ids: vec!["playground-group".into()],
                protocols: vec!["chat".into()],
                enabled: true,
                ..Default::default()
            });
            config.accounts.push(Account {
                id: "responses-only".into(),
                name: "Responses".into(),
                models: vec!["not-chat".into()],
                group_ids: vec!["playground-group".into()],
                protocols: vec!["responses".into()],
                enabled: true,
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();

    let (status, models) = request(
        &state,
        "GET",
        "/api/playground/models?key_id=playground-key",
        Value::Null,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models["models"], json!(["actual-model", "display-model"]));
    assert!(!models.to_string().contains("sk-zako-playground-test"));

    let (status, completion) = request(
        &state,
        "POST",
        "/api/playground",
        json!({
            "key_id":"playground-key", "model":"display-model",
            "messages":[{"role":"system","content":"Be brief"},{"role":"user","content":"Hi"}],
            "temperature":0.7,"max_tokens":64,"stream":false
        }),
        true,
    )
    .await;
    task.abort();
    assert_eq!(status, StatusCode::OK, "{completion}");
    assert_eq!(completion["choices"][0]["message"]["content"], "Hello");
    assert_eq!(completion["echo"]["model"], "actual-model");
    assert_eq!(completion["echo"]["temperature"], 0.7);
    assert!(completion["echo"].get("key_id").is_none());
    assert!(!completion.to_string().contains("sk-zako-playground-test"));
}

#[tokio::test]
async fn playground_streams_chat_completion_events() {
    let (_dir, state) = fixture();
    let events = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let router = Router::new().route(
        "/v1/chat/completions",
        post(move || async move {
            ([(header::CONTENT_TYPE, "text/event-stream")], events).into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let sealed_upstream = state.store.seal("upstream-secret").unwrap();
    state
        .store
        .update(|config| {
            config.accounts.push(Account {
                id: "stream-account".into(),
                name: "Stream".into(),
                endpoint,
                api_key: sealed_upstream,
                models: vec!["demo".into()],
                group_ids: vec!["playground-group".into()],
                protocols: vec!["chat".into()],
                enabled: true,
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    let response = app(state).oneshot(Request::builder()
        .method("POST").uri("/api/playground")
        .header("content-type", "application/json")
        .header("x-console-path", "/")
        .header("cookie", "zako_session=playground-console-session")
        .body(Body::from(json!({"key_id":"playground-key","model":"demo","messages":[{"role":"user","content":"Hi"}],"stream":true}).to_string())).unwrap())
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    task.abort();
    let output = String::from_utf8(body.to_vec()).unwrap();
    assert!(output.contains("\"content\":\"Hello\""), "{output}");
    assert!(output.contains("data: [DONE]"), "{output}");
}

#[tokio::test]
async fn attachment_upload_requires_auth_and_checks_type_and_size() {
    let (_dir, state) = fixture();
    let text = json!({"name":"../notes.txt","mime_type":"text/plain","data_base64":STANDARD.encode("Hello from a file")});
    assert_eq!(
        request(
            &state,
            "POST",
            "/api/playground/attachment",
            text.clone(),
            false
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, uploaded) =
        request(&state, "POST", "/api/playground/attachment", text, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(uploaded["name"], "notes.txt");
    assert_eq!(uploaded["kind"], "text");
    assert_eq!(uploaded["text"], "Hello from a file");
    let pdf = json!({"name":"paper.pdf","mime_type":"application/pdf","data_base64":STANDARD.encode(sample_pdf())});
    let (status, uploaded_pdf) =
        request(&state, "POST", "/api/playground/attachment", pdf, true).await;
    assert_eq!(status, StatusCode::OK, "{uploaded_pdf}");
    assert_eq!(uploaded_pdf["kind"], "text");
    assert!(
        uploaded_pdf["text"]
            .as_str()
            .unwrap()
            .contains("Hello PDF attachment")
    );
    let invalid_image = json!({"name":"fake.png","mime_type":"image/png","data_base64":STANDARD.encode(b"not-a-png")});
    assert_eq!(
        request(
            &state,
            "POST",
            "/api/playground/attachment",
            invalid_image,
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let oversized = json!({"name":"large.txt","mime_type":"text/plain","data_base64":STANDARD.encode(vec![b'a'; 100_001])});
    assert_eq!(
        request(
            &state,
            "POST",
            "/api/playground/attachment",
            oversized,
            true
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        request(
            &state,
            "POST",
            "/api/playground/search",
            json!({"query":"test"}),
            false
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &state,
            "POST",
            "/api/playground/search",
            json!({"query":""}),
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn attached_text_and_search_results_are_forwarded_as_reference_data() {
    let (_dir, state) = fixture();
    let router = Router::new().route(
        "/v1/chat/completions",
        post(|Json(body): Json<Value>| async move {
            Json(json!({"choices":[{"message":{"role":"assistant","content":"Done"}}],"echo":body}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let sealed = state.store.seal("upstream-secret").unwrap();
    state
        .store
        .update(|config| {
            config.accounts.push(Account {
                id: "attachment-account".into(),
                name: "Attachment".into(),
                endpoint,
                api_key: sealed,
                models: vec!["demo".into()],
                group_ids: vec!["playground-group".into()],
                protocols: vec!["chat".into()],
                enabled: true,
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    let (status, result) = request(&state, "POST", "/api/playground", json!({
        "key_id":"playground-key", "model":"demo", "stream":false,
        "messages":[{"role":"user","content":"Summarize this","attachments":[{"kind":"text","name":"notes.txt","text":"Important memo"}]}],
        "search_results":[{"title":"Source","url":"https://example.com/article","snippet":"A useful fact"}]
    }), true).await;
    task.abort();
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["choices"][0]["message"]["content"], "Done");
    assert!(
        result["echo"]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("untrusted reference data")
    );
    assert!(
        result["echo"]["messages"][1]["content"][1]["text"]
            .as_str()
            .unwrap()
            .contains("Important memo")
    );
    assert!(result["echo"]["messages"][1].get("attachments").is_none());
    assert!(result["echo"].get("search_results").is_none());
}
