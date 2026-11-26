//! Interactive Full-Screen Terminal UI built with `ratatui` + `crossterm`.
//!
//! Displays exclusively real state from `engine.db`, `memory.db`, and
//! `providers.json` — zero mock data. Supports live keyboard navigation,
//! run-log inspection, task retry/cancel, and provider cooldown reset.

use crate::engine::{Engine, ProviderReport};
use crate::memory::MemoryRecord;
use crate::model::{EventRecord, RunRecord, Task};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap},
    Frame, Terminal,
};
use std::fs;
use std::io;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Tasks,
    RunsAndLogs,
    Providers,
    SkillsAndMemory,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Tasks, Tab::RunsAndLogs, Tab::Providers, Tab::SkillsAndMemory];

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&t| t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }
}

pub struct TuiSnapshot {
    pub repo: String,
    pub tasks: Vec<Task>,
    pub runs: Vec<RunRecord>,
    pub providers: Vec<ProviderReport>,
    pub events: Vec<EventRecord>,
    pub memories: Vec<MemoryRecord>,
    pub skills_count: usize,
    pub selected_idx: usize,
    pub tab: Tab,
    pub status_banner: String,
    pub log_tail: String,
}

impl TuiSnapshot {
    pub fn load(engine: &Engine, tab: Tab, selected_idx: usize, banner: String) -> crate::Result<Self> {
        let tasks = engine.store().list_tasks()?;
        let runs = engine.store().recent_runs(40)?;
        let providers = engine.provider_reports()?;
        let events = engine.store().events(None, 12)?;
        let memories = engine.memory().recent(20)?;
        let log_tail = runs
            .get(selected_idx.min(runs.len().saturating_sub(1)))
            .and_then(|r| r.log_path.as_ref())
            .and_then(|p| fs::read_to_string(p).ok())
            .map(|txt| {
                let lines: Vec<&str> = txt.lines().collect();
                let start = lines.len().saturating_sub(18);
                lines[start..].join("\n")
            })
            .unwrap_or_else(|| "No run log selected yet.".into());

        Ok(TuiSnapshot {
            repo: engine.config().repo.display().to_string(),
            tasks,
            runs,
            providers,
            events,
            memories,
            skills_count: engine.skills().len(),
            selected_idx,
            tab,
            status_banner: banner,
            log_tail,
        })
    }
}

pub fn draw_ui(f: &mut Frame<'_>, snap: &TuiSnapshot) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(7),
            Constraint::Length(2),
        ])
        .split(f.area());

    // Header + Tabs
    let titles: Vec<Line> = [
        format!("1:Tasks ({})", snap.tasks.len()),
        format!("2:Runs & Logs ({})", snap.runs.len()),
        format!("3:Providers ({})", snap.providers.len()),
        format!("4:Memory ({}) & Skills ({})", snap.memories.len(), snap.skills_count),
    ]
    .into_iter()
    .map(Line::from)
    .collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(format!(" AI-Fleet Live SQLite TUI — {} ", snap.repo)))
        .select(snap.tab.index())
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, chunks[0]);

    // Main Body
    match snap.tab {
        Tab::Tasks => draw_tasks_tab(f, chunks[1], snap),
        Tab::RunsAndLogs => draw_runs_tab(f, chunks[1], snap),
        Tab::Providers => draw_providers_tab(f, chunks[1], snap),
        Tab::SkillsAndMemory => draw_memory_tab(f, chunks[1], snap),
    }

    // Recent Events pane
    let ev_lines: Vec<Line> = if snap.events.is_empty() {
        vec![Line::from(Span::styled("No events recorded in engine.db yet.", Style::default().fg(Color::DarkGray)))]
    } else {
        snap.events
            .iter()
            .rev()
            .take(5)
            .map(|e| {
                let t = e.ts.get(11..19).unwrap_or(&e.ts);
                Line::from(format!("{} [{:<18}] {}: {}", t, e.kind, e.task_id.as_deref().unwrap_or("-"), e.message))
            })
            .collect()
    };
    let ev_box = Paragraph::new(ev_lines)
        .block(Block::default().borders(Borders::ALL).title(" SHA-256 Event Log (engine.db) "))
        .wrap(Wrap { trim: true });
    f.render_widget(ev_box, chunks[2]);

    // Footer keybindings
    let footer = Paragraph::new(format!(
        " [Tab/1-4] Switch View  [j/k or ↑/↓] Select  [s] Run Queue  [r] Retry Task  [c] Cancel Task  [x] Ready Provider  [q/Esc] Quit | {}",
        snap.status_banner
    ))
    .style(Style::default().fg(Color::Yellow));
    f.render_widget(footer, chunks[3]);
}

