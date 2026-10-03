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

Sign-in uses Knotree Accounts only (`SSO_ENABLED=true`); administrators are the Knotree account ids in `SSO_ADMIN_SUBJECTS`. After signing in, create a narrowly scoped access token in the dashboard for `docker login` and CI. The compose profile binds the registry only to localhost, uses PostgreSQL for durable state, serves the frontend from the image, and runs as a non-root read-only container with a writable data volume.

The current runtime snapshot model is deliberately single-instance: do not run multiple registry containers against the same database until the normalized PostgreSQL repositories are enabled. Use R2 and the documented backup procedure before treating the machine as disposable.

## Architecture

`registry-core` owns digest, repository and authorization-scope types without depending on HTTP or storage. `registry-storage` owns the object-store contract and safe local test implementations. `registry-db` owns PostgreSQL connectivity and migrations. `registry-server` is the Axum interface and orchestration layer. OCI `/v2` handlers and the `/api/v1` control plane will remain separate as later stories land.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/decisions/DEC-001.md](docs/decisions/DEC-001.md), and the Harness story files for the implementation trace.

Authentication details and the Docker token exchange are documented in [docs/product/AUTHENTICATION.md](docs/product/AUTHENTICATION.md). The OCI data-plane contract and upload semantics are documented in [docs/product/OCI.md](docs/product/OCI.md).
The dashboard/API split and local frontend workflow are documented in [docs/product/CONTROL_PLANE.md](docs/product/CONTROL_PLANE.md).

## Knotree accounts and repository namespaces

Registry has no passwords of its own. The web UI signs in through Knotree
Accounts (`/api/v1/auth/sso/start`; `?intent=signup` opens account creation)
with browser-bound single-use state, S256 PKCE and live userinfo from the
pinned issuer. Production refuses to start without SSO. Each Knotree account
maps to a stable `kt-...` username and private repository prefix
`<username>/...`; token issuance and bearer checks enforce this namespace.
Administrators are the account ids listed in `SSO_ADMIN_SUBJECTS`, applied at
every sign-in. Passwords, emails and two-step verification are managed in the
Knotree account. `docker login` uses access tokens created after signing in.

### Knotree Cloud

Cloud uses the same Knotree account, so users never connect the two services.
Cloud calls the cluster-only internal API (`INTERNAL_BIND_ADDR`, Kubernetes
TokenReview, see [docs/OPERATIONS.md](docs/OPERATIONS.md)) for the signed-in
account: it lists that account's repositories and tags, resolves digests, and
obtains per-repository pull-only credentials that it renews itself.

**Deployments → Knotree Cloud** shows whether the managed `tag_updated` webhook to Cloud is configured, how many deliveries are queued, and the last 25 attempts with their HTTP status (the last 100 attempts across all webhooks are kept in the persisted runtime state and served at `GET /api/v1/webhooks/deliveries`). `GET /api/v1/integrations/cloud` returns the same status for admins and never includes the signing secret. The managed webhook endpoint is marked as such on the Webhooks page and cannot be disabled from the dashboard.

### Production settings through GitHub Actions

Production configuration is now managed exclusively by GitHub Actions. Follow [the CI configuration guide](deploy/ci/README.md) and its JSON examples; previous server bootstrap/secret-copy instructions are superseded. A missing required setting fails deploy preflight before production changes.
