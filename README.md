# Knotree Registry

Knotree Registry is a private OCI/Docker registry under active implementation. The backend is a Rust workspace with a protocol-independent core, a storage boundary, PostgreSQL migrations and an Axum service. Cloudflare R2 is the target production object store; the foundation slice also provides deterministic memory and local-file stores for development and tests.

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

## Architecture

`registry-core` owns digest, repository and authorization-scope types without depending on HTTP or storage. `registry-storage` owns the object-store contract and safe local test implementations. `registry-db` owns PostgreSQL connectivity and migrations. `registry-server` is the Axum interface and orchestration layer. OCI `/v2` handlers and the `/api/v1` control plane will remain separate as later stories land.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/decisions/DEC-001.md](docs/decisions/DEC-001.md), and the Harness story files for the implementation trace.

Authentication details and the Docker token exchange are documented in [docs/product/AUTHENTICATION.md](docs/product/AUTHENTICATION.md). The OCI data-plane contract and upload semantics are documented in [docs/product/OCI.md](docs/product/OCI.md).
The dashboard/API split and local frontend workflow are documented in [docs/product/CONTROL_PLANE.md](docs/product/CONTROL_PLANE.md).
