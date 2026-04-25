use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event as CEvent, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Terminal,
};
use tokio::sync::{broadcast, mpsc};

use crate::config::Config;
use crate::stats::{format_hashrate, Stats};
use crate::stratum::{Event, Submission, Work};

struct TuiState {
    log:       Vec<String>,
    job_id:    String,
    connected: bool,
    accepted:  u64,
    rejected:  u64,
}

impl TuiState {
    fn push_log(&mut self, msg: String) {
        self.log.push(msg);
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }
}

pub async fn run(config: Arc<Config>, stats: Arc<Stats>) -> Result<()> {
    // Channels
    let (work_tx, _)         = broadcast::channel::<Work>(16);
    let (sub_tx,  sub_rx)    = mpsc::channel::<Submission>(64);
    let (event_tx, mut ev_rx) = mpsc::channel::<Event>(256);

    // Stratum
    let cfg2 = config.clone(); let st2 = stats.clone();
    let wtx2 = work_tx.clone(); let etx2 = event_tx.clone();
    tokio::spawn(async move { crate::stratum::run(cfg2, st2, wtx2, sub_rx, etx2).await; });

    // Mining threads
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    for tid in 0..config.threads {
        let mut wrx = work_tx.subscribe();
        let stx     = sub_tx.clone();
        let st      = stats.clone();
        let stp     = stop.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all().build().unwrap();
            rt.block_on(crate::miner::mine_thread_pub(tid, &mut wrx, stx, st, stp));
        });
    }

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = TuiState {
        log: vec!["Starting…".into()],
        job_id: "—".into(),
        connected: false,
        accepted:  0,
        rejected:  0,
    };

    let tick = Duration::from_millis(500);

    'main: loop {
        // Drain events (non-blocking)
        loop {
            match ev_rx.try_recv() {
                Ok(ev) => {
                    match &ev {
                        Event::Connected         => { state.connected = true;  state.push_log("Pool connected".into()); }
                        Event::Disconnected      => { state.connected = false; state.push_log("Pool disconnected".into()); }
                        Event::NewJob(id)        => { state.job_id = id.clone(); state.push_log(format!("New job: {id}")); }
                        Event::ShareAccepted     => { state.accepted += 1; state.push_log("✓ Share accepted".into()); }
                        Event::ShareRejected(r)  => { state.rejected += 1; state.push_log(format!("✗ Rejected: {r}")); }
                        Event::Error(msg)        => { state.push_log(format!("ERR {msg}")); }
                    }
                }
                Err(_) => break,
            }
        }

        terminal.draw(|f| draw(f, &config, &stats, &state))?;

        // Input
        if event::poll(tick)? {
            if let CEvent::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    break 'main;
                }
            }
        }
    }

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    println!("Goodbye.");
    Ok(())
}

fn draw(f: &mut ratatui::Frame, config: &Config, stats: &Stats, state: &TuiState) {
    let area = f.area();

    // Outer vertical split: header / body / footer
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // header
            Constraint::Min(10),     // body
            Constraint::Length(3),   // footer
        ])
        .split(area);

    draw_header(f, rows[0], config, stats, state);
    draw_body(f, rows[1], config, stats, state);
    draw_footer(f, rows[2]);
}

fn draw_header(f: &mut ratatui::Frame, area: Rect, config: &Config, stats: &Stats, state: &TuiState) {
    let hr = format_hashrate(stats.hashrate());
    let conn_str = if state.connected {
        Span::styled("● CONNECTED", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("○ CONNECTING…", Style::default().fg(Color::Yellow))
    };

    let text = vec![
        Line::from(vec![
            Span::styled("⛏  KASPA MINER  ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            conn_str,
            Span::raw(format!("   {}  ", config.pool)),
            Span::styled(hr, Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let p = Paragraph::new(text).block(block);
    f.render_widget(p, area);
}

fn draw_body(f: &mut ratatui::Frame, area: Rect, config: &Config, stats: &Stats, state: &TuiState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    draw_left_panel(f, cols[0], config, stats, state);
    draw_log(f, cols[1], state);
}

fn draw_left_panel(f: &mut ratatui::Frame, area: Rect, config: &Config, stats: &Stats, state: &TuiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(5)])
        .split(area);

    // Stats block
    let total   = stats.total_hashes();
    let elapsed = stats.elapsed_secs();
    let acc     = stats.accepted_count();
    let rej     = stats.rejected_count();
    let hr      = format_hashrate(stats.hashrate());

    let items: Vec<Line> = vec![
        kv("Hashrate",  &hr),
        kv("Hashes",    &format!("{total}")),
        kv("Elapsed",   &fmt_elapsed(elapsed)),
        kv("Accepted",  &format!("{acc}")),
        kv("Rejected",  &format!("{rej}")),
        kv("Job ID",    &state.job_id),
        kv("Wallet",    &config.wallet[..config.wallet.len().min(28)]),
        kv("Worker",    &config.worker),
    ];

    let p = Paragraph::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Stats ").border_style(Style::default().fg(Color::Blue)));
    f.render_widget(p, rows[0]);

    // Per-thread hashrates
    let thread_items: Vec<ListItem> = (0..config.threads)
        .map(|i| {
            let hr = format_hashrate(stats.thread_hashrate(i));
            ListItem::new(format!("  Thread {:2}  {}", i, hr))
        })
        .collect();

    let list = List::new(thread_items)
        .block(Block::default().borders(Borders::ALL).title(" Threads ").border_style(Style::default().fg(Color::Blue)));
    f.render_widget(list, rows[1]);
}

fn draw_log(f: &mut ratatui::Frame, area: Rect, state: &TuiState) {
    let h = area.height.saturating_sub(2) as usize;
    let visible: Vec<ListItem> = state.log.iter().rev().take(h).rev()
        .map(|msg| {
            let style = if msg.contains('✓') {
                Style::default().fg(Color::Green)
            } else if msg.contains('✗') || msg.contains("ERR") {
                Style::default().fg(Color::Red)
            } else if msg.contains("New job") {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            ListItem::new(Span::styled(msg.as_str(), style))
        })
        .collect();

    let list = List::new(visible)
        .block(Block::default().borders(Borders::ALL).title(" Log ").border_style(Style::default().fg(Color::Blue)));
    f.render_widget(list, area);
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect) {
    let text = Line::from(vec![
        Span::styled(" [Q]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" quit  "),
    ]);
    let p = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(p, area);
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn kv(key: &str, val: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key:<12}"), Style::default().fg(Color::Gray)),
        Span::styled(val.to_string(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ])
}

fn fmt_elapsed(secs: f64) -> String {
    let s = secs as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let s = s % 60;
    if h > 0 { format!("{h}h {m}m {s}s") } else { format!("{m}m {s}s") }
}
