$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

npm --prefix web run typecheck
npm --prefix web run build

npm --prefix edge/blob-worker run generate-types
npm --prefix edge/blob-worker run typecheck
npm --prefix edge/blob-worker test

Push-Location edge/blob-worker
try {
    npx wrangler deploy --dry-run
} finally {
    Pop-Location
}
