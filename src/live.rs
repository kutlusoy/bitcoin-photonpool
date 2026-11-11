use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

const TAU: f64 = 300.0;
const HISTORY_POINTS: usize = 1440;

/// Hashrate estimate from accepted share difficulty with exponential decay (time constant 5 minutes).
pub struct Meter {
    v: f64,
    t: Instant,
    born: Instant,
}

impl Meter {
    pub fn new() -> Self {
        let now = Instant::now();
        Self { v: 0.0, t: now, born: now }
    }

    fn decayed(&self) -> f64 {
        self.v * (-self.t.elapsed().as_secs_f64() / TAU).exp()
    }

    pub fn add(&mut self, diff: f64) {
        self.v = self.decayed() + diff;
        self.t = Instant::now();
    }

    pub fn hashrate(&self) -> f64 {
        let age = self.born.elapsed().as_secs_f64().max(10.0);
        self.decayed() / (TAU * (1.0 - (-age / TAU).exp())) * 4_294_967_296.0
    }
}

pub struct Worker {
    pub meter: Meter,
    pub accepted: u64,
    pub rejected: u64,
    pub last_share: u64,
    pub agent: String,
    pub conns: u32,
}

pub struct Addr {
    pub meter: Meter,
    pub accepted: u64,
    pub rejected: u64,
    pub last_share: u64,
    pub workers: HashMap<String, Worker>,
}

pub struct Point {
    pub t: u64,
    pub hashrate: f64,
    pub conns: u64,
}

pub struct Event {
    pub t: u64,
    pub level: &'static str,
    pub msg: String,
}

pub struct Live {
    pub pool: Mutex<Meter>,
    pub addrs: Mutex<HashMap<String, Addr>>,
    pub history: Mutex<VecDeque<Point>>,
    pub events: Mutex<VecDeque<Event>>,
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Live {
    pub fn new() -> Self {
        Self { pool: Mutex::new(Meter::new()), addrs: Mutex::new(HashMap::new()), history: Mutex::new(VecDeque::new()), events: Mutex::new(VecDeque::new()) }
    }

    fn entry<'a>(m: &'a mut HashMap<String, Addr>, addr: &str, worker: &str) -> (&'a mut Addr, String) {
        let a = m.entry(addr.to_string()).or_insert_with(|| Addr { meter: Meter::new(), accepted: 0, rejected: 0, last_share: 0, workers: HashMap::new() });
        (a, worker.to_string())
    }

    pub fn connect(&self, addr: &str, worker: &str, agent: &str) {
        let mut m = self.addrs.lock().unwrap();
        let (a, w) = Self::entry(&mut m, addr, worker);
        let ws = a.workers.entry(w).or_insert_with(|| Worker { meter: Meter::new(), accepted: 0, rejected: 0, last_share: 0, agent: String::new(), conns: 0 });
        ws.conns += 1;
        ws.agent = agent.chars().take(48).collect();
    }

    pub fn disconnect(&self, addr: &str, worker: &str) {
        if let Some(w) = self.addrs.lock().unwrap().get_mut(addr).and_then(|a| a.workers.get_mut(worker)) {
            w.conns = w.conns.saturating_sub(1);
        }
    }

    pub fn share(&self, addr: &str, worker: &str, diff: f64) {
        self.pool.lock().unwrap().add(diff);
        let now = unix_now();
        let mut m = self.addrs.lock().unwrap();
        if let Some(a) = m.get_mut(addr) {
            a.meter.add(diff);
            a.accepted += 1;
            a.last_share = now;
            if let Some(w) = a.workers.get_mut(worker) {
                w.meter.add(diff);
                w.accepted += 1;
                w.last_share = now;
            }
        }
    }

    pub fn reject(&self, addr: &str, worker: &str) {
        if let Some(a) = self.addrs.lock().unwrap().get_mut(addr) {
            a.rejected += 1;
            if let Some(w) = a.workers.get_mut(worker) {
                w.rejected += 1;
            }
        }
    }

    pub fn event(&self, level: &'static str, msg: String) {
        let mut q = self.events.lock().unwrap();
        q.push_back(Event { t: unix_now(), level, msg });
        if q.len() > 100 {
            q.pop_front();
        }
    }

    /// Records a history point and forgets addresses that have been gone for hours.
    pub fn sample(&self, conns: u64) {
        let hashrate = self.pool.lock().unwrap().hashrate();
        let mut h = self.history.lock().unwrap();
        h.push_back(Point { t: unix_now(), hashrate, conns });
        if h.len() > HISTORY_POINTS {
            h.pop_front();
        }
        let cutoff = unix_now().saturating_sub(6 * 3600);
        self.addrs.lock().unwrap().retain(|_, a| a.workers.values().any(|w| w.conns > 0) || a.last_share >= cutoff);
    }

    pub fn workers_online(&self) -> u64 {
        self.addrs.lock().unwrap().values().map(|a| a.workers.values().filter(|w| w.conns > 0).count() as u64).sum()
    }

    /// Addresses with at least one connected worker, best first: (address, hashrate, online workers, accepted shares).
    pub fn top(&self, n: usize) -> Vec<(String, f64, u32, u64)> {
        let m = self.addrs.lock().unwrap();
        let mut v: Vec<(String, f64, u32, u64)> = m
            .iter()
            .filter(|(_, a)| a.workers.values().any(|w| w.conns > 0))
            .map(|(k, a)| (k.clone(), a.meter.hashrate(), a.workers.values().filter(|w| w.conns > 0).count() as u32, a.accepted))
            .collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(n);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_estimates_the_rate_of_a_steady_stream() {
        let mut m = Meter::new();
        m.born = Instant::now() - std::time::Duration::from_secs(3000);
        m.t = Instant::now() - std::time::Duration::from_secs(3000);
        let steady = 1000.0 * TAU;
        m.v = steady;
        let h = m.hashrate();
        assert!(h < steady * 4_294_967_296.0 / TAU * 1e-3, "decayed after a long pause: {h}");
        let mut fresh = Meter::new();
        for _ in 0..10 {
            fresh.add(100.0);
        }
        assert!(fresh.hashrate() > 0.0);
    }

    #[test]
    fn shares_and_connections_are_tracked_per_worker() {
        let l = Live::new();
        l.connect("a", "w1", "bitaxe");
        l.connect("a", "w2", "x");
        l.share("a", "w1", 5.0);
        l.reject("a", "w2");
        assert_eq!(l.workers_online(), 2);
        l.disconnect("a", "w2");
        assert_eq!(l.workers_online(), 1);
        let top = l.top(5);
        assert_eq!((top[0].0.as_str(), top[0].2, top[0].3), ("a", 1, 1));
    }
}
