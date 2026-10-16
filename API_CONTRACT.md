# Console API contract

All administration endpoints use `/api/`. Session cookie is `zako_session`, HttpOnly, SameSite=Strict. All requests include `X-Console-Path` equal to `location.pathname` (the security entry). Errors are `{ "error": "message" }`. JSON POST/PUT bodies use snake_case.

- GET `/api/bootstrap`: `{initialized:boolean, authenticated:boolean, username:string}`
- POST `/api/setup`, POST `/api/login`: `{username,password}` -> session cookie
- POST `/api/logout`
- GET `/api/groups`: array `{id,name,profile_ids:string[]}`
- POST `/api/groups`, PUT `/api/groups/:id`: `{name,profile_ids:string[]}`. Apart from its name, a group configures only bundled profile selections. `profile_ids` independently selects strategies for `gpt-5.6-sol`, `gpt-6.1-sol`, and `gpt-6-astra`; matching uses the exact actual model after account mapping. Only that model's selected profile is applied, never another model's profile. Unknown profile IDs are rejected and duplicates deduplicated. Explicit `profile_ids:[]` disables all strategy injection. Old stored unmodified presets without `profile_ids` are migrated automatically; other legacy custom configuration does not become active.
- DELETE `/api/groups/:id` (reject referenced groups)
- GET `/api/accounts`: array `{id,name,endpoint,models:string[],model_mappings:Record<string,string>,group_ids,rpm,concurrency,priority,enabled,protocols,has_api_key,runtime:{active,failures,cooldown_until}}`. `models` contains selected actual upstream model IDs. `model_mappings` maps requested model IDs to actual upstream model IDs. Secrets omitted. Lower priority number wins.
- POST `/api/accounts`, PUT `/api/accounts/:id`: `{name,endpoint,api_key,models:string[],model_mappings:Record<string,string>,group_ids,protocols,rpm,concurrency,priority,enabled}`. Blank `api_key` on update retains the previous secret. `model_mappings` defaults to `{}` when omitted; every target must appear in the account's selected `models`. Each requested model is resolved once, with no recursive or chained mapping; absent a mapping, the requested model ID is used unchanged and must itself be selected in `models`. Model IDs must contain 1-256 bytes without whitespace or control characters. At most 500 selected models and 500 mappings are accepted. Protocols: `["responses","compact"]`. `rpm`/`concurrency` 0 means unlimited.
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
- GET `/api/profiles`: `{id,name,models:string[],instruction:string,model_family:string}[]` bundled instruction profile choices. Profile `models` defines exact actual upstream model IDs to match after account mapping; `model_family` is display metadata, not a wildcard. These profile fields are not group configuration fields.
- GET `/health`: `{status:"ok"}`

## Configuration Examples

Create a group with two independently selected profiles. Obtain current profile IDs from `/api/profiles`:

```json
{
  "name": "Primary",
  "profile_ids": ["gpt-5.6-sol-v45", "gpt-6-astra-v2-rc1"]
}
```

Create an account with selected actual models and a requested-model alias. Replace `GROUP_ID` with the created group's ID:

```json
{
  "name": "Primary upstream",
  "endpoint": "https://upstream.example/v1",
  "api_key": "YOUR_UPSTREAM_API_KEY",
  "models": ["gpt-5.6-sol", "gpt-6-astra"],
  "model_mappings": {"chat-main": "gpt-6-astra"},
  "group_ids": ["GROUP_ID"],
  "protocols": ["responses", "compact"],
  "rpm": 0,
  "concurrency": 0,
  "priority": 0,
  "enabled": true
}
```

## Public Routing

GET `/v1/models`, POST `/v1/responses`, POST `/v1/responses/compact` use a generated proxy key as Bearer authentication. The model list includes selected actual models and valid requested-model aliases from enabled accounts in the key's group. The upstream endpoint may include `/v1` or omit it. Streaming passes raw SSE.

Both request protocols resolve the original client's `model` independently for each eligible account. The account must match the group and protocol, and its single-hop mapping result must be selected in `models`. Before sending upstream, the proxy sets `model` to this actual model, then prepends at most the group's selected profile for that exact actual model to the original client's `instructions`. If no selected profile matches, no strategy is injected. Existing client instructions and other request fields are preserved. Every failover attempt starts from the original client request; mappings and strategies from earlier accounts are never reused or stacked.

With the configuration above, `{"model":"chat-main","input":"Hello"}` sends `model: "gpt-6-astra"` with only the selected `gpt-6-astra` profile. An unmapped `gpt-5.6-sol` request keeps that model ID and uses only its own selected profile. An unmapped, unselected model makes this account ineligible. These rules are identical for `/v1/responses` and `/v1/responses/compact`.
