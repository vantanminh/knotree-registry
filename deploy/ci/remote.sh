#!/usr/bin/env bash
set -Eeuo pipefail
script_dir="$(cd "$(dirname "$0")" && pwd)"
payload="${1:?CI runtime payload required}"
image="${2:?CI digest required}"
[[ "$image" =~ ^ghcr\.io/vantanminh/knotree-registry@sha256:[a-f0-9]{64}$ ]] || exit 1
export KUBECONFIG=/etc/rancher/k3s/k3s.yaml
# Existing stateful deployment is preserved; first installation remains manifest-based.
k3s kubectl -n knotree-registry get deployment registry >/dev/null
encryption_status="$(k3s secrets-encrypt status)" || {
  echo 'Deploy failed: could not check k3s Secret encryption status.' >&2
  exit 1
}
if ! grep -qx 'Encryption Status: Enabled' <<< "$encryption_status" || \
   ! grep -qx 'Current Rotation Stage: reencrypt_finished' <<< "$encryption_status"; then
  echo 'Deploy failed: enable and finish k3s Secret encryption before applying runtime Secrets.' >&2
  exit 1
fi
python3 "$script_dir/runtime.py" apply "$payload"
python3 - "$payload" "$image" "$script_dir/patch.json" <<'PY'
import hashlib, json, sys
from pathlib import Path
packet = json.loads(Path(sys.argv[1]).read_text())
checksum = hashlib.sha256(json.dumps(packet, sort_keys=True).encode()).hexdigest()
patch = {"spec":{"template":{"metadata":{"annotations":{"github-runtime/checksum":checksum}},"spec":{"containers":[{"name":"registry","image":sys.argv[2],"env":None,"envFrom":[{"configMapRef":{"name":"registry-config"}},{"secretRef":{"name":"registry-app"}}]}]}}}}
Path(sys.argv[3]).write_text(json.dumps(patch))
PY
k3s kubectl -n knotree-registry patch deployment registry --type=strategic --patch-file "$script_dir/patch.json" >/dev/null
bash "$script_dir/../k8s/update.sh" "$image"
