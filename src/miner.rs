use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{broadcast, mpsc};

use crate::algorithm::JobContext;
use crate::config::Config;
use crate::stats::Stats;
use crate::stratum::{Event, Submission, Work};

/// Number of nonces each thread tries before checking for new work.
const BATCH: u64 = 1_024;

/// Spin up all worker threads and the stratum client; return when killed.
pub async fn run(config: Arc<Config>, stats: Arc<Stats>) -> anyhow::Result<()> {
    let (work_tx,  _work_rx)  = broadcast::channel::<Work>(16);
    let (sub_tx,    sub_rx)   = mpsc::channel::<Submission>(64);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);

    // ── stratum task ─────────────────────────────────────────────────────────
    let cfg2  = config.clone();
    let st2   = stats.clone();
    let wtx2  = work_tx.clone();
    let etx2  = event_tx.clone();
    tokio::spawn(async move {
        crate::stratum::run(cfg2, st2, wtx2, sub_rx, etx2).await;
    });

    // ── mining threads ───────────────────────────────────────────────────────
    let stop = Arc::new(AtomicBool::new(false));

    for tid in 0..config.threads {
        let mut work_rx  = work_tx.subscribe();
        let sub_tx_t     = sub_tx.clone();
        let stats_t      = stats.clone();
        let stop_t       = stop.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(mine_thread(tid, &mut work_rx, sub_tx_t, stats_t, stop_t));
        });
    }

    // ── event logger (plain mode) ─────────────────────────────────────────────
    while let Some(ev) = event_rx.recv().await {
        match ev {
            Event::Connected    => println!("[pool] connected"),
            Event::Disconnected => println!("[pool] disconnected"),
            Event::NewJob(id)   => println!("[job]  new job {id}"),
            Event::ShareAccepted          => println!("[share] ACCEPTED ✓"),
            Event::ShareRejected(reason)  => println!("[share] rejected: {reason}"),
            Event::Error(msg)             => println!("[err]  {msg}"),
        }
    }

    stop.store(true, Ordering::Relaxed);
    Ok(())
}

async fn mine_thread(
    tid:      usize,
    work_rx:  &mut broadcast::Receiver<Work>,
    sub_tx:   mpsc::Sender<Submission>,
    stats:    Arc<Stats>,
    stop:     Arc<AtomicBool>,
) {
    let mut job: Option<JobContext> = None;
    let mut current_job_id = String::new();

    // start nonce spread across threads to avoid overlap
    let mut nonce: u64 = (tid as u64).wrapping_mul(0x9e3779b97f4a7c15);

    loop {
        if stop.load(Ordering::Relaxed) { return; }

        // non-blocking check for new work
        match work_rx.try_recv() {
            Ok(work) => {
                current_job_id = work.job_id.clone();
                job = Some(JobContext::new(work.pre_pow_hash, work.timestamp, work.target));
            }
            Err(broadcast::error::TryRecvError::Lagged(_)) => {
                // missed messages; drain to latest
                while let Ok(w) = work_rx.try_recv() {
                    current_job_id = w.job_id.clone();
                    job = Some(JobContext::new(w.pre_pow_hash, w.timestamp, w.target));
                }
            }
            Err(_) => {}
        }

        if let Some(ref ctx) = job {
            // mine a batch
            for _ in 0..BATCH {
                nonce = nonce.wrapping_add(1);
                if let Some(found) = ctx.try_nonce(nonce) {
                    let _ = sub_tx.try_send(Submission {
                        job_id: current_job_id.clone(),
                        nonce:  found,
                    });
                }
            }
            stats.add_hashes(tid, BATCH);
        } else {
            // no work yet — wait without burning CPU
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }
}
