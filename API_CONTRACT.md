# Console API contract

All administration endpoints use `/api/`. Session cookie is `zako_session`, HttpOnly, SameSite=Strict. All requests include `X-Console-Path` equal to `location.pathname` (the security entry). Errors are `{ "error": "message" }`. JSON POST/PUT bodies use snake_case.

- GET `/api/bootstrap`: `{initialized:boolean, authenticated:boolean, username:string}`
- POST `/api/setup`, POST `/api/login`: `{username,password}` -> session cookie
- POST `/api/logout`
- GET `/api/groups`: array `{id,name,profile_ids:string[],models:string[],instruction:string}`
- POST `/api/groups`, PUT `/api/groups/:id`: `{name,profile_ids,models,instruction}`. `profile_ids` independently selects bundled strategies for `gpt-5.6-sol`, `gpt-6.1-sol`, and `gpt-6-astra`; only the exact requested selected model receives its own profile, never another model's profile. Unknown profile IDs are rejected and duplicates deduplicated. Unselected bundled models remain unchanged, including when legacy custom fields are present. Legacy `models`/`instruction` remain an exact-model custom fallback for other model IDs; bundled template text cannot be used as this fallback. Old stored preset groups without `profile_ids` are migrated automatically. Explicit `profile_ids:[]` disables bundled strategies and is never migrated back to a selection.
- DELETE `/api/groups/:id` (reject referenced groups)
- GET `/api/accounts`: array `{id,name,endpoint,models,group_ids,rpm,concurrency,priority,enabled,protocols,has_api_key, runtime:{active,failures,cooldown_until}}`. Secrets omitted. Lower priority number wins.
- POST `/api/accounts`, PUT `/api/accounts/:id`: same fields plus `api_key`. Blank api_key on update retains previous secret. Protocols: `["responses","compact"]`. rpm/concurrency 0 unlimited.
- DELETE `/api/accounts/:id`
- POST `/api/models/discover`: `{endpoint,api_key,account_id?}` -> `{models:string[]}`. Account ID enables reusing stored secret.
- GET `/api/keys`: array `{id,name,group_id,prefix,enabled,created_at,usage:{input_tokens,output_tokens,cached_tokens}}`
- POST `/api/keys`: `{name,group_id}` -> `{id,key}`
- PUT `/api/keys/:id`: `{name,group_id,enabled}`
- DELETE `/api/keys/:id`
- POST `/api/keys/:id/reveal`: `{key:string}`
- GET `/api/settings`: `{security_path,max_retries,failure_threshold,cooldown_seconds,request_timeout_seconds,retention_days}`
- PUT `/api/settings`: same object. Security path `""` or e.g. `"console-secret"`. After success frontend navigates to new path.
- POST `/api/password`: `{current_password,new_password,confirm_password}` -> all sessions revoked, return to login
- GET `/api/updates`: updater configuration, current version, last checked release and persisted job. A configured but unreachable controller returns an `error` field so reconnect polling can continue.
- GET `/api/updates/latest`: check the fixed official repository's latest non-draft, non-prerelease `vX.Y.Z` release.
- POST `/api/updates`: strict `{tag,current_password}` starts an asynchronous source build of that latest release. Requires the current administrator password, with five attempts per minute. Wrong password returns 403 without invalidating the session; an active update returns 409. No user-supplied repository, URL, command, or arbitrary ref is accepted. Downgrades are rejected.
- Update response: `{current_version,configured,latest:{tag,version,url,published_at,notes}|null,update_available,job:{id,status,version,started_at,finished_at,message,logs:string[]}|null,error?}`. Times are Unix seconds. Statuses: `fetching`, `building`, `deploying`, `verifying`, `succeeded`, `failed`, `rolled_back`. A same-version source rebuild is supported. Poll `/api/updates` through a brief proxy restart; sessions and settings are persisted in the data volume.
- GET `/api/stats?from=unix_seconds&to=unix_seconds`: `{summary:{input_tokens,output_tokens,cached_tokens,uncached_tokens,total_tokens,cache_hit_rate,rpm,tpm,total_requests,success_rate,avg_latency_ms,cyber_count},daily:[{date,input_tokens,output_tokens,cached_tokens,total_requests,success_count,cyber_count}],recent:RequestLog[],accounts:{total,healthy,cooling},keys:number}`. Rates as percentages 0-100. Recent limited to 100. Dates UTC.
- GET `/api/profiles`: `{id,name,models:string[],instruction:string,model_family:string}[]` upstream instruction profile choices. `models` defines exact supported request model IDs; `model_family` is display metadata, not a wildcard.
- GET `/health`: `{status:"ok"}`

Public API: GET `/v1/models`, POST `/v1/responses`, POST `/v1/responses/compact` with Bearer generated proxy key. Upstream endpoint may include `/v1` or omit it. Streaming passes raw SSE.
