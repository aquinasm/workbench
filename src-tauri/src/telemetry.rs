use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, Networks, RefreshKind, System};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStats {
    pub cpu_percent: f32,
    pub cpu_cores: usize,
    pub cpu_brand: String,
    pub load_avg_one: f64,
    pub load_avg_five: f64,
    pub load_avg_fifteen: f64,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub memory_percent: f32,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub net_rx_bytes_per_sec: u64,
    pub net_tx_bytes_per_sec: u64,
    pub uptime_secs: u64,
    pub os_name: String,
}

pub struct TelemetryState {
    sys: System,
    networks: Networks,
    last_net_sample: Instant,
    os_name: String,
    cpu_brand: String,
}

pub type SharedTelemetry = Arc<Mutex<TelemetryState>>;

impl Default for TelemetryState {
    fn default() -> Self {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        let networks = Networks::new_with_refreshed_list();
        let cpu_brand = sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "CPU".to_string());

        let os_name = match (System::name(), System::os_version()) {
            (Some(n), Some(v)) => format!("{} {}", n, v),
            (Some(n), None) => n,
            _ => std::env::consts::OS.to_string(),
        };

        Self {
            sys,
            networks,
            last_net_sample: Instant::now(),
            os_name,
            cpu_brand,
        }
    }
}

impl TelemetryState {
    pub fn sample(&mut self) -> SystemStats {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.networks.refresh(true);

        let now = Instant::now();
        let elapsed_secs = now
            .duration_since(self.last_net_sample)
            .as_secs_f64()
            .max(0.2);
        self.last_net_sample = now;

        let cpu_percent = self.sys.global_cpu_usage().clamp(0.0, 100.0);
        let cpu_cores = self.sys.cpus().len();
        let load = System::load_average();

        let memory_total_bytes = self.sys.total_memory();
        let memory_used_bytes = self.sys.used_memory();
        let memory_percent = if memory_total_bytes > 0 {
            ((memory_used_bytes as f64 / memory_total_bytes as f64) * 100.0) as f32
        } else {
            0.0
        };

        let swap_total_bytes = self.sys.total_swap();
        let swap_used_bytes = self.sys.used_swap();

        let mut rx_delta: u64 = 0;
        let mut tx_delta: u64 = 0;
        for (iface, data) in self.networks.iter() {
            let lower = iface.to_ascii_lowercase();
            // Skip loopback interfaces so local IPC doesn't skew external network graph
            if lower == "lo" || lower == "lo0" || lower.starts_with("loopback") {
                continue;
            }
            rx_delta = rx_delta.saturating_add(data.received());
            tx_delta = tx_delta.saturating_add(data.transmitted());
        }

        let net_rx_bytes_per_sec = (rx_delta as f64 / elapsed_secs) as u64;
        let net_tx_bytes_per_sec = (tx_delta as f64 / elapsed_secs) as u64;

        SystemStats {
            cpu_percent,
            cpu_cores,
            cpu_brand: self.cpu_brand.clone(),
            load_avg_one: load.one,
            load_avg_five: load.five,
            load_avg_fifteen: load.fifteen,
            memory_used_bytes,
            memory_total_bytes,
            memory_percent,
            swap_used_bytes,
            swap_total_bytes,
            net_rx_bytes_per_sec,
            net_tx_bytes_per_sec,
            uptime_secs: System::uptime(),
            os_name: self.os_name.clone(),
        }
    }
}
