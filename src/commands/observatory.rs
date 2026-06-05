use crate::report::{ExecutionOutcome, ExecutionReport};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Tabs},
    Terminal,
};
use std::{
    cmp::Reverse,
    fs, io,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy)]
enum TabKind {
    Summary,
    Recent,
    Failures,
}

impl TabKind {
    fn titles() -> Vec<&'static str> {
        vec!["Summary", "Recent", "Failures"]
    }

    fn next(self) -> Self {
        match self {
            Self::Summary => Self::Recent,
            Self::Recent => Self::Failures,
            Self::Failures => Self::Summary,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Summary => Self::Failures,
            Self::Recent => Self::Summary,
            Self::Failures => Self::Recent,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Summary => 0,
            Self::Recent => 1,
            Self::Failures => 2,
        }
    }
}

struct ObservatoryApp {
    reports: Vec<ExecutionReport>,
    selected: usize,
    tab: TabKind,
    refresh_secs: u64,
    limit: usize,
    last_refresh: Instant,
}

impl ObservatoryApp {
    fn new(refresh_secs: u64, limit: usize) -> Self {
        let reports = load_reports(limit);
        Self {
            reports,
            selected: 0,
            tab: TabKind::Summary,
            refresh_secs,
            limit,
            last_refresh: Instant::now(),
        }
    }

    fn refresh(&mut self) {
        self.reports = load_reports(self.limit);
        if self.selected >= self.reports.len() && !self.reports.is_empty() {
            self.selected = self.reports.len() - 1;
        } else if self.reports.is_empty() {
            self.selected = 0;
        }
        self.last_refresh = Instant::now();
    }

    fn selected_report(&self) -> Option<&ExecutionReport> {
        self.reports.get(self.selected)
    }

