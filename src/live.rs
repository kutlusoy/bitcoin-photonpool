use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

const HISTORY_POINTS: usize = 60;
const EVENTS_KEPT: usize = 200;
const LATENCY_SAMPLES: usize = 200;

/// Hashrate estimate from accepted share difficulty with exponential decay of time constant `tau` seconds.
pub struct Meter {
    v: f64,
    t: Instant,
    born: Instant,
    tau: f64,
}

impl Meter {
    pub fn new(tau: f64) -> Self {
        let now = Instant::now();
        Self { v: 0.0, t: now, born: now, tau }
    }

    fn decayed(&self) -> f64 {
        self.v * (-self.t.elapsed().as_secs_f64() / self.tau).exp()
    }

    pub fn add(&mut self, diff: f64) {
        self.v = self.decayed() + diff;
        self.t = Instant::now();
    }

    pub fn hashrate(&self) -> f64 {
        let age = self.born.elapsed().as_secs_f64().max(10.0);
        self.decayed() / (self.tau * (1.0 - (-age / self.tau).exp())) * 4_294_967_296.0
    }
}

pub struct Worker {
    pub meter: Meter,
    pub accepted: u64,
    pub stale: u64,
    pub invalid: u64,
    pub last_seen: u64,
    pub agent: String,
    pub conns: u32,
    pub diff: f64,
    pub best: f64,
}

pub struct Addr {
    pub meter: Meter,
    pub last_share: u64,
    pub workers: HashMap<String, Worker>,
}

pub struct Event {
    pub seq: u64,
    pub t: u64,
    pub level: &'static str,
    pub msg: String,
}

#[derive(Default, Clone, Copy)]
pub struct Rates {
    pub shares_per_sec: f64,
    pub jobs_per_sec: f64,
    pub connects_per_sec: f64,
    pub in_bps: f64,
    pub out_bps: f64,
}

#[derive(Default)]
struct Last {
    counters: [u64; 5],
    at: Option<Instant>,
}

