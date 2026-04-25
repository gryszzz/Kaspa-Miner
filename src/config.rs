use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub pool:    String,
    pub wallet:  String,
    pub worker:  String,
    pub threads: usize,
}

impl Config {
    pub fn load(path: &Path, pool: Option<String>, wallet: Option<String>, worker: String, threads: Option<usize>) -> Result<Self> {
        let mut cfg = if path.exists() {
            let raw = std::fs::read_to_string(path)
                .with_context(|| format!("Reading {}", path.display()))?;
            toml::from_str::<Config>(&raw).context("Parsing config.toml")?
        } else {
            Config::default()
        };

        if let Some(p) = pool    { cfg.pool   = p; }
        if let Some(w) = wallet  { cfg.wallet = w; }
        cfg.worker  = worker;
        if let Some(t) = threads { cfg.threads = t; }

        if cfg.pool.is_empty() {
            anyhow::bail!(
                "Pool required.  Use --pool stratum+tcp://host:port  or set pool in config.toml"
            );
        }
        if cfg.wallet.is_empty() {
            anyhow::bail!("Wallet required.  Use --wallet <kaspa:...>  or set wallet in config.toml");
        }

        Ok(cfg)
    }

    /// Parse "stratum+tcp://host:port" → (host, port).
    pub fn pool_host_port(&self) -> Result<(String, u16)> {
        let url = self.pool
            .trim_start_matches("stratum+tcp://")
            .trim_start_matches("stratum://")
            .trim_start_matches("tcp://");

        let (host, port_str) = url.rsplit_once(':')
            .ok_or_else(|| anyhow::anyhow!("Pool URL missing port: {}", self.pool))?;

        let port = port_str.parse::<u16>()
            .with_context(|| format!("Invalid port: {port_str}"))?;

        Ok((host.to_string(), port))
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pool:    String::new(),
            wallet:  String::new(),
            worker:  "worker1".into(),
            threads: num_cpus::get(),
        }
    }
}
