//! Minimal CPU Stratum V1 client for regtest tests: mines `--shares` shares and reports how many were accepted.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

fn sha256d(d: &[u8]) -> [u8; 32] {
    Sha256::digest(Sha256::digest(d)).into()
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

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (addr, user, want): (&str, &str, u32) = (&args[1], &args[2], args[3].parse().unwrap());
    let sock = TcpStream::connect(addr).await.unwrap();
    let (r, mut w) = sock.into_split();
    let mut lines = BufReader::new(r).lines();
    let send = |v: Value| format!("{v}\n");
    w.write_all(send(json!({"id": 1, "method": "mining.configure", "params": [["version-rolling"], {"version-rolling.mask": "ffffffff"}]})).as_bytes()).await.unwrap();
    w.write_all(send(json!({"id": 2, "method": "mining.subscribe", "params": ["testminer/1"]})).as_bytes()).await.unwrap();
    w.write_all(send(json!({"id": 3, "method": "mining.authorize", "params": [user, "x"]})).as_bytes()).await.unwrap();
    let (mut en1, mut diff, mut job): (Vec<u8>, f64, Option<Job>) = (vec![], 1.0, None);
    let (mut ok, mut bad, mut sent, mut n2) = (0u32, 0u32, 0u32, 0u32);
    while ok + bad < want {
        if let Some(j) = &job {
            if sent == ok + bad {
                let target = target(diff);
                let cb_en2 = n2.to_be_bytes();
                n2 += 1;
                let mut cb = j.coinb1.clone();
                cb.extend_from_slice(&en1);
                cb.extend_from_slice(&cb_en2);
                cb.extend_from_slice(&j.coinb2);
                let mut root = sha256d(&cb);
                for b in &j.branch {
                    let mut buf = root.to_vec();
                    buf.extend_from_slice(b);
                    root = sha256d(&buf);
                }
                for nonce in 0u32.. {
                    let mut h = [0u8; 80];
                    h[0..4].copy_from_slice(&j.version.to_le_bytes());
                    h[4..36].copy_from_slice(&j.prev);
                    h[36..68].copy_from_slice(&root);
                    h[68..72].copy_from_slice(&j.ntime.to_le_bytes());
                    h[72..76].copy_from_slice(&j.bits.to_le_bytes());
                    h[76..80].copy_from_slice(&nonce.to_le_bytes());
                    let mut hash = sha256d(&h);
                    hash.reverse();
                    if hash <= target {
                        let m = json!({"id": 100 + sent, "method": "mining.submit", "params": [user, j.id, hex::encode(cb_en2), format!("{:08x}", j.ntime), format!("{nonce:08x}")]});
                        w.write_all(send(m).as_bytes()).await.unwrap();
                        sent += 1;
                        break;
                    }
                }
            }
        }
        let Some(line) = lines.next_line().await.unwrap() else { break };
        let v: Value = serde_json::from_str(&line).unwrap();
        match v["method"].as_str() {
            Some("mining.set_difficulty") => diff = v["params"][0].as_f64().unwrap(),
            Some("mining.notify") => {
                let p = &v["params"];
                job = Some(Job {
                    id: p[0].as_str().unwrap().into(),
                    prev: unswap(p[1].as_str().unwrap()),
                    coinb1: hex::decode(p[2].as_str().unwrap()).unwrap(),
                    coinb2: hex::decode(p[3].as_str().unwrap()).unwrap(),
                    branch: p[4].as_array().unwrap().iter().map(|b| hex::decode(b.as_str().unwrap()).unwrap().try_into().unwrap()).collect(),
                    version: u32::from_str_radix(p[5].as_str().unwrap(), 16).unwrap(),
                    bits: u32::from_str_radix(p[6].as_str().unwrap(), 16).unwrap(),
                    ntime: u32::from_str_radix(p[7].as_str().unwrap(), 16).unwrap(),
                });
                sent = ok + bad;
            }
            _ => {
                let id = v["id"].as_u64().unwrap_or(0);
                if id == 2 {
                    en1 = hex::decode(v["result"][1].as_str().unwrap()).unwrap();
                } else if id >= 100 {
                    if v["result"] == json!(true) { ok += 1 } else { bad += 1; eprintln!("rejected: {}", v["error"]) }
                } else if id == 3 && v["result"] != json!(true) {
                    eprintln!("authorize failed: {}", v["error"]);
                    std::process::exit(2);
                }
            }
        }
    }
    println!("accepted {ok} rejected {bad}");
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
