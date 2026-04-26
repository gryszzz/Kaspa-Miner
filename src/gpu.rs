use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;

use crate::config::Config;

#[derive(Debug, Clone, Deserialize)]
pub struct GpuConfig {
    /// External optimized GPU miner executable.
    pub command: String,
    /// Command-line arguments. Supports {pool}, {wallet}, {worker}, {login}, {devices}.
    #[serde(default)]
    pub args: Vec<String>,
    /// Device selector substituted into {devices}.
    #[serde(default)]
    pub devices: String,
    /// Environment variables for the child process. Values support the same placeholders as args.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Restart the GPU engine when it exits.
    #[serde(default = "default_restart")]
    pub restart: bool,
    /// Delay before restarting a failed GPU engine.
    #[serde(default = "default_restart_delay_secs")]
    pub restart_delay_secs: u64,
    /// Optional restart cap. Omit for unlimited watchdog restarts.
    #[serde(default)]
    pub max_restarts: Option<u32>,
}

impl GpuConfig {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            anyhow::bail!(
                "GPU config not found: {}. Copy gpu.example.toml to gpu.toml, then set command and args for your GPU miner.",
                path.display()
            );
        }

        let raw =
            std::fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))?;
        let cfg = toml::from_str::<GpuConfig>(&raw)
            .with_context(|| format!("Parsing {}", path.display()))?;
        cfg.validate(path)?;
        Ok(cfg)
    }

    fn validate(&self, path: &Path) -> Result<()> {
        if self.command.trim().is_empty() || self.command.contains("YOUR_GPU_MINER_BINARY") {
            anyhow::bail!(
                "Set command in {} to an installed Kaspa GPU miner executable",
                path.display()
            );
        }
        if self.args.is_empty() {
            anyhow::bail!("GPU config {} must include args", path.display());
        }
        Ok(())
    }
}

pub async fn run(config: Arc<Config>, gpu_config: &Path, once: bool) -> Result<()> {
    let gpu = GpuConfig::load(gpu_config)?;
    let mut restarts = 0u32;

    loop {
        let status = run_child(&config, &gpu).await?;
        if status.success() {
            println!("[gpu] engine exited cleanly");
            return Ok(());
        }

        if once || !gpu.restart {
            anyhow::bail!("GPU engine exited with status {status}");
        }

        restarts = restarts.saturating_add(1);
        if let Some(max) = gpu.max_restarts {
            if restarts > max {
                anyhow::bail!("GPU engine restart cap reached after {max} restarts");
            }
        }

        println!(
            "[gpu] engine exited with {status}; restarting in {}s ({restarts})",
            gpu.restart_delay_secs
        );
        tokio::time::sleep(Duration::from_secs(gpu.restart_delay_secs)).await;
    }
}

async fn run_child(config: &Config, gpu: &GpuConfig) -> Result<std::process::ExitStatus> {
    let args = expand_args(&gpu.args, config, &gpu.devices);
    let env = expand_env(&gpu.env, config, &gpu.devices);

    println!("[gpu] launching {}", gpu.command);
    println!("[gpu] pool {}", config.pool);
    if !gpu.devices.trim().is_empty() {
        println!("[gpu] devices {}", gpu.devices);
    }

    let mut child = Command::new(&gpu.command)
        .args(args)
        .envs(env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Starting GPU engine {}", gpu.command))?;

    let stdout = child.stdout.take().map(|out| pipe_lines(out, "out"));
    let stderr = child.stderr.take().map(|err| pipe_lines(err, "err"));

    let status = tokio::select! {
        status = child.wait() => status.context("Waiting for GPU engine")?,
        _ = tokio::signal::ctrl_c() => {
            println!("[gpu] shutdown requested");
            let _ = child.kill().await;
            child.wait().await.context("Stopping GPU engine")?
        }
    };

    if let Some(handle) = stdout {
        let _ = handle.await;
    }
    if let Some(handle) = stderr {
        let _ = handle.await;
    }

    Ok(status)
}

fn pipe_lines<R>(reader: R, stream: &'static str) -> tokio::task::JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            println!("[gpu:{stream}] {line}");
        }
    })
}

pub fn print_info() {
    println!("KASPilot GPU discovery");
    println!("Native CUDA/OpenCL kernels are not compiled into this build.");
    println!("Use --gpu with gpu.toml to supervise an installed Kaspa kHeavyHash GPU engine.\n");

    probe("nvidia-smi", &["-L"]);
    probe("rocm-smi", &["--showproductname"]);

    if cfg!(target_os = "macos") {
        probe("system_profiler", &["SPDisplaysDataType"]);
    }
}

fn probe(command: &str, args: &[&str]) {
    match std::process::Command::new(command).args(args).output() {
        Ok(output) if output.status.success() => {
            let out = String::from_utf8_lossy(&output.stdout);
            let out = out.trim();
            if out.is_empty() {
                println!("[gpu-info] {command}: found");
            } else {
                println!("[gpu-info] {command}:\n{out}\n");
            }
        }
        _ => println!("[gpu-info] {command}: not found"),
    }
}

fn expand_args(args: &[String], config: &Config, devices: &str) -> Vec<String> {
    args.iter()
        .map(|arg| expand_placeholders(arg, config, devices))
        .collect()
}

fn expand_env(
    env: &BTreeMap<String, String>,
    config: &Config,
    devices: &str,
) -> BTreeMap<String, String> {
    env.iter()
        .map(|(key, value)| (key.clone(), expand_placeholders(value, config, devices)))
        .collect()
}

fn expand_placeholders(value: &str, config: &Config, devices: &str) -> String {
    value
        .replace("{pool}", &config.pool)
        .replace("{wallet}", &config.wallet)
        .replace("{worker}", &config.worker)
        .replace("{login}", &config.login())
        .replace("{devices}", devices)
}

fn default_restart() -> bool {
    true
}

fn default_restart_delay_secs() -> u64 {
    10
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            pool: "stratum+ssl://kaspa.pool:5555".into(),
            wallet: "kaspa:qexample".into(),
            worker: "rig-01".into(),
            threads: 1,
            batch_size: crate::miner::DEFAULT_BATCH_SIZE,
            reconnect_secs: 5,
        }
    }

    #[test]
    fn expands_gpu_args() {
        let args = vec![
            "--pool".to_string(),
            "{pool}".to_string(),
            "--user".to_string(),
            "{login}".to_string(),
            "--devices={devices}".to_string(),
        ];

        assert_eq!(
            expand_args(&args, &config(), "0,1"),
            vec![
                "--pool",
                "stratum+ssl://kaspa.pool:5555",
                "--user",
                "kaspa:qexample.rig-01",
                "--devices=0,1",
            ]
        );
    }

    #[test]
    fn expands_gpu_env() {
        let mut env = BTreeMap::new();
        env.insert("CUDA_VISIBLE_DEVICES".to_string(), "{devices}".to_string());
        env.insert("KASPA_USER".to_string(), "{login}".to_string());

        let expanded = expand_env(&env, &config(), "2");
        assert_eq!(expanded["CUDA_VISIBLE_DEVICES"], "2");
        assert_eq!(expanded["KASPA_USER"], "kaspa:qexample.rig-01");
    }
}
