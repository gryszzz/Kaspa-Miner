use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc};
use tokio::time::timeout;

use super::protocol::{
    authorize_msg, extranonce_to_mask, max_target, submit_msg, subscribe_msg,
    target_from_difficulty, Response, Work,
};
use crate::algorithm::kheavyhash::Target;
use crate::config::{Config, PoolEndpoint};
use crate::stats::Stats;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> AsyncReadWrite for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

#[derive(Debug, Clone)]
pub enum Event {
    Connected,
    Disconnected,
    NewJob(String),
    ShareAccepted,
    ShareRejected(String),
    Difficulty(f64),
    Extranonce(String),
    Error(String),
}

pub struct Submission {
    pub job_id: String,
    pub nonce: u64,
}

/// Runs the stratum client forever, reconnecting on failure.
pub async fn run(
    config: Arc<Config>,
    stats: Arc<Stats>,
    work_tx: broadcast::Sender<Work>,
    mut sub_rx: mpsc::Receiver<Submission>,
    event_tx: mpsc::Sender<Event>,
) {
    let mut submit_id: u64 = 10;
    loop {
        let endpoint = match config.pool_endpoint() {
            Ok(v) => v,
            Err(e) => {
                let _ = event_tx.send(Event::Error(e.to_string())).await;
                tokio::time::sleep(Duration::from_secs(config.reconnect_secs)).await;
                continue;
            }
        };

        match session(
            &endpoint,
            &config,
            &stats,
            &work_tx,
            &mut sub_rx,
            &event_tx,
            &mut submit_id,
        )
        .await
        {
            Ok(_) => {}
            Err(e) => {
                let _ = event_tx.send(Event::Disconnected).await;
                let _ = event_tx
                    .send(Event::Error(format!(
                        "Reconnecting in {}s: {e}",
                        config.reconnect_secs
                    )))
                    .await;
                tokio::time::sleep(Duration::from_secs(config.reconnect_secs)).await;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn session(
    endpoint: &PoolEndpoint,
    config: &Config,
    stats: &Stats,
    work_tx: &broadcast::Sender<Work>,
    sub_rx: &mut mpsc::Receiver<Submission>,
    event_tx: &mpsc::Sender<Event>,
    submit_id: &mut u64,
) -> Result<()> {
    let stream = connect_pool(endpoint).await?;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();

    let _ = event_tx.send(Event::Connected).await;

    let agent = format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    writer.write_all(subscribe_msg(&agent).as_bytes()).await?;
    writer
        .write_all(authorize_msg(&config.wallet, &config.worker).as_bytes())
        .await?;

    let mut current_target: Target = target_from_difficulty(1.0).unwrap_or_else(|_| max_target());
    let mut nonce_fixed = 0u64;
    let mut nonce_mask = u64::MAX;

    loop {
        tokio::select! {
            // Pool → miner
            line = lines.next_line() => {
                match line? {
                    None => anyhow::bail!("pool closed connection"),
                    Some(raw) => {
                        handle_server_msg(
                            &raw,
                            work_tx,
                            event_tx,
                            stats,
                            &mut current_target,
                            &mut nonce_fixed,
                            &mut nonce_mask,
                        ).await;
                    }
                }
            }
            // Miner → pool
            Some(sub) = sub_rx.recv() => {
                *submit_id += 1;
                let msg = submit_msg(*submit_id, &config.login(), &sub.job_id, sub.nonce);
                writer.write_all(msg.as_bytes()).await?;
            }
        }
    }
}

async fn connect_pool(endpoint: &PoolEndpoint) -> Result<Box<dyn AsyncReadWrite>> {
    let stream = timeout(
        CONNECT_TIMEOUT,
        TcpStream::connect((endpoint.host.as_str(), endpoint.port)),
    )
    .await
    .context("pool connection timed out")??;

    stream
        .set_nodelay(true)
        .context("enabling TCP_NODELAY for pool connection")?;

    if endpoint.tls {
        let connector = native_tls::TlsConnector::builder()
            .build()
            .context("building TLS connector")?;
        let connector = tokio_native_tls::TlsConnector::from(connector);
        let tls_stream = timeout(CONNECT_TIMEOUT, connector.connect(&endpoint.host, stream))
            .await
            .context("pool TLS handshake timed out")??;
        Ok(Box::new(tls_stream))
    } else {
        Ok(Box::new(stream))
    }
}

async fn handle_server_msg(
    raw: &str,
    work_tx: &broadcast::Sender<Work>,
    event_tx: &mpsc::Sender<Event>,
    stats: &Stats,
    current_target: &mut Target,
    nonce_fixed: &mut u64,
    nonce_mask: &mut u64,
) {
    let resp: Response = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Server-push notification
    if let Some(method) = &resp.method {
        match method.as_str() {
            "mining.notify" => {
                if let Some(params) = &resp.params {
                    match Work::from_notify(params, *current_target, *nonce_fixed, *nonce_mask) {
                        Ok(work) => {
                            let job_id = work.job_id.clone();
                            let _ = work_tx.send(work);
                            let _ = event_tx.send(Event::NewJob(job_id)).await;
                        }
                        Err(e) => {
                            let _ = event_tx
                                .send(Event::Error(format!("bad notify: {e}")))
                                .await;
                        }
                    }
                }
            }
            "mining.set_difficulty" => {
                if let Some(diff) = resp
                    .params
                    .as_ref()
                    .and_then(|p| p.as_array())
                    .and_then(|p| p.first())
                    .and_then(|v| v.as_f64())
                {
                    match target_from_difficulty(diff) {
                        Ok(target) => {
                            *current_target = target;
                            let _ = event_tx.send(Event::Difficulty(diff)).await;
                        }
                        Err(e) => {
                            let _ = event_tx
                                .send(Event::Error(format!("bad difficulty: {e}")))
                                .await;
                        }
                    }
                }
            }
            "mining.set_extranonce" | "set_extranonce" => {
                if let Some(arr) = resp.params.as_ref().and_then(|p| p.as_array()) {
                    let prefix = arr.first().and_then(|v| v.as_str()).unwrap_or("");
                    let nonce_size = arr.get(1).and_then(|v| v.as_u64());
                    match extranonce_to_mask(prefix, nonce_size) {
                        Ok((fixed, mask)) => {
                            *nonce_fixed = fixed;
                            *nonce_mask = mask;
                            let _ = event_tx.send(Event::Extranonce(prefix.to_string())).await;
                        }
                        Err(e) => {
                            let _ = event_tx
                                .send(Event::Error(format!("bad extranonce: {e}")))
                                .await;
                        }
                    }
                }
            }
            _ => {}
        }
        return;
    }

    // Reply to our submit
    if resp.id.is_some_and(|id| id >= 10) {
        if let Some(error) = resp.error.filter(|error| *error != serde_json::Value::Null) {
            let msg = error.to_string();
            stats.add_rejected();
            let _ = event_tx.send(Event::ShareRejected(msg)).await;
        } else if resp.result == Some(serde_json::Value::Bool(true)) {
            stats.add_accepted();
            let _ = event_tx.send(Event::ShareAccepted).await;
        }
    }
}
