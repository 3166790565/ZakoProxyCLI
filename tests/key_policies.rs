use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use zakoai_proxy::{
    Shared, app, new_app, profiles,
    store::{Account, Group, ProviderBinding},
};

fn fixture() -> (tempfile::TempDir, Shared) {
    let dir = tempfile::tempdir().unwrap();
    let state = new_app(dir.path(), dir.path().join("web")).unwrap();
    state.store.session("key-policy-session").unwrap();
    state
        .store
        .update(|config| {
            for id in ["first", "second", "alias-only"] {
                config.groups.push(Group {
                    id: id.into(),
                    name: id.into(),
                    ..Default::default()
                });
            }
            Ok(())
        })
        .unwrap();
    (dir, state)
}

async fn console(state: &Shared, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header("x-console-path", "/")
                .header("cookie", "zako_session=key-policy-session")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes))),
    )
}

#[tokio::test]
async fn group_description_defaults_persists_multiline_and_clears() {
    let (dir, state) = fixture();
    let (_, initial) = console(&state, "GET", "/api/groups", Value::Null).await;
    assert_eq!(initial[0]["description"], "");
    assert!(initial[0].get("profile_ids").is_none());
    assert!(initial[0].get("default_strategy_enabled").is_none());
    let description =
        "可用模型介绍\n第二行：支持换行与 emoji 🐾\nhttps://example.test/long-description";
    let (status, saved) = console(
        &state,
        "PUT",
        "/api/groups/first",
        json!({"name":"First group", "description":description}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["description"], description);
    drop(state);
    let state = new_app(dir.path(), dir.path().join("web")).unwrap();
    let (_, listed) = console(&state, "GET", "/api/groups", Value::Null).await;
    assert_eq!(listed[0]["description"], description);
    let (status, cleared) = console(
        &state,
        "PUT",
        "/api/groups/first",
        json!({"name":"First group", "description":""}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["description"], "");
    drop(state);
    let state = new_app(dir.path(), dir.path().join("web")).unwrap();
    assert!(
        state.store.config().unwrap().groups[0]
            .description
            .is_empty()
    );
    let (status, created) = console(
        &state,
        "POST",
        "/api/groups",
        json!({"name":"No introduction"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["description"], "");
}

#[tokio::test]
async fn malformed_descriptions_do_not_replace_existing_group() {
    let (_dir, state) = fixture();
    for invalid in [Value::Null, json!(42), json!(false), json!([]), json!({})] {
        let (status, rejected) = console(
            &state,
            "PUT",
            "/api/groups/first",
            json!({"name":"Invalid", "description":invalid}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{rejected}");
        let group = &state.store.config().unwrap().groups[0];
        assert_eq!(group.name, "first");
        assert!(group.description.is_empty());
    }
}

#[tokio::test]
async fn key_profile_support_uses_enabled_group_accounts_actual_models_and_prunes_on_switch() {
    let (_dir, state) = fixture();
    let first = &profiles::all()[0];
    let second = profiles::all()
        .iter()
        .find(|profile| {
            profile
                .models
                .iter()
                .all(|model| !first.models.contains(model))
        })
        .unwrap();
    state
        .store
        .update(|config| {
            config.accounts.extend([
                Account {
                    id: "first-account".into(),
                    name: "First".into(),
                    models: vec![first.models[0].clone()],
                    group_ids: vec!["first".into()],
                    model_mappings: [("public-alias".into(), first.models[0].clone())].into(),
                    enabled: true,
                    ..Default::default()
                },
                Account {
                    id: "disabled-account".into(),
                    name: "Disabled".into(),
                    models: second.models.clone(),
                    group_ids: vec!["first".into()],
                    enabled: false,
                    ..Default::default()
                },
                Account {
                    id: "second-account".into(),
                    name: "Second".into(),
                    models: second.models.clone(),
                    group_ids: vec!["second".into()],
                    enabled: true,
                    ..Default::default()
                },
                Account {
                    id: "alias-account".into(),
                    name: "Alias".into(),
                    models: vec!["plain".into()],
                    group_ids: vec!["alias-only".into()],
                    model_mappings: [(first.models[0].clone(), "plain".into())].into(),
                    enabled: true,
                    ..Default::default()
                },
            ]);
            Ok(())
        })
        .unwrap();
    for (group, profile) in [("first", &second.id), ("alias-only", &first.id)] {
        let (status, rejected) = console(
            &state,
            "POST",
            "/api/keys",
            json!({"name":"Unsupported", "group_id":group, "profile_ids":[profile]}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}");
    }
    assert!(state.store.config().unwrap().keys.is_empty());
    let (status, created) = console(&state, "POST", "/api/keys", json!({
        "name":"Scoped key", "group_id":"first", "profile_ids":[first.id], "default_strategy_enabled":true
    })).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let key_path = format!("/api/keys/{}", created["id"].as_str().unwrap());
    let (status, rejected) = console(
        &state,
        "PUT",
        &key_path,
        json!({
            "name":"Unsupported edit", "group_id":"first", "profile_ids":[second.id]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}");
    let key = &state.store.config().unwrap().keys[0];
    assert_eq!(key.name, "Scoped key");
    assert_eq!(key.profile_ids, vec![first.id.clone()]);
    assert!(key.default_strategy_enabled);
    let (status, switched) = console(
        &state,
        "PUT",
        &key_path,
        json!({"name":"Second group", "group_id":"second"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{switched}");
    let key = &state.store.config().unwrap().keys[0];
    assert_eq!(key.group_id, "second");
    assert!(key.profile_ids.is_empty());
    assert!(key.default_strategy_enabled);
    let (status, selected) = console(&state, "PUT", &key_path, json!({
        "name":"Second group", "group_id":"second", "profile_ids":[second.id], "default_strategy_enabled":false
    })).await;
    assert_eq!(status, StatusCode::OK, "{selected}");
    let (_, listed) = console(&state, "GET", "/api/keys", Value::Null).await;
    assert_eq!(listed[0]["profile_ids"], json!([second.id]));
    assert_eq!(listed[0]["default_strategy_enabled"], false);
}

async fn relay_models(state: &Shared, secret: &str) -> (StatusCode, Value) {
    let response = app(state.clone())
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("authorization", format!("Bearer {secret}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn unavailable_provider_bindings_exclude_models_and_prune_key_strategy_choices() {
    let (_dir, state) = fixture();
    let profile = &profiles::all()[0];
    let model = &profile.models[0];
    let encrypted = state.store.seal("provider-upstream-secret").unwrap();
    state
        .store
        .update(|config| {
            config.accounts.push(Account {
                id: "provider-account".into(),
                name: "Provider account".into(),
                endpoint: "https://provider.example.test".into(),
                api_key: encrypted.clone(),
                models: vec![model.clone()],
                group_ids: vec!["first".into()],
                provider_binding: Some(ProviderBinding {
                    provider_id: "provider".into(),
                    key_id: "upstream-key".into(),
                    status: "active".into(),
                    ..Default::default()
                }),
                enabled: true,
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    let (status, created) = console(
        &state,
        "POST",
        "/api/keys",
        json!({
            "name":"Provider strategy", "group_id":"first", "profile_ids":[profile.id]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let path = format!("/api/keys/{}", created["id"].as_str().unwrap());
    let secret = created["key"].as_str().unwrap();
    let (status, models) = relay_models(&state, secret).await;
    assert_eq!(status, StatusCode::OK, "{models}");
    assert_eq!(models["data"][0]["id"], *model);
    for (binding_status, has_secret, enabled) in [
        ("disabled", true, true),
        ("expired", true, true),
        ("quota_exhausted", true, true),
        ("missing", true, true),
        ("provider_missing", true, true),
        ("provider_changed", true, true),
        ("active", false, true),
        ("active", true, false),
    ] {
        // Re-establish a valid selection before each independent invalidation.
        state
            .store
            .update(|config| {
                let account = &mut config.accounts[0];
                account.provider_binding.as_mut().unwrap().status = "active".into();
                account.api_key = encrypted.clone();
                account.enabled = true;
                Ok(())
            })
            .unwrap();
        let (status, selected) = console(
            &state,
            "PUT",
            &path,
            json!({
                "name":"Provider strategy", "group_id":"first", "profile_ids":[profile.id]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{selected}");
        state
            .store
            .update(|config| {
                let account = &mut config.accounts[0];
                account.provider_binding.as_mut().unwrap().status = binding_status.into();
                account.api_key = if has_secret {
                    encrypted.clone()
                } else {
                    String::new()
                };
                account.enabled = enabled;
                Ok(())
            })
            .unwrap();
        let (status, models) = relay_models(&state, secret).await;
        assert_eq!(status, StatusCode::OK, "{models}");
        assert_eq!(
            models["data"],
            json!([]),
            "{binding_status}/{has_secret}/{enabled}"
        );
        for (method, target) in [("POST", "/api/keys"), ("PUT", path.as_str())] {
            let (status, rejected) = console(
                &state,
                method,
                target,
                json!({
                    "name":"Unsupported", "group_id":"first", "profile_ids":[profile.id]
                }),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "{binding_status}/{has_secret}/{enabled}: {rejected}"
            );
        }
        let config = state.store.config().unwrap();
        assert_eq!(config.keys.len(), 1);
        assert_eq!(config.keys[0].profile_ids, vec![profile.id.clone()]);
        let (status, pruned) = console(
            &state,
            "PUT",
            &path,
            json!({"name":"Pruned unavailable strategy", "group_id":"first"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{pruned}");
        let (_, keys) = console(&state, "GET", "/api/keys", Value::Null).await;
        assert_eq!(keys[0]["profile_ids"], json!([]));
    }
    // An unavailable provider must not hide a model supplied by another live account.
    state
        .store
        .update(|config| {
            config.accounts.push(Account {
                id: "manual-account".into(),
                name: "Manual fallback".into(),
                endpoint: "https://manual.example.test".into(),
                api_key: encrypted,
                models: vec![model.clone()],
                group_ids: vec!["first".into()],
                enabled: true,
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    let (status, selected) = console(
        &state,
        "PUT",
        &path,
        json!({
            "name":"Manual fallback strategy", "group_id":"first", "profile_ids":[profile.id]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{selected}");
    let (status, models) = relay_models(&state, secret).await;
    assert_eq!(status, StatusCode::OK, "{models}");
    assert_eq!(models["data"][0]["id"], *model);
}

#[tokio::test]
async fn legacy_strategy_migration_preserves_providers_bindings_and_ciphertexts_across_reopens() {
    let (dir, state) = fixture();
    let profile = &profiles::all()[0];
    let password = state.store.seal("provider-password").unwrap();
    let access_token = state.store.seal("provider-access-token").unwrap();
    let refresh_token = state.store.seal("provider-refresh-token").unwrap();
    let upstream_key = state.store.seal("provider-upstream-key").unwrap();
    let proxy_url = state
        .store
        .seal("socks5://user:password@127.0.0.1:9999")
        .unwrap();
    let relay_secret = state.store.seal("existing-relay-secret").unwrap();
    let expected_providers = json!([{
        "id":"retained-provider", "name":"Retained provider", "kind":"sub2api",
        "base_url":"https://provider.example.test", "auth_mode":"password",
        "username":"provider-user", "user_id":"42", "password":password,
        "access_token":access_token, "refresh_token":refresh_token, "token_type":"Bearer",
        "price_multiplier":1.5, "notes":"Keep provider notes unchanged",
        "balance":12.5, "today_usage":0.75, "total_usage":23.25, "total_recharged":40.0,
        "total_recharged_estimated":false, "currency":"USD", "status":"connected",
        "last_error":null, "last_synced_at":1700000000,
        "created_at":1690000000, "updated_at":1700000000
    }]);
    let expected_accounts = json!([Account {
        id: "bound-account".into(),
        name: "Bound account".into(),
        endpoint: "https://provider.example.test".into(),
        api_key: upstream_key.clone(),
        proxy_url: proxy_url.clone(),
        models: vec![profile.models[0].clone()],
        group_ids: vec!["legacy-group".into()],
        protocols: vec!["responses".into()],
        provider_binding: Some(ProviderBinding {
            provider_id: "retained-provider".into(),
            key_id: "remote-key-7".into(),
            key_name: "Imported provider key".into(),
            group_name: "vip".into(),
            group_multiplier: Some(1.5),
            status: "active".into(),
            last_synced_at: 1700000000,
        }),
        enabled: true,
        ..Default::default()
    }]);
    let mut legacy = serde_json::to_value(state.store.config().unwrap()).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("key_strategy_schema");
    legacy["providers"] = expected_providers.clone();
    legacy["accounts"] = expected_accounts.clone();
    legacy["groups"] = json!([{
        "id":"legacy-group", "name":"Legacy provider group", "description":"Imported group introduction",
        "profile_ids":[profile.id], "default_strategy_enabled":true
    }]);
    legacy["keys"] = json!([
        {
            "id":"inherited-key", "name":"Existing inherited key", "group_id":"legacy-group",
            "secret":relay_secret, "hash":zakoai_proxy::store::digest("existing-relay-secret")
        },
        {
            "id":"explicit-key", "name":"Explicitly disabled key", "group_id":"legacy-group",
            "profile_ids":[], "default_strategy_enabled":false
        }
    ]);
    let write_raw = |value: &Value| {
        let db = rusqlite::Connection::open(dir.path().join("zako.db")).unwrap();
        db.execute("UPDATE config SET value=?1 WHERE id=1", [value.to_string()])
            .unwrap();
    };
    let read_raw = || -> Value {
        let db = rusqlite::Connection::open(dir.path().join("zako.db")).unwrap();
        let raw: String = db
            .query_row("SELECT value FROM config WHERE id=1", [], |row| row.get(0))
            .unwrap();
        serde_json::from_str(&raw).unwrap()
    };
    write_raw(&legacy);
    drop(state);
    let mut expected_description = "Imported group introduction";
    for reopen in 0..3 {
        let state = new_app(dir.path(), dir.path().join("web")).unwrap();
        let current = state.store.config().unwrap();
        let current_json = serde_json::to_value(&current).unwrap();
        assert_eq!(
            current_json["providers"], expected_providers,
            "reopen {reopen}"
        );
        assert_eq!(
            current_json["accounts"], expected_accounts,
            "reopen {reopen}"
        );
        assert_eq!(current_json["key_strategy_schema"], 1);
        assert_eq!(current.groups[0].description, expected_description);
        assert_eq!(current.keys[0].secret, relay_secret);
        assert_eq!(
            state.store.unseal(&current.keys[0].secret).unwrap(),
            "existing-relay-secret"
        );
        assert_eq!(
            current.keys[0].profile_ids,
            if reopen == 0 {
                vec![profile.id.clone()]
            } else {
                Vec::new()
            }
        );
        assert_eq!(current.keys[0].default_strategy_enabled, reopen == 0);
        assert!(current.keys[1].profile_ids.is_empty());
        assert!(!current.keys[1].default_strategy_enabled);
        assert_eq!(
            state.store.unseal(&current.providers[0].password).unwrap(),
            "provider-password"
        );
        assert_eq!(
            state
                .store
                .unseal(&current.providers[0].access_token)
                .unwrap(),
            "provider-access-token"
        );
        assert_eq!(
            state
                .store
                .unseal(&current.providers[0].refresh_token)
                .unwrap(),
            "provider-refresh-token"
        );
        assert_eq!(
            state.store.unseal(&current.accounts[0].api_key).unwrap(),
            "provider-upstream-key"
        );
        let persisted = read_raw();
        assert_eq!(persisted["key_strategy_schema"], 1);
        assert_eq!(persisted["providers"], expected_providers);
        assert_eq!(persisted["accounts"], expected_accounts);
        assert!(persisted["groups"][0].get("profile_ids").is_none());
        assert!(
            persisted["groups"][0]
                .get("default_strategy_enabled")
                .is_none()
        );
        if reopen < 2 {
            expected_description = if reopen == 0 {
                "Edited group introduction\nSecond line"
            } else {
                ""
            };
            let (status, updated) = console(
                &state,
                "PUT",
                "/api/groups/legacy-group",
                json!({
                    "name":"Legacy provider group", "description":expected_description
                }),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{updated}");
            assert_eq!(updated["description"], expected_description);
            let after_description = serde_json::to_value(state.store.config().unwrap()).unwrap();
            assert_eq!(after_description["providers"], expected_providers);
            assert_eq!(after_description["accounts"], expected_accounts);
        }
        if reopen == 0 {
            let (status, cleared) = console(
                &state,
                "PUT",
                "/api/keys/inherited-key",
                json!({
                    "name":"Explicitly cleared migrated key", "group_id":"legacy-group",
                    "profile_ids":[], "default_strategy_enabled":false
                }),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{cleared}");
            drop(state);
            // A stale legacy field must not trigger another migration after schema 1.
            let mut stale = read_raw();
            stale["groups"][0]["profile_ids"] = json!([profile.id]);
            stale["groups"][0]["default_strategy_enabled"] = json!(true);
            write_raw(&stale);
        }
    }
}
