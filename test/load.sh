#!/usr/bin/env bash
# Load test: photond against a mock node with N simulated miners (src/bin/loadtest.rs).
# Usage: test/load.sh [conns] [mode: pplns|solo] [duration secs] [share secs] [block secs] [txs]
# Pool and simulated miners share the machine; each connection needs two file descriptors, so raise ulimit -n for large runs.
set -euo pipefail
cd "$(dirname "$0")/.."
CONNS=${1:-2000}; MODE=${2:-pplns}; DUR=${3:-60}; SHARE=${4:-10}; BLOCK=${5:-10}; TXS=${6:-3000}
cargo build --release -q
W=$(mktemp -d)
cat > "$W/pool.toml" <<CFG
[node]
rpc_url = "http://127.0.0.1:18999"
auth = "user:pass"
[pool]
network = "mainnet"
mode = "$MODE"
address = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"
stratum_listen = "127.0.0.1:3333"
refresh_secs = 30
coinbase_reserved_weight = 100000
[difficulty]
start = 1e-7
min = 1e-7
max = 1e-7
[ledger]
path = "$W/ledger.db"
CFG
# the mock node starts with the load generator, so start it first and give it a moment before photond connects
./target/release/loadtest --conns "$CONNS" --share-secs "$SHARE" --block-secs "$BLOCK" --duration "$DUR" --txs "$TXS" > "$W/load.out" 2>&1 &
LT=$!
sleep 1
RUST_LOG=${RUST_LOG:-info} ./target/release/photond -c "$W/pool.toml" run > "$W/pool.log" 2>&1 &
PD=$!
trap 'kill $PD $LT 2>/dev/null || true' EXIT
wait $LT || true
cat "$W/load.out"
echo "--- photond: $(grep -c . "$W/pool.log") log lines, max RSS $(awk '/VmHWM/{print $2/1024 " MB"}' /proc/$PD/status 2>/dev/null)"
grep -iE "warn|error" "$W/pool.log" | head -5 || true
