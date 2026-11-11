//! Load generator: a mock Bitcoin node (RPC) plus N simulated Stratum miners that mine real shares at a tiny difficulty.
//! Reports share throughput and the delay from a new block at the node to the arrival of the clean job at each miner.
//!
//! photond must point at `--mock` with network mainnet, difficulty start = min = max = `--diff`, e.g. test/load.sh.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

static START: OnceLock<Instant> = OnceLock::new();
fn now_ns() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

fn sha256d(d: &[u8]) -> [u8; 32] {
    Sha256::digest(Sha256::digest(d)).into()
}

struct Args {
    conns: usize,
    share_secs: f64,
    block_secs: f64,
    duration: f64,
    txs: usize,
    pool: String,
    mock: String,
    diff: f64,
    ramp_per_sec: f64,
}

fn args() -> Args {
    let mut a = Args { conns: 1000, share_secs: 10.0, block_secs: 10.0, duration: 60.0, txs: 3000, pool: "127.0.0.1:3333".into(), mock: "127.0.0.1:18999".into(), diff: 1e-7, ramp_per_sec: 1000.0 };
    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i + 1 < v.len() {
        let x = &v[i + 1];
        match v[i].as_str() {
            "--conns" => a.conns = x.parse().unwrap(),
            "--share-secs" => a.share_secs = x.parse().unwrap(),
            "--block-secs" => a.block_secs = x.parse().unwrap(),
            "--duration" => a.duration = x.parse().unwrap(),
            "--txs" => a.txs = x.parse().unwrap(),
            "--pool" => a.pool = x.clone(),
            "--mock" => a.mock = x.clone(),
            "--diff" => a.diff = x.parse().unwrap(),
            "--ramp" => a.ramp_per_sec = x.parse().unwrap(),
            o => panic!("unknown option {o}"),
        }
        i += 2;
    }
    a
}

struct Mock {
    tip: watch::Sender<(u64, u64)>,
    released_ns: AtomicU64,
    txs: Vec<(String, String)>,
}

fn tip_hash(n: u64) -> String {
    hex::encode(Sha256::digest(n.to_be_bytes()))
}

async fn mock_conn(m: Arc<Mock>, sock: TcpStream) {
    let _ = sock.set_nodelay(true);
    let (r, mut w) = sock.into_split();
    let mut r = BufReader::new(r);
    loop {
        let mut len = 0usize;
        let mut line = String::new();
        loop {
            line.clear();
            match r.read_line(&mut line).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            if line == "\r\n" {
                break;
            }
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0u8; len];
        if r.read_exact(&mut body).await.is_err() {
            return;
        }
        let req: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let (n, h) = *m.tip.borrow();
        let result = match req["method"].as_str().unwrap_or("") {
            "getblockchaininfo" => json!({"chain": "main", "blocks": h, "initialblockdownload": false}),
            "getbestblockhash" => json!(tip_hash(n)),
            "waitfornewblock" => {
                let timeout = req["params"][0].as_u64().unwrap_or(1000);
                let cur = req["params"][1].as_str().unwrap_or("").to_string();
                let mut rx = m.tip.subscribe();
                let _ = tokio::time::timeout(Duration::from_millis(timeout), async {
                    while tip_hash(rx.borrow().0) == cur {
                        if rx.changed().await.is_err() {
                            break;
                        }
                    }
                })
                .await;
                let (n, h) = *m.tip.borrow();
                json!({"hash": tip_hash(n), "height": h})
            }
            "getblocktemplate" => {
                let txs: Vec<Value> = m.txs.iter().map(|(d, t)| json!({"data": d, "txid": t, "hash": t, "fee": 2000, "weight": 800})).collect();
                json!({
                    "version": 0x2000_0000, "previousblockhash": tip_hash(n), "height": h + 1, "coinbasevalue": 625_000_000u64 + 2000 * m.txs.len() as u64,
                    "bits": "1703a30c", "target": "0000000000000000000300000000000000000000000000000000000000000000",
                    "curtime": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(), "mintime": 1_700_000_000u64,
                    "default_witness_commitment": format!("6a24aa21a9ed{}", "ab".repeat(32)), "transactions": txs
                })
            }
            "submitblock" => Value::Null,
            _ => Value::Null,
        };
        let out = serde_json::to_vec(&json!({"result": result, "error": null, "id": req["id"]})).unwrap();
        let mut resp = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n", out.len()).into_bytes();
        resp.extend_from_slice(&out);
        if w.write_all(&resp).await.is_err() {
            return;
        }
    }
}

#[derive(Default)]
struct Stats {
    connected: AtomicU64,
    accepted: AtomicU64,
    rejected: AtomicU64,
    stale: AtomicU64,
    lat: Mutex<Vec<u64>>,
}

