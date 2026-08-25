#!/usr/bin/env bash
set -Eeuo pipefail

usage() {
  printf '%s\n' \
    'usage: pre_release_client_gate.sh --archive FILE --client-archive FILE --version VERSION --arch amd64|arm64' \
    '       pre_release_client_gate.sh --preflight-only'
}

archive=''
client_archive=''
version=''
arch=''
preflight_only=0
while (($#)); do
  case "$1" in
    --archive) archive=${2:?}; shift 2 ;;
    --client-archive) client_archive=${2:?}; shift 2 ;;
    --version) version=${2:?}; shift 2 ;;
    --arch) arch=${2:?}; shift 2 ;;
    --preflight-only) preflight_only=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

for command in cargo curl docker helix; do
  command -v "$command" >/dev/null || {
    printf 'pre-release client gate requires %s\n' "$command" >&2
    exit 1
  }
done
[[ "$(helix --version)" == *'2.3.5'* ]] || {
  printf '%s\n' 'pre-release client gate requires Helix CLI v2.3.5' >&2
  exit 1
}

assert_docker_alive() {
  docker info >/dev/null 2>&1 || {
    printf '%s\n' 'Docker Engine is unavailable; aborting the client gate' >&2
    exit 1
  }
}

assert_disposable_docker() {
  assert_docker_alive
  if [[ ${HELIXIR_CLIENT_GATE_DISPOSABLE_DOCKER:-0} == 1 ]]; then
    return
  fi

  local containers volumes
  containers=$(docker ps -a --format '{{.Names}}\t{{.Ports}}' 2>/dev/null) || {
    printf '%s\n' 'cannot inspect the Docker daemon; aborting the client gate' >&2
    exit 1
  }
  volumes=$(docker volume ls -q 2>/dev/null) || {
    printf '%s\n' 'cannot inspect Docker volumes; aborting the client gate' >&2
    exit 1
  }
  if [[ -n "$containers" || -n "$volumes" ]]; then
    printf '%s\n' \
      'refusing to run the destructive Docker client gate on a daemon with existing containers or volumes' \
      'run this gate in a disposable VM/CI runner or set HELIXIR_CLIENT_GATE_DISPOSABLE_DOCKER=1 only when the entire daemon is disposable' >&2
    exit 1
  fi
}

assert_disposable_docker
if ((preflight_only)); then
  printf '%s\n' 'client gate Docker preflight passed'
  exit 0
fi

[[ -f "$archive" && -f "$client_archive" && -n "$version" ]] || {
  usage >&2
  exit 2
}
case "$arch" in
  amd64|arm64) ;;
  *) printf 'unsupported Debian architecture: %s\n' "$arch" >&2; exit 2 ;;
esac

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/helixir-client-gate.XXXXXX")
gate_id="helixir-client-gate-$(date -u +%Y%m%d%H%M%S)-$$"
network="$gate_id"
db_container="$gate_id-db"
bootstrap_container="$gate_id-bootstrap"
gateway_container="$gate_id-gateway"
embedding_container="$gate_id-embeddings"
client_one="$gate_id-client-one"
client_two="$gate_id-client-two"
db_image="$gate_id-db:local"
failed=1
current_stage='initialization'

report_failure() {
  local status=$1
  if ! docker info >/dev/null 2>&1; then
    printf 'BLOCKER: Docker Engine disappeared during client-gate stage: %s\n' \
      "$current_stage" >&2
    printf '%s\n' \
      'stop the release, preserve the daemon diagnostics, and file an infrastructure blocker before retrying on a fresh disposable runner' >&2
  fi
  return "$status"
}

cleanup() {
  if ((failed)); then
    printf '%s\n' 'pre-release client gate failed; recent service logs follow' >&2
    docker inspect --format \
      'gateway state={{.State.Status}} exit={{.State.ExitCode}} error={{.State.Error}}' \
      "$gateway_container" >&2 2>/dev/null || true
    docker logs --tail 120 "$gateway_container" >&2 2>/dev/null || true
    docker logs --tail 120 "$db_container" >&2 2>/dev/null || true
  fi
  docker rm -f "$client_one" "$client_two" "$gateway_container" \
    "$embedding_container" "$bootstrap_container" "$db_container" \
    >/dev/null 2>&1 || true
  docker network rm "$network" >/dev/null 2>&1 || true
  docker image rm "$db_image" >/dev/null 2>&1 || true
  if docker image inspect debian:12-slim >/dev/null 2>&1; then
    docker run --rm -v "$work:/work" debian:12-slim \
      chown -R "$(id -u):$(id -g)" /work >/dev/null 2>&1 || true
  fi
  rm -rf -- "$work"
}
trap 'report_failure "$?"' ERR
trap cleanup EXIT INT TERM

