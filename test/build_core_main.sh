#!/usr/bin/env bash
# Builds bitcoind and bitcoin-cli from the current bitcoin/bitcoin master into $1 (default ./core-main) for test/regtest_e2e.sh.
# Needs cmake, ninja, a C++ compiler, libevent-dev, libboost-dev and libsqlite3-dev.
set -euo pipefail
DIR=$(realpath -m "${1:-core-main}")
if [ -d "$DIR/.git" ]; then
  git -C "$DIR" pull --ff-only --depth 1 origin master
else
  git clone --depth 1 https://github.com/bitcoin/bitcoin "$DIR"
fi
cmake -S "$DIR" -B "$DIR/build" -G Ninja -DBUILD_GUI=OFF -DBUILD_TESTS=OFF -DBUILD_BENCH=OFF -DBUILD_FUZZ_BINARY=OFF -DENABLE_IPC=OFF -DWITH_ZMQ=OFF -DCMAKE_BUILD_TYPE=Release
cmake --build "$DIR/build" --target bitcoind bitcoin-cli -j"$(nproc)"
echo "BITCOIND=$DIR/build/bin/bitcoind BITCOINCLI=$DIR/build/bin/bitcoin-cli"
