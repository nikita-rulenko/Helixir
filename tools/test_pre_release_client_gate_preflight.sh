#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/helixir-client-gate-preflight.XXXXXX")
generated_dockerfile="$repo_root/helixir/.helix/dev/Dockerfile"
had_generated_dockerfile=0
if [[ -f "$generated_dockerfile" ]]; then
  had_generated_dockerfile=1
  cp "$generated_dockerfile" "$work/generated-Dockerfile.before"
fi
cleanup() {
  if ((had_generated_dockerfile)); then
    mkdir -p "$(dirname "$generated_dockerfile")"
    cp "$work/generated-Dockerfile.before" "$generated_dockerfile"
  else
    rm -f -- "$generated_dockerfile"
  fi
  rm -rf -- "$work"
}
trap cleanup EXIT INT TERM

for command in cargo curl; do
  printf '#!/usr/bin/env bash\nexit 0\n' >"$work/$command"
  chmod +x "$work/$command"
done
printf '#!/usr/bin/env bash\nprintf "Helix CLI 2.3.5\\n"\n' >"$work/helix"
chmod +x "$work/helix"

write_fake_docker() {
  local ps_output=$1
  cat >"$work/docker" <<EOF
#!/usr/bin/env bash
case "\${1:-}" in
  info) exit 0 ;;
  ps) printf '%s\\n' '$ps_output' ;;
  *) exit 0 ;;
esac
EOF
  chmod +x "$work/docker"
}

write_fake_docker 'helixir-control-plane\t0.0.0.0:6971->6971/tcp'
if PATH="$work:$PATH" "$repo_root/tools/pre_release_client_gate.sh" --preflight-only \
  >"$work/out" 2>"$work/err"; then
  printf '%s\n' 'expected production-daemon preflight to fail' >&2
  exit 1
fi
grep -q 'refusing to run' "$work/err"

write_fake_docker 'unrelated-service\t127.0.0.1:8080->8080/tcp'
if PATH="$work:$PATH" "$repo_root/tools/pre_release_client_gate.sh" --preflight-only \
  >"$work/out" 2>"$work/err"; then
  printf '%s\n' 'expected any non-empty daemon to fail closed' >&2
  exit 1
fi
grep -q 'existing containers or volumes' "$work/err"

cat >"$work/docker" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
  info) exit 0 ;;
  ps) exit 0 ;;
  volume)
    [[ ${2:-} == ls ]] && printf '%s\n' 'production-volume'
    ;;
esac
EOF
chmod +x "$work/docker"
if PATH="$work:$PATH" "$repo_root/tools/pre_release_client_gate.sh" --preflight-only \
  >"$work/out" 2>"$work/err"; then
  printf '%s\n' 'expected an existing Docker volume to fail closed' >&2
  exit 1
fi
grep -q 'existing containers or volumes' "$work/err"

write_fake_docker ''
PATH="$work:$PATH" "$repo_root/tools/pre_release_client_gate.sh" --preflight-only \
  | grep -q 'preflight passed'

# The release workflow builds the maintained CLI in-tree and binds that exact
# path explicitly. Prove an unrelated PATH-level CLI cannot override it.
printf '#!/usr/bin/env bash\nprintf "Helix CLI 3.0.0\\n"\n' >"$work/helix"
printf '#!/usr/bin/env bash\nprintf "Helix CLI 2.3.5\\n"\n' >"$work/helix-explicit"
chmod +x "$work/helix" "$work/helix-explicit"
HELIXIR_HELIX_BIN="$work/helix-explicit" PATH="$work:$PATH" \
  "$repo_root/tools/pre_release_client_gate.sh" --preflight-only \
  | grep -q 'preflight passed'
printf '#!/usr/bin/env bash\nprintf "Helix CLI 2.3.5\\n"\n' >"$work/helix"
chmod +x "$work/helix"

write_fake_docker 'helixir-control-plane\t0.0.0.0:6971->6971/tcp'
HELIXIR_CLIENT_GATE_DISPOSABLE_DOCKER=1 PATH="$work:$PATH" \
  "$repo_root/tools/pre_release_client_gate.sh" --preflight-only \
  | grep -q 'preflight passed'

