use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};

enum Msg {
    Share { ts: i64, addr: String, worker: String, diff: f64 },
    Block { ts: i64, height: u32, hash: String, addr: String, reward: u64, result: String },
}

/// Share and block log in SQLite. Writes go through a dedicated thread that batches them, so a share costs the connection tasks one channel send.
#[derive(Clone)]
pub struct Ledger {
    tx: Sender<Msg>,
    path: String,
}

fn open(path: &str) -> Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.pragma_update(None, "journal_mode", "WAL")?;
    c.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(c)
}

fn writer(rx: Receiver<Msg>, mut c: Connection) {
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while batch.len() < 2000 {
            match rx.try_recv() {
                Ok(m) => batch.push(m),
                Err(_) => break,
            }
        }
        let res = (|| -> rusqlite::Result<()> {
            let t = c.transaction()?;
            for m in &batch {
                match m {
                    Msg::Share { ts, addr, worker, diff } => {
                        t.execute("INSERT INTO shares (ts, addr, worker, diff) VALUES (?1, ?2, ?3, ?4)", params![ts, addr, worker, diff])?;
                    }
                    Msg::Block { ts, height, hash, addr, reward, result } => {
                        t.execute("INSERT INTO blocks (ts, height, hash, addr, reward, result) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![ts, height, hash, addr, *reward as i64, result])?;
                    }
                }
            }
            t.commit()
        })();
        if let Err(e) = res {
            tracing::error!("ledger write failed: {e}");
        }
    }
}

impl Ledger {
    pub fn open(path: &str) -> Result<Self> {
        let c = open(path)?;
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS shares (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, addr TEXT NOT NULL, worker TEXT NOT NULL, diff REAL NOT NULL);
             CREATE TABLE IF NOT EXISTS blocks (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, height INTEGER NOT NULL, hash TEXT NOT NULL, addr TEXT NOT NULL, reward INTEGER NOT NULL, result TEXT NOT NULL);",
        )?;
        let (tx, rx) = channel();
        std::thread::Builder::new().name("ledger".into()).spawn(move || writer(rx, c))?;
        Ok(Self { tx, path: path.to_string() })
    }

    pub fn share(&self, addr: &str, worker: &str, diff: f64) {
        let _ = self.tx.send(Msg::Share { ts: unix_now(), addr: addr.into(), worker: worker.into(), diff });
    }

    pub fn block(&self, height: u32, hash: &str, addr: &str, reward: u64, result: &str) {
        let _ = self.tx.send(Msg::Block { ts: unix_now(), height, hash: hash.into(), addr: addr.into(), reward, result: result.into() });
    }

    /// Difficulty per address over the newest shares adding up to `limit`, and the oldest share id worth keeping (twice the limit); older shares are deleted.
    pub fn window(&self, limit: f64) -> Result<Vec<(String, f64)>> {
        let c = open(&self.path)?;
        let mut st = c.prepare("SELECT id, addr, diff FROM shares ORDER BY id DESC")?;
        let mut rows = st.query([])?;
        let (mut sum, mut agg, mut keep_from): (f64, HashMap<String, f64>, Option<i64>) = (0.0, HashMap::new(), None);
        while let Some(r) = rows.next()? {
            let (id, addr, diff): (i64, String, f64) = (r.get(0)?, r.get(1)?, r.get(2)?);
            if sum < limit {
                *agg.entry(addr).or_insert(0.0) += diff;
            }
            sum += diff;
            if sum >= 2.0 * limit {
                keep_from = Some(id);
                break;
            }
        }
        drop(rows);
        drop(st);
        if let Some(id) = keep_from {
            c.execute("DELETE FROM shares WHERE id < ?1", params![id])?;
        }
        Ok(agg.into_iter().collect())
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_aggregates_newest_shares_and_prunes_old_ones() {
        let dir = std::env::temp_dir().join(format!("photond-ledger-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let path = dir.to_str().unwrap().to_string();
        let l = Ledger::open(&path).unwrap();
        for i in 0..100 {
            l.share(if i % 2 == 0 { "a" } else { "b" }, "w", 1.0);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        let w: HashMap<_, _> = l.window(10.0).unwrap().into_iter().collect();
        assert_eq!(w.values().sum::<f64>(), 10.0);
        let left: i64 = open(&path).unwrap().query_row("SELECT COUNT(*) FROM shares", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 20);
        let _ = std::fs::remove_file(&path);
    }
}