wait_published_port() {
  local container=$1 container_port=$2 published=''
  for _ in $(seq 1 30); do
    published=$(docker port "$container" "$container_port/tcp" 2>/dev/null || true)
    if [[ $published =~ :([0-9]+)$ ]]; then
      printf '%s\n' "${BASH_REMATCH[1]}"
      return 0
    fi
    if ! docker inspect -f '{{.State.Running}}' "$container" 2>/dev/null | grep -qx true; then
      printf 'container %s exited before publishing %s/tcp\n' \
        "$container" "$container_port" >&2
      return 1
    fi
    sleep 1
  done
  printf 'container %s did not publish %s/tcp within 30 seconds\n' \
    "$container" "$container_port" >&2
  return 1
}

mkdir -p "$work/runtime" "$work/client-runtime" "$work/apt" \
  "$work/client-one" "$work/client-two"
tar -xzf "$archive" -C "$work/runtime"
tar -xzf "$client_archive" -C "$work/client-runtime"
for required in helixir skills/helixir-memory/SKILL.md \
  integration/AGENTS.md integration/SKILLS.md; do
  [[ -e "$work/runtime/$required" ]] || {
    printf 'server release archive is missing %s\n' "$required" >&2
    exit 1
  }
done
compgen -G "$work/runtime/libonnxruntime.so*" >/dev/null || {
  printf '%s\n' 'server release archive is missing the required ONNX Runtime library' >&2
  exit 1
}
for required in helixir-client skills/helixir-memory/SKILL.md \
  integration/AGENTS.md integration/SKILLS.md; do
  [[ -e "$work/client-runtime/$required" ]] || {
    printf 'client release archive is missing %s\n' "$required" >&2
    exit 1
  }
done
[[ ! -e "$work/runtime/helixir-client" && ! -e "$work/client-runtime/helixir" ]] || {
  printf '%s\n' 'server and client release archives overlap executable ownership' >&2
  exit 1
}

printf '%s\n' '[1/7] Compile the current HQL contract and build an isolated HelixDB image'
current_stage='1/7: isolated HelixDB image build'
assert_docker_alive
helix_bin=${HELIXIR_HELIX_BIN:-"$repo_root/helixdb/target/release/helix"}
if [[ ! -x "$helix_bin" ]]; then
  cargo build --release --locked --manifest-path "$repo_root/helixdb/Cargo.toml" -p helix-cli
fi
(cd "$repo_root/helixir" && \
  HELIX_REPO_PATH="$repo_root/helixdb" "$helix_bin" check && \
  HELIX_REPO_PATH="$repo_root/helixdb" "$helix_bin" build -i dev --quiet)
test -f "$repo_root/helixir/.helix/dev/Dockerfile"
docker image inspect helix-helixir-dev:latest >/dev/null
docker tag helix-helixir-dev:latest "$db_image"

printf '%s\n' '[2/7] Build a real helixir-client package and local APT index'
current_stage='2/7: APT package and index build'
assert_docker_alive
cp "$client_archive" "$work/client-archive.tar.gz"
docker run --rm --platform "linux/$arch" \
  -v "$repo_root:/src:ro" -v "$work:/work" -w /work debian:12-slim sh -ec '
    rm -f /etc/apt/sources.list.d/debian.sources
    printf "deb http://deb.debian.org/debian bookworm main\n" >/etc/apt/sources.list
    apt-get update >/dev/null
    DEBIAN_FRONTEND=noninteractive apt-get install -y dpkg-dev >/dev/null
    /src/tools/build_client_deb.sh --archive /work/client-archive.tar.gz \
      --version "$1" --arch "$2" --output /work/apt
    cd /work/apt
    dpkg-scanpackages . /dev/null >Packages
    gzip -9c Packages >Packages.gz
  ' sh "$version" "$arch"

