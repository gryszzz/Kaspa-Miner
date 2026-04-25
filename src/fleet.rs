use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
    pub telemetry: Option<AsicTelemetry>,
}

#[derive(Debug, Clone, Default)]
pub struct AsicTelemetry {
    pub hashrate_ths: Option<f64>,
    pub avg_hashrate_ths: Option<f64>,
    pub temp_c: Option<f64>,
    pub fan_rpm: Option<u64>,
    pub uptime_secs: Option<u64>,
    pub pool_url: Option<String>,
    pub accepted: Option<u64>,
    pub rejected: Option<u64>,
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
    let telemetry = match device.api_port {
        Some(port) => query_cgminer(&device.host, port, request_timeout).await,
        None => None,
    };
    let api_online = match device.api_port {
        Some(port) => {
            Some(telemetry.is_some() || port_open(&device.host, port, request_timeout).await)
        }
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
        telemetry,
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
    let live_ths: f64 = statuses.iter().filter_map(DeviceStatus::hashrate_ths).sum();
    let reachable_expected_ths: f64 = statuses
        .iter()
        .filter(|status| status.online())
        .map(DeviceStatus::expected_or_live_ths)
        .sum();
    let configured_ths: f64 = cfg
        .devices
        .iter()
        .filter(|device| device.enabled)
        .map(|device| device.expected_hashrate_ths)
        .sum();
    let accepted: u64 = statuses
        .iter()
        .filter_map(|status| {
            status
                .telemetry
                .as_ref()
                .and_then(|telemetry| telemetry.accepted)
        })
        .sum();
    let rejected: u64 = statuses
        .iter()
        .filter_map(|status| {
            status
                .telemetry
                .as_ref()
                .and_then(|telemetry| telemetry.rejected)
        })
        .sum();

    println!(
        "\nKASPilot Fleet | online {}/{} | live {:.2} TH/s | reachable {:.2}/{:.2} TH/s | shares A/R {}/{} | poll {}s",
        online,
        statuses.len(),
        live_ths.max(0.0),
        reachable_expected_ths.max(0.0),
        configured_ths,
        accepted,
        rejected,
        cfg.poll_secs.max(1)
    );
    println!(
        "{:<18} {:<16} {:<12} {:<5} {:<5} {:>9} {:>7} {:>7} {:>7} {:>8}  pool",
        "worker", "host", "model", "api", "web", "TH/s", "temp", "fan", "rej", "uptime"
    );
    println!("{}", "-".repeat(122));

    for status in statuses {
        println!(
            "{:<18} {:<16} {:<12} {:<5} {:<5} {:>9} {:>7} {:>7} {:>7} {:>8}  {}",
            truncate(&status.device.name, 18),
            truncate(&status.device.host, 16),
            truncate(model_or_unknown(&status.device), 12),
            port_status(status.api_online),
            port_status(status.web_online),
            fmt_f64(
                status
                    .hashrate_ths()
                    .or(Some(status.device.expected_hashrate_ths))
            ),
            fmt_temp(
                status
                    .telemetry
                    .as_ref()
                    .and_then(|telemetry| telemetry.temp_c)
            ),
            fmt_fan(
                status
                    .telemetry
                    .as_ref()
                    .and_then(|telemetry| telemetry.fan_rpm)
            ),
            fmt_u64(
                status
                    .telemetry
                    .as_ref()
                    .and_then(|telemetry| telemetry.rejected)
            ),
            fmt_uptime(
                status
                    .telemetry
                    .as_ref()
                    .and_then(|telemetry| telemetry.uptime_secs)
            ),
            truncate(
                status
                    .telemetry
                    .as_ref()
                    .and_then(|telemetry| telemetry.pool_url.as_deref())
                    .unwrap_or(&status.device.location),
                28,
            )
        );
    }
}

async fn query_cgminer(host: &str, port: u16, request_timeout: Duration) -> Option<AsicTelemetry> {
    let summary = cgminer_command(host, port, "summary", request_timeout).await;
    let pools = cgminer_command(host, port, "pools", request_timeout).await;
    let devs = cgminer_command(host, port, "devs", request_timeout).await;
    let stats = cgminer_command(host, port, "stats", request_timeout).await;

    if summary.is_none() && pools.is_none() && devs.is_none() && stats.is_none() {
        return None;
    }

    Some(normalize_cgminer(
        summary.as_ref(),
        pools.as_ref(),
        devs.as_ref(),
        stats.as_ref(),
    ))
}

async fn cgminer_command(
    host: &str,
    port: u16,
    command: &str,
    request_timeout: Duration,
) -> Option<Value> {
    let mut stream = timeout(request_timeout, TcpStream::connect((host, port)))
        .await
        .ok()?
        .ok()?;

    let request = format!(r#"{{"command":"{command}"}}"#);
    timeout(request_timeout, stream.write_all(request.as_bytes()))
        .await
        .ok()?
        .ok()?;

    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match timeout(Duration::from_millis(250), stream.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => buf.extend_from_slice(&chunk[..n]),
            Ok(Err(_)) | Err(_) => break,
        }
    }

    parse_cgminer_response(&buf)
}

fn parse_cgminer_response(raw: &[u8]) -> Option<Value> {
    let cleaned = String::from_utf8_lossy(raw)
        .trim_matches(char::from(0))
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return None;
    }

    serde_json::from_str(&cleaned).ok()
}

