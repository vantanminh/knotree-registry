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

The production image builds the React dashboard and serves it from the same Axum process, so the Cloudflare Tunnel should target `http://127.0.0.1:8080`. Copy `deploy/.env.example` to a secret-managed environment file, replace every placeholder, and start the stack:

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
