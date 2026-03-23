#!/usr/bin/env bash

# One-click smoke test for:
#   Claude Code -> Isartor -> GitHub Copilot-backed L3
#
# What it does:
#   1. Reads the saved Copilot credential from ~/.isartor/providers/copilot.json
#   2. Probes a small set of Copilot-backed models and picks the first one that works
#   3. Starts a temporary Isartor instance on a local demo port
#   4. Runs a Claude Code smoke request against /v1/messages
#   5. Runs a code-oriented ROI suite that proves:
#        - first request reaches L3
#        - exact repeat hits L1a
#        - semantic variants hit L1b
#   6. Prints the outcome clearly and stops the temporary server
#
# Usage:
#   ./scripts/claude-copilot-smoke-test.sh
#   ./scripts/claude-copilot-smoke-test.sh --port 8098
#   ./scripts/claude-copilot-smoke-test.sh --binary ./target/release/isartor
#   ./scripts/claude-copilot-smoke-test.sh --model gpt-4.1

set -euo pipefail

PORT="${ISARTOR_SMOKE_PORT:-8098}"
BINARY="${ISARTOR_BINARY:-./target/debug/isartor}"
MODEL="${ISARTOR_SMOKE_MODEL:-auto}"
TOKEN_FILE="${HOME}/.isartor/providers/copilot.json"
SERVER_PID=""

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

log() { echo -e "${CYAN}[claude-copilot-smoke]${RESET} $*"; }
pass() { echo -e "${GREEN}✓${RESET} $*"; }
fail() { echo -e "${RED}✗${RESET} $*" >&2; exit 1; }
warn() { echo -e "${YELLOW}○${RESET} $*"; }

cleanup() {
  if [[ -n "${SERVER_PID}" ]]; then
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --port)
        PORT="$2"
        shift 2
        ;;
      --binary)
        BINARY="$2"
        shift 2
        ;;
      --model)
        MODEL="$2"
        shift 2
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        fail "Unknown option: $1"
        ;;
    esac
  done
}

require_prereqs() {
  [[ -f "${BINARY}" ]] || fail "isartor binary not found at ${BINARY}"
  command -v python3 >/dev/null 2>&1 || fail "python3 is required"
  command -v curl >/dev/null 2>&1 || fail "curl is required"
  command -v claude >/dev/null 2>&1 || fail "claude CLI is required"
  [[ -f "${TOKEN_FILE}" ]] || fail "Missing saved Copilot credential: ${TOKEN_FILE}"
}

read_token() {
  python3 - <<'PY' "${TOKEN_FILE}"
import json, sys
with open(sys.argv[1]) as f:
    data = json.load(f)
token = data.get("github_token", "")
if not token:
    raise SystemExit(1)
print(token)
PY
}

probe_model() {
  local token="$1"
  local model="$2"
  local port="$3"
  local log_file="/tmp/isartor-claude-copilot-probe-${port}.log"
  ISARTOR__HOST_PORT="127.0.0.1:${port}" \
  ISARTOR__ENABLE_SLM_ROUTER=false \
  ISARTOR__LLM_PROVIDER=copilot \
  ISARTOR__EXTERNAL_LLM_MODEL="${model}" \
  ISARTOR__EXTERNAL_LLM_API_KEY="${token}" \
  "${BINARY}" >"${log_file}" 2>&1 &
  local pid=$!

  local ready=false
  for _ in $(seq 1 25); do
    if curl -fsS "http://localhost:${port}/health" >/dev/null 2>&1; then
      ready=true
      break
    fi
    sleep 1
  done

  if [[ "${ready}" != true ]]; then
    kill "${pid}" 2>/dev/null || true
    wait "${pid}" 2>/dev/null || true
    return 1
  fi

  local result
  result=$(python3 - <<'PY' "${port}"
import json, sys, urllib.request
port = sys.argv[1]
body = json.dumps({
    "model": "ignored",
    "max_tokens": 32,
    "messages": [{"role": "user", "content": [{"type": "text", "text": "Reply with exactly one word: hello"}]}],
}).encode()
req = urllib.request.Request(
    f"http://localhost:{port}/v1/messages",
    data=body,
    headers={"content-type": "application/json"},
    method="POST",
)
try:
    with urllib.request.urlopen(req) as r:
        print(r.status)
except urllib.error.HTTPError as e:
    print(e.code)
PY
)

  kill "${pid}" 2>/dev/null || true
  wait "${pid}" 2>/dev/null || true

  [[ "${result}" == "200" ]]
}

choose_model() {
  local token="$1"
  if [[ "${MODEL}" != "auto" ]]; then
    echo "${MODEL}"
    return 0
  fi

  local candidates=("gpt-4o-mini" "gpt-4o" "gpt-4.1")
  local probe_port=8110
  for candidate in "${candidates[@]}"; do
    echo -e "${CYAN}[claude-copilot-smoke]${RESET} Probing model support: ${candidate}" >&2
    if probe_model "${token}" "${candidate}" "${probe_port}"; then
      echo "${candidate}"
      return 0
    fi
    probe_port=$((probe_port + 1))
  done

  fail "No supported Copilot-backed model found from: ${candidates[*]}"
}

