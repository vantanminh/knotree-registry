# k3s production deployment

The manifest runs one registry replica and one PostgreSQL replica in the
existing single-node k3s cluster. The registry image is built and pushed by
`.github/workflows/container.yml`; the VPS never builds the backend image.

The cluster's `nginx-edge` service terminates TLS and needs the checked-in
`registry-edge-route.conf` mounted into its existing deployment. The route
uses the wildcard Cloudflare Origin certificate already used for
`*.knotree.com`.

## First deployment

Run these commands from the VPS after the container workflow succeeds. They
create secrets without writing credentials to Git:

```bash
export KUBECONFIG=/etc/rancher/k3s/k3s.yaml
namespace=knotree-registry

k3s kubectl apply -f deploy/k8s/registry.yaml

db_password="$(openssl rand -hex 32)"
admin_password="$(openssl rand -hex 32)"
umask 077
mkdir -p .runtime
printf '%s\n' "$admin_password" > .runtime/registry-admin-password

k3s kubectl -n "$namespace" create secret generic registry-postgres \
  --from-literal=password="$db_password" \
  --dry-run=client -o yaml | k3s kubectl apply -f -
k3s kubectl -n "$namespace" create secret generic registry-app \
  --from-literal=DATABASE_URL="postgres://registry:${db_password}@postgres.${namespace}.svc.cluster.local:5432/registry" \
  --from-literal=BOOTSTRAP_ADMIN_USERNAME=admin \
  --from-literal=BOOTSTRAP_ADMIN_PASSWORD="$admin_password" \
  --dry-run=client -o yaml | k3s kubectl apply -f -
```

Copy the existing GHCR pull secret into the namespace if the package is
private (the source secret is not printed):

```bash
k3s kubectl get secret -n knotree registry-credentials -o json \
  | jq 'del(.metadata.namespace,.metadata.resourceVersion,.metadata.uid,.metadata.creationTimestamp,.metadata.managedFields,.metadata.annotations)' \
  | jq '.metadata.name="registry-credentials" | .metadata.namespace="knotree-registry"' \
  | k3s kubectl apply -f -
```

Apply the Nginx route using the existing deployment's ConfigMap. The command
keeps the current routes and only appends this server block:

```bash
route="$(cat deploy/k8s/registry-edge-route.conf)"
k3s kubectl -n nginx-edge get configmap nginx-edge-config -o json \
  | jq --arg route "$route" '.data["default.conf"] += "\n\n" + $route' \
  | k3s kubectl apply -f -
k3s kubectl -n nginx-edge rollout restart deployment/nginx-edge
k3s kubectl -n nginx-edge rollout status deployment/nginx-edge --timeout=120s
```

Then set the image to the immutable digest printed by the successful
container workflow and wait for the single-instance rollout:

```bash
digest='sha256:REPLACE_WITH_WORKFLOW_DIGEST'
k3s kubectl -n "$namespace" set image deployment/registry \
  registry="ghcr.io/vantanminh/knotree-registry@${digest}"
k3s kubectl -n "$namespace" rollout status deployment/registry --timeout=180s
k3s kubectl -n "$namespace" get pods,svc,pvc
```

Create a proxied DNS `A` record for `registry.knotree.com` pointing to
`15.235.210.66` (or add the hostname to the existing Cloudflare Tunnel and
point it at the Nginx edge). The public certificate already covers
`*.knotree.com`; Cloudflare should use Full (strict) TLS to the origin.

## R2 configuration

The checked-in default is `STORAGE_BACKEND=local`, which is valid for this
single-node deployment but makes the registry data PVC part of the backup
plan. To use private Cloudflare R2, create `registry-r2` with all four values,
then switch the ConfigMap and restart only the registry deployment:

```bash
k3s kubectl -n "$namespace" create secret generic registry-r2 \
  --from-literal=R2_ENDPOINT='https://<account-id>.r2.cloudflarestorage.com' \
  --from-literal=R2_BUCKET='<bucket>' \
  --from-literal=R2_ACCESS_KEY_ID='<access-key-id>' \
  --from-literal=R2_SECRET_ACCESS_KEY='<secret-access-key>' \
  --dry-run=client -o yaml | k3s kubectl apply -f -
k3s kubectl -n "$namespace" patch configmap registry-config \
  --type merge -p '{"data":{"STORAGE_BACKEND":"r2"}}'
k3s kubectl -n "$namespace" rollout restart deployment/registry
k3s kubectl -n "$namespace" rollout status deployment/registry --timeout=180s
```

Do not set `STORAGE_BACKEND=r2` until `R2_ENDPOINT`, `R2_BUCKET`,
`R2_ACCESS_KEY_ID`, and `R2_SECRET_ACCESS_KEY` are all present. `PULL_MODE=edge`
also requires `EDGE_DOWNLOAD_URL` and `EDGE_DOWNLOAD_SECRET` in the optional
`registry-edge` secret.
