# Signed events and optional deployment agent

`registry-events` provides the shared event envelope, replay-bounded HMAC-SHA256 webhook signer, and exponential retry planner. A delivery signs the exact bytes of:

```text
<unix-timestamp>.<delivery-uuid>.<request-body>
```

The receiver should reject timestamps outside its configured replay window before comparing the `sha256=...` signature. Permanent 4xx responses stop retries; timeouts, 408, 429, 5xx responses, and transport failures are scheduled with bounded exponential backoff.

The optional `registry-agent` crate exposes a safe `DeploymentDriver` boundary and a Docker Compose implementation. A deployment spec explicitly names the registry, repository, mutable tag, Compose project, service, working directory, and pull-only robot credential. Rollouts:

1. authenticate Docker with the robot secret through stdin;
2. pull `registry/repository@sha256:…` by immutable digest;
3. retag that exact local image for the configured Compose tag;
4. restart only the configured Compose service;
5. require the container health status to become `healthy`;
6. pin and restart the previous digest if the replacement is unhealthy.

The command runner never invokes a shell, and project/service/tag values reject shell metacharacters. The control plane should persist watcher configuration and deliver signed events only after the operator explicitly opts in. The agent is deliberately not a remote command runner.

The current repository includes the event/signature and rollout primitives plus their unit tests. A production deployment should connect them to the PostgreSQL `webhooks`, `webhook_deliveries`, and agent watcher tables from the initial migration, and run the agent as a separately sandboxed service account.
