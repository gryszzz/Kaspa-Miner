use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, Event as CEvent, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Sparkline},
    Terminal,
};
use tokio::sync::{broadcast, mpsc};

use crate::config::Config;
use crate::stats::{format_hashrate, Stats};
use crate::stratum::{Event, Submission, Work};

const MAX_LOG_LINES: usize = 240;
const MAX_HASHRATE_SAMPLES: usize = 90;
const PREVIEW_WIDTH: u16 = 128;
const PREVIEW_HEIGHT: u16 = 38;

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(std::io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
    }
}

struct TuiState {
    log: VecDeque<String>,
    hashrate_history: VecDeque<u64>,
    job_id: String,
    connected: bool,
    difficulty: f64,
    extranonce: String,
    current_hashrate: f64,
    peak_hashrate: f64,
    last_hashes: u64,
    last_sample: Instant,
    last_job_at: Option<Instant>,
    last_share_at: Option<Instant>,
    reconnects: u64,
    error_count: u64,
    tick: u64,
}

impl TuiState {
    fn new() -> Self {
        let now = Instant::now();
        let mut state = Self {
            log: VecDeque::new(),
            hashrate_history: VecDeque::new(),
            job_id: "-".into(),
            connected: false,
            difficulty: 1.0,
            extranonce: "-".into(),
            current_hashrate: 0.0,
            peak_hashrate: 0.0,
            last_hashes: 0,
            last_sample: now,
            last_job_at: None,
            last_share_at: None,
            reconnects: 0,
            error_count: 0,
            tick: 0,
        };
        state.push_log("boot", "KASPilot real-time cockpit online");
        state
    }

    fn push_log(&mut self, level: &str, msg: impl Into<String>) {
        let stamp = fmt_runtime(self.last_sample.elapsed().as_secs());
        self.log
            .push_back(format!("[{stamp}] {:<5} {}", level, msg.into()));
        while self.log.len() > MAX_LOG_LINES {
            self.log.pop_front();
        }
    }

    fn sample(&mut self, stats: &Stats) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_sample).as_secs_f64();
        if elapsed < 0.25 {
            return;
        }

        let hashes = stats.total_hashes();
        let delta = hashes.saturating_sub(self.last_hashes);
        self.current_hashrate = delta as f64 / elapsed.max(0.001);
        self.peak_hashrate = self.peak_hashrate.max(self.current_hashrate);
        self.last_hashes = hashes;
        self.last_sample = now;
        self.tick = self.tick.wrapping_add(1);

        self.hashrate_history
            .push_back(self.current_hashrate.max(0.0).round() as u64);
        while self.hashrate_history.len() > MAX_HASHRATE_SAMPLES {
            self.hashrate_history.pop_front();
        }
    }
}

pub async fn run(config: Arc<Config>, stats: Arc<Stats>) -> Result<()> {
    let (work_tx, _) = broadcast::channel::<Work>(16);
    let (sub_tx, sub_rx) = mpsc::channel::<Submission>(64);
    let (event_tx, mut ev_rx) = mpsc::channel::<Event>(256);

    let cfg2 = config.clone();
    let st2 = stats.clone();
    let wtx2 = work_tx.clone();
    let etx2 = event_tx.clone();
    tokio::spawn(async move {
        crate::stratum::run(cfg2, st2, wtx2, sub_rx, etx2).await;
    });

    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut handles = Vec::with_capacity(config.threads);
    for tid in 0..config.threads {
        let mut wrx = work_tx.subscribe();
        let stx = sub_tx.clone();
        let st = stats.clone();
        let stp = stop.clone();
        let threads = config.threads;
        let batch_size = config.batch_size;
        handles.push(std::thread::spawn(move || {
            crate::miner::mine_thread_pub(tid, threads, batch_size, &mut wrx, stx, st, stp);
        }));
    }

    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut state = TuiState::new();
    let tick = Duration::from_millis(250);

    'main: loop {
        while let Ok(ev) = ev_rx.try_recv() {
            handle_event(&mut state, ev);
        }

        state.sample(&stats);
        terminal.draw(|f| draw(f, &config, &stats, &state))?;

        if event::poll(tick)? {
            if let CEvent::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break 'main,
                    KeyCode::Char('c') => state.log.clear(),
                    _ => {}
                }
            }
        }
    }

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for handle in handles {
        let _ = handle.join();
    }
    Ok(())
}