printf '%s\n' '[3/7] Bootstrap a disposable database and start one shared MCP gateway'
current_stage='3/7: disposable database and gateway bootstrap'
assert_docker_alive
docker network create "$network" >/dev/null
docker run -d --name "$embedding_container" --network "$network" \
  python:3.12-slim python -u -c '
import hashlib
import json
from http.server import BaseHTTPRequestHandler, HTTPServer

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        payload = json.loads(self.rfile.read(length) or b"{}")
        text = str(payload.get("prompt", payload.get("input", "")))
        digest = hashlib.sha256(text.encode()).digest()
        vector = [((digest[i % len(digest)] / 255.0) - 0.5) for i in range(768)]
        body = json.dumps({"embedding": vector}).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format, *_args):
        return

HTTPServer(("0.0.0.0", 11434), Handler).serve_forever()
  ' >/dev/null
docker run -d --name "$db_container" --network "$network" \
  --tmpfs /data:rw,size=768m -p 127.0.0.1::6969 "$db_image" >/dev/null
db_port=$(wait_published_port "$db_container" 6969)
docker run --rm --name "$bootstrap_container" --network "$network" \
  -v "$work/runtime:/runtime:ro" \
  -e HELIX_HOST="$db_container" -e HELIX_PORT=6969 \
  -e HELIXIR_RBAC_ACTOR=pre-release-admin \
  -e HELIXIR_MODE=collective \
  -e HELIXIR_RETRIEVAL_PROFILE=algo_opt \
  -e HELIX_EMBEDDING_PROVIDER=ollama \
  -e HELIX_EMBEDDING_MODEL=nomic-embed-text \
  -e HELIX_EMBEDDING_URL="http://$embedding_container:11434" \
  -e LD_LIBRARY_PATH=/runtime \
  debian:12-slim sh -ec '
    apt-get update >/dev/null
    DEBIAN_FRONTEND=noninteractive apt-get install -y ca-certificates libssl3 >/dev/null
    for attempt in $(seq 1 60); do
      if /runtime/helixir rbac bootstrap --operator pre-release-admin --json; then
        exit 0
      fi
      printf "RBAC bootstrap attempt %s/60 failed; retrying\n" "$attempt" >&2
      sleep 1
    done
    exit 1
  '
docker run -d --name "$gateway_container" --network "$network" \
  -p 127.0.0.1::8765 -v "$work/runtime:/runtime:ro" \
  -e HELIX_HOST="$db_container" -e HELIX_PORT=6969 \
  -e HELIXIR_RBAC_ACTOR=pre-release-admin \
  -e HELIXIR_MODE=collective \
  -e HELIXIR_RETRIEVAL_PROFILE=algo_opt \
  -e HELIX_EMBEDDING_PROVIDER=ollama \
  -e HELIX_EMBEDDING_MODEL=nomic-embed-text \
  -e HELIX_EMBEDDING_URL="http://$embedding_container:11434" \
  -e LD_LIBRARY_PATH=/runtime \
  debian:12-slim sh -ec '
    apt-get update >/dev/null
    DEBIAN_FRONTEND=noninteractive apt-get install -y ca-certificates libssl3 >/dev/null
    exec /runtime/helixir gateway run --bind 0.0.0.0:8765
  ' >/dev/null
gateway_port=$(wait_published_port "$gateway_container" 8765)
gateway_ready=0
for _ in $(seq 1 90); do
  if curl -fsS -X POST "http://127.0.0.1:$gateway_port/mcp" \
    -H 'Content-Type: application/json' \
    -H 'Accept: application/json, text/event-stream' \
    --data-binary '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"pre-release-probe","version":"1"}}}' \
    >/dev/null 2>&1; then
    gateway_ready=1
    break
  fi
  if ! docker inspect -f '{{.State.Running}}' "$gateway_container" | grep -qx true; then
    printf '%s\n' 'gateway exited before becoming ready' >&2
    exit 1
  fi
  sleep 1
