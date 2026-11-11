#!/usr/bin/env bash
# End-to-end test against a real Bitcoin Core regtest node.
# Usage: BITCOIND=/path/to/bitcoind BITCOINCLI=/path/to/bitcoin-cli test/regtest_e2e.sh
# Build both from bitcoin/bitcoin master with test/build_core_main.sh.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${BITCOIND:?set BITCOIND}" "${BITCOINCLI:?set BITCOINCLI}"
W=$(mktemp -d)
cli() { "$BITCOINCLI" -regtest -datadir="$W/node" -rpcport=18443 "$@"; }
cleanup() { pkill -f "photond -c $W" || true; cli stop >/dev/null 2>&1 || true; }
trap cleanup EXIT
cargo build -q
mkdir -p "$W/node"
"$BITCOIND" -regtest -daemon -datadir="$W/node" -rpcport=18443 -fallbackfee=0.0001 >/dev/null
for _ in $(seq 30); do cli getblockcount >/dev/null 2>&1 && break; sleep 1; done
cli createwallet w >/dev/null
POOL=$(cli getnewaddress); MA=$(cli getnewaddress); MB=$(cli getnewaddress "" legacy)
cli generatetoaddress 1 "$POOL" >/dev/null

run_mode() {
  local mode=$1
  cat > "$W/$mode.toml" <<CFG
[node]
rpc_url = "http://127.0.0.1:18443"
auth = "cookie:$W/node/regtest/.cookie"
[pool]
network = "regtest"
mode = "$mode"
address = "$POOL"
stratum_listen = "127.0.0.1:3333"
refresh_secs = 5
[difficulty]
start = 0.000000001
min = 0.000000001
[pplns]
window_factor = 1000000000.0
fee_percent = 2.0
[ledger]
path = "$W/$mode.db"
CFG
  ./target/debug/photond -c "$W/$mode.toml" run > "$W/$mode.log" 2>&1 &
  sleep 2
  ./target/debug/testminer 127.0.0.1:3333 "$MA.rigA" 5 || true
  ./target/debug/testminer 127.0.0.1:3333 "$MB.rigB" 5 || true
  sleep 6
  ./target/debug/testminer 127.0.0.1:3333 "$MA.rigA" 3 || true
  pkill -f "photond -c $W/$mode.toml"; sleep 1
}

run_mode solo
H=$(cli getblockcount)
test "$H" -gt 3 || { echo "FAIL: no blocks accepted in solo mode"; exit 1; }
echo "solo: chain height $H"
run_mode pplns
python3 - "$(cli getblock "$(cli getbestblockhash)" 2)" "$POOL" <<'PY'
import json, sys
b = json.loads(sys.argv[1]); pool = sys.argv[2]
outs = [(o["value"], o["scriptPubKey"].get("address")) for o in b["tx"][0]["vout"]]
paid = {a for _, a in outs if a}
assert pool in paid and len(paid) >= 3, outs
assert abs(sum(v for v, _ in outs) - 50.0) < 1e-8, outs
print("pplns: coinbase", outs)
PY
echo PASS