start_server() {
  local token="$1"
  local model="$2"
  log "Starting demo Isartor on http://localhost:${PORT} with model ${model}"
  ISARTOR__HOST_PORT="127.0.0.1:${PORT}" \
  ISARTOR__ENABLE_SLM_ROUTER=false \
  ISARTOR__LLM_PROVIDER=copilot \
  ISARTOR__EXTERNAL_LLM_MODEL="${model}" \
  ISARTOR__EXTERNAL_LLM_API_KEY="${token}" \
  "${BINARY}" >/tmp/isartor-claude-copilot-smoke.log 2>&1 &
  SERVER_PID=$!

  for _ in $(seq 1 30); do
    if curl -fsS "http://localhost:${PORT}/health" >/dev/null 2>&1; then
      pass "Isartor demo gateway is ready on :${PORT}"
      return 0
    fi
    sleep 1
  done

  fail "Timed out waiting for Isartor on :${PORT}"
}

run_claude_smoke() {
  local model="$1"
  log "Running Claude Code smoke request"
  local output
  output=$(
    ANTHROPIC_BASE_URL="http://localhost:${PORT}" \
    ANTHROPIC_AUTH_TOKEN=dummy \
    ANTHROPIC_MODEL="${model}" \
    ANTHROPIC_DEFAULT_SONNET_MODEL="${model}" \
    ANTHROPIC_DEFAULT_HAIKU_MODEL="${model}" \
    DISABLE_NON_ESSENTIAL_MODEL_CALLS=1 \
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
    ENABLE_TOOL_SEARCH=true \
    CLAUDE_CODE_MAX_OUTPUT_TOKENS=16000 \
    timeout 60 claude -p 'Write a tiny Rust function add(a: i32, b: i32) -> i32 and nothing else.' --output-format json --verbose 2>&1
  ) || true

  echo "${output}"

  if grep -q 'fn add(a: i32, b: i32) -> i32' <<<"${output}"; then
    pass "Claude Code returned Rust code through Isartor + Copilot L3"
  else
    fail "Claude Code smoke request did not return the expected Rust function"
  fi
}

run_roi_suite() {
  log "Running code-oriented ROI suite"
  python3 - <<'PY' "${PORT}"
import json, sys, urllib.request
port = sys.argv[1]
base = f"http://localhost:{port}"

def health():
    with urllib.request.urlopen(base + "/health") as r:
        return json.load(r)

def call(prompt):
    body = json.dumps({
        "model": "gpt-4o-mini",
        "max_tokens": 120,
        "messages": [{"role": "user", "content": [{"type": "text", "text": prompt}]}],
    }).encode()
    req = urllib.request.Request(
        base + "/v1/messages",
        data=body,
        headers={"content-type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req) as r:
        payload = json.load(r)
        text = ""
        for item in payload.get("content", []):
            if item.get("type") == "text":
                text = item.get("text", "")
                break
        return {
            "status": r.status,
            "layer": r.headers.get("X-Isartor-Layer"),
            "deflected": r.headers.get("X-Isartor-Deflected"),
            "text": text.strip(),
        }

before = health()
print("\nROI demo: Rust coding prompts via /v1/messages")
print(f"before => total={before['prompt_total_requests']}, deflected={before['prompt_total_deflected_requests']}")

cases = [
    ("cloud_seed", "Write a tiny Rust function add(a: i32, b: i32) -> i32 and nothing else."),
    ("exact_hit", "Write a tiny Rust function add(a: i32, b: i32) -> i32 and nothing else."),
    ("semantic_hit_1", "Return only a minimal Rust add function for two i32 numbers."),
    ("semantic_hit_2", "Give just the Rust code for fn add(a: i32, b: i32) -> i32."),
]

for label, prompt in cases:
    out = call(prompt)
    print(f"{label:15} status={out['status']} layer={out['layer']} deflected={out['deflected']} text={out['text'][:90]}")

after = health()
delta_total = after["prompt_total_requests"] - before["prompt_total_requests"]
delta_deflected = after["prompt_total_deflected_requests"] - before["prompt_total_deflected_requests"]
print(f"after  => total={after['prompt_total_requests']}, deflected={after['prompt_total_deflected_requests']}")
print(f"delta  => requests={delta_total}, deflected={delta_deflected}, roi={delta_deflected}/{delta_total} locally resolved")

if delta_total < 4:
    raise SystemExit("Expected at least 4 requests in ROI suite")
if delta_deflected < 3:
    raise SystemExit("Expected at least 3 deflected requests in ROI suite")
PY
  pass "ROI suite showed exact and semantic cache savings"
}

main() {
  parse_args "$@"
  require_prereqs

  log "Reading saved Copilot credential from ${TOKEN_FILE}"
  local token
  token="$(read_token)" || fail "Could not read github_token from ${TOKEN_FILE}"

  local chosen_model
  chosen_model="$(choose_model "${token}")"
  pass "Using Copilot-backed model: ${chosen_model}"

  start_server "${token}" "${chosen_model}"
  run_claude_smoke "${chosen_model}"
  run_roi_suite

  echo
  pass "One-click Claude Code + Isartor + Copilot smoke test passed"
}

main "$@"