pub fn write_preview_svg(path: &Path) -> Result<()> {
    std::fs::write(path, render_preview_svg()?)?;
    Ok(())
}

fn render_preview_svg() -> Result<String> {
    let config = preview_config();
    let stats = preview_stats(config.threads);
    let state = preview_state();
    let backend = TestBackend::new(PREVIEW_WIDTH, PREVIEW_HEIGHT);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| draw(f, &config, &stats, &state))?;
    Ok(buffer_to_svg(terminal.backend().buffer()))
}

fn handle_event(state: &mut TuiState, ev: Event) {
    match ev {
        Event::Connected => {
            state.connected = true;
            state.push_log("pool", "connected");
        }
        Event::Disconnected => {
            if state.connected {
                state.reconnects += 1;
            }
            state.connected = false;
            state.push_log("pool", "disconnected");
        }
        Event::NewJob(id) => {
            state.job_id = id.clone();
            state.last_job_at = Some(Instant::now());
            state.push_log("job", format!("new work {}", truncate(&id, 24)));
        }
        Event::Difficulty(d) => {
            state.difficulty = d;
            state.push_log("diff", format!("{d:.4}"));
        }
        Event::Extranonce(prefix) => {
            state.extranonce = prefix.clone();
            state.push_log("nonce", format!("prefix {}", truncate(&prefix, 24)));
        }
        Event::ShareAccepted => {
            state.last_share_at = Some(Instant::now());
            state.push_log("share", "accepted");
        }
        Event::ShareRejected(reason) => {
            state.last_share_at = Some(Instant::now());
            state.push_log("reject", classify_reject(&reason));
        }
        Event::Error(msg) => {
            state.error_count += 1;
            state.push_log("err", msg);
        }
    }
}

fn preview_config() -> Config {
    Config {
        pool: "stratum+ssl://kaspa.pool.example:5555".into(),
        wallet: "kaspa:qrx3v6m4r9demoaddress7v0pilot9z6".into(),
        worker: "rig-preview-01".into(),
        threads: 8,
        batch_size: 4096,
        reconnect_secs: 5,
    }
}

fn preview_stats(threads: usize) -> Stats {
    let stats = Stats::new(threads);
    let samples = [
        92_000, 88_000, 95_000, 81_000, 104_000, 99_000, 87_000, 96_000,
    ];
    for (id, hashes) in samples.iter().enumerate().take(threads) {
        stats.add_hashes(id, *hashes);
    }
    for _ in 0..14 {
        stats.add_accepted();
    }
    stats
}

fn preview_state() -> TuiState {
    let now = Instant::now();
    let mut state = TuiState::new();
    state.connected = true;
    state.job_id = "82df19b72b88e7ca9f014e96a7f6c001".into();
    state.difficulty = 128.0;
    state.extranonce = "00a7ff12".into();
    state.current_hashrate = 22_840.0;
    state.peak_hashrate = 28_110.0;
    state.last_job_at = Some(now - Duration::from_secs(2));
    state.last_share_at = Some(now - Duration::from_secs(4));
    state.tick = 42;
    state.hashrate_history = VecDeque::from(vec![
        14_200, 18_600, 16_400, 22_800, 20_300, 24_900, 19_800, 26_500, 23_100, 28_110, 22_840,
    ]);
    state.log.clear();
    state.push_log("pool", "connected");
    state.push_log("diff", "128.0000");
    state.push_log("job", "new work 82df19b72b88e7ca9f014e9");
    state.push_log("share", "accepted");
    state.push_log("share", "accepted");
    state.push_log("nonce", "prefix 00a7ff12");
    state
}

fn draw(f: &mut ratatui::Frame, config: &Config, stats: &Stats, state: &TuiState) {
    let area = f.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Min(12),
            Constraint::Length(3),
        ])
        .split(area);

    draw_header(f, rows[0], config, state);
    draw_kpis(f, rows[1], config, stats, state);
    draw_body(f, rows[2], config, stats, state);
    draw_footer(f, rows[3]);
}

