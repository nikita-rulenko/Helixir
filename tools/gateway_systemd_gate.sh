#!/usr/bin/env bash
set -Eeuo pipefail

usage() {
  printf '%s\n' \
    'usage: gateway_systemd_gate.sh --archive FILE --mock FILE'
}

archive=''
mock=''
while (($#)); do
  case "$1" in
    --archive) archive=${2:?}; shift 2 ;;
    --mock) mock=${2:?}; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

[[ -f "$archive" && -f "$mock" ]] || {
  usage >&2
  exit 2
}
for command in docker python3; do
  command -v "$command" >/dev/null || {
    printf 'gateway systemd gate requires %s\n' "$command" >&2
    exit 1
  }
done
docker info >/dev/null 2>&1 || {
  printf '%s\n' 'Docker Engine is unavailable; aborting the systemd gate' >&2
  exit 1
}

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
gate_id="helixir-gateway-systemd-$(date -u +%Y%m%d%H%M%S)-$$"
image="$gate_id:local"
container="$gate_id"
failed=1

cleanup() {
  if ((failed)); then
    printf '%s\n' 'gateway systemd gate failed; diagnostics follow' >&2
    docker exec "$container" systemctl --no-pager --full status user@2000.service \
      >&2 2>/dev/null || true
    docker exec "$container" runuser -u operator -- env \
      HOME=/home/operator XDG_RUNTIME_DIR=/run/user/2000 \
      DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/2000/bus \
      systemctl --user --no-pager --full status helixir-gateway.service \
      >&2 2>/dev/null || true
    docker exec "$container" journalctl --no-pager -n 160 >&2 2>/dev/null || true
  fi
  docker rm -f "$container" >/dev/null 2>&1 || true
  docker image rm "$image" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

docker build -t "$image" "$repo_root/tools/gateway-systemd-gate" >/dev/null
docker run -d --privileged --cgroupns=host --name "$container" \
  --tmpfs /run --tmpfs /run/lock \
  -v /sys/fs/cgroup:/sys/fs/cgroup:rw \
  -v "$(cd "$(dirname "$archive")" && pwd)/$(basename "$archive"):/input/helixir.tar.gz:ro" \
  -v "$(cd "$(dirname "$mock")" && pwd)/$(basename "$mock"):/input/helixdb-mock:ro" \
  -v "$repo_root/tools/mcp_gateway_readonly_probe.py:/input/mcp_gateway_readonly_probe.py:ro" \
  "$image" >/dev/null

for _ in $(seq 1 60); do
  system_state=$(docker exec "$container" systemctl is-system-running 2>/dev/null || true)
  [[ $system_state == running || $system_state == degraded ]] && break
  state=$(docker inspect -f '{{.State.Running}}' "$container")
  [[ $state == true ]] || {
    printf '%s\n' 'privileged Ubuntu container exited before systemd initialized' >&2
    exit 1
  }
  sleep 1
done
system_state=$(docker exec "$container" systemctl is-system-running 2>/dev/null || true)
[[ $system_state == running || $system_state == degraded ]]

docker exec "$container" bash -euxo pipefail -c '
  install -d -m 0755 /opt/helixir-gate /home/operator/.helixir/bin
  tar -xzf /input/helixir.tar.gz -C /home/operator/.helixir/bin
  install -m 0755 /input/helixdb-mock /opt/helixir-gate/helixdb-mock
  test -x /home/operator/.helixir/bin/helixir
  test -f /home/operator/.helixir/bin/schema/queries.hx
  chown -R operator:operator /home/operator/.helixir

  systemd-run --unit=helixdb-mock --property=Restart=always \
    /opt/helixir-gate/helixdb-mock --listen 127.0.0.1:16969 \
      --profile fast --scenario baseline-5k
  for attempt in $(seq 1 60); do
    curl -fsS http://127.0.0.1:16969/health >/dev/null && break
    test "$attempt" -lt 60
    sleep 1
  done

  loginctl enable-linger operator
  timeout 20 systemctl start user@2000.service
  for attempt in $(seq 1 60); do
    test -S /run/user/2000/bus && break
    test "$attempt" -lt 60
    sleep 1
  done
'

run_user() {
  docker exec "$container" runuser -u operator -- env \
    HOME=/home/operator \
    XDG_RUNTIME_DIR=/run/user/2000 \
    DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/2000/bus \
    "$@"
}

helixir=/home/operator/.helixir/bin/helixir
run_user "$helixir" config set host 127.0.0.1
run_user "$helixir" config set port 16969
run_user "$helixir" config set instance dev
run_user "$helixir" config set mode collective
run_user "$helixir" config set gateway.default_bind 127.0.0.1:18765
run_user test -s /home/operator/.helixir/helixir.toml

wait_for_listener() {
  for attempt in $(seq 1 60); do
    if docker exec "$container" ss -H -ltn 'sport = :18765' | grep -q .; then
      return 0
    fi
    test "$attempt" -lt 60
    sleep 1
  done
}

assert_one_listener() {
  count=$(docker exec "$container" bash -c \
    "ss -H -ltn 'sport = :18765' | wc -l | tr -d ' '")
  [[ $count == 1 ]] || {
    printf 'expected exactly one gateway listener, found %s\n' "$count" >&2
    exit 1
  }
}

printf '%s\n' '[1/5] Start the promoted archive through systemd --user'
run_user "$helixir" gateway start --bind 127.0.0.1:18765
wait_for_listener
run_user "$helixir" gateway status
assert_one_listener
first_pid=$(run_user systemctl --user show -p MainPID --value helixir-gateway.service)
[[ $first_pid =~ ^[1-9][0-9]*$ ]]

printf '%s\n' '[2/5] Repeat start and prove idempotent single ownership'
run_user "$helixir" gateway start --bind 127.0.0.1:18765
wait_for_listener
run_user "$helixir" gateway status
assert_one_listener
run_user systemctl --user is-enabled helixir-gateway.service | grep -qx enabled
pre_cold_pid=$(run_user systemctl --user show -p MainPID --value helixir-gateway.service)
[[ $pre_cold_pid =~ ^[1-9][0-9]*$ ]]

printf '%s\n' '[3/5] Prove MCP initialize/tools and model-free read operations'
docker exec "$container" python3 /input/mcp_gateway_readonly_probe.py \
  --gateway http://127.0.0.1:18765/mcp --actor codex

printf '%s\n' '[4/5] Restart the user manager and prove automatic recovery'
docker exec "$container" timeout 20 systemctl stop user@2000.service
for attempt in $(seq 1 30); do
  docker exec "$container" ss -H -ltn 'sport = :18765' | grep -q . || break
  test "$attempt" -lt 30
  sleep 1
done
docker exec "$container" timeout 20 systemctl start user@2000.service
for attempt in $(seq 1 60); do
  docker exec "$container" test -S /run/user/2000/bus && break
  test "$attempt" -lt 60
  sleep 1
done
wait_for_listener
run_user "$helixir" gateway status
assert_one_listener
second_pid=$(run_user systemctl --user show -p MainPID --value helixir-gateway.service)
[[ $second_pid =~ ^[1-9][0-9]*$ && $second_pid != "$pre_cold_pid" ]]
docker exec "$container" python3 /input/mcp_gateway_readonly_probe.py \
  --gateway http://127.0.0.1:18765/mcp --actor codex

printf '%s\n' '[5/5] Reject an HTTP listener without the canonical managed owner'
run_user "$helixir" gateway stop
if run_user "$helixir" gateway status >/dev/null 2>&1; then
  printf '%s\n' 'inactive gateway status unexpectedly succeeded' >&2
  exit 1
fi
docker exec "$container" systemd-run --unit=helixir-unmanaged-gateway \
  --property=User=operator \
  --setenv=HOME=/home/operator \
  --setenv=HELIXIR_CONFIG=/home/operator/.helixir/helixir.toml \
  "$helixir" gateway run --bind 127.0.0.1:18765 >/dev/null
wait_for_listener
assert_one_listener
if run_user "$helixir" gateway status >/dev/null 2>&1; then
  printf '%s\n' 'unmanaged HTTP listener incorrectly satisfied gateway status' >&2
  exit 1
fi
docker exec "$container" systemctl stop helixir-unmanaged-gateway.service

failed=0
printf '%s\n' \
  'PASS: promoted Linux archive, durable config, systemd user ownership, idempotent start, cold user-manager recovery, MCP transport/read probe, and unmanaged-listener rejection'
