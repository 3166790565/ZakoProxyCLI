use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use zakoai_proxy::{Shared, app, new_app};

fn fixture() -> (tempfile::TempDir, Shared) {
    let dir = tempfile::tempdir().unwrap();
    let state = new_app(dir.path(), dir.path().join("web")).unwrap();
    (dir, state)
}

#[tokio::test]
async fn local_fonts_have_font_mime_type() {
    let (dir, s) = fixture();
    let assets = dir.path().join("web/assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("inter.woff2"), b"font-test").unwrap();
    let response = app(s)
        .oneshot(
            Request::builder()
                .uri("/assets/inter.woff2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "font/woff2");
}
async fn request(
    s: &Shared,
    method: &str,
    path: &str,
    body: Value,
    cookie: &str,
    entry: &str,
) -> (StatusCode, Value, String) {
    let response = app(s.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .header("x-console-path", entry)
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .to_owned();
    let bytes = to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(json!(null)),
        cookie,
    )
}
async fn setup(s: &Shared) -> String {
    let (status, _, cookie) = request(
        s,
        "POST",
        "/api/setup",
        json!({"username":"admin","password":"integration-password-123"}),
        "",
        "/",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!cookie.is_empty());
    cookie
}

#[tokio::test]
async fn first_run_crud_and_authentication() {
    let (_dir, s) = fixture();
    assert_eq!(
        request(&s, "GET", "/api/groups", Value::Null, "", "/")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let c = setup(&s).await;
    assert_eq!(
        request(
            &s,
            "POST",
            "/api/setup",
            json!({"username":"takeover","password":"takeover-password"}),
            "",
            "/"
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, g, _) = request(
        &s,
        "POST",
        "/api/groups",
        json!({"name":"Production","models":["mock-model"],"instruction":"Be concise."}),
        &c,
        "/",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let gid = g["id"].as_str().unwrap();
    let (status, key, _) = request(
        &s,
        "POST",
        "/api/keys",
        json!({"name":"Development","group_id":gid}),
        &c,
        "/",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let kid = key["id"].as_str().unwrap();
    let raw = key["key"].as_str().unwrap();
    assert!(raw.starts_with("sk-zako-"));
    let (_, revealed, _) = request(
        &s,
        "POST",
        &format!("/api/keys/{kid}/reveal"),
        json!({}),
        &c,
        "/",
    )
    .await;
    assert_eq!(revealed["key"], raw);
    let (_, keys, _) = request(&s, "GET", "/api/keys", Value::Null, &c, "/").await;
    assert!(keys[0].get("secret").is_none());
    assert!(keys[0].get("hash").is_none());
    assert_eq!(
        request(
            &s,
            "DELETE",
            &format!("/api/groups/{gid}"),
            Value::Null,
            &c,
            "/"
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let response = app(s.clone())
        .oneshot(
            Request::builder()
                .uri("/api/groups")
                .header("cookie", &c)
                .header("host", "localhost:1990")
                .header("origin", "https://attacker.invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(request(&s,"POST","/api/password",json!({"current_password":"wrong","new_password":"new-password-123","confirm_password":"new-password-123"}),&c,"/").await.0,StatusCode::BAD_REQUEST);
    assert_eq!(request(&s,"POST","/api/password",json!({"current_password":"integration-password-123","new_password":"new-password-123","confirm_password":"new-password-123"}),&c,"/").await.0,StatusCode::OK);
    assert_eq!(
        request(&s, "GET", "/api/keys", Value::Null, &c, "/")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &s,
            "POST",
            "/api/login",
            json!({"username":"admin","password":"new-password-123"}),
            "",
            "/"
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn security_entry_hides_console_and_administration() {
    let (_dir, s) = fixture();
    let c = setup(&s).await;
    let settings = json!({"security_path":"secret-entry","max_retries":2,"failure_threshold":2,"cooldown_seconds":60,"request_timeout_seconds":120,"retention_days":90});
    assert_eq!(
        request(&s, "PUT", "/api/settings", settings, &c, "/")
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&s, "GET", "/api/bootstrap", Value::Null, "", "/")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &s,
            "GET",
            "/api/bootstrap",
            Value::Null,
            "",
            "/secret-entry"
        )
        .await
        .0,
        StatusCode::OK
    );
    let r = app(s)
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

async fn upstream(failing: bool) -> (String, tokio::task::JoinHandle<()>) {
    let router=Router::new().route("/v1/models",get(||async{axum::Json(json!({"data":[{"id":"mock-model"}]}))}))
        .route("/v1/responses",post(move|headers:axum::http::HeaderMap,axum::Json(body):axum::Json<Value>|async move{
            assert_eq!(headers["authorization"],"Bearer upstream-secret");
            if failing{return (StatusCode::SERVICE_UNAVAILABLE,axum::Json(json!({"error":"temporary"}))).into_response();}
            assert_eq!(body["model"],"mock-model");
            if body["stream"]==true {
                let stream=futures_util::stream::iter(vec![Ok::<_,std::io::Error>(bytes::Bytes::from_static(b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_mock\",\"usage\":{\"input_tokens\":100,\"output_tokens\":20,\"input_tokens_details\":{\"cached_tokens\":80}}}}\n\n"))]);
                return ([("content-type","text/event-stream")],Body::from_stream(stream)).into_response();
            }
            (StatusCode::OK,axum::Json(json!({"id":"resp_mock","object":"response","instructions":body["instructions"],"usage":{"input_tokens":100,"output_tokens":20,"input_tokens_details":{"cached_tokens":80}}}))).into_response()
        }))
        .route("/v1/responses/compact",post(||async{axum::Json(json!({"object":"response.compaction","output":[],"usage":{"input_tokens":12,"output_tokens":3}}))}));
    use axum::response::IntoResponse;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (format!("http://{addr}"), handle)
}
async fn proxy_request(s: &Shared, path: &str, key: &str, body: Value) -> (StatusCode, Vec<u8>) {
    let response = app(s.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    (
        status,
        to_bytes(response.into_body(), 8 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
}

#[tokio::test]
async fn routing_failover_compact_streaming_and_statistics() {
    let (_dir, s) = fixture();
    let c = setup(&s).await;
    let (bad_url, bad_task) = upstream(true).await;
    let (good_url, good_task) = upstream(false).await;
    let (_, g, _) = request(
        &s,
        "POST",
        "/api/groups",
        json!({"name":"main","models":["mock-model"],"instruction":"Be concise."}),
        &c,
        "/",
    )
    .await;
    let gid = g["id"].as_str().unwrap();
    for (endpoint, priority) in [(bad_url, 0), (good_url, 1)] {
        let (status,account,_)=request(&s,"POST","/api/accounts",json!({"name":"upstream","endpoint":endpoint,"api_key":"upstream-secret","models":["mock-model"],"group_ids":[gid],"priority":priority,"rpm":0,"concurrency":0,"enabled":true,"protocols":["responses","compact"]}),&c,"/").await;
        assert_eq!(status, StatusCode::OK, "{account}");
        assert!(account.get("api_key").is_none());
    }
    let (_, key, _) = request(
        &s,
        "POST",
        "/api/keys",
        json!({"name":"local","group_id":gid}),
        &c,
        "/",
    )
    .await;
    let key = key["key"].as_str().unwrap();
    let (status, body) = proxy_request(
        &s,
        "/v1/responses",
        key,
        json!({"model":"mock-model","input":"hello","instructions":"Original."}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let output: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        output["instructions"]
            .as_str()
            .unwrap()
            .contains("Original.")
    );
    assert!(
        output["instructions"]
            .as_str()
            .unwrap()
            .contains("Be concise.")
    );
    assert_eq!(
        proxy_request(
            &s,
            "/v1/responses/compact",
            key,
            json!({"model":"mock-model","input":[]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, stream) = proxy_request(
        &s,
        "/v1/responses",
        key,
        json!({"model":"mock-model","input":"hello","stream":true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&stream).contains("response.completed"));
    assert_eq!(
        proxy_request(
            &s,
            "/v1/responses",
            "invalid",
            json!({"model":"mock-model","input":"hello"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (_, stats, _) = request(&s, "GET", "/api/stats", Value::Null, &c, "/").await;
    assert_eq!(stats["summary"]["total_requests"], 3);
    assert_eq!(stats["summary"]["input_tokens"], 212);
    assert_eq!(stats["summary"]["cached_tokens"], 160);
    assert!(
        stats["recent"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["attempts"].as_u64().unwrap_or(0) > 1)
    );
    bad_task.abort();
    good_task.abort();
}

#[tokio::test]
async fn concurrent_first_run_has_single_owner() {
    let (_dir, s) = fixture();
    let a = Arc::clone(&s);
    let b = Arc::clone(&s);
    let (one, two) = tokio::join!(
        request(
            &a,
            "POST",
            "/api/setup",
            json!({"username":"one","password":"password-owner-one"}),
            "",
            "/"
        ),
        request(
            &b,
            "POST",
            "/api/setup",
            json!({"username":"two","password":"password-owner-two"}),
            "",
            "/"
        )
    );
    assert_eq!(
        [one.0, two.0]
            .iter()
            .filter(|v| **v == StatusCode::OK)
            .count(),
        1
    );
}

#[tokio::test]
async fn simultaneous_password_changes_only_accept_one_current_session() {
    let (_dir, s) = fixture();
    let cookie = setup(&s).await;
    let first = json!({"current_password":"integration-password-123","new_password":"first-new-password","confirm_password":"first-new-password"});
    let second = json!({"current_password":"integration-password-123","new_password":"second-new-password","confirm_password":"second-new-password"});
    let (one, two) = tokio::join!(
        request(&s, "POST", "/api/password", first, &cookie, "/"),
        request(&s, "POST", "/api/password", second, &cookie, "/")
    );
    assert_eq!(
        [one.0, two.0]
            .iter()
            .filter(|s| **s == StatusCode::OK)
            .count(),
        1
    );
    assert_eq!(
        request(&s, "GET", "/api/keys", Value::Null, &cookie, "/")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn discovery_handles_models_and_rejects_oversized_bodies() {
    let (_dir, s) = fixture();
    let cookie = setup(&s).await;
    let router = Router::new()
        .route(
            "/normal/v1/models",
            get(|| async { axum::Json(json!({"data":[{"id":"b"},{"id":"a"},{"id":"a"}]})) }),
        )
        .route(
            "/fixed/v1/models",
            get(|| async { "x".repeat(1024 * 1024 + 1) }),
        )
        .route(
            "/chunked/v1/models",
            get(|| async {
                Body::from_stream(futures_util::stream::iter((0..17).map(|_| {
                    Ok::<_, std::io::Error>(bytes::Bytes::from(vec![b'x'; 64 * 1024]))
                })))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let (status, models, _) = request(
        &s,
        "POST",
        "/api/models/discover",
        json!({"endpoint":format!("{endpoint}/normal"),"api_key":"test-key"}),
        &cookie,
        "/",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models["models"], json!(["a", "b"]));
    for suffix in ["fixed", "chunked"] {
        let (status, body, _) = request(
            &s,
            "POST",
            "/api/models/discover",
            json!({"endpoint":format!("{endpoint}/{suffix}"),"api_key":"test-key"}),
            &cookie,
            "/",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "Upstream model response is too large");
    }
    task.abort();
}