pub struct Live {
    pub pool: Mutex<Meter>,
    pub m1: Mutex<Meter>,
    pub h1: Mutex<Meter>,
    pub addrs: Mutex<HashMap<String, Addr>>,
    pub hist: Mutex<VecDeque<f64>>,
    pub events: Mutex<VecDeque<Event>>,
    pub rates: Mutex<Rates>,
    pub tip_ready_ms: Mutex<VecDeque<f64>>,
    pub device_best: Mutex<HashMap<String, f64>>,
    seq: AtomicU64,
    last: Mutex<Last>,
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// One online device group: key, working connections, hashrate, best share.
pub struct DeviceRow {
    pub key: String,
    pub name: String,
    pub url: Option<&'static str>,
    pub open_source: bool,
    pub working: u64,
    pub hashrate: f64,
    pub best: f64,
}

impl Live {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(Meter::new(300.0)),
            m1: Mutex::new(Meter::new(60.0)),
            h1: Mutex::new(Meter::new(3600.0)),
            addrs: Mutex::new(HashMap::new()),
            hist: Mutex::new(VecDeque::new()),
            events: Mutex::new(VecDeque::new()),
            rates: Mutex::new(Rates::default()),
            tip_ready_ms: Mutex::new(VecDeque::new()),
            device_best: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(0),
            last: Mutex::new(Last::default()),
        }
    }

    pub fn connect(&self, addr: &str, worker: &str, agent: &str) {
        let mut m = self.addrs.lock().unwrap();
        let a = m.entry(addr.to_string()).or_insert_with(|| Addr { meter: Meter::new(300.0), last_share: 0, workers: HashMap::new() });
        let w = a.workers.entry(worker.to_string()).or_insert_with(|| Worker { meter: Meter::new(300.0), accepted: 0, stale: 0, invalid: 0, last_seen: 0, agent: String::new(), conns: 0, diff: 0.0, best: 0.0 });
        w.conns += 1;
        w.last_seen = unix_now();
        w.agent = crate::devices::clean_agent(agent);
    }

    pub fn disconnect(&self, addr: &str, worker: &str) {
        if let Some(w) = self.addrs.lock().unwrap().get_mut(addr).and_then(|a| a.workers.get_mut(worker)) {
            w.conns = w.conns.saturating_sub(1);
        }
    }

    /// An accepted share credited with `diff`; `actual` is the difficulty the hash really reached.
    pub fn share(&self, addr: &str, worker: &str, diff: f64, actual: f64) {
        self.pool.lock().unwrap().add(diff);
        self.m1.lock().unwrap().add(diff);
        self.h1.lock().unwrap().add(diff);
        let now = unix_now();
        let mut agent = None;
        if let Some(a) = self.addrs.lock().unwrap().get_mut(addr) {
            a.meter.add(diff);
            a.last_share = now;
            if let Some(w) = a.workers.get_mut(worker) {
                w.meter.add(diff);
                w.accepted += 1;
                w.last_seen = now;
                w.diff = diff;
                if actual > w.best {
                    w.best = actual;
                    agent = Some(w.agent.clone());
                }
            }
        }
        if let Some(agent) = agent {
            let key = crate::devices::classify(&agent).key;
            let mut b = self.device_best.lock().unwrap();
            let e = b.entry(key).or_insert(0.0);
            if actual > *e {
                *e = actual;
            }
        }
    }

    pub fn reject(&self, addr: &str, worker: &str, stale: bool) {
        if let Some(w) = self.addrs.lock().unwrap().get_mut(addr).and_then(|a| a.workers.get_mut(worker)) {
            if stale {
                w.stale += 1;
            } else {
                w.invalid += 1;
            }
            w.last_seen = unix_now();
        }
    }

    pub fn event(&self, level: &'static str, msg: String) {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let mut q = self.events.lock().unwrap();
        q.push_back(Event { seq, t: unix_now(), level, msg });
        if q.len() > EVENTS_KEPT {
            q.pop_front();
        }
    }

    pub fn tip_ready(&self, ms: f64) {
        let mut q = self.tip_ready_ms.lock().unwrap();
        q.push_back(ms);
        if q.len() > LATENCY_SAMPLES {
            q.pop_front();
        }
    }

    /// Per-second rates from the pool counters: shares, jobs, connects, bytes in, bytes out.
    pub fn tick(&self, counters: [u64; 5]) {
        let mut l = self.last.lock().unwrap();
        if let Some(at) = l.at {
            let dt = at.elapsed().as_secs_f64().max(0.001);
            let d = |i: usize| counters[i].saturating_sub(l.counters[i]) as f64 / dt;
            *self.rates.lock().unwrap() = Rates { shares_per_sec: d(0), jobs_per_sec: d(1), connects_per_sec: d(2), in_bps: d(3), out_bps: d(4) };
        }
        l.counters = counters;
        l.at = Some(Instant::now());
    }

    /// Once a minute: appends the hashrate to the short history of the pool view and forgets addresses that have been gone for hours.
    pub fn sample(&self) -> f64 {
        let hashrate = self.pool.lock().unwrap().hashrate();
        let mut h = self.hist.lock().unwrap();
        h.push_back(hashrate);
        if h.len() > HISTORY_POINTS {
            h.pop_front();
        }
        let cutoff = unix_now().saturating_sub(6 * 3600);
        self.addrs.lock().unwrap().retain(|_, a| a.workers.values().any(|w| w.conns > 0) || a.last_share >= cutoff);
        hashrate
    }

    pub fn workers_online(&self) -> u64 {
        self.addrs.lock().unwrap().values().map(|a| a.workers.values().filter(|w| w.conns > 0).count() as u64).sum()
    }

    /// Online devices grouped by the type they report in their user agent.
    pub fn devices(&self) -> Vec<DeviceRow> {
        let m = self.addrs.lock().unwrap();
        let mut groups: HashMap<String, DeviceRow> = HashMap::new();
        for w in m.values().flat_map(|a| a.workers.values()).filter(|w| w.conns > 0) {
            let d = crate::devices::classify(&w.agent);
            let e = groups.entry(d.key.clone()).or_insert(DeviceRow { key: d.key, name: d.name, url: d.url, open_source: d.open_source, working: 0, hashrate: 0.0, best: 0.0 });
            e.working += w.conns as u64;
            e.hashrate += w.meter.hashrate();
            e.best = e.best.max(w.best);
        }
        let bests = self.device_best.lock().unwrap();
        let mut v: Vec<DeviceRow> = groups.into_values().collect();
        for r in v.iter_mut() {
            r.best = r.best.max(bests.get(&r.key).copied().unwrap_or(0.0));
        }
        v.sort_by(|a, b| b.hashrate.partial_cmp(&a.hashrate).unwrap_or(std::cmp::Ordering::Equal));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_decays_and_starts_calibrated() {
        let mut fresh = Meter::new(300.0);
        for _ in 0..10 {
            fresh.add(100.0);
        }
        assert!(fresh.hashrate() > 0.0);
        let mut old = Meter::new(300.0);
        old.born = Instant::now() - std::time::Duration::from_secs(3000);
        old.t = Instant::now() - std::time::Duration::from_secs(3000);
        old.v = 1000.0;
        assert!(old.hashrate() < 1e-3 * (1000.0 / 300.0 * 4_294_967_296.0), "decayed after a long pause");
    }

    #[test]
    fn shares_and_connections_are_tracked_per_worker_and_device() {
        let l = Live::new();
        l.connect("a", "w1", "bitaxe/BM1370/v2");
        l.connect("a", "w2", "cpuminer/2.5");
        l.share("a", "w1", 5.0, 40.0);
        l.reject("a", "w2", false);
        assert_eq!(l.workers_online(), 2);
        l.disconnect("a", "w2");
        assert_eq!(l.workers_online(), 1);
        let d = l.devices();
        assert_eq!((d[0].name.as_str(), d[0].working, d[0].best), ("Bitaxe", 1, 40.0));
    }

    #[test]
    fn rates_come_from_counter_deltas() {
        let l = Live::new();
        l.tick([0; 5]);
        std::thread::sleep(std::time::Duration::from_millis(50));
        l.tick([10, 20, 1, 1000, 2000]);
        let r = *l.rates.lock().unwrap();
        assert!(r.shares_per_sec > 50.0 && r.in_bps > 5000.0);
    }
}
