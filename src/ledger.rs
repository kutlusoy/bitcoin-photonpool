use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Shares are summed per address into buckets of this many seconds; the PPLNS window is cut at a bucket boundary (the oldest bucket is weighted fractionally).
const BUCKET_SECS: i64 = 600;
const FLUSH_EVERY: Duration = Duration::from_secs(5);
/// Buckets that left the window stay in the database this long, in case the network difficulty rises and the window grows again.
const KEEP_EXTRA_SECS: i64 = 7 * 86_400;

struct Bucket {
    start: i64,
    sum: f64,
    per: HashMap<String, f64>,
}

#[derive(Default)]
struct State {
    buckets: VecDeque<Bucket>,
    totals: HashMap<String, f64>,
    total: f64,
    /// Not yet written: (bucket start, address) -> (difficulty, share count).
    pending: HashMap<(i64, String), (f64, u64)>,
}

type Job = Box<dyn FnOnce(&mut Connection) + Send>;
enum Msg {
    Run(Job),
}

/// A found block as the pages show it.
pub struct BlockRow {
    pub ts: i64,
    pub height: u32,
    pub hash: String,
    pub addr: String,
    pub mode: String,
    pub status: String,
}

/// How long statistics are kept.
const KEEP_DAYS: i64 = 60;

/// Share accounting for the PPLNS window and block log. The window lives in memory (one add per share under a short lock) and is persisted
/// as per-address bucket sums every few seconds by a writer thread, so neither the share rate nor the window size reaches SQLite.
#[derive(Clone)]
pub struct Ledger {
    tx: Sender<Msg>,
    state: Arc<Mutex<State>>,
    path: String,
}

fn open(path: &str) -> Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(Duration::from_secs(5))?;
    c.pragma_update(None, "journal_mode", "WAL")?;
    c.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(c)
}

fn flush(c: &mut Connection, state: &Mutex<State>) {
    let pending = std::mem::take(&mut state.lock().unwrap().pending);
    if pending.is_empty() {
        return;
    }
    let res = (|| -> rusqlite::Result<()> {
        let t = c.transaction()?;
        {
            let mut st = t.prepare_cached("INSERT INTO buckets (start, addr, diff, n) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(start, addr) DO UPDATE SET diff = diff + excluded.diff, n = n + excluded.n")?;
            for ((start, addr), (d, n)) in &pending {
                st.execute(params![start, addr, d, *n as i64])?;
            }
        }
        t.commit()
    })();
    if let Err(e) = res {
        tracing::error!("ledger flush failed: {e}");
        let mut s = state.lock().unwrap();
        for (k, v) in pending {
            let e = s.pending.entry(k).or_insert((0.0, 0));
            e.0 += v.0;
            e.1 += v.1;
        }
    }
}