fn draw_header(f: &mut ratatui::Frame, area: Rect, config: &Config, state: &TuiState) {
    let status = if state.connected {
        Span::styled(
            "● LIVE",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "○ SEEKING POOL",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    };
    let pulse = if state.tick.is_multiple_of(2) {
        "KASPILOT"
    } else {
        "KASPA/OPS"
    };

    let line = Line::from(vec![
        Span::styled(
            format!(" {pulse} "),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" // "),
        status,
        Span::raw(" // "),
        Span::styled(
            truncate(&config.pool, 54),
            Style::default().fg(Color::White),
        ),
        Span::raw(" // worker "),
        Span::styled(&config.worker, Style::default().fg(Color::Magenta)),
    ]);

    f.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        area,
    );
}

fn draw_kpis(f: &mut ratatui::Frame, area: Rect, config: &Config, stats: &Stats, state: &TuiState) {
    let cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(area);

    let accepted = stats.accepted_count();
    let rejected = stats.rejected_count();
    let shares = accepted + rejected;
    let accept_ratio = if shares == 0 {
        0.0
    } else {
        accepted as f64 / shares as f64
    };

    draw_kpi(
        f,
        cells[0],
        "NOW",
        &format_hashrate(state.current_hashrate),
        Color::Cyan,
    );
    draw_kpi(
        f,
        cells[1],
        "PEAK",
        &format_hashrate(state.peak_hashrate.max(stats.hashrate())),
        Color::Magenta,
    );
    draw_kpi(
        f,
        cells[2],
        "SHARES",
        &format!("{accepted}/{rejected}"),
        share_color(accept_ratio, shares),
    );
    draw_kpi(
        f,
        cells[3],
        "JOB AGE",
        &state
            .last_job_at
            .map(|t| fmt_age(t.elapsed()))
            .unwrap_or_else(|| "-".to_string()),
        job_color(state.last_job_at),
    );
    draw_kpi(
        f,
        cells[4],
        "THREADS",
        &format!("{} x {}", config.threads, config.batch_size),
        Color::White,
    );
}

fn draw_kpi(f: &mut ratatui::Frame, area: Rect, label: &str, value: &str, color: Color) {
    let text = vec![
        Line::from(Span::styled(
            label,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        Line::from(Span::styled(
            value,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
    ];
    f.render_widget(
        Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(color)),
        ),
        area,
    );
}

fn draw_body(f: &mut ratatui::Frame, area: Rect, config: &Config, stats: &Stats, state: &TuiState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);

    draw_left_panel(f, cols[0], config, stats, state);
    draw_right_panel(f, cols[1], config, stats, state);
}

fn draw_left_panel(
    f: &mut ratatui::Frame,
    area: Rect,
    config: &Config,
    stats: &Stats,
    state: &TuiState,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(11),
            Constraint::Length(3),
            Constraint::Min(6),
        ])
        .split(area);

    let total = stats.total_hashes();
    let accepted = stats.accepted_count();
    let rejected = stats.rejected_count();
    let reject_rate = if accepted + rejected == 0 {
        0.0
    } else {
        rejected as f64 / (accepted + rejected) as f64
    };

    let items = vec![
        kv("pool", &truncate(&config.pool, 34), Color::White),
        kv("wallet", &truncate(&config.wallet, 34), Color::White),
        kv("job", &truncate(&state.job_id, 34), Color::Yellow),
        kv("diff", &format!("{:.4}", state.difficulty), Color::Cyan),
        kv(
            "extranonce",
            &truncate(&state.extranonce, 34),
            Color::Magenta,
        ),
        kv("hashes", &format!("{total}"), Color::White),
        kv(
            "uptime",
            &fmt_runtime(stats.elapsed_secs() as u64),
            Color::White,
        ),
        kv("reconnects", &state.reconnects.to_string(), Color::Yellow),
        kv("errors", &state.error_count.to_string(), Color::Red),
    ];

    f.render_widget(
        Paragraph::new(items).block(panel(" Signal ", Color::Blue)),
        rows[0],
    );

    f.render_widget(
        Gauge::default()
            .block(panel(
                " Share Quality ",
                share_color(1.0 - reject_rate, accepted + rejected),
            ))
            .gauge_style(Style::default().fg(share_color(1.0 - reject_rate, accepted + rejected)))
            .ratio((1.0 - reject_rate).clamp(0.0, 1.0))
            .label(format!("{} accepted / {} rejected", accepted, rejected)),
        rows[1],
    );

    draw_threads(f, rows[2], config, stats, state);
}

