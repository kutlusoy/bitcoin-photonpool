use crate::addr::script_for_address;
use crate::config::Mode;
use crate::diff::{difficulty_from_target, target_from_difficulty};
use crate::pool::Pool;
use crate::template::Template;
use crate::tx::{coinbase_with_witness, merkle_root, put_varint, sha256d, EN1_BYTES, EN2_BYTES};
use crate::template::JobData;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{tcp::OwnedWriteHalf, TcpListener, TcpStream};
use tokio::sync::mpsc;

const VERSION_MASK: u32 = 0x1fff_e000;
const MAX_LINE: u64 = 16 * 1024;
const MAX_JOBS: usize = 8;
const IDLE: Duration = Duration::from_secs(15 * 60);
const GRACE: Duration = Duration::from_secs(10);

pub async fn serve(pool: Arc<Pool>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&pool.cfg.pool.stratum_listen).await?;
    tracing::info!("stratum listening on {}", pool.cfg.pool.stratum_listen);
    loop {
        let (sock, peer) = listener.accept().await?;
        if pool.stats.conns.load(Ordering::Relaxed) as usize >= pool.cfg.pool.max_connections {
            pool.stats.refused.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let _ = sock.set_nodelay(true);
        let pool = pool.clone();
        tokio::spawn(async move {
            pool.stats.conns.fetch_add(1, Ordering::Relaxed);
            pool.stats.connects.fetch_add(1, Ordering::Relaxed);
            if let Err(e) = Session::run(pool.clone(), sock).await {
                tracing::debug!("{peer}: {e}");
            }
            pool.stats.conns.fetch_sub(1, Ordering::Relaxed);
            pool.stats.closed.fetch_add(1, Ordering::Relaxed);
        });
    }
}

struct SessionJob {
    tpl: Arc<Template>,
    data: Arc<JobData>,
    seen: HashSet<(u32, u32, u32, u32)>,
}

struct Session {
    pool: Arc<Pool>,
    w: OwnedWriteHalf,
    en1: [u8; EN1_BYTES],
    subscribed: bool,
    agent: String,
    addr: String,
    worker: String,
    script: Vec<u8>,
    authorized: bool,
    diff: f64,
    prev_diff: Option<(f64, Instant)>,
    mask: u32,
    jobs: HashMap<String, SessionJob>,
    order: VecDeque<String>,
    tip: String,
    shares: u32,
    last_retarget: Instant,
    bad_streak: u32,
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.authorized {
            self.pool.live.disconnect(&self.addr, &self.worker);
        }
    }
}