done
docker inspect -f '{{.State.Running}}' "$gateway_container" | grep -qx true
((gateway_ready)) || {
  printf '%s\n' 'gateway stayed alive but never completed MCP initialization' >&2
  exit 1
}

printf '%s\n' '[4/7] Install through apt in two clean client containers'
current_stage='4/7: clean APT client installations'
assert_docker_alive
for spec in "$client_one:$work/client-one" "$client_two:$work/client-two"; do
  name=${spec%%:*}
  state=${spec#*:}
  docker run -d --name "$name" --network "$network" --platform "linux/$arch" \
    -v "$work/apt:/apt:ro" -v "$state:/state" debian:12-slim sh -ec '
      printf "deb [trusted=yes] file:/apt ./\n" >/etc/apt/sources.list.d/helixir.list
      apt-get update >/dev/null
      DEBIAN_FRONTEND=noninteractive apt-get install -y helixir-client python3 >/dev/null
      exec sleep infinity
    ' >/dev/null
done
for name in "$client_one" "$client_two"; do
  for _ in $(seq 1 90); do
    docker exec "$name" test -x /usr/bin/helixir-client && break
    sleep 1
  done
  docker exec "$name" helixir-client --version
  docker exec "$name" test ! -e /usr/lib/helixir-client/helixir-mcp
  docker exec "$name" test ! -e /usr/lib/helixir-client/schema
done

connect_client() {
  local container=$1 principal=$2 owner=$3 profile=$4 project=$5
  docker exec "$container" mkdir -p "$project"
  docker exec "$container" helixir-client --profile "$profile" connect \
    --gateway "http://$gateway_container:8765/mcp" \
    --principal "$principal" --owner "$owner" --project "$project" \
    --client cursor --yes --replace
  docker exec "$container" helixir-client --profile "$profile" doctor
}

printf '%s\n' '[5/7] Exercise concurrent distinct and idempotent shared enrollment'
current_stage='5/7: concurrent client enrollment'
assert_docker_alive
connect_client "$client_one" gate-client-one gate-user-one \
  /state/distinct.json /state/project-one &
pid_one=$!
connect_client "$client_two" gate-client-two gate-user-two \
  /state/distinct.json /state/project-two &
pid_two=$!
wait "$pid_one"
wait "$pid_two"
docker exec "$client_one" grep -q '"owner_id": "gate-user-one"' /state/distinct.json
docker exec "$client_two" grep -q '"owner_id": "gate-user-two"' /state/distinct.json

connect_client "$client_one" gate-shared gate-shared-owner \
  /state/shared.json /state/shared-project &
pid_one=$!
connect_client "$client_two" gate-shared gate-shared-owner \
  /state/shared.json /state/shared-project &
pid_two=$!
wait "$pid_one"
wait "$pid_two"

printf '%s\n' '[6/7] Prove direct-network MCP read/write and reject the HelixDB port'
current_stage='6/7: remote MCP read/write smoke'
assert_docker_alive
docker cp "$repo_root/tools/mcp_gateway_visibility_smoke.py" \
  "$client_one:/tmp/mcp_gateway_visibility_smoke.py"
docker exec "$client_one" python3 /tmp/mcp_gateway_visibility_smoke.py \
  --gateway "http://$gateway_container:8765/mcp" \
  --database "http://$db_container:6969/mcp" \
  --principal gate-network-client --owner gate-network-client \
  --tag "$gate_id"

printf '%s\n' '[7/7] Prove group-scoped write denial, dedup scopes, and memory visibility'
current_stage='7/7: RBAC visibility and charter contracts'
assert_docker_alive
HELIXIR_E2E_DISPOSABLE=1 \
HELIX_HOST=127.0.0.1 HELIX_PORT="$db_port" \
HELIXIR_RBAC_ACTOR=pre-release-admin HELIXIR_E2E_GROUP=default \
HELIXIR_CLIENT_GATE_SHARED_PRINCIPAL=gate-shared \
python3 "$repo_root/tools/e2e_matrix.py" --run --topology client-gate

failed=0
printf '%s\n' 'PASS: APT install, direct-network MCP read/write, two remote clients, concurrent enrollment, RBAC memory isolation, and charter C2/C4 guards'