fn draw_threads(
    f: &mut ratatui::Frame,
    area: Rect,
    config: &Config,
    stats: &Stats,
    state: &TuiState,
) {
    let peak = (0..config.threads)
        .map(|id| stats.thread_hashrate(id))
        .fold(
            state.current_hashrate / config.threads.max(1) as f64,
            f64::max,
        )
        .max(1.0);

    let h = area.height.saturating_sub(2) as usize;
    let items: Vec<ListItem> = (0..config.threads)
        .take(h)
        .map(|id| {
            let hr = stats.thread_hashrate(id);
            let ratio = (hr / peak).clamp(0.0, 1.0);
            let bar = bar(ratio, 16);
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {:02} ", id), Style::default().fg(Color::DarkGray)),
                Span::styled(bar, Style::default().fg(thread_color(ratio))),
                Span::raw(" "),
                Span::styled(format_hashrate(hr), Style::default().fg(Color::White)),
            ]))
        })
        .collect();

    f.render_widget(
        List::new(items).block(panel(" Thread Heat ", Color::Blue)),
        area,
    );
}

fn draw_right_panel(
    f: &mut ratatui::Frame,
    area: Rect,
    config: &Config,
    stats: &Stats,
    state: &TuiState,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(8)])
        .split(area);

    draw_hashrate_chart(f, rows[0], state);
    draw_event_stream(f, rows[1], config, stats, state);
}

fn draw_hashrate_chart(f: &mut ratatui::Frame, area: Rect, state: &TuiState) {
    let data: Vec<u64> = state.hashrate_history.iter().copied().collect();
    let max = data.iter().copied().max().unwrap_or(1).max(1);
    f.render_widget(
        Sparkline::default()
            .block(panel(
                &format!(
                    " Hashrate Trace  now {}  peak {} ",
                    format_hashrate(state.current_hashrate),
                    format_hashrate(state.peak_hashrate)
                ),
                Color::Cyan,
            ))
            .data(&data)
            .max(max)
            .style(Style::default().fg(Color::Cyan)),
        area,
    );
}

