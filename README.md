# ZakoAI Proxy

[中文文档](README.zh-CN.md) · [Security](SECURITY.md) · [API contract](API_CONTRACT.md)

A self-hosted Rust Responses API relay with a responsive desktop/mobile administration console, SQLite persistence, and Docker Compose deployment. The console includes a dashboard, groups, upstream accounts, client API keys, and system settings.

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

## Configure The Relay

1. **Groups:** create a group and independently select strategies for `gpt-5.6-sol`, `gpt-6.1-sol`, and `gpt-6-astra`. Each selected model uses its own imported instruction profile; unselected models are forwarded unchanged. Multiple selections never concatenate different models' profiles. Existing unmodified preset groups are recognized automatically; custom exact-model instructions remain available under custom configuration. Referenced groups must be detached from accounts and keys before deletion.
2. **Accounts:** enter an upstream endpoint and API key, discover/select models, enable `responses` and/or `compact`, and assign groups. Endpoint roots with or without `/v1` work. Set RPM and concurrency to `0` for unlimited. A blank secret when editing retains the existing secret.
3. **API keys:** create a named client key for a group. Copy it immediately or reveal/copy it later in the authenticated console. Its group can be changed without rotating the key. Disabling/deleting the key blocks future requests. The displayed API base URL uses the current site origin plus `/v1`.
4. **System settings:** set the console entry path, retry budget, failure threshold, cooldown, request timeout, and statistics retention. Changing a password requires the current password and matching new-password confirmation; it revokes all sessions. Confirmation is not TOTP or another independent authentication factor.

Eligible upstream accounts must match the client's group, model, and protocol; accounts with no selected models do not receive requests. **Lower priority numbers are attempted first**; equal priorities use a stable account-ID order. Each client request uses at most `max_retries + 1` distinct upstream accounts. Transport failures, HTTP 401, 403, 408, 429, and HTTP 5xx permit failover before a response starts. An account enters cooldown once its consecutive retryable failure count reaches `failure_threshold`; after `cooldown_seconds` it becomes eligible again. Other 4xx responses are returned without failover. Active streaming responses are not replayed if they fail midway. RPM windows, concurrency counters, and cooldown state are process-local and reset on restart; run one instance per data volume.

The security entry moves the console from `/` to `/<entry>` while leaving `/health` and `/v1/*` unchanged. It supplements the login, not an independent authentication factor. Save the new URL before leaving the page.

## Client API

Supported endpoints are `GET /v1/models`, `POST /v1/responses`, and `POST /v1/responses/compact`. Authentication uses a generated proxy key, not your administrator password or upstream key. This project does not expose a Chat Completions compatibility route.

```sh
export ZAKO_API_KEY='YOUR_GENERATED_PROXY_KEY'
curl http://localhost:1990/v1/responses \
  -H "Authorization: Bearer $ZAKO_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"YOUR_UPSTREAM_MODEL_ID","input":"Hello"}'

curl -N http://localhost:1990/v1/responses \
  -H "Authorization: Bearer $ZAKO_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"YOUR_UPSTREAM_MODEL_ID","input":"Hello","stream":true}'
```

For compaction, send a valid upstream compact request to `/v1/responses/compact`; support depends on the account's upstream provider. Responses JSON fields pass through, except that a selected group profile prepends its configured text to the request's `instructions` field. Existing instructions and other request fields are preserved. SSE events pass through while usage and completion status are observed; credential reflection is redacted. Provider behavior and model refusals remain controlled by the upstream service; a profile is not a guarantee of changed behavior.

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

## Upstream Attribution And License

Frontend dependency notices, including Inter's SIL Open Font License, are collected from the locked packages during each build. The resulting `assets/third-party-licenses.txt` is served by the app and included in the container at `/app/licenses/frontend-notices.txt`. Third-party components retain their own license terms.

This project is released under the [MIT License](LICENSE). The bundled instruction profile text is adapted from **[MDX-Tom/gpt-instruct](https://github.com/MDX-Tom/gpt-instruct)**, supplied as `gpt-instruct-main.zip`, also MIT licensed. Its original copyright/license notice is retained in [third_party/gpt-instruct.LICENSE](third_party/gpt-instruct.LICENSE). Profiles are data imported by `scripts/import-profiles.ps1`, not upstream application source or an upstream model implementation. Retain both MIT notices when redistributing the project or container.
