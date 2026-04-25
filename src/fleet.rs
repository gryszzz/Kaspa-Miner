use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::time::timeout;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetConfig {
    #[serde(default = "default_poll_secs")]
    pub poll_secs: u64,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub devices: Vec<AsicDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsicDevice {
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub expected_hashrate_ths: f64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub api_port: Option<u16>,
    #[serde(default)]
    pub web_port: Option<u16>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub device: AsicDevice,
    pub api_online: Option<bool>,
    pub web_online: Option<bool>,
}

impl FleetConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Reading fleet config {}", path.display()))?;
        let cfg = toml::from_str::<FleetConfig>(&raw).context("Parsing fleet config")?;
        if cfg.devices.is_empty() {
            anyhow::bail!("Fleet config has no devices");
        }
        if cfg.timeout_ms == 0 {
            anyhow::bail!("Fleet timeout_ms must be at least 1");
        }
        Ok(cfg)
    }
}

pub async fn run(path: &Path, once: bool) -> Result<()> {
    let cfg = FleetConfig::load(path)?;
    loop {
        let statuses = poll_all(&cfg).await;
        print_report(&cfg, &statuses);

        if once {
            return Ok(());
        }

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("Fleet controller stopped.");
                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_secs(cfg.poll_secs.max(1))) => {}
        }
    }
}

async fn poll_all(cfg: &FleetConfig) -> Vec<DeviceStatus> {
    let mut handles = Vec::new();
    for device in cfg.devices.iter().filter(|device| device.enabled).cloned() {
        let timeout_ms = cfg.timeout_ms;
        handles.push(tokio::spawn(async move {
            poll_device(device, Duration::from_millis(timeout_ms)).await
        }));
    }

    let mut statuses = Vec::with_capacity(handles.len());
    for handle in handles {
        if let Ok(status) = handle.await {
            statuses.push(status);
        }
    }
    statuses
}

async fn poll_device(device: AsicDevice, request_timeout: Duration) -> DeviceStatus {
    let api_online = match device.api_port {
        Some(port) => Some(port_open(&device.host, port, request_timeout).await),
        None => None,
    };
    let web_online = match device.web_port {
        Some(port) => Some(port_open(&device.host, port, request_timeout).await),
        None => None,
    };

    DeviceStatus {
        device,
        api_online,
        web_online,
    }
}

async fn port_open(host: &str, port: u16, request_timeout: Duration) -> bool {
    timeout(request_timeout, TcpStream::connect((host, port)))
        .await
        .map(|result| result.is_ok())
        .unwrap_or(false)
}

fn print_report(cfg: &FleetConfig, statuses: &[DeviceStatus]) {
    let online = statuses.iter().filter(|status| status.online()).count();
    let reachable_ths: f64 = statuses
        .iter()
        .filter(|status| status.online())
        .map(|status| status.device.expected_hashrate_ths)
        .sum();
    let configured_ths: f64 = cfg
        .devices
        .iter()
        .filter(|device| device.enabled)
        .map(|device| device.expected_hashrate_ths)
        .sum();

    println!(
        "\nKASPilot Fleet | online {}/{} | reachable {:.2}/{:.2} TH/s | poll {}s",
        online,
        statuses.len(),
        reachable_ths.max(0.0),
        configured_ths,
        cfg.poll_secs.max(1)
    );
    println!(
        "{:<18} {:<16} {:<14} {:<8} {:<8} {:>10}  location",
        "worker", "host", "model", "api", "web", "exp TH/s"
    );
    println!("{}", "-".repeat(94));

    for status in statuses {
        println!(
            "{:<18} {:<16} {:<14} {:<8} {:<8} {:>10.2}  {}",
            truncate(&status.device.name, 18),
            truncate(&status.device.host, 16),
            truncate(model_or_unknown(&status.device), 14),
            port_status(status.api_online),
            port_status(status.web_online),
            status.device.expected_hashrate_ths,
            status.device.location
        );
    }
}

impl DeviceStatus {
    fn online(&self) -> bool {
        self.api_online.unwrap_or(false) || self.web_online.unwrap_or(false)
    }
}

fn port_status(status: Option<bool>) -> &'static str {
    match status {
        Some(true) => "up",
        Some(false) => "down",
        None => "-",
    }
}

fn model_or_unknown(device: &AsicDevice) -> &str {
    if device.model.is_empty() {
        "unknown"
    } else {
        &device.model
    }
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }

    value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "."
}

fn default_poll_secs() -> u64 {
    30
}

fn default_timeout_ms() -> u64 {
    750
}

fn default_enabled() -> bool {
    true
}