fn unswap(h: &str) -> [u8; 32] {
    let b = hex::decode(h).unwrap();
    let mut out = [0u8; 32];
    for (i, w) in b.chunks(4).enumerate() {
        for (j, x) in w.iter().rev().enumerate() {
            out[i * 4 + j] = *x;
        }
    }
    out
}

fn target(d: f64) -> [u8; 32] {
    let divisor = ((d * 1_048_576.0).round() as u128).max(1);
    let mut num = [0u8; 32];
    num[1] = 0x0f;
    num[2] = 0xff;
    num[3] = 0xf0;
    let (mut out, mut rem) = ([0u8; 32], 0u128);
    for i in 0..32 {
        rem = (rem << 8) | num[i] as u128;
        out[i] = (rem / divisor) as u8;
        rem %= divisor;
    }
    out
}

struct Job {
    id: String,
    prev: [u8; 32],
    coinb1: Vec<u8>,
    coinb2: Vec<u8>,
    branch: Vec<[u8; 32]>,
    version: u32,
    bits: u32,
    ntime: u32,
}

async fn miner(idx: usize, a: Arc<Args>, st: Arc<Stats>, released: Arc<Mock>) {
    let hrp = bech32::Hrp::parse("bc").unwrap();
    let mut prog = [0u8; 20];
    prog[..8].copy_from_slice(&(idx as u64 + 1).to_be_bytes());
    let user = format!("{}.w{idx}", bech32::segwit::encode(hrp, bech32::segwit::VERSION_0, &prog).unwrap());
    let Ok(sock) = TcpStream::connect(&a.pool).await else { return };
    let _ = sock.set_nodelay(true);
    let (r, mut w) = sock.into_split();
    let mut lines = BufReader::new(r).lines();
    for m in [json!({"id": 2, "method": "mining.subscribe", "params": ["loadtest/1"]}), json!({"id": 3, "method": "mining.authorize", "params": [user, "x"]})] {
        if w.write_all(format!("{m}\n").as_bytes()).await.is_err() {
            return;
        }
    }
    let (mut en1, mut diff, mut job) = (Vec::new(), a.diff, None::<Job>);
    let mut counted = false;
    let jitter = 0.5 + (idx % 100) as f64 / 100.0;
    let mut next_share = tokio::time::Instant::now() + Duration::from_secs_f64(a.share_secs * jitter);
    let mut n2 = idx as u32 * 7919;
    let mut seq = 0u64;
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { return };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                match v["method"].as_str() {
                    Some("mining.set_difficulty") => diff = v["params"][0].as_f64().unwrap_or(diff),
                    Some("mining.notify") => {
                        let p = &v["params"];
                        if p[8] == json!(true) {
                            let rel = released.released_ns.load(Ordering::Relaxed);
                            if rel > 0 { st.lat.lock().unwrap().push(now_ns().saturating_sub(rel)); }
                        }
                        job = Some(Job {
                            id: p[0].as_str().unwrap().into(), prev: unswap(p[1].as_str().unwrap()),
                            coinb1: hex::decode(p[2].as_str().unwrap()).unwrap(), coinb2: hex::decode(p[3].as_str().unwrap()).unwrap(),
                            branch: p[4].as_array().unwrap().iter().map(|b| hex::decode(b.as_str().unwrap()).unwrap().try_into().unwrap()).collect(),
                            version: u32::from_str_radix(p[5].as_str().unwrap(), 16).unwrap(), bits: u32::from_str_radix(p[6].as_str().unwrap(), 16).unwrap(),
                            ntime: u32::from_str_radix(p[7].as_str().unwrap(), 16).unwrap(),
                        });
                    }
                    _ => {
                        let id = v["id"].as_u64().unwrap_or(0);
                        if id == 2 {
                            en1 = hex::decode(v["result"][1].as_str().unwrap_or("00000000")).unwrap();
                        } else if id == 3 {
                            if v["result"] != json!(true) { eprintln!("authorize failed: {v}"); return; }
                            if !counted { counted = true; st.connected.fetch_add(1, Ordering::Relaxed); }
                        } else if id >= 100 {
                            if v["result"] == json!(true) { st.accepted.fetch_add(1, Ordering::Relaxed); }
                            else if v["error"][0] == json!(21) { st.stale.fetch_add(1, Ordering::Relaxed); }
                            else { st.rejected.fetch_add(1, Ordering::Relaxed); }
                        }
                    }
                }
            }
            _ = tokio::time::sleep_until(next_share), if job.is_some() => {
                let j = job.as_ref().unwrap();
                next_share = tokio::time::Instant::now() + Duration::from_secs_f64(a.share_secs);
                n2 = n2.wrapping_add(1);
                let en2 = n2.to_be_bytes();
                let mut cb = j.coinb1.clone();
                cb.extend_from_slice(&en1);
                cb.extend_from_slice(&en2);
                cb.extend_from_slice(&j.coinb2);
                let mut root = sha256d(&cb);
                for b in &j.branch {
                    let mut buf = root.to_vec();
                    buf.extend_from_slice(b);
                    root = sha256d(&buf);
                }
                let tg = target(diff);
                let mut h = [0u8; 80];
                h[0..4].copy_from_slice(&j.version.to_le_bytes());
                h[4..36].copy_from_slice(&j.prev);
                h[36..68].copy_from_slice(&root);
                h[68..72].copy_from_slice(&j.ntime.to_le_bytes());
                h[72..76].copy_from_slice(&j.bits.to_le_bytes());
                let mut found = None;
                for nonce in 0u32..u32::MAX {
                    h[76..80].copy_from_slice(&nonce.to_le_bytes());
                    let mut hash = sha256d(&h);
                    hash.reverse();
                    if hash <= tg { found = Some(nonce); break; }
                }
                if let Some(nonce) = found {
                    seq += 1;
                    let m = json!({"id": 100 + seq, "method": "mining.submit", "params": [user, j.id, hex::encode(en2), format!("{:08x}", j.ntime), format!("{nonce:08x}")]});
                    if w.write_all(format!("{m}\n").as_bytes()).await.is_err() { return; }
                }
            }
        }
    }
}

