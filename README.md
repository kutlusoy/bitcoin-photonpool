# bitcoin-photonpool

Headless Stratum V1 mining pool daemon (`photond`) for Bitcoin Core, written in Rust. It talks to a stock Bitcoin Core node over RPC (`getblocktemplate`, `submitblock`), needs no node patch and works with a pruned node.

Status: initial version, tested end to end against Bitcoin Core 29 on regtest (`test/regtest_e2e.sh`). Not yet tested on mainnet or on real ASIC hardware.

## Modes

- `solo`: every miner is paid directly by the coinbase of the blocks it finds. Workers log in as `<address>.<worker>`.
- `pplns`: the reward of a found block is split in the coinbase between the miners of the share window, proportional to their share difficulty (non-custodial: the pool never holds the money). The pool keeps the fee (`fee_percent`), the rounding remainder, parts below `dust_sats` and everything beyond `max_outputs` in its own output.

Payout addresses: P2PKH, P2SH, P2WPKH, P2WSH and P2TR.

## Features

- Stratum V1 with version rolling (BIP310 `mining.configure`), `mining.suggest_difficulty`, vardiff per connection, duplicate and stale share detection.
- Tip changes are detected by polling `getbestblockhash` (`poll_ms`) and pushed to all connections at once; a refreshed job is sent every `refresh_secs`.
- Shares and found blocks are logged to SQLite (WAL, batched writer thread). The PPLNS window is read from it and pruned automatically.
- The coinbase is built by the pool: BIP34 height, pool tag, 4 byte extranonce1 per connection, 4 byte extranonce2 rolled by the miner, witness commitment from the node.

## Build and run

```text
cargo build --release
cp photond.example.toml photond.toml     # edit node auth, network and pool address
target/release/photond -c photond.toml check
target/release/photond -c photond.toml run
```

`bitcoin.conf.example` shows a lean pruned node configuration for the pool.

## Tests

```text
cargo test                                                              # unit tests
BITCOIND=... BITCOINCLI=... test/regtest_e2e.sh                         # real node, solo and pplns, block acceptance by Bitcoin Core
```

`src/bin/testminer.rs` is a minimal CPU Stratum client used by the end to end test (regtest difficulty only).

## Limits of this version

- Stratum V2 and signet are not supported.
- No payout wallet: in `pplns` mode miners are paid by the coinbase itself, so there is nothing to send. Miners whose share is below `dust_sats` are not paid in that block and the amount goes to the pool output; the window keeps their shares for the following blocks.
- No web UI or admin API yet; the daemon logs a summary every minute.
