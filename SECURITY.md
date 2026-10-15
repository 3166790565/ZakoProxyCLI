# Security

## Deployment

- Complete first-run administrator setup on a private interface. There is no default password. Choose a unique password of at least 12 characters.
- Serve public traffic through HTTPS. Compose binds `127.0.0.1:1990` by default; do not expose that port alongside a reverse proxy. Set `TRUST_PROXY=1` only when a trusted proxy overwrites `X-Forwarded-Proto` and preserves the original `Host`.
- The console security path adds entry obscurity, not a second authentication factor. A password change requires the old password and matching new-password confirmation; it is not MFA and revokes existing sessions.
- The container runs as UID/GID `10001:10001`. Its root filesystem is read-only in Compose, with only `/data` and a bounded temporary filesystem writable. Keep the Docker socket and host credentials outside the container.
- Administrators can configure arbitrary HTTP(S) upstream endpoints, including private-network services. Treat console access as privileged network access and enforce egress controls in your deployment if required. Redirects are not followed.
- Use one application instance per data volume. Runtime scheduler state and login throttling are local to a process. A distributed deployment requires a different shared-state design.

## Credentials And Data

Administrator passwords use Argon2 hashes. Session cookies are HttpOnly and SameSite=Strict, expire after 24 hours, and gain `Secure` when trusted forwarded HTTPS is enabled. Login requests are throttled by a process-local global budget. Session tokens are stored as SHA-256 digests.

Upstream API keys and revealable client API keys are encrypted using AES-256-GCM with the 32-byte `/data/master.key`. Client API-key authentication checks a separate SHA-256 digest. Encryption protects secret fields in the database; access to both the database and master key permits recovery. Other configuration, request metadata, and SQLite files are not encrypted as a whole.

Back up the complete `/data` directory, including `master.key` and SQLite sidecar files, while stopped. Protect backups as credentials. Losing the matching master key prevents recovery of encrypted keys. Keep it out of Git, logs, screenshots, and support reports.

Request history records identifiers, model, protocol, status, latency, token usage, attempts, and a Cyber-marker boolean. Prompt and response bodies are not persisted in request logs. Retention defaults to 90 days and is applied when new requests are logged. Application/infrastructure debug logging and reverse-proxy access logs require separate review. Do not log Authorization headers.

Instruction profiles alter only configured group/model request instructions. They do not change the upstream provider's access controls or guarantee particular model behavior. Clients and upstream providers remain responsible for their own request handling.

## Reporting

Use the repository's private GitHub vulnerability reporting feature when available. Do not post credentials, database files, master keys, prompts, or full upstream responses in public issues. Provide the affected commit, deployment method, redacted reproduction, and observed impact. Revoke exposed keys, rotate affected credentials, and revoke sessions after a suspected compromise.

CI checks cover defined test cases and container startup. Passing CI is not a guarantee that no vulnerabilities or bugs exist.
