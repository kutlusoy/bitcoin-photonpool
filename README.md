# bitcoin-photonpool

Headless Stratum V1 mining pool daemon (`photond`) for Bitcoin Core, written in Rust. It talks to a stock Bitcoin Core node over RPC (`getblocktemplate`, `submitblock`), needs no node patch and works with a pruned node.

Status: initial version, tested end to end against Bitcoin Core 32.1 (and 29.0) on regtest (`test/regtest_e2e.sh`). Not yet tested on mainnet or on real ASIC hardware.

## Modes

- `solo`: every miner is paid directly by the coinbase of the blocks it finds. Workers log in as `<address>.<worker>`.
- `pplns`: the reward of a found block is split in the coinbase between the miners of the share window, proportional to their share difficulty (non-custodial: the pool never holds the money). The pool keeps the fee (`fee_percent`), the rounding remainder, parts below `dust_sats` and everything beyond `max_outputs` in its own output.

Payout addresses: P2PKH, P2SH, P2WPKH, P2WSH and P2TR.

## Features

- Stratum V1 with version rolling (BIP310 `mining.configure`), `mining.suggest_difficulty`, vardiff per connection, duplicate and stale share detection.
- Tip changes are detected by polling `getbestblockhash` (`poll_ms`) and pushed to all connections at once; a refreshed job is sent every `refresh_secs`.
- Shares and found blocks are logged to SQLite (WAL, batched writer thread). The PPLNS window is read from it and pruned automatically.
- Chain parameters are not hardcoded: bits, target, reward, height and the witness commitment all come from `getblocktemplate`. Only the address prefixes depend on `pool.network`.
- The coinbase is built by the pool: BIP34 height, pool tag, 4 byte extranonce1 per connection, 4 byte extranonce2 rolled by the miner, witness commitment from the node.

## Web page

The daemon serves the pages itself (all assets are embedded in the binary) on `[http] listen` (default `127.0.0.1:8080`):

- A terminal style single page app with the Bitcoin logo, a dark design and a choice of colour themes (`theme <name>` in the console of the page; default `bitcoin`).
- Pool: hashrate and history charts, miners by device type (Bitaxe, NerdMiner, cgminer family, ...), found blocks with maturity state, coinbase payout plan of the next block.
- Engine: chain tip, template timings, shares, connections and traffic, event feed, and the Photon Core, a live 3D view of node, template, coinbase, share check and miners.
- Miner: `account <address>` or `/miner/<address>` shows workers, hashrate history, shares, and what the coinbase of found blocks paid to that address (maturing and received).
- Optional background music: put audio files and an optional `playlist.json` into `music/` (see `music/README.md`); without files there is no player. Check the license of a track before you publish it.

The JSON API under `/public/v1/` (`pool`, `engine`, `blocks`, `devices`, `history`, `history/devices`, `events`, `account/<address>`) is read only and rate limited per client. Addresses of other miners are shortened.

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

## Performance design

- New tips are awaited with `waitfornewblock` (no polling; nodes without it are polled). On a new tip a valid block without transactions is published at once (mainnet, not on retarget blocks), the full template follows when `getblocktemplate` returns.
- One job per template is rendered once and shared by all connections (pplns) or per payout address (solo); a tip change costs each connection a socket write.
- The PPLNS window lives in memory as per-address buckets and is persisted every few seconds, so share rate and window size do not reach SQLite.
- Found blocks are submitted before the share is acknowledged, and to `[[extra_nodes]]` in parallel.
- `test/load.sh [conns] [pplns|solo] [secs]` runs the pool against a mock node with simulated miners. 4000 connections on a 4 core machine that also runs the simulated miners: 400 shares/s, no rejected shares, clean job delay at a new block about 50 ms median and about 110 ms at p99 (of which about 20 ms is the mock node), 73 MB RSS.

## Limits of this version

- The number of PPLNS coinbase outputs is capped by `pool.coinbase_reserved_weight`, which must equal the node setting `blockreservedweight` (default 8000 since Bitcoin Core 30, about 40 outputs). Otherwise a block with a large coinbase could exceed the maximum block weight.
- Stratum V2 and signet are not supported.
- No payout wallet: in `pplns` mode miners are paid by the coinbase itself, so there is nothing to send. Miners whose share is below `dust_sats` are not paid in that block and the amount goes to the pool output; the window keeps their shares for the following blocks.
- No web UI or admin API yet; the daemon logs a summary every minute.
