use sysinfo::{Networks, System};
use std::time::Instant;

pub struct Metrics {
    pub sys: System,
    pub networks: Networks,
    pub cpu_pct: f64,
    pub mem_pct: f64,
    pub net_pct: f64,
    last_net_rx: u64,
    last_net_tx: u64,
    last_update: Instant,
    pub demo: bool,
    pub demo_start: Instant,
}

impl Metrics {
    pub fn new(demo: bool) -> Self {
        if demo {
            return Metrics {
                sys: System::new(),
                networks: Networks::new(),
                cpu_pct: 0.0,
                mem_pct: 0.0,
                net_pct: 0.0,
                last_net_rx: 0,
                last_net_tx: 0,
                last_update: Instant::now(),
                demo: true,
                demo_start: Instant::now(),
            };
        }

        let mut sys = System::new_all();
        sys.refresh_cpu_usage();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        let networks = Networks::new_with_refreshed_list();
        let (rx, tx) = total_net(&networks);

        let cpu_pct = avg_cpu(&sys);
        let mem_pct = mem_frac(&sys);

        Metrics {
            sys, networks, cpu_pct, mem_pct, net_pct: 0.0,
            last_net_rx: rx, last_net_tx: tx,
            last_update: Instant::now(), demo: false,
            demo_start: Instant::now(),
        }
    }

    pub fn refresh(&mut self) {
        if self.demo {
            const PERIOD: f64 = 60.0;
            let t = self.demo_start.elapsed().as_secs_f64() % PERIOD;
            let simulated = if t < 10.0 {
                0.0
            } else if t < 25.0 {
                (t - 10.0) / 15.0
            } else if t < 35.0 {
                1.0
            } else if t < 50.0 {
                1.0 - (t - 35.0) / 15.0
            } else {
                0.0
            };
            self.cpu_pct = (simulated * 1.2).clamp(0.0, 1.0);
            self.mem_pct = (simulated * 0.7).clamp(0.0, 1.0);
            self.net_pct = (simulated * 0.4).clamp(0.0, 1.0);
            return;
        }

        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.networks.refresh(true);

        self.cpu_pct = avg_cpu(&self.sys);
        self.mem_pct = mem_frac(&self.sys);

        let (rx, tx) = total_net(&self.networks);
        let net_bytes = (rx.saturating_sub(self.last_net_rx)
                       + tx.saturating_sub(self.last_net_tx)) as f64;
        self.last_net_rx = rx;
        self.last_net_tx = tx;

        let elapsed = self.last_update.elapsed().as_secs_f64().max(0.001);
        self.net_pct = (net_bytes / elapsed / 10_000_000.0).clamp(0.0, 1.0);
        self.last_update = Instant::now();
    }

    pub fn load(&self) -> f64 {
        (self.cpu_pct * 0.6 + self.mem_pct * 0.3 + self.net_pct * 0.1).clamp(0.0, 1.0)
    }

    pub fn tick_ms(&self, min_ms: u64, max_ms: u64) -> u64 {
        max_ms - (self.load() * (max_ms - min_ms) as f64) as u64
    }

    pub fn demo_phase(&self) -> &'static str {
        let t = self.demo_start.elapsed().as_secs_f64() % 60.0;
        if t < 10.0      { "DEMO:idle" }
        else if t < 25.0 { "DEMO:ramping up" }
        else if t < 35.0 { "DEMO:stressed" }
        else if t < 50.0 { "DEMO:cooling down" }
        else             { "DEMO:idle" }
    }
}

fn avg_cpu(sys: &System) -> f64 {
    let cpus = sys.cpus();
    if cpus.is_empty() { 0.0 }
    else { cpus.iter().map(|c| c.cpu_usage() as f64).sum::<f64>() / cpus.len() as f64 / 100.0 }
}

fn mem_frac(sys: &System) -> f64 {
    let total = sys.total_memory();
    if total == 0 { 0.0 } else { sys.used_memory() as f64 / total as f64 }
}

pub fn total_net(networks: &Networks) -> (u64, u64) {
    networks.iter().fold((0, 0), |(rx, tx), (_, d)| (rx + d.received(), tx + d.transmitted()))
}
