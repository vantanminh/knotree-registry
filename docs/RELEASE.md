# Release gates

The repository has a repeatable local quality gate in `scripts/quality.ps1`. It runs Rust formatting, warnings-as-errors clippy, the full workspace tests, and both frontend builds. The edge Worker has its own typecheck, unit tests, generated binding types, and Wrangler dry-run.

Before calling a deployment production-ready, run the two environment-dependent gates:

1. `scripts/oci-conformance.ps1` against a disposable HTTPS registry and the current OCI Distribution Spec conformance suite.
2. `scripts/docker-smoke.ps1` with a pull/push credential against a disposable repository. Cover login, multi-layer push/pull, tag mutation, digest pull, pull-only denial, revocation, and cross-repository isolation.

The official conformance tool can be run from its `conformance` directory with Go 1.24+ or through its Docker image. Results belong in an ignored output directory and must be reviewed, not merely generated. R2 integration tests require disposable credentials and a test prefix; no live R2 or Docker gate is claimed by the local unit-test suite.

Do not publish an OCI conformance badge until the full supported-feature report passes. If a feature is intentionally unsupported, document the exact API setting and client behavior in the release notes.
