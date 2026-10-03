# Knotree Registry

Knotree Registry is a private OCI/Docker registry. The backend is a Rust workspace with a protocol-independent core, a storage boundary, PostgreSQL migrations and an Axum service. Runtime metadata, credentials, catalog indexes, upload sessions, events and webhook registrations are restored from PostgreSQL on restart. Cloudflare R2 is the recommended production object store; local storage remains available for a single-machine deployment.

## Development

```powershell
cargo fmt --all
cargo test --workspace
cargo run -p registry-server
```

In a second terminal, run the control-plane dashboard during development:

```powershell
cd web
npm install
npm run dev
```

The default development process listens on `127.0.0.1:8080`, uses an in-memory object store only when `STORAGE_BACKEND=memory` is set, and does not require PostgreSQL unless `REQUIRE_DATABASE=true`. Copy `.env.example` to `.env` for a configured deployment. Keep the R2 bucket private and provide credentials only through the runtime environment.

Smoke checks:

```powershell
curl http://127.0.0.1:8080/livez
curl http://127.0.0.1:8080/readyz
curl http://127.0.0.1:8080/v2/
```

## Production deployment

The production image builds the React dashboard and serves it from the same Axum process, so the Cloudflare edge should route `registry.knotree.com` to the k3s Nginx edge. Copy `deploy/.env.example` to a secret-managed environment file, replace every placeholder, and start the stack:

```powershell
docker compose --env-file deploy/.env -f deploy/docker-compose.yml up -d --build
docker compose --env-file deploy/.env -f deploy/docker-compose.yml ps
Invoke-RestMethod http://127.0.0.1:8080/readyz
```

The first startup creates the configured bootstrap administrator. Keep that password in the secret manager; after the first login, create a narrowly scoped PAT in the dashboard for Docker/CI and do not use the browser password with `docker login`. The compose profile binds the registry only to localhost, uses PostgreSQL for durable state, serves the frontend from the image, and runs as a non-root read-only container with a writable data volume.

The current runtime snapshot model is deliberately single-instance: do not run multiple registry containers against the same database until the normalized PostgreSQL repositories are enabled. Use R2 and the documented backup procedure before treating the machine as disposable.

## Architecture

`registry-core` owns digest, repository and authorization-scope types without depending on HTTP or storage. `registry-storage` owns the object-store contract and safe local test implementations. `registry-db` owns PostgreSQL connectivity and migrations. `registry-server` is the Axum interface and orchestration layer. OCI `/v2` handlers and the `/api/v1` control plane will remain separate as later stories land.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/decisions/DEC-001.md](docs/decisions/DEC-001.md), and the Harness story files for the implementation trace.

Authentication details and the Docker token exchange are documented in [docs/product/AUTHENTICATION.md](docs/product/AUTHENTICATION.md). The OCI data-plane contract and upload semantics are documented in [docs/product/OCI.md](docs/product/OCI.md).
The dashboard/API split and local frontend workflow are documented in [docs/product/CONTROL_PLANE.md](docs/product/CONTROL_PLANE.md).

## Central identity and repository namespaces

The authentication service supports federated subjects for the upcoming
Accounts sign-in integration. Each issuer/subject pair maps to a stable
`kt-...` username and private repository prefix `<username>/...`, persisted
with existing authentication state. New federated users are never instance
administrators and have no Registry password. Token issuance and bearer
checks enforce this namespace, including denial of instance/admin scopes.
The control plane filters repository inventory and audit events for these
users and keeps global webhook/upload/GC operations restricted to admins.
The existing bootstrap admin and its repositories remain under operator
control. Accounts login is available through `/api/v1/auth/sso/start` and its
server-side callback when `SSO_ENABLED=true`. The login screen checks
`/api/v1/auth/sso/config` before offering central sign-in. Set `SSO_ISSUER`,
`SSO_CLIENT_ID=knotree-registry` and `SSO_REDIRECT_URI` only after Accounts
and the exact callback are ready. The API uses browser-bound single-use state,
S256 PKCE and verified live userinfo from the pinned issuer, then persists a
local session. Tokens and authorization codes are not written to logs.
Pending login requests are memory-only and expire after ten minutes; restart
sign-in after a Registry process restart. Central SSO is disabled by default.

Cloud authorization and live end-to-end verification are still pending;
these code changes do not prove production SSO is active.

### Knotree Cloud auto deploy

**Deployments → Knotree Cloud** shows whether the managed `tag_updated` webhook to Cloud is configured, how many deliveries are queued, and the last 25 attempts with their HTTP status (the last 100 attempts across all webhooks are kept in the persisted runtime state and served at `GET /api/v1/webhooks/deliveries`). `GET /api/v1/integrations/cloud` returns the same status for admins and never includes the signing secret. To deploy on push, add a Knotree Registry App service in Knotree Cloud, choose **Connect Knotree Registry** (approve the Registry consent request, or paste an Access Token scoped to `repository:<name>:pull` only), then enable auto deploy under the service's **Settings → Auto updates**; the managed webhook endpoint is marked as such on the Webhooks page and cannot be disabled from the dashboard.

### Cloud pull authorization

With Accounts SSO enabled, Cloud can request explicit, repository-scoped pull consent. `POST /api/v1/cloud-grants/requests` accepts `client_id=knotree-cloud`, the exact callback `https://cloud.knotree.com/api/v1/auth/knotree-registry/callback`, a random `state` (32–128 URL-safe characters), `repository`, `code_challenge`, `code_challenge_method=S256`, and `expected_issuer`/`expected_subject` from the Cloud Accounts identity. The result contains `request_id`, `authorization_url` and `expires_in=600`. Only that federated identity with access to that repository may review and decide the request at `/cloud/authorize/{request_id}`. SSO preserves only this local UUID route as its return path.

The browser posts `{ "allow": true }` or `{ "allow": false }` to `/api/v1/cloud-grants/requests/{id}/decision` with its Registry session and same-origin Origin header. It receives a callback URL with state and a single-use code, or `error=access_denied`. Cloud exchanges the code using `POST /api/v1/cloud-grants/exchange` with `client_id`, `redirect_uri`, `code`, `code_verifier`. The server verifies S256 PKCE and the live approving session before minting a pull-only credential lasting 30 days. The no-store JSON response contains username, credential, credential_id, exact repository, Accounts issuer/subject, expiry and `actions=["pull"]`. Cloud must validate these fields and encrypt the credential; never place it in browser storage or callback URLs. Users revoke credentials in Access Tokens.

Pending requests and codes are bounded to 4096 entries, kept in memory and expire after ten/two minutes; restart invalidates pending consent. No credential exists until code exchange. The callback/client pair is fixed; custom Cloud domains require a reviewed server-side allowlist change. This local implementation still requires GitHub backend CI and live consent/pull/revocation testing before production enablement.

### Production settings through GitHub Actions

Production configuration is now managed exclusively by GitHub Actions. Follow [the CI configuration guide](deploy/ci/README.md) and its JSON examples; previous server bootstrap/secret-copy instructions are superseded. A missing required setting fails deploy preflight before production changes.