fn draw_event_stream(
    f: &mut ratatui::Frame,
    area: Rect,
    _config: &Config,
    _stats: &Stats,
    state: &TuiState,
) {
    let h = area.height.saturating_sub(2) as usize;
    let visible: Vec<ListItem> = state
        .log
        .iter()
        .rev()
        .take(h)
        .rev()
        .map(|msg| ListItem::new(Span::styled(msg.as_str(), log_style(msg))))
        .collect();

    f.render_widget(
        List::new(visible).block(panel(" Event Stream ", Color::Blue)),
        area,
    );
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect) {
    let text = Line::from(vec![
        Span::styled(
            " [Q]/[ESC] ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("quit  "),
        Span::styled(
            " [C] ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("clear events  "),
        Span::styled(
            "KASPILOT REAL-TIME MINING COCKPIT",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(
        Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        area,
    );
}

fn panel(title: &str, color: Color) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(color))
}

fn kv(key: &str, val: &str, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key:<11}"), Style::default().fg(Color::DarkGray)),
        Span::styled(
            val.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn fmt_runtime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

fn fmt_age(age: Duration) -> String {
    let secs = age.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        fmt_runtime(secs)
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let take = max.saturating_sub(1);
    format!("{}…", value.chars().take(take).collect::<String>())
}

fn bar(ratio: f64, width: usize) -> String {
    let filled = (ratio * width as f64).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn share_color(ratio: f64, shares: u64) -> Color {
    if shares == 0 {
        Color::DarkGray
    } else if ratio >= 0.98 {
        Color::Green
    } else if ratio >= 0.90 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn job_color(last_job_at: Option<Instant>) -> Color {
    match last_job_at.map(|t| t.elapsed().as_secs()) {
        None => Color::DarkGray,
        Some(age) if age <= 3 => Color::Green,
        Some(age) if age <= 10 => Color::Yellow,
        Some(_) => Color::Red,
    }
}

fn thread_color(ratio: f64) -> Color {
    if ratio >= 0.75 {
        Color::Green
    } else if ratio >= 0.35 {
        Color::Yellow
    } else {
        Color::DarkGray
    }
}

fn log_style(msg: &str) -> Style {
    if msg.contains("share accepted") {
        Style::default().fg(Color::Green)
    } else if msg.contains("reject") || msg.contains("err") || msg.contains("disconnected") {
        Style::default().fg(Color::Red)
    } else if msg.contains("job") || msg.contains("diff") {
        Style::default().fg(Color::Yellow)
    } else if msg.contains("pool") || msg.contains("boot") {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    }
}

fn classify_reject(reason: &str) -> String {
    let lower = reason.to_ascii_lowercase();
    let label = if lower.contains("stale") {
        "stale"
    } else if lower.contains("duplicate") {
        "duplicate"
    } else if lower.contains("low") || lower.contains("difficulty") {
        "low difficulty"
    } else if lower.contains("unauthorized") || lower.contains("auth") {
        "unauthorized"
    } else if lower.contains("job") {
        "bad job"
    } else {
        "pool rejected"
    };
    format!("{label}: {}", truncate(reason, 72))
}

fn buffer_to_svg(buffer: &Buffer) -> String {
    let cell_width = 9u16;
    let cell_height = 18u16;
    let pad = 26u16;
    let width = buffer.area.width * cell_width + pad * 2;
    let height = buffer.area.height * cell_height + pad * 2;
    let content_width = width - 24;
    let content_height = height - 24;
    let mut svg = String::new();

    svg.push_str(&format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-labelledby="title desc">
  <title id="title">KASPilot mining CLI preview</title>
  <desc id="desc">Actual Ratatui-rendered preview frame of the KASPilot real-time Kaspa mining cockpit.</desc>
  <defs>
    <linearGradient id="scanline" x1="0" x2="0" y1="0" y2="1">
      <stop offset="0%" stop-color="#28e7ff" stop-opacity="0"/>
      <stop offset="50%" stop-color="#28e7ff" stop-opacity="0.16"/>
      <stop offset="100%" stop-color="#28e7ff" stop-opacity="0"/>
    </linearGradient>
  </defs>
  <rect width="100%" height="100%" fill="#050814"/>
  <rect x="12" y="12" width="{content_width}" height="{content_height}" rx="10" fill="#07101f" stroke="#28e7ff" stroke-opacity="0.4"/>
  <rect class="scan" x="18" y="-90" width="{}" height="96" fill="url(#scanline)"/>
  <style>
    .term {{ font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, &quot;Liberation Mono&quot;, monospace; font-size: 15px; dominant-baseline: text-before-edge; white-space: pre; }}
    .bold {{ font-weight: 800; }}
    .scan {{ animation: scan 4.5s linear infinite; }}
    @keyframes scan {{
      0% {{ transform: translateY(0); opacity: 0; }}
      10% {{ opacity: 1; }}
      90% {{ opacity: 0.7; }}
      100% {{ transform: translateY({height}px); opacity: 0; }}
    }}
  </style>
"##,
        width - 36,
    ));

    for y in 0..buffer.area.height {
        let mut x = 0;
        while x < buffer.area.width {
            let cell = &buffer[(x, y)];
            let bold = if cell.modifier.contains(Modifier::BOLD) {
                " bold"
            } else {
                ""
            };
            let mut text = String::new();
            let start_x = x;
            let fg = cell.fg;
            let modifier = cell.modifier;

            while x < buffer.area.width {
                let c = &buffer[(x, y)];
                if c.fg != fg || c.modifier != modifier {
                    break;
                }
                text.push_str(c.symbol());
                x += 1;
            }

            let text = text.trim_end();
            if text.is_empty() {
                continue;
            }

            let color = color_to_hex(fg);
            svg.push_str(&format!(
                r#"  <text class="term{bold}" x="{}" y="{}" fill="{color}">{}</text>
"#,
                pad + start_x * cell_width,
                pad + y * cell_height,
                escape_xml(text)
            ));
        }
    }

    svg.push_str("</svg>\n");
    svg
}

fn color_to_hex(color: Color) -> String {
    match color {
        Color::Black => "#050814".into(),
        Color::Red | Color::LightRed => "#ff5b6c".into(),
        Color::Green | Color::LightGreen => "#3cff9e".into(),
        Color::Yellow | Color::LightYellow => "#ffd166".into(),
        Color::Blue | Color::LightBlue => "#5aa7ff".into(),
        Color::Magenta | Color::LightMagenta => "#ff4fd8".into(),
        Color::Cyan | Color::LightCyan => "#28e7ff".into(),
        Color::Gray | Color::DarkGray => "#7d8aa6".into(),
        Color::White => "#d9f2ff".into(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(_) | Color::Reset => "#d9f2ff".into(),
    }
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