fn normalize_cgminer(
    summary: Option<&Value>,
    pools: Option<&Value>,
    devs: Option<&Value>,
    stats: Option<&Value>,
) -> AsicTelemetry {
    let summary_row = first_row(summary, "SUMMARY");
    let pool_rows = rows(pools, "POOLS");
    let dev_rows = rows(devs, "DEVS");
    let stat_rows = rows(stats, "STATS");

    let pool = pool_rows
        .iter()
        .find(|row| {
            string_field(row, &["Status"]).is_some_and(|value| value.eq_ignore_ascii_case("alive"))
        })
        .or_else(|| pool_rows.first());

    AsicTelemetry {
        hashrate_ths: summary_row
            .and_then(hashrate_5s)
            .or_else(|| summary_row.and_then(hashrate_avg)),
        avg_hashrate_ths: summary_row.and_then(hashrate_avg),
        temp_c: max_named_number(&dev_rows, &["temp", "temperature"])
            .or_else(|| max_named_number(&stat_rows, &["temp", "temperature"])),
        fan_rpm: max_named_number(&dev_rows, &["fan"])
            .or_else(|| max_named_number(&stat_rows, &["fan"]))
            .and_then(f64_to_u64),
        uptime_secs: summary_row
            .and_then(|row| number_field(row, &["Elapsed", "Uptime"]).map(|value| value as u64)),
        pool_url: pool
            .and_then(|row| string_field(row, &["URL", "Url", "Pool"]))
            .map(str::to_string),
        accepted: summary_row
            .and_then(|row| number_field(row, &["Accepted", "Difficulty Accepted"]))
            .or_else(|| pool.and_then(|row| number_field(row, &["Accepted", "DiffA"])))
            .map(|value| value as u64),
        rejected: summary_row
            .and_then(|row| number_field(row, &["Rejected", "Difficulty Rejected"]))
            .or_else(|| pool.and_then(|row| number_field(row, &["Rejected", "DiffR"])))
            .map(|value| value as u64),
    }
}

fn first_row<'a>(value: Option<&'a Value>, section: &str) -> Option<&'a Value> {
    rows(value, section).into_iter().next()
}

fn rows<'a>(value: Option<&'a Value>, section: &str) -> Vec<&'a Value> {
    value
        .and_then(|value| value.get(section))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().collect())
        .unwrap_or_default()
}

fn hashrate_5s(row: &Value) -> Option<f64> {
    hash_rate_field(
        row,
        &[
            ("THS 5s", 1.0),
            ("GHS 5s", 1.0 / 1_000.0),
            ("MHS 5s", 1.0 / 1_000_000.0),
            ("KHS 5s", 1.0 / 1_000_000_000.0),
            ("HS 5s", 1.0 / 1_000_000_000_000.0),
            ("GHS 1m", 1.0 / 1_000.0),
            ("MHS 1m", 1.0 / 1_000_000.0),
        ],
    )
}

fn hashrate_avg(row: &Value) -> Option<f64> {
    hash_rate_field(
        row,
        &[
            ("THS av", 1.0),
            ("GHS av", 1.0 / 1_000.0),
            ("MHS av", 1.0 / 1_000_000.0),
            ("KHS av", 1.0 / 1_000_000_000.0),
            ("HS av", 1.0 / 1_000_000_000_000.0),
        ],
    )
}

fn hash_rate_field(row: &Value, keys: &[(&str, f64)]) -> Option<f64> {
    keys.iter()
        .find_map(|(key, scale)| number_field(row, &[*key]).map(|value| value * scale))
}

fn number_field(row: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        let value = row.get(*key)?;
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
    })
}

fn string_field<'a>(row: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| row.get(*key).and_then(Value::as_str))
}

fn max_named_number(rows: &[&Value], needles: &[&str]) -> Option<f64> {
    rows.iter()
        .filter_map(|row| row.as_object())
        .flat_map(|object| object.iter())
        .filter(|(key, _)| {
            let key = key.to_ascii_lowercase();
            needles.iter().any(|needle| key.contains(needle))
        })
        .filter_map(|(_, value)| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
        })
        .filter(|value| *value > 0.0)
        .max_by(f64::total_cmp)
}

impl DeviceStatus {
    fn online(&self) -> bool {
        self.api_online.unwrap_or(false) || self.web_online.unwrap_or(false)
    }

    fn hashrate_ths(&self) -> Option<f64> {
        self.telemetry
            .as_ref()
            .and_then(|telemetry| telemetry.hashrate_ths.or(telemetry.avg_hashrate_ths))
    }

    fn expected_or_live_ths(&self) -> f64 {
        self.hashrate_ths()
            .unwrap_or(self.device.expected_hashrate_ths)
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

fn fmt_f64(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| "-".into())
}

fn fmt_temp(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.0}C"))
        .unwrap_or_else(|| "-".into())
}

fn fmt_fan(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".into())
}

fn fmt_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".into())
}

fn fmt_uptime(value: Option<u64>) -> String {
    let Some(seconds) = value else {
        return "-".into();
    };
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    if days > 0 {
        format!("{days}d{hours}h")
    } else {
        format!("{hours}h")
    }
}

fn f64_to_u64(value: f64) -> Option<u64> {
    if value.is_finite() && value >= 0.0 && value <= u64::MAX as f64 {
        Some(value.round() as u64)
    } else {
        None
    }
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
