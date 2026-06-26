#!/usr/bin/env bash
set -euo pipefail

# Compares ylong_http_client with curl/libcurl on repeated HTTP requests through
# an HTTPS proxy. Required env:
#   BENCH_URL=http://127.0.0.1:3000/data
#   PROXY_URL=https://127.0.0.1:8443    # HTTPS_PROXY is also accepted
# Optional env:
#   BENCH_REQUESTS=1000
#   PROXY_CA_FILE=/path/proxy-ca.pem
#   PROXY_CERT_FILE=/path/client-cert.pem
#   PROXY_KEY_FILE=/path/client-key.pem
#   PROXY_INSECURE=1

REQ="${BENCH_REQUESTS:-100}"
PROXY="${PROXY_URL:-${HTTPS_PROXY:-}}"
TARGET="${BENCH_URL:-}"
FEATURES="${FEATURES:-async,http1_1,tokio_base,c_openssl_3_0}"

if [[ -z "$PROXY" || -z "$TARGET" ]]; then
  echo "usage: BENCH_URL=http://host/path PROXY_URL=https://proxy:port BENCH_REQUESTS=1000 $0" >&2
  exit 2
fi

export BENCH_REQUESTS="$REQ"
export PROXY_URL="$PROXY"

if [[ -z "${OPENSSL_LIB_DIR:-}" && "$(uname -s)" == "Darwin" ]] && command -v brew >/dev/null 2>&1; then
  openssl_prefix="$(brew --prefix openssl@3 2>/dev/null || true)"
  if [[ -n "$openssl_prefix" ]]; then
    export OPENSSL_LIB_DIR="$openssl_prefix/lib"
    export OPENSSL_INCLUDE_DIR="${OPENSSL_INCLUDE_DIR:-$openssl_prefix/include}"
  fi
fi

echo "== ylong_http_client =="
cargo run -p ylong_http_client --release --example async_https_proxy_bench \
  --no-default-features --features "$FEATURES"

tmp_cfg="$(mktemp)"
trap 'rm -f "$tmp_cfg"' EXIT
for _ in $(seq 1 "$REQ"); do
  printf 'url = "%s"\noutput = "/dev/null"\n' "$TARGET" >>"$tmp_cfg"
done

curl_args=(--proxy "$PROXY" --silent --show-error)
if [[ "${PROXY_INSECURE:-}" == "1" || "${PROXY_INSECURE:-}" == "true" ]]; then
  curl_args+=(--proxy-insecure)
fi
if [[ -n "${PROXY_CA_FILE:-}" ]]; then
  curl_args+=(--proxy-cacert "$PROXY_CA_FILE")
fi
if [[ -n "${PROXY_CERT_FILE:-}" ]]; then
  curl_args+=(--proxy-cert "$PROXY_CERT_FILE")
fi
if [[ -n "${PROXY_KEY_FILE:-}" ]]; then
  curl_args+=(--proxy-key "$PROXY_KEY_FILE")
fi

start=$(python3 - <<'PY'
import time
print(time.perf_counter())
PY
)

echo "== curl/libcurl =="
curl "${curl_args[@]}" --config "$tmp_cfg" >/dev/null

end=$(python3 - <<'PY'
import time
print(time.perf_counter())
PY
)

python3 - <<PY
req = int("$REQ")
start = float("$start")
end = float("$end")
elapsed = end - start
rps = req / elapsed if elapsed > 0 else 0.0
print(f"client=curl requests={req} elapsed_ms={elapsed * 1000:.0f} req_per_sec={rps:.2f}")
PY
