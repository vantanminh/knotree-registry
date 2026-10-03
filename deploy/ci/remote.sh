#!/usr/bin/env bash
set -Eeuo pipefail
script_dir="$(cd "$(dirname "$0")" && pwd)"
payload="${1:?CI runtime payload required}"
image="${2:?CI digest required}"
[[ "$image" =~ ^ghcr\.io/vantanminh/knotree-registry@sha256:[a-f0-9]{64}$ ]] || exit 1
# Existing stateful deployment is preserved; first installation remains manifest-based.
k3s kubectl -n knotree-registry get deployment registry >/dev/null
python3 "$script_dir/runtime.py" apply "$payload"
# ServiceAccount, TokenReview binding, internal Service and NetworkPolicy for
# the cluster-only API used by Knotree Cloud.
k3s kubectl apply -f "$script_dir/../k8s/internal.yaml" >/dev/null
python3 - "$payload" "$image" "$script_dir/patch.json" <<'PY'
import hashlib, json, sys
from pathlib import Path
packet = json.loads(Path(sys.argv[1]).read_text())
checksum = hashlib.sha256(json.dumps(packet, sort_keys=True).encode()).hexdigest()
patch = {"spec":{"template":{"metadata":{"annotations":{"github-runtime/checksum":checksum}},"spec":{"serviceAccountName":"registry","automountServiceAccountToken":True,"containers":[{"name":"registry","image":sys.argv[2],"env":None,"envFrom":[{"configMapRef":{"name":"registry-config"}},{"secretRef":{"name":"registry-app"}}],"ports":[{"name":"http","containerPort":8080},{"name":"internal","containerPort":8081}]}]}}}}
Path(sys.argv[3]).write_text(json.dumps(patch))
PY
k3s kubectl -n knotree-registry patch deployment registry --type=strategic --patch-file "$script_dir/patch.json" >/dev/null
bash "$script_dir/../k8s/update.sh" "$image"
