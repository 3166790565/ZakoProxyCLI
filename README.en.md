# ZakoAI Proxy

[简体中文](README.md) · English · [Security](SECURITY.md) · [API contract](API_CONTRACT.md)

A self-hosted Rust Responses / Chat Completions / Anthropic Messages API relay with a responsive desktop/mobile administration console, SQLite persistence, and Docker Compose deployment. The console includes a dashboard, groups, upstream accounts, client API keys, and system settings.

## Relay Recommendation: 杂鱼 API

[Visit 杂鱼 API](https://zayuapi.com)

![杂鱼 API](https://www.mizaawa.com/usr/uploads/2026/10/2898908578.png)

[Deployment tutorial (Chinese)](https://www.mizaawa.com/index.php/archives/95/)

## Deploy With Docker Compose

Requires Docker Engine/Desktop with Docker Compose v2. Build from the repository:

```sh
git clone https://github.com/mizaawa/zakoai-proxy-cli.git
cd zakoai-proxy-cli
docker compose pull
docker compose build --pull
docker compose up -d --no-build
docker compose ps
```

Open **http://localhost:1990**. On a fresh data volume, create your administrator username and a password of at least 12 characters. There are no built-in administrator credentials. Subsequent visits use the same account. Initialize the account before making the service reachable by other people.

The pull command downloads the published image; `build --pull` refreshes its Node/Rust/Debian base images and rebuilds the application from this checkout. `up -d` starts it. The Compose image name is `ghcr.io/mizaawa/zakoai-proxy-cli:latest`; successful default-branch CI runs also publish this image and a commit-SHA tag.

Compose binds port **1990 to loopback by default**. Set `ZAKO_BIND_IP=0.0.0.0` only when you intend to expose the port. `ZAKO_PORT` changes the host port. Put these values in an uncommitted `.env`, for example:

```dotenv
ZAKO_BIND_IP=127.0.0.1
ZAKO_PORT=1990
TRUST_PROXY=0
```

For public deployments, terminate HTTPS at a trusted reverse proxy and keep the application port private. Set `TRUST_PROXY=1` only behind that proxy; preserve the original `Host` and send `X-Forwarded-Proto: https` so sessions receive `Secure` cookies. Forward streaming responses without buffering and allow a read timeout longer than the configured request timeout.

```sh
docker compose logs --tail=100 -f
curl --fail http://localhost:1990/health
docker compose down
```

`down` preserves the named data volume. `down -v` deletes the administrator account, configuration, keys, and statistics.

### Update From The Console

From v0.3.0, **System Settings → GitHub Source Updates** checks the latest stable GitHub Release. Starting an update requires the current administrator password again. The updater checks out the official release tag, builds a new image from source, and reports build, deployment, and health-check logs. The old version keeps serving during the build; switching containers briefly interrupts connections, after which the page can reconnect to show the result.

Compose also runs an isolated `updater` service with no published host port. The public app uses the internal `UPDATER_URL=http://updater:9000/` and a read-only `/control/token` mount. Only the updater receives the Docker socket. Docker control grants effectively root-level access to the host, so deploy it only on a trusted server and never expose its internal interface or token.

After a successful build, the updater stops the proxy, backs up its complete data directory, and starts the new image. Failed deployment or health checks restore the previous version and its matching data backup. Update state and backups persist in `update-state`; they do not replace an off-host backup. Accounts, API keys, groups, statistics, and `master.key` remain in the data volume. Only stable tags from the fixed official repository are accepted, not arbitrary URLs or commands.

Existing installations must first pull this source version and run the Compose build/start commands above once to install the updater. Keep `COMPOSE_PROJECT_NAME` consistent when using a custom project name. Without an updater, such as in local development, the console shows version information but cannot execute updates. Updating the proxy image does not replace the updater itself or change the host's Compose file; deployment changes called out in release notes still require administrator action.

## Configure The Relay

1. **Groups:** create a group and use `profile_ids` to independently select model strategies. The existing three GPT profiles are retained, with 28 additional model-specific Claude, Grok, Gemini, DeepSeek, and GLM profiles, for 31 total, listed below. Apart from its name, a group configures only profile selections, not a custom model list or instructions. Profiles match the exact actual model after account mapping: each selected model uses only its own profile, while an unselected model receives no injected strategy. Multiple selections never concatenate different profiles. Existing unmodified preset groups are recognized automatically. Referenced groups must be detached from accounts and keys before deletion.
2. **Accounts:** enter an upstream endpoint and API key, keep or adjust the default `responses`, `compact`, `chat`, and `anthropic` selections according to your relay provider, discover/select actual upstream models, and assign groups. `chat` uses `/v1/chat/completions`; `anthropic` uses `/v1/messages` and Anthropic authentication for model discovery. Model names do not impose a protocol: Grok, Gemini, DeepSeek, or GLM can use the OpenAI-compatible or Anthropic-compatible interface exposed by your relay provider. Optional `model_mappings` maps requested model IDs to actual upstream model IDs; every target must be selected in the account's available models. Resolution is single-hop, never recursive; a request without a mapping uses the same model ID. Endpoint roots with or without `/v1` work. Set RPM and concurrency to `0` for unlimited. A blank secret when editing retains the existing secret.
3. **API keys:** create a named client key for a group. Copy it immediately or reveal/copy it later in the authenticated console. Its group can be changed without rotating the key. Disabling/deleting the key blocks future requests. The displayed API base URL uses the current site origin plus `/v1`.
4. **System settings:** set the console entry path, retry budget, failure threshold, cooldown, request timeout, and statistics retention. Changing a password requires the current password and matching new-password confirmation; it revokes all sessions. Confirmation is not TOTP or another independent authentication factor.

For example, the account configuration fragment `{"models":["gpt-6-astra"],"model_mappings":{"chat-main":"gpt-6-astra"}}` accepts a client request for `chat-main` and sends `model: "gpt-6-astra"` upstream. Selecting the group's `gpt-6-astra` strategy applies that actual model's profile, not a profile for the alias. An unmapped request for `gpt-6-astra` still uses that same actual model. These mapping and profile matching rules apply to every supported protocol.

### Bundled Profile Models

| Family | Exact actual model IDs |
| --- | --- |
| GPT (retained) | `gpt-5.6-sol`, `gpt-6.1-sol`, `gpt-6-astra` |
| Claude | `claude-fable-5`, `claude-fable-5-1`, `claude-haiku-4-5`, `claude-haiku-5-5`, `claude-opus-4-5`, `claude-opus-4-6`, `claude-opus-4-7`, `claude-opus-4-8`, `claude-opus-5`, `claude-opus-5-5`, `claude-sonnet-4-5`, `claude-sonnet-4-6`, `claude-sonnet-5`, `claude-sonnet-5-5` |
| Grok | `grok-4.7` |
| Gemini | `gemini-3-flash`, `gemini-3.1-flash-image`, `gemini-3.1-pro`, `gemini-3.1-pro-preview`, `gemini-3.6-flash`, `gemini-3.7-flash`, `gemini-3.8-flash` |
| DeepSeek | `deepseek-v4-flash`, `deepseek-v4-flash-0731`, `deepseek-v4-pro`, `deepseek-v4-pro-0813`, `deepseek-v4.1-flash` |
| GLM | `glm-5.3` |

Profiles are selected per model, without family wildcards. This catalog lists profile choices, not guaranteed upstream model availability; use the models and protocols actually provided by your relay. New profiles adapt coldcoffee's client seat instructions into inline text, including the DeepSeek Skill text. The proxy does not install local Skills, tools, or client configuration, or fabricate tool results. Text is sent with the request; model behavior must be checked against your actual relay provider.

Eligible upstream accounts must match the client's group and protocol, and resolve the requested model to a selected actual model; accounts with no selected models do not receive requests. **Lower priority numbers are attempted first**; equal priorities use a stable account-ID order. Each client request uses at most `max_retries + 1` distinct upstream accounts. Every attempt starts from the original client request, applies that account's single-hop mapping, and then selects the group profile for its actual model. Failover resolves each account independently and never carries over or stacks a previous attempt's strategy. Transport failures, HTTP 401, 403, 408, 429, and HTTP 5xx permit failover before a response starts. An account enters cooldown once its consecutive retryable failure count reaches `failure_threshold`; after `cooldown_seconds` it becomes eligible again. Other 4xx responses are returned without failover. Active streaming responses are not replayed if they fail midway. RPM windows, concurrency counters, and cooldown state are process-local and reset on restart; run one instance per data volume.

The security entry moves the console from `/` to `/<entry>` while leaving `/health` and `/v1/*` unchanged. It supplements the login, not an independent authentication factor. Save the new URL before leaving the page.

## Client API

Supported endpoints are `GET /v1/models`, `POST /v1/responses`, `POST /v1/responses/compact`, `POST /v1/chat/completions`, and `POST /v1/messages`. `/v1/models` includes selected actual models and valid requested-model aliases from accounts in the key's group. Authentication uses a generated proxy key via `Authorization: Bearer` or `x-api-key`. Client and upstream account use the same protocol; the proxy does not convert API protocols or add provider-native interfaces such as Google's.

For Chat Completions clients, use this site's origin plus `/v1` as the base URL and enable the account's `chat` protocol. `/v1/chat/completions` preserves existing messages, tool fields, and SSE. A matching selected profile is prepended as a separate `system` message, preserving original client messages and other request fields. JSON and SSE responses retain the upstream Chat Completions format.

For Anthropic clients, use this site's origin as the base URL and select models on accounts with `anthropic` enabled. `/v1/messages` preserves native messages, tool calls, and SSE, forwards `anthropic-version` (default `2023-06-01`) and `anthropic-beta`, and prepends a matching selected profile to `system` while preserving strings or text block arrays. Mapping, failover, limits, and logging use the same routing rules. Total input usage includes Anthropic cache reads and writes; cached usage counts reads.

```sh
export ZAKO_API_KEY='YOUR_GENERATED_PROXY_KEY'
curl http://localhost:1990/v1/responses \
  -H "Authorization: Bearer $ZAKO_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"YOUR_REQUEST_MODEL_ID","input":"Hello"}'

curl -N http://localhost:1990/v1/responses \
  -H "Authorization: Bearer $ZAKO_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"YOUR_REQUEST_MODEL_ID","input":"Hello","stream":true}'
```

`YOUR_REQUEST_MODEL_ID` may be a selected actual model ID or a requested-model alias configured on an account. For compaction, send a valid upstream compact request to `/v1/responses/compact`; support depends on the account's upstream provider. Responses and Compact first replace the request's `model` with the actual model resolved for the current account. If the group selects a profile for that exact actual model, its text is prepended to `instructions`; otherwise no strategy is injected. Existing client instructions and other request fields are preserved. SSE events pass through while usage and completion status are observed; credential reflection is redacted. Provider behavior and model refusals remain controlled by the upstream service; a profile is not a guarantee of changed behavior.

## Statistics And Storage

- Dashboard: input/output/cached/uncached tokens, cache hit rate, RPM, TPM, request count, success rate, average duration, daily charts, date-range history, and Cyber marker count.
- Cached tokens are a subset of input tokens; total tokens are input plus output, without double counting cache. Token usage depends on upstream usage fields, including a completed SSE event. Missing usage remains zero; it is not estimated.
- RPM/TPM cover the most recent 60 seconds of completed/logged requests. Daily buckets use UTC. Totals and per-key usage cover retained request metadata, not an immutable lifetime billing ledger. Default retention is 90 days; expired rows are pruned as new requests are logged.
- Cyber count is the number of responses containing the case-insensitive `cyber` marker, at most once per response. It is a text heuristic, not proof of a specific popup, refusal, or model behavior.
- `/data/zako.db` stores configuration, hashed sessions, and request metadata. Upstream/client key values are encrypted using AES-256-GCM with `/data/master.key`; administrator passwords are Argon2 hashes. Prompts and response text are not persisted in request logs. SQLite files and other metadata are not whole-disk encrypted.

### Backup And Restore

Back up the **entire `/data` directory, including `master.key`**, to restricted storage. A consistent portable backup can be made while stopped:

```sh
docker compose stop
docker compose cp proxy:/data ./zako-backup
docker compose start
```

Restore the contents of that directory to the volume's `/data` root while the service is stopped, retaining ownership `10001:10001`. Restore `zako.db`, any SQLite sidecar files, and `master.key` from the same backup. A database without its matching master key cannot decrypt stored API keys. Verify `/health`, administrator login, and a small upstream request after restoration. Do not commit backups to Git.

## Local Development And CI

Requires Rust **1.99**, Node.js **22**, and npm. The SQLite library is bundled in the Rust build.

```sh
cd web
npm ci
npm run build
cd ..
cargo run --locked
```

The server defaults to `0.0.0.0:1990`, `DATA_DIR=data`, and `WEB_DIR=web/dist`. Use `BIND_ADDR=127.0.0.1:1990` for a loopback-only local server. `RUST_LOG` controls tracing verbosity.

GitHub Actions contains four checks: **Rust Quality** (`cargo fmt`, strict Clippy), **Backend Tests** (unit/integration tests with mock upstreams), **Frontend** (clean install and TypeScript/production build), and **Docker** (Compose validation, image build, nonroot HTTP smoke test, and persisted setup after restart). Docker runs after the other three checks, then publishes the tested image on default-branch pushes. CI uses generated ephemeral credentials, not a live provider key. Passing checks establishes covered behavior, not an absence of all possible bugs.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

## License And Third-Party Notices

Frontend dependency notices, including Inter's SIL Open Font License, are collected from the locked packages during each build. The resulting `assets/third-party-licenses.txt` is served by the app and included in the container at `/app/licenses/frontend-notices.txt`. Third-party components retain their own license terms.

This project is released under the [MIT License](LICENSE). The bundled instruction profiles include text adapted from [MDX-Tom/gpt-instruct](https://github.com/MDX-Tom/gpt-instruct), also MIT licensed. Its required copyright and license notice is retained in [third_party/gpt-instruct.LICENSE](third_party/gpt-instruct.LICENSE). Profiles are imported configuration data, not upstream application source or a model implementation. Retain both MIT notices when redistributing source or containers.

New Claude, Grok, Gemini, DeepSeek, and GLM profiles reference and adapt the coldcoffee project [3641397194-wq/gpt6-Astra](https://github.com/3641397194-wq/gpt6-Astra) at commit [`09c7799a1fc2ef5f6f6cf4223b7a48abb39d2732`](https://github.com/3641397194-wq/gpt6-Astra/tree/09c7799a1fc2ef5f6f6cf4223b7a48abb39d2732). Thanks to its author and contributors.

- **Copyright and license:** Copyright (c) 2026 3641397194-wq, MIT License. The original notice is retained in [third_party/coldcoffee.LICENSE](third_party/coldcoffee.LICENSE) and shipped at `/app/licenses/coldcoffee.LICENSE` in the container.
- **Adaptation:** Claude, Grok, Gemini, DeepSeek, and GLM client seat instructions and Skill text are adapted into per-model inline API request profiles. The upstream desktop installer, private relay workflows, and local Skill execution environment are not included.
- **Scope:** attribution does not imply upstream endorsement or guarantee refusal bypass, tool availability, or model capabilities. Preserve this project's license and these third-party notices when redistributing source or containers.

Coldcoffee profiles retain the upstream seat text, workflow, and full routing skills, adding approximately 64–66 KB of UTF-8 instructions per matched request. This consumes context and input tokens. [profiles/coldcoffee/SOURCE.json](profiles/coldcoffee/SOURCE.json) records provenance, hashes, and adaptations. Reproduce the import with `python scripts/import-coldcoffee.py UPSTREAM_CHECKOUT` against the pinned clean checkout; the importer reads data without executing upstream installers or Skills.
