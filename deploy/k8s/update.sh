#!/usr/bin/env bash
set -Eeuo pipefail
export KUBECONFIG="${KUBECONFIG:-/etc/rancher/k3s/k3s.yaml}"
[[ $# == 1 && "$1" =~ ^ghcr\.io/vantanminh/knotree-registry@sha256:[a-f0-9]{64}$ ]] || {
  echo "Expected a CI-produced Knotree Registry image digest" >&2
  exit 2
}
# First-install provisioning is separate. Preserve runtime secrets, PVCs,
# PostgreSQL, routing and operator configuration during image updates.
k3s kubectl -n knotree-registry get deployment registry >/dev/null
k3s kubectl -n knotree-registry get secret registry-app >/dev/null
# Pull and runtime secrets have already been applied from the validated CI payload.
k3s kubectl -n knotree-registry set image deployment/registry registry="$1"
k3s kubectl -n knotree-registry rollout status deployment/registry --timeout=600s
k3s kubectl -n knotree-registry get deployment registry -o json |
  python3 -c 'import json,sys; d=json.load(sys.stdin); image=sys.argv[1]; assert d["spec"]["template"]["spec"]["containers"][0]["image"]==image; assert d["status"].get("availableReplicas",0)>0; print("Verified ready registry image: "+image)' "$1"