enum Reject {
    Stale,
    Duplicate,
    Low,
    Other(&'static str),
}

struct Accepted {
    credited: f64,
    actual: f64,
    block: Option<(Vec<u8>, String, Arc<Template>)>,
}

impl Session {
    async fn run(pool: Arc<Pool>, sock: TcpStream) -> anyhow::Result<()> {
        let (r, w) = sock.into_split();
        let (tx, mut rx) = mpsc::channel::<String>(64);
        tokio::spawn(async move {
            let mut r = BufReader::new(r);
            loop {
                let mut line = String::new();
                match (&mut r).take(MAX_LINE).read_line(&mut line).await {
                    Ok(n) if n > 0 && line.ends_with('\n') => {
                        if tx.send(line).await.is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        });
        let start = pool.cfg.difficulty.start;
        let mut s = Session {
            en1: pool.next_en1(), pool, w, subscribed: false, agent: String::new(), addr: String::new(), worker: String::new(), script: Vec::new(), authorized: false, diff: start, prev_diff: None, mask: 0,
            jobs: HashMap::new(), order: VecDeque::new(), tip: String::new(), shares: 0, last_retarget: Instant::now(), bad_streak: 0,
        };
        let mut tpl_rx = s.pool.template.subscribe();
        let mut tick = tokio::time::interval(Duration::from_secs(15));
        let mut last_seen = Instant::now();
        loop {
            tokio::select! {
                line = rx.recv() => {
                    let Some(line) = line else { return Ok(()) };
                    last_seen = Instant::now();
                    s.pool.stats.bytes_in.fetch_add(line.len() as u64, Ordering::Relaxed);
                    s.handle(&line).await?;
                }
                changed = tpl_rx.changed() => {
                    changed?;
                    if s.authorized {
                        let t = tpl_rx.borrow_and_update().clone();
                        if let Some(t) = t {
                            let clean = t.tip_hex != s.tip;
                            s.send_job(&t, clean).await?;
                        }
                    }
                }
                _ = tick.tick() => {
                    if last_seen.elapsed() > IDLE {
                        return Ok(());
                    }
                    if s.authorized {
                        s.retarget(false).await?;
                    }
                }
            }
        }
    }

    async fn send(&mut self, v: Value) -> anyhow::Result<()> {
        let mut line = serde_json::to_vec(&v)?;
        line.push(b'\n');
        self.pool.stats.bytes_out.fetch_add(line.len() as u64, Ordering::Relaxed);
        tokio::time::timeout(Duration::from_secs(10), self.w.write_all(&line)).await??;
        Ok(())
    }

    async fn reply(&mut self, id: &Value, result: Value, error: Value) -> anyhow::Result<()> {
        self.send(json!({"id": id, "result": result, "error": error})).await
    }

    async fn handle(&mut self, line: &str) -> anyhow::Result<()> {
        let Ok(v) = serde_json::from_str::<Value>(line) else { anyhow::bail!("bad json") };
        let id = v.get("id").cloned().unwrap_or(Value::Null);
        let params = v.get("params").and_then(Value::as_array).cloned().unwrap_or_default();
        match v.get("method").and_then(Value::as_str).unwrap_or("") {
            "mining.configure" => {
                let mut res = serde_json::Map::new();
                for ext in params.first().and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                    if ext == "version-rolling" {
                        let asked = params.get(1).and_then(|o| o.get("version-rolling.mask")).and_then(Value::as_str).and_then(|m| u32::from_str_radix(m, 16).ok()).unwrap_or(0);
                        self.mask = asked & VERSION_MASK;
                        res.insert(ext.into(), json!(self.mask != 0));
                        res.insert("version-rolling.mask".into(), json!(format!("{:08x}", self.mask)));
                    } else {
                        res.insert(ext.into(), json!(false));
                    }
                }
                self.reply(&id, Value::Object(res), Value::Null).await
            }
            "mining.subscribe" => {
                self.subscribed = true;
                self.agent = params.first().and_then(Value::as_str).unwrap_or("").to_string();
                let sid = hex::encode(self.en1);
                let r = json!([[["mining.set_difficulty", sid], ["mining.notify", sid]], sid, EN2_BYTES]);
                self.reply(&id, r, Value::Null).await
            }
            "mining.extranonce.subscribe" => self.reply(&id, json!(true), Value::Null).await,
            "mining.suggest_difficulty" => {
                if let Some(d) = params.first().and_then(Value::as_f64) {
                    if self.shares == 0 {
                        let c = &self.pool.cfg.difficulty;
                        self.diff = d.clamp(c.min, c.max);
                        if self.authorized {
                            let d = self.diff;
                            self.send(json!({"id": null, "method": "mining.set_difficulty", "params": [d]})).await?;
                        }
                    }
                }
                Ok(())
            }
            "mining.authorize" => {
                let user = params.first().and_then(Value::as_str).unwrap_or("");
                if self.authorized {
                    return self.reply(&id, json!(true), Value::Null).await;
                }
                let (addr, worker) = user.split_once('.').unwrap_or((user, "default"));
                match script_for_address(addr, self.pool.net) {
                    Ok(script) if self.subscribed => {
                        self.addr = addr.to_string();
                        self.worker = worker.chars().take(64).collect();
                        self.script = script;
                        self.authorized = true;
                        self.pool.live.connect(&self.addr, &self.worker, &self.agent);
                        self.reply(&id, json!(true), Value::Null).await?;
                        let d = self.diff;
                        self.send(json!({"id": null, "method": "mining.set_difficulty", "params": [d]})).await?;
                        self.last_retarget = Instant::now();
                        let t = self.pool.template.borrow().clone();
                        if let Some(t) = t {
                            self.send_job(&t, true).await?;
                        }
                        Ok(())
                    }
                    Ok(_) => self.reply(&id, json!(false), json!([25, "Not subscribed", null])).await,
                    Err(_) => self.reply(&id, json!(false), json!([24, "Invalid payout address, use <address>.<worker>", null])).await,
                }
            }
            "mining.submit" => {
                if !self.authorized {
                    return self.reply(&id, json!(false), json!([24, "Unauthorized worker", null])).await;
                }
                match self.check_share(&params) {
                    Ok(acc) => {
                        self.bad_streak = 0;
                        self.shares += 1;
                        let st = &self.pool.stats;
                        st.accepted.fetch_add(1, Ordering::Relaxed);
                        *st.accepted_diff.lock().unwrap() += acc.credited;
                        self.pool.ledger.share(&self.addr, acc.credited);
                        self.pool.live.share(&self.addr, &self.worker, acc.credited, acc.actual);
                        if let Some((block_hex, hash, tpl)) = acc.block {
                            self.found_block(block_hex, hash, tpl);
                        }
                        self.reply(&id, json!(true), Value::Null).await?;
                        self.retarget(false).await
                    }
                    Err(rej) => {
                        let (code, msg) = match rej {
                            Reject::Stale => (21, "Job not found"),
                            Reject::Duplicate => (22, "Duplicate share"),
                            Reject::Low => (23, "Low difficulty share"),
                            Reject::Other(m) => (20, m),
                        };
                        self.pool.live.reject(&self.addr, &self.worker, matches!(rej, Reject::Stale));
                        if matches!(rej, Reject::Stale) {
                            self.pool.stats.stale.fetch_add(1, Ordering::Relaxed);
                        } else {
                            self.pool.stats.rejected.fetch_add(1, Ordering::Relaxed);
                            let counter = if matches!(rej, Reject::Duplicate) { &self.pool.stats.dup } else { &self.pool.stats.inv };
                            counter.fetch_add(1, Ordering::Relaxed);
                            self.bad_streak += 1;
                            if self.bad_streak >= 100 {
                                anyhow::bail!("too many invalid shares");
                            }
                        }
                        self.reply(&id, json!(null), json!([code, msg, null])).await
                    }
                }
            }
            _ => self.reply(&id, Value::Null, json!([20, "Unknown method", null])).await,
        }
    }

    async fn send_job(&mut self, t: &Arc<Template>, clean: bool) -> anyhow::Result<()> {
        let data = t.job_for(&self.script);
        let id = t.job_id();
        if clean {
            self.jobs.clear();
            self.order.clear();
        }
        self.tip = t.tip_hex.clone();
        if !self.jobs.contains_key(&id) {
            self.jobs.insert(id.clone(), SessionJob { tpl: t.clone(), data: data.clone(), seen: HashSet::new() });
            self.order.push_back(id);
        }
        while self.order.len() > MAX_JOBS {
            if let Some(old) = self.order.pop_front() {
                self.jobs.remove(&old);
            }
        }
        let line = data.notify(clean);
        self.pool.stats.jobs.fetch_add(1, Ordering::Relaxed);
        self.pool.stats.bytes_out.fetch_add(line.len() as u64, Ordering::Relaxed);
        tokio::time::timeout(Duration::from_secs(10), self.w.write_all(&line)).await??;
        Ok(())
    }

    fn check_share(&mut self, p: &[Value]) -> Result<Accepted, Reject> {
        let s = |i: usize| p.get(i).and_then(Value::as_str);
        let (Some(job_id), Some(en2), Some(ntime), Some(nonce)) = (s(1), s(2), s(3), s(4)) else { return Err(Reject::Other("Malformed submit")) };
        let hex32 = |h: &str| -> Option<u32> { if h.len() == 8 { u32::from_str_radix(h, 16).ok() } else { None } };
        let (Some(en2_n), Some(ntime), Some(nonce)) = (hex32(en2), hex32(ntime), hex32(nonce)) else { return Err(Reject::Other("Malformed submit")) };
        let vbits = s(5).and_then(hex32).unwrap_or(0);

        let job = self.jobs.get_mut(job_id).ok_or(Reject::Stale)?;
        let t = job.tpl.clone();
        if t.tip_hex != self.tip {
            return Err(Reject::Stale);
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        if ntime < t.mintime || ntime as u64 > now + 7200 {
            return Err(Reject::Other("ntime out of range"));
        }
        if !job.seen.insert((en2_n, ntime, nonce, vbits)) {
            return Err(Reject::Duplicate);
        }
        let version = if self.mask == 0 { t.version } else { (t.version & !self.mask) | (vbits & self.mask) };

        let cb = job.data.parts.assemble(&self.en1, &en2_n.to_be_bytes());
        let root = merkle_root(&sha256d(&cb), &t.branch);
        let mut header = [0u8; 80];
        header[0..4].copy_from_slice(&version.to_le_bytes());
        header[4..36].copy_from_slice(&t.prev);
        header[36..68].copy_from_slice(&root);
        header[68..72].copy_from_slice(&ntime.to_le_bytes());
        header[72..76].copy_from_slice(&t.bits.to_le_bytes());
        header[76..80].copy_from_slice(&nonce.to_le_bytes());
        let mut hash = sha256d(&header);
        hash.reverse();

        let block = (hash <= t.target).then(|| {
            let mut b = Vec::with_capacity(1000);
            b.extend_from_slice(&header);
            put_varint(&mut b, t.txs.len() as u64 + 1);
            b.extend_from_slice(&if t.witness_commitment.is_some() { coinbase_with_witness(&cb) } else { cb });
            t.txs.iter().for_each(|x| b.extend_from_slice(x));
            (b, hex::encode(hash), t.clone())
        });
        let credited = if hash <= target_from_difficulty(self.diff) {
            self.diff
        } else {
            match self.prev_diff {
                Some((d, until)) if Instant::now() < until && hash <= target_from_difficulty(d) => d,
                _ if block.is_some() => self.diff,
                _ => return Err(Reject::Low),
            }
        };
        Ok(Accepted { credited, actual: difficulty_from_target(&hash), block })
    }

    fn found_block(&self, block: Vec<u8>, hash: String, tpl: Arc<Template>) {
        let pool = self.pool.clone();
        let (height, reward) = (tpl.height, tpl.coinbase_value);
        let (mode, finder) = (if pool.cfg.pool.mode == Mode::Solo { "solo" } else { "pplns" }, self.addr.clone());
        let credits: Vec<(String, u64)> = match &tpl.plan {
            Some(plan) => plan.iter().zip(&tpl.plan_labels).filter(|(_, l)| *l != "pool").map(|(o, l)| (l.clone(), o.value)).collect(),
            None => vec![(finder.clone(), reward)],
        };
        let block_hex = Arc::new(hex::encode(block));
        for n in pool.extra_nodes.iter().cloned() {
            let b = block_hex.clone();
            tokio::spawn(async move {
                match n.submit_block(&b).await {
                    Ok(r) => tracing::info!("extra node submitblock: {}", r.unwrap_or_else(|| "accepted".into())),
                    Err(e) => tracing::warn!("extra node submitblock failed: {e}"),
                }
            });
        }
        tokio::spawn(async move {
            let res = match pool.node.submit_block(&block_hex).await {
                Ok(None) => "accepted".to_string(),
                Ok(Some(r)) => r,
                Err(e) => format!("error: {e}"),
            };
            let who = crate::public::mask(&finder);
            pool.event(if res == "accepted" { "blk" } else { "warn" }, format!("BLOCK FOUND {height} [{mode}] {hash} by {who}: {res}"));
            if res == "accepted" {
                pool.stats.blocks.fetch_add(1, Ordering::Relaxed);
            }
            pool.ledger.record_block(height, &hash, &finder, mode, reward, &res, credits);
        });
    }

    /// Vardiff: aims at one share per `target_share_secs`; changes are limited to a factor of 4 per step and ignored below 25 percent.
    async fn retarget(&mut self, force: bool) -> anyhow::Result<()> {
        let c = self.pool.cfg.difficulty.clone();
        let elapsed = self.last_retarget.elapsed().as_secs_f64();
        if !force && !(elapsed >= 60.0 || (self.shares >= 30 && elapsed >= 8.0)) {
            return Ok(());
        }
        let rate = self.shares as f64 / elapsed.max(1.0);
        let want = if self.shares == 0 { self.diff / 2.0 } else { self.diff * rate * c.target_share_secs };
        let new = want.clamp(self.diff / 4.0, self.diff * 4.0).clamp(c.min, c.max);
        self.shares = 0;
        self.last_retarget = Instant::now();
        if (new / self.diff - 1.0).abs() < 0.25 {
            return Ok(());
        }
        self.prev_diff = Some((self.diff, Instant::now() + Duration::from_secs(self.pool.cfg.pool.refresh_secs) + GRACE));
        self.diff = new;
        self.send(json!({"id": null, "method": "mining.set_difficulty", "params": [new]})).await
    }
}