fn draw_tasks_tab(f: &mut Frame<'_>, area: Rect, snap: &TuiSnapshot) {
    if snap.tasks.is_empty() {
        let empty = Paragraph::new("Queue is empty in engine.db.\nSubmit a task with: ai-fleet submit \"...\" or ai-fleet do \"...\"")
            .block(Block::default().borders(Borders::ALL).title(" SQLite Tasks "));
        f.render_widget(empty, area);
        return;
    }
    let rows: Vec<Row> = snap
        .tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if i == snap.selected_idx % snap.tasks.len() {
                Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let color = match t.status.as_str() {
                "done" => Color::Green,
                "running" | "reviewing" => Color::Cyan,
                "failed" | "blocked" => Color::Red,
                _ => Color::White,
            };
            Row::new(vec![
                Cell::from(t.id.clone()),
                Cell::from(t.status.as_str()).style(Style::default().fg(color)),
                Cell::from(t.complexity.as_str()),
                Cell::from(format!("{}/{}", t.attempts, t.handoffs)),
                Cell::from(format!(
                    "{}/{}",
                    t.last_provider.as_deref().unwrap_or("-"),
                    t.last_model.as_deref().unwrap_or("-")
                )),
                Cell::from(t.title.clone()),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(28),
            Constraint::Length(15),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(20),
            Constraint::Min(20),
        ],
    )
    .header(Row::new(["ID", "Status", "Tier", "Att/Hnd", "Provider/Model", "Title"]).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
    .block(Block::default().borders(Borders::ALL).title(" Persistent SQLite Task Queue "));
    f.render_widget(table, area);
}

fn draw_runs_tab(f: &mut Frame<'_>, area: Rect, snap: &TuiSnapshot) {
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(area);

    let rows: Vec<Row> = snap
        .runs
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let sel = !snap.runs.is_empty() && i == snap.selected_idx % snap.runs.len();
            let style = if sel { Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD) } else { Style::default() };
            Row::new(vec![
                Cell::from(format!("#{}", r.id)),
                Cell::from(r.role.clone()),
                Cell::from(format!("{}/{}", r.provider, r.model)),
                Cell::from(r.effort.clone()),
                Cell::from(r.failure_kind.as_deref().or(r.outcome.as_deref()).unwrap_or("running").to_string()),
                Cell::from(format!("{}ms", r.duration_ms.unwrap_or(0))),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(9),
            Constraint::Length(20),
            Constraint::Length(8),
            Constraint::Length(16),
            Constraint::Min(8),
        ],
    )
    .header(Row::new(["Run", "Role", "Provider/Model", "Effort", "Outcome", "Time"]).style(Style::default().fg(Color::Cyan)))
    .block(Block::default().borders(Borders::ALL).title(" Recent Runs "));
    f.render_widget(table, split[0]);

    let log_view = Paragraph::new(snap.log_tail.as_str())
        .block(Block::default().borders(Borders::ALL).title(" Selected Run Output Tail "))
        .wrap(Wrap { trim: false });
    f.render_widget(log_view, split[1]);
}

fn draw_providers_tab(f: &mut Frame<'_>, area: Rect, snap: &TuiSnapshot) {
    let rows: Vec<Row> = snap
        .providers
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let sel = !snap.providers.is_empty() && i == snap.selected_idx % snap.providers.len();
            let style = if sel { Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD) } else { Style::default() };
            let state = if !p.enabled {
                ("DISABLED".into(), Color::DarkGray)
            } else if let Some(u) = &p.cooldown_until {
                (format!("COOLDOWN until {u}"), Color::Yellow)
            } else if p.resolved.is_some() {
                ("READY".into(), Color::Green)
            } else {
                ("NOT ON PATH".into(), Color::Red)
            };
            let models: Vec<String> = p.models.iter().map(|(m, t, _, n)| format!("{m}(T{t},{n}runs)")).collect();
            Row::new(vec![
                Cell::from(p.id.clone()),
                Cell::from(state.0).style(Style::default().fg(state.1)),
                Cell::from(p.resolved.clone().unwrap_or_else(|| p.binary.clone())),
                Cell::from(models.join(", ")),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [Constraint::Length(14), Constraint::Length(28), Constraint::Length(38), Constraint::Min(24)],
    )
    .header(Row::new(["Provider", "Status", "Resolved Binary", "Models"]).style(Style::default().fg(Color::Cyan)))
    .block(Block::default().borders(Borders::ALL).title(" Configured Subscription CLI Providers "));
    f.render_widget(table, area);
}

fn draw_memory_tab(f: &mut Frame<'_>, area: Rect, snap: &TuiSnapshot) {
    let rows: Vec<Row> = snap
        .memories
        .iter()
        .map(|m| {
            Row::new(vec![
                Cell::from(format!("#{}", m.id)),
                Cell::from(m.kind.as_str()),
                Cell::from(m.title.clone()),
                Cell::from(m.content.replace('\n', " ")),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [Constraint::Length(6), Constraint::Length(12), Constraint::Length(34), Constraint::Min(30)],
    )
    .header(Row::new(["ID", "Kind", "Title", "Content"]).style(Style::default().fg(Color::Cyan)))
    .block(Block::default().borders(Borders::ALL).title(format!(" Institutional Memories & Reflexion Lessons (memory.db) | {} Skills Embedded ", snap.skills_count)));
    f.render_widget(table, area);
}

pub async fn run_interactive_tui(engine: Engine) -> crate::Result<()> {
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        orig_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = tui_event_loop(&mut terminal, engine).await;

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    res
}

async fn tui_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    engine: Engine,
) -> crate::Result<()> {
    let mut tab = Tab::Tasks;
    let mut selected = 0usize;
    let mut banner = "Ready".to_string();
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(500);

    loop {
        let snap = TuiSnapshot::load(&engine, tab, selected, banner.clone())?;
        terminal.draw(|f| draw_ui(f, &snap))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Tab => {
                            tab = tab.next();
                            selected = 0;
                        }
                        KeyCode::Char('1') => { tab = Tab::Tasks; selected = 0; }
                        KeyCode::Char('2') => { tab = Tab::RunsAndLogs; selected = 0; }
                        KeyCode::Char('3') => { tab = Tab::Providers; selected = 0; }
                        KeyCode::Char('4') => { tab = Tab::SkillsAndMemory; selected = 0; }
                        KeyCode::Down | KeyCode::Char('j') => selected = selected.wrapping_add(1),
                        KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                        KeyCode::Char('s') => {
                            let eng = engine.clone();
                            tokio::spawn(async move {
                                if let Err(e) = eng.run_until_idle().await {
                                    let _ = eng.store().append_event(None, "tui.run_error", &e.to_string());
                                }
                            });
                            banner = "Triggered async run_until_idle()".into();
                        }
                        KeyCode::Char('r') if tab == Tab::Tasks && !snap.tasks.is_empty() => {
                            let id = &snap.tasks[selected % snap.tasks.len()].id;
                            match engine.retry(id) {
                                Ok(_) => banner = format!("Re-queued {id}"),
                                Err(e) => banner = format!("Retry error: {e}"),
                            }
                        }
                        KeyCode::Char('c') if tab == Tab::Tasks && !snap.tasks.is_empty() => {
                            let id = &snap.tasks[selected % snap.tasks.len()].id;
                            match engine.cancel_task(id) {
                                Ok(_) => banner = format!("Cancelled {id}"),
                                Err(e) => banner = format!("Cancel error: {e}"),
                            }
                        }
                        KeyCode::Char('x') if tab == Tab::Providers && !snap.providers.is_empty() => {
                            let pid = &snap.providers[selected % snap.providers.len()].id;
                            match engine.clear_cooldown(pid) {
                                Ok(()) => banner = format!("Cleared cooldown on {pid}"),
                                Err(e) => banner = format!("Cooldown error: {e}"),
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineConfig;
    use crate::providers::ModelSpec;
    use crate::{Effort, NewTask, ProviderSpec};
    use ratatui::backend::TestBackend;
    use std::process::Command;

    #[test]
    fn ratatui_renders_real_sqlite_tasks_without_mock_data() {
        let dir = tempfile::tempdir().unwrap();
        let run_git = |args: &[&str]| {
            Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap();
        };
        run_git(&["init", "-q"]);
        std::fs::write(dir.path().join("README.md"), "hi\n").unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-q", "-m", "init"]);

        let spec = ProviderSpec::generic("claude", "claude", &[], vec![ModelSpec { name: "opus".into(), tier: 3, max_effort: Effort::Max }]);
        let engine = Engine::open(EngineConfig::new(dir.path().to_path_buf(), vec![spec])).unwrap();
        engine.submit(NewTask::new("audit-ledger-invariants", "check sql")).unwrap();

        let snap = TuiSnapshot::load(&engine, Tab::Tasks, 0, "ok".into()).unwrap();
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw_ui(f, &snap)).unwrap();

        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(rendered.contains("audit-ledger-invariants"));
        assert!(!rendered.contains("arch-verify"), "must never contain legacy hardcoded mock tasks");
    }
}