fn writer(rx: Receiver<Msg>, mut c: Connection, state: Arc<Mutex<State>>) {
    let mut last = Instant::now();
    loop {
        match rx.recv_timeout(FLUSH_EVERY) {
            Ok(Msg::Run(job)) => job(&mut c),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                flush(&mut c, &state);
                return;
            }
        }
        if last.elapsed() >= FLUSH_EVERY {
            flush(&mut c, &state);
            last = Instant::now();
        }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl Ledger {
    pub fn open(path: &str) -> Result<Self> {
        let c = open(path)?;
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS buckets (start INTEGER NOT NULL, addr TEXT NOT NULL, diff REAL NOT NULL, n INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (start, addr)) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS blocks (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, height INTEGER NOT NULL, hash TEXT NOT NULL, addr TEXT NOT NULL, reward INTEGER NOT NULL, result TEXT NOT NULL, mode TEXT NOT NULL DEFAULT 'solo', status TEXT NOT NULL DEFAULT 'maturing');
             CREATE TABLE IF NOT EXISTS credits (id INTEGER PRIMARY KEY, height INTEGER NOT NULL, hash TEXT NOT NULL, addr TEXT NOT NULL, amount INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS credits_addr ON credits (addr, id);
             CREATE TABLE IF NOT EXISTS pool_history (minute INTEGER PRIMARY KEY, hashrate REAL NOT NULL, miners INTEGER NOT NULL, ok INTEGER NOT NULL, stale INTEGER NOT NULL, dup INTEGER NOT NULL, nd REAL NOT NULL);
             CREATE TABLE IF NOT EXISTS device_history (minute INTEGER NOT NULL, key TEXT NOT NULL, name TEXT NOT NULL, hashrate REAL NOT NULL, workers INTEGER NOT NULL, PRIMARY KEY (minute, key)) WITHOUT ROWID;",
        )?;
        for alter in ["ALTER TABLE buckets ADD COLUMN n INTEGER NOT NULL DEFAULT 0", "ALTER TABLE blocks ADD COLUMN mode TEXT NOT NULL DEFAULT 'solo'", "ALTER TABLE blocks ADD COLUMN status TEXT NOT NULL DEFAULT 'maturing'"] {
            let _ = c.execute(alter, []);
        }
        let mut state = State::default();
        {
            let mut st = c.prepare("SELECT start, addr, diff FROM buckets ORDER BY start")?;
            let mut rows = st.query([])?;
            while let Some(r) = rows.next()? {
                let (start, addr, d): (i64, String, f64) = (r.get(0)?, r.get(1)?, r.get(2)?);
                state.add(start, addr, d);
            }
        }
        let state = Arc::new(Mutex::new(state));
        let (tx, rx) = channel();
        let st2 = state.clone();
        std::thread::Builder::new().name("ledger".into()).spawn(move || writer(rx, c, st2))?;
        Ok(Self { tx, state, path: path.to_string() })
    }

    pub fn share(&self, addr: &str, diff: f64) {
        let start = unix_now() / BUCKET_SECS * BUCKET_SECS;
        let mut s = self.state.lock().unwrap();
        let e = s.pending.entry((start, addr.to_string())).or_insert((0.0, 0));
        e.0 += diff;
        e.1 += 1;
        s.add(start, addr.to_string(), diff);
    }

    fn run(&self, f: impl FnOnce(&mut Connection) + Send + 'static) {
        let _ = self.tx.send(Msg::Run(Box::new(f)));
    }

    /// Stores a found block and what its coinbase pays to whom (`credits`: address, sats).
    pub fn record_block(&self, height: u32, hash: &str, addr: &str, mode: &str, reward: u64, status: &str, credits: Vec<(String, u64)>) {
        let (hash, addr, mode, status) = (hash.to_string(), addr.to_string(), mode.to_string(), status.to_string());
        self.run(move |c| {
            let res = (|| -> rusqlite::Result<()> {
                let t = c.transaction()?;
                t.execute("INSERT INTO blocks (ts, height, hash, addr, reward, result, mode, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![unix_now(), height, hash, addr, reward as i64, status, mode, if status == "accepted" { "maturing" } else { "orphaned" }])?;
                if status == "accepted" {
                    for (a, v) in &credits {
                        t.execute("INSERT INTO credits (height, hash, addr, amount) VALUES (?1, ?2, ?3, ?4)", params![height, hash, a, *v as i64])?;
                    }
                }
                t.commit()
            })();
            if let Err(e) = res {
                tracing::error!("ledger block write failed: {e}");
            }
        });
    }

    pub fn set_block_status(&self, hash: &str, status: &str) {
        let (hash, status) = (hash.to_string(), status.to_string());
        self.run(move |c| {
            let _ = c.execute("UPDATE blocks SET status = ?1 WHERE hash = ?2", params![status, hash]);
        });
    }

    pub fn add_history(&self, minute: i64, hashrate: f64, miners: u64, ok: u64, stale: u64, dup: u64, nd: f64, devices: Vec<(String, String, f64, u64)>) {
        self.run(move |c| {
            let _ = c.execute("INSERT OR REPLACE INTO pool_history (minute, hashrate, miners, ok, stale, dup, nd) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![minute, hashrate, miners as i64, ok as i64, stale as i64, dup as i64, nd]);
            for (k, n, hr, w) in devices {
                let _ = c.execute("INSERT OR REPLACE INTO device_history (minute, key, name, hashrate, workers) VALUES (?1, ?2, ?3, ?4, ?5)", params![minute, k, n, hr, w as i64]);
            }
            let old = minute - KEEP_DAYS * 1440;
            let _ = c.execute("DELETE FROM pool_history WHERE minute < ?1", params![old]);
            let _ = c.execute("DELETE FROM device_history WHERE minute < ?1", params![old]);
        });
    }

    fn read(&self) -> Option<Connection> {
        open(&self.path).ok()
    }

    pub fn blocks_page(&self, offset: i64, n: i64) -> Vec<BlockRow> {
        let Some(c) = self.read() else { return Vec::new() };
        let Ok(mut st) = c.prepare("SELECT ts, height, hash, addr, mode, status FROM blocks ORDER BY id DESC LIMIT ?1 OFFSET ?2") else { return Vec::new() };
        st.query_map(params![n, offset], |r| Ok(BlockRow { ts: r.get(0)?, height: r.get(1)?, hash: r.get(2)?, addr: r.get(3)?, mode: r.get(4)?, status: r.get(5)? })).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    pub fn account_blocks_page(&self, addr: &str, offset: i64, n: i64) -> (Vec<BlockRow>, i64) {
        let Some(c) = self.read() else { return (Vec::new(), 0) };
        let total = c.query_row("SELECT COUNT(*) FROM blocks WHERE addr = ?1", params![addr], |r| r.get(0)).unwrap_or(0);
        let Ok(mut st) = c.prepare("SELECT ts, height, hash, addr, mode, status FROM blocks WHERE addr = ?1 ORDER BY id DESC LIMIT ?2 OFFSET ?3") else { return (Vec::new(), total) };
        let rows = st.query_map(params![addr, n, offset], |r| Ok(BlockRow { ts: r.get(0)?, height: r.get(1)?, hash: r.get(2)?, addr: r.get(3)?, mode: r.get(4)?, status: r.get(5)? })).map(|rows| rows.flatten().collect()).unwrap_or_default();
        (rows, total)
    }

    pub fn block_count(&self) -> i64 {
        self.read().and_then(|c| c.query_row("SELECT COUNT(*) FROM blocks", [], |r| r.get(0)).ok()).unwrap_or(0)
    }

    pub fn blocks_by_status(&self) -> Vec<(String, i64)> {
        let Some(c) = self.read() else { return Vec::new() };
        let Ok(mut st) = c.prepare("SELECT status, COUNT(*) FROM blocks GROUP BY status") else { return Vec::new() };
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    pub fn blocks_since(&self, ts: i64) -> i64 {
        self.read().and_then(|c| c.query_row("SELECT COUNT(*) FROM blocks WHERE ts >= ?1 AND status != 'orphaned'", params![ts], |r| r.get(0)).ok()).unwrap_or(0)
    }

    /// Blocks whose maturity is still being watched: (height, hash).
    pub fn maturing_blocks(&self) -> Vec<(u32, String)> {
        let Some(c) = self.read() else { return Vec::new() };
        let Ok(mut st) = c.prepare("SELECT height, hash FROM blocks WHERE status = 'maturing'") else { return Vec::new() };
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    /// Coinbase payouts to an address: (height, hash, sats, block status), newest first, and the total count.
    pub fn account_credits_page(&self, addr: &str, offset: i64, n: i64) -> (Vec<(u32, String, i64, String)>, i64) {
        let Some(c) = self.read() else { return (Vec::new(), 0) };
        let total = c.query_row("SELECT COUNT(*) FROM credits WHERE addr = ?1", params![addr], |r| r.get(0)).unwrap_or(0);
        let Ok(mut st) = c.prepare("SELECT c.height, c.hash, c.amount, COALESCE(b.status, 'maturing') FROM credits c LEFT JOIN blocks b ON b.hash = c.hash WHERE c.addr = ?1 ORDER BY c.id DESC LIMIT ?2 OFFSET ?3") else { return (Vec::new(), total) };
        let rows = st.query_map(params![addr, n, offset], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map(|rows| rows.flatten().collect()).unwrap_or_default();
        (rows, total)
    }

    /// (maturing, received) sats of an address: credits of blocks that are not yet mature, and of mature ones.
    pub fn account_balance(&self, addr: &str) -> (i64, i64) {
        let Some(c) = self.read() else { return (0, 0) };
        let sum = |status: &str| c.query_row("SELECT COALESCE(SUM(c.amount), 0) FROM credits c JOIN blocks b ON b.hash = c.hash WHERE c.addr = ?1 AND b.status = ?2", params![addr, status], |r| r.get(0)).unwrap_or(0);
        (sum("maturing"), sum("mature"))
    }

    /// Summed difficulty and share count of an address since `since` (unix time), by bucket.
    pub fn account_shares(&self, addr: &str, since: i64) -> (f64, i64) {
        let Some(c) = self.read() else { return (0.0, 0) };
        c.query_row("SELECT COALESCE(SUM(diff), 0), COALESCE(SUM(n), 0) FROM buckets WHERE addr = ?1 AND start >= ?2", params![addr, since / BUCKET_SECS * BUCKET_SECS], |r| Ok((r.get(0)?, r.get(1)?))).unwrap_or((0.0, 0))
    }

    /// (bucket start, summed difficulty) of an address since `since`.
    pub fn account_history(&self, addr: &str, since: i64) -> Vec<(i64, f64)> {
        let Some(c) = self.read() else { return Vec::new() };
        let Ok(mut st) = c.prepare("SELECT start, diff FROM buckets WHERE addr = ?1 AND start >= ?2 ORDER BY start") else { return Vec::new() };
        st.query_map(params![addr, since / BUCKET_SECS * BUCKET_SECS], |r| Ok((r.get(0)?, r.get(1)?))).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    /// Pool history in slots of `slot` minutes: (minute, hashrate, miners, ok, stale, dup, network difficulty).
    pub fn history(&self, since_minute: i64, slot: i64) -> Vec<(i64, f64, i64, i64, i64, i64, f64)> {
        let Some(c) = self.read() else { return Vec::new() };
        let Ok(mut st) = c.prepare("SELECT (minute / ?2) * ?2, AVG(hashrate), MAX(miners), SUM(ok), SUM(stale), SUM(dup), AVG(nd) FROM pool_history WHERE minute >= ?1 GROUP BY 1 ORDER BY 1") else { return Vec::new() };
        st.query_map(params![since_minute, slot], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    /// Device history in slots of `slot` minutes: (minute, key, name, hashrate, workers).
    pub fn device_history(&self, since_minute: i64, slot: i64) -> Vec<(i64, String, String, f64, i64)> {
        let Some(c) = self.read() else { return Vec::new() };
        let Ok(mut st) = c.prepare("SELECT (minute / ?2) * ?2, key, MAX(name), AVG(hashrate), MAX(workers) FROM device_history WHERE minute >= ?1 GROUP BY 1, 2 ORDER BY 1") else { return Vec::new() };
        st.query_map(params![since_minute, slot], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    /// Per-address difficulty of the newest shares adding up to `limit`; buckets beyond that are dropped from memory and, with a margin, from the database.
    pub fn window(&self, limit: f64) -> Vec<(String, f64)> {
        let (out, prune_before) = self.state.lock().unwrap().window(limit);
        if let Some(before) = prune_before {
            if let Ok(c) = open(&self.path) {
                let _ = c.execute("DELETE FROM buckets WHERE start < ?1", params![before - KEEP_EXTRA_SECS]);
            }
        }
        out
    }

    /// Writes pending shares now (shutdown).
    pub fn flush_now(&self) {
        if let Ok(mut c) = open(&self.path) {
            flush(&mut c, &self.state);
        }
    }
}

impl State {
    fn add(&mut self, start: i64, addr: String, diff: f64) {
        if self.buckets.back().map_or(true, |b| b.start < start) {
            self.buckets.push_back(Bucket { start, sum: 0.0, per: HashMap::new() });
        }
        let b = self.buckets.iter_mut().rev().find(|b| b.start == start).unwrap_or_else(|| unreachable!("bucket order"));
        b.sum += diff;
        *b.per.entry(addr.clone()).or_insert(0.0) += diff;
        *self.totals.entry(addr).or_insert(0.0) += diff;
        self.total += diff;
    }

    fn window(&mut self, limit: f64) -> (Vec<(String, f64)>, Option<i64>) {
        let mut dropped = None;
        while self.buckets.len() > 1 && self.total - self.buckets[0].sum >= limit {
            let b = self.buckets.pop_front().unwrap();
            for (a, d) in &b.per {
                if let Some(t) = self.totals.get_mut(a) {
                    *t -= d;
                    if *t <= 1e-12 {
                        self.totals.remove(a);
                    }
                }
            }
            self.total -= b.sum;
            dropped = Some(b.start);
        }
        let mut out: HashMap<String, f64> = self.totals.clone();
        let excess = self.total - limit;
        if excess > 0.0 {
            if let Some(b) = self.buckets.front().filter(|b| b.sum > 0.0) {
                let f = (excess / b.sum).min(1.0);
                for (a, d) in &b.per {
                    if let Some(t) = out.get_mut(a) {
                        *t -= d * f;
                    }
                }
            }
        }
        (out.into_iter().filter(|(_, d)| *d > 0.0).collect(), dropped.and_then(|_| self.buckets.front().map(|b| b.start)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_cuts_at_limit_with_a_fractional_oldest_bucket() {
        let mut s = State::default();
        s.add(0, "a".into(), 10.0);
        s.add(600, "a".into(), 10.0);
        s.add(600, "b".into(), 10.0);
        s.add(1200, "b".into(), 10.0);
        let (w, _) = s.window(30.0);
        let w: HashMap<_, _> = w.into_iter().collect();
        assert!((w["a"] - 10.0).abs() < 1e-9);
        assert!((w.values().sum::<f64>() - 30.0).abs() < 1e-9);
        assert!((w["b"] - 20.0).abs() < 1e-9);
    }

    #[test]
    fn old_buckets_are_dropped_and_totals_stay_consistent() {
        let mut s = State::default();
        for i in 0..100 {
            s.add(i * 600, if i % 2 == 0 { "a".into() } else { "b".into() }, 1.0);
        }
        let (w, pruned) = s.window(10.0);
        assert_eq!(s.buckets.len(), 10);
        assert!(pruned.is_some());
        assert!((w.iter().map(|(_, d)| d).sum::<f64>() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn persists_and_reloads_buckets() {
        let path = std::env::temp_dir().join(format!("photond-ledger-{}.db", std::process::id()));
        let path = path.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&path);
        let l = Ledger::open(&path).unwrap();
        l.share("a", 2.0);
        l.share("b", 3.0);
        l.share("a", 1.0);
        l.flush_now();
        drop(l);
        let l2 = Ledger::open(&path).unwrap();
        let w: HashMap<_, _> = l2.window(1e9).into_iter().collect();
        assert_eq!((w["a"], w["b"]), (3.0, 3.0));
        let _ = std::fs::remove_file(&path);
    }
}