fn pct(v: &[u64], p: f64) -> f64 {
    if v.is_empty() { return 0.0; }
    v[((v.len() - 1) as f64 * p) as usize] as f64 / 1e6
}

#[tokio::main]
async fn main() {
    let a = Arc::new(args());
    let txs: Vec<(String, String)> = (0..a.txs).map(|i| (hex::encode([i as u8; 200]), hex::encode(sha256d(&(i as u64).to_be_bytes())))).collect();
    let (tx, _) = watch::channel((1u64, 800_000u64));
    let mock = Arc::new(Mock { tip: tx, released_ns: AtomicU64::new(0), txs });
    let listener = TcpListener::bind(&a.mock).await.unwrap();
    let m2 = mock.clone();
    tokio::spawn(async move {
        loop {
            let (s, _) = listener.accept().await.unwrap();
            tokio::spawn(mock_conn(m2.clone(), s));
        }
    });
    let m3 = mock.clone();
    let block_secs = a.block_secs;
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs_f64(block_secs)).await;
            m3.released_ns.store(now_ns(), Ordering::Relaxed);
            m3.tip.send_modify(|t| { t.0 += 1; t.1 += 1; });
        }
    });
    println!("mock node on {}, {} connections to {}, {} s per block, {} tx per template", a.mock, a.conns, a.pool, a.block_secs, a.txs);
    let st = Arc::new(Stats::default());
    let (a2, st2, m4) = (a.clone(), st.clone(), mock.clone());
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        for i in 0..a2.conns {
            tokio::spawn(miner(i, a2.clone(), st2.clone(), m4.clone()));
            tokio::time::sleep(Duration::from_secs_f64(1.0 / a2.ramp_per_sec)).await;
        }
    });
    let t0 = Instant::now();
    let (mut last_acc, mut last_t) = (0u64, Instant::now());
    let mut all_lat: Vec<u64> = Vec::new();
    while t0.elapsed().as_secs_f64() < a.duration {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let acc = st.accepted.load(Ordering::Relaxed);
        let mut lat = std::mem::take(&mut *st.lat.lock().unwrap());
        lat.sort_unstable();
        println!(
            "t={:>4.0}s conns {:>6} | shares/s {:>7.1} | rejected {} stale {} | clean job delay ms p50 {:.1} p95 {:.1} p99 {:.1} max {:.1} (n={})",
            t0.elapsed().as_secs_f64(), st.connected.load(Ordering::Relaxed), (acc - last_acc) as f64 / last_t.elapsed().as_secs_f64(),
            st.rejected.load(Ordering::Relaxed), st.stale.load(Ordering::Relaxed), pct(&lat, 0.5), pct(&lat, 0.95), pct(&lat, 0.99), pct(&lat, 1.0), lat.len()
        );
        all_lat.extend(lat);
        last_acc = acc;
        last_t = Instant::now();
    }
    all_lat.sort_unstable();
    println!(
        "TOTAL accepted {} rejected {} stale {} | clean job delay ms p50 {:.1} p95 {:.1} p99 {:.1} max {:.1} (n={})",
        st.accepted.load(Ordering::Relaxed), st.rejected.load(Ordering::Relaxed), st.stale.load(Ordering::Relaxed),
        pct(&all_lat, 0.5), pct(&all_lat, 0.95), pct(&all_lat, 0.99), pct(&all_lat, 1.0), all_lat.len()
    );
}