cat >"$work/docker" <<'EOF'
#!/usr/bin/env bash
[[ ${1:-} == info ]] && exit 1
exit 0
EOF
chmod +x "$work/docker"
if PATH="$work:$PATH" "$repo_root/tools/pre_release_client_gate.sh" --preflight-only \
  >"$work/out" 2>"$work/err"; then
  printf '%s\n' 'expected unavailable-daemon preflight to fail' >&2
  exit 1
fi
grep -q 'Docker Engine is unavailable' "$work/err"

# The daemon may disappear after a successful preflight (the production
# incident happened inside BuildKit). Prove the next stage boundary aborts
# before invoking a build.
mkdir -p "$work/server/skills/helixir-memory" "$work/server/integration" \
  "$work/client/skills/helixir-memory" "$work/client/integration"
touch "$work/server/helixir" "$work/server/libonnxruntime.so" \
  "$work/server/skills/helixir-memory/SKILL.md" \
  "$work/server/integration/AGENTS.md" "$work/server/integration/SKILLS.md" \
  "$work/client/helixir-client" "$work/client/skills/helixir-memory/SKILL.md" \
  "$work/client/integration/AGENTS.md" "$work/client/integration/SKILLS.md"
tar -czf "$work/server.tar.gz" -C "$work/server" .
tar -czf "$work/client.tar.gz" -C "$work/client" .
counter="$work/docker-info-count"
cat >"$work/docker" <<EOF
#!/usr/bin/env bash
if [[ \${1:-} == info ]]; then
  count=0
  [[ -f '$counter' ]] && count=\$(< '$counter')
  count=\$((count + 1))
  printf '%s' "\$count" >'$counter'
  ((count == 1)) && exit 0
  exit 1
fi
if [[ \${1:-} == ps ]]; then
  exit 0
fi
exit 0
EOF
chmod +x "$work/docker"
if PATH="$work:$PATH" "$repo_root/tools/pre_release_client_gate.sh" \
  --archive "$work/server.tar.gz" --client-archive "$work/client.tar.gz" \
  --version 0.0.0 --arch amd64 >"$work/out" 2>"$work/err"; then
  printf '%s\n' 'expected daemon loss after preflight to fail closed' >&2
  exit 1
fi
grep -q 'Docker Engine is unavailable' "$work/err"

# A daemon can also die inside a Docker command after both preflight and the
# stage-boundary liveness check passed. That is the exact BuildKit failure mode
# that took down the dogfood daemon; require a dedicated release-blocking
# diagnostic instead of generic cleanup noise.
rm -f "$counter"
cat >"$work/helix" <<'EOF'
#!/usr/bin/env bash
if [[ ${1:-} == --version ]]; then
  printf 'Helix CLI 2.3.5\n'
elif [[ ${1:-} == build ]]; then
  mkdir -p .helix/dev
  printf 'FROM scratch\n' >.helix/dev/Dockerfile
fi
EOF
chmod +x "$work/helix"
cat >"$work/docker" <<EOF
#!/usr/bin/env bash
if [[ \${1:-} == info ]]; then
  count=0
  [[ -f '$counter' ]] && count=\$(< '$counter')
  count=\$((count + 1))
  printf '%s' "\$count" >'$counter'
  ((count <= 2)) && exit 0
  exit 1
fi
if [[ \${1:-} == ps ]]; then
  exit 0
fi
if [[ \${1:-} == tag ]]; then
  exit 125
fi
exit 0
EOF
chmod +x "$work/docker"
if PATH="$work:$PATH" "$repo_root/tools/pre_release_client_gate.sh" \
  --archive "$work/server.tar.gz" --client-archive "$work/client.tar.gz" \
  --version 0.0.0 --arch amd64 >"$work/out" 2>"$work/err"; then
  printf '%s\n' 'expected an in-command daemon crash to fail closed' >&2
  exit 1
fi
grep -q 'BLOCKER: Docker Engine disappeared during client-gate stage: 1/7' "$work/err"

printf '%s\n' 'pre-release client gate preflight tests passed'