    fn move_down(&mut self) {
        if !self.reports.is_empty() && self.selected + 1 < self.reports.len() {
            self.selected += 1;
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }
}

pub fn run_observatory(refresh_secs: u64, limit: usize) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, refresh_secs, limit);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    refresh_secs: u64,
    limit: usize,
) -> Result<()> {
    let mut app = ObservatoryApp::new(refresh_secs, limit);

    loop {
        if app.last_refresh.elapsed() >= Duration::from_secs(app.refresh_secs.max(1)) {
            app.refresh();
        }

        terminal.draw(|f| {
            let size = f.area();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(8),
                    Constraint::Length(7),
                ])
                .split(size);

            let title = Paragraph::new(Line::from(vec![
                Span::styled(
                    "SEL Agent Observatory",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  |  "),
                Span::raw(format!(
                    "reports: {}  refresh: {}s  q=quit  r=refresh  tab=next  ←/→ tabs  j/k move",
                    app.reports.len(),
                    app.refresh_secs
                )),
            ]))
            .block(Block::default().borders(Borders::ALL).title("Status"));
            f.render_widget(title, chunks[0]);

            let tabs = Tabs::new(TabKind::titles())
                .select(app.tab.index())
                .block(Block::default().borders(Borders::ALL).title("Views"))
                .highlight_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_widget(tabs, chunks[1]);

            match app.tab {
                TabKind::Summary => render_summary(f, chunks[2], &app),
                TabKind::Recent => render_recent(f, chunks[2], &app),
                TabKind::Failures => render_failures(f, chunks[2], &app),
            }

            render_footer(f, chunks[3], &app);
        })?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('r') => app.refresh(),
                    KeyCode::Tab | KeyCode::Right => app.tab = app.tab.next(),
                    KeyCode::Left => app.tab = app.tab.prev(),
                    KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                    KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

fn render_summary(f: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect, app: &ObservatoryApp) {
    let total = app.reports.len();
    let passed = app
        .reports
        .iter()
        .filter(|r| matches!(r.outcome, ExecutionOutcome::Pass))
        .count();
    let failed = total.saturating_sub(passed);

    let replay = app.reports.iter().filter(|r| r.mode == "replay").count();
    let record = app.reports.iter().filter(|r| r.mode == "record").count();
    let live = total.saturating_sub(replay + record);

    let avg_repairs = if total > 0 {
        app.reports
            .iter()
            .map(|r| r.repair_attempts as f64)
            .sum::<f64>()
            / total as f64
    } else {
        0.0
    };

    let avg_duration = if total > 0 {
        app.reports
            .iter()
            .map(|r| r.duration_secs as f64)
            .sum::<f64>()
            / total as f64
    } else {
        0.0
    };

    let success_rate = if total > 0 {
        (passed as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    let lines = vec![
        Line::from(format!("Total runs:      {}", total)),
        Line::from(format!("Passed:          {}", passed)),
        Line::from(format!("Failed:          {}", failed)),
        Line::from(format!("Success rate:    {:.1}%", success_rate)),
        Line::from(format!("Avg repairs:     {:.2}", avg_repairs)),
        Line::from(format!("Avg duration:    {:.1}s", avg_duration)),
        Line::from(""),
        Line::from(format!("Mode replay:     {}", replay)),
        Line::from(format!("Mode record:     {}", record)),
        Line::from(format!("Mode live/other: {}", live)),
        Line::from(""),
        Line::from(format!("Reports dir:     {}", reports_dir().display())),
    ];

    let p = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Summary"));
    f.render_widget(p, area);
}

fn render_recent(f: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect, app: &ObservatoryApp) {
    let items: Vec<ListItem<'_>> = if app.reports.is_empty() {
        vec![ListItem::new("No reports found")]
    } else {
        app.reports
            .iter()
            .enumerate()
            .map(|(idx, r)| {
                let icon = match r.outcome {
                    ExecutionOutcome::Pass => "✅",
                    ExecutionOutcome::Fail => "❌",
                };
                let prefix = if idx == app.selected { "▶" } else { " " };
                let line = format!(
                    "{} {} {:6} {:>3}s r:{:<2} {}",
                    prefix,
                    icon,
                    r.mode,
                    r.duration_secs,
                    r.repair_attempts,
                    truncate(&r.goal, 52)
                );
                ListItem::new(line)
            })
            .collect()
    };

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Recent runs"));
    f.render_widget(list, area);
}

fn render_failures(f: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect, app: &ObservatoryApp) {
    let failed: Vec<&ExecutionReport> = app
        .reports
        .iter()
        .filter(|r| matches!(r.outcome, ExecutionOutcome::Fail))
        .collect();

    let items: Vec<ListItem<'_>> = if failed.is_empty() {
        vec![ListItem::new("No failed reports in current window")]
    } else {
        failed
            .iter()
            .map(|r| {
                let reason = r.failure_reason.as_deref().unwrap_or("unknown");
                let line = format!(
                    "❌ {:6} {:>3}s {} | {}",
                    r.mode,
                    r.duration_secs,
                    truncate(&r.goal, 24),
                    truncate(reason, 36)
                );
                ListItem::new(line)
            })
            .collect()
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Failure reasons"),
    );
    f.render_widget(list, area);
}

fn render_footer(f: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect, app: &ObservatoryApp) {
    let lines = if let Some(r) = app.selected_report() {
        vec![
            Line::from(format!("Goal: {}", truncate(&r.goal, 90))),
            Line::from(format!("Workspace: {}", truncate(&r.workspace, 90))),
            Line::from(format!(
                "Mode={}  Outcome={:?}  Repairs={}  AutoFix={}  TestsOK={}  Mutation={}",
                r.mode,
                r.outcome,
                r.repair_attempts,
                r.autofix_count,
                r.tests_passed,
                format_mutation(r.mutation_score)
            )),
            Line::from(format!(
                "Model={}  LLM calls={}  Tokens(in/out)={}/{}  Time={}",
                truncate(&r.provider_model, 48),
                r.llm_calls,
                r.tokens_in,
                r.tokens_out,
                r.timestamp_utc
            )),
        ]
    } else {
        vec![Line::from("No report selected")]
    };

    let p =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Selected run"));
    f.render_widget(p, area);
}

fn reports_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".sel-agent")
        .join("reports")
}

fn load_reports(limit: usize) -> Vec<ExecutionReport> {
    let dir = reports_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let mut files: Vec<_> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| {
                let name = e.file_name();
                let n = name.to_string_lossy();
                n.ends_with(".json") && n != "latest.json"
            })
            .collect(),
        Err(_) => return Vec::new(),
    };

    files.sort_by_key(|b| Reverse(b.file_name()));
    files.truncate(limit);

    files
        .into_iter()
        .filter_map(|entry| {
            let data = fs::read_to_string(entry.path()).ok()?;
            serde_json::from_str::<ExecutionReport>(&data).ok()
        })
        .collect()
}

fn truncate(s: &str, max_chars: usize) -> String {
    let out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        format!("{out}…")
    } else {
        out
    }
}

fn format_mutation(score: f64) -> String {
    if score < 0.0 {
        "N/A".to_string()
    } else {
        format!("{:.0}%", score * 100.0)
    }
}
