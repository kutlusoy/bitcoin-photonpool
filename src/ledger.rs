use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
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
    /// Not yet written: (bucket start, address) -> difficulty.
    pending: HashMap<(i64, String), f64>,
}

enum Msg {
    Block { ts: i64, height: u32, hash: String, addr: String, reward: u64, result: String },
}

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
            let mut st = t.prepare_cached("INSERT INTO buckets (start, addr, diff) VALUES (?1, ?2, ?3) ON CONFLICT(start, addr) DO UPDATE SET diff = diff + excluded.diff")?;
            for ((start, addr), d) in &pending {
                st.execute(params![start, addr, d])?;
            }
        }
        t.commit()
    })();
    if let Err(e) = res {
        tracing::error!("ledger flush failed: {e}");
        let mut s = state.lock().unwrap();
        for (k, v) in pending {
            *s.pending.entry(k).or_insert(0.0) += v;
        }
    }
}

fn writer(rx: std::sync::mpsc::Receiver<Msg>, mut c: Connection, state: Arc<Mutex<State>>) {
    let mut last = Instant::now();
    loop {
        match rx.recv_timeout(FLUSH_EVERY) {
            Ok(Msg::Block { ts, height, hash, addr, reward, result }) => {
                if let Err(e) = c.execute("INSERT INTO blocks (ts, height, hash, addr, reward, result) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![ts, height, hash, addr, reward as i64, result]) {
                    tracing::error!("ledger block write failed: {e}");
                }
            }
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
            "CREATE TABLE IF NOT EXISTS buckets (start INTEGER NOT NULL, addr TEXT NOT NULL, diff REAL NOT NULL, PRIMARY KEY (start, addr)) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS blocks (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, height INTEGER NOT NULL, hash TEXT NOT NULL, addr TEXT NOT NULL, reward INTEGER NOT NULL, result TEXT NOT NULL);",
        )?;
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
        *s.pending.entry((start, addr.to_string())).or_insert(0.0) += diff;
        s.add(start, addr.to_string(), diff);
    }

    pub fn block(&self, height: u32, hash: &str, addr: &str, reward: u64, result: &str) {
        let _ = self.tx.send(Msg::Block { ts: unix_now(), height, hash: hash.into(), addr: addr.into(), reward, result: result.into() });
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
