//! flaco-tui-v2 — ratatui-based shiny TUI over flaco-core.
//!
//! Layout:
//!   ┌──────────────────────────────────────────────┐
//!   │ flaco v2 · unified brain                     │ header
//!   ├──────────────────────────────────────────────┤
//!   │ [assistant stream] [tool calls as chips]    │ chat history
//!   │                                              │
//!   ├──────────────────────────────────────────────┤
//!   │ > type a message, /research, /shortcut, /q  │ input
//!   └──────────────────────────────────────────────┘
//!
//! Commands:
//!   /q                quit
//!   /research <topic>
//!   /shortcut <name>: <desc>
//!   /scaffold <idea>
//!   /memories
//!   /remember <text>

use std::io;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event as CtEvent, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use flaco_core::features::Features;
use flaco_core::runtime::{Runtime, Surface};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;

#[derive(Clone, Debug)]
enum LogEntry {
    User(String),
    Assistant(String),
    Tool(String, String, bool),
    System(String),
    Error(String),
}

pub async fn run(runtime: Arc<Runtime>, features: Arc<Features>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let result = run_loop(&mut terminal, runtime, features).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}

async fn run_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    runtime: Arc<Runtime>,
    features: Arc<Features>,
) -> Result<()> {
    let mut input = String::new();
    let user_id = "chris".to_string();

    // Seed the scrollback. If this is the user's first TUI launch on
    // this DB, show the full onboarding banner (fires exactly once per
    // (user, surface) via memory::user_state). Otherwise, a short
    // session banner. Failing silently falls back to the short banner.
    let mut log: Vec<LogEntry> = Vec::new();
    if let Some(banner) =
        flaco_core::welcome::maybe_show(&runtime.memory, &user_id, Surface::Tui)
    {
        // Render the welcome as a System line per non-empty paragraph
        // so ratatui's wrap works naturally.
        for line in banner.lines() {
            log.push(LogEntry::System(line.to_string()));
        }
    } else {
        log.push(LogEntry::System(
            "flaco v2 · memory is shared with Slack + Web · type /q to quit".into(),
        ));
    }
    let mut scroll: u16 = 0;

    let model = runtime.ollama.model().to_string();
    let fact_count = runtime.memory.all_facts(&user_id, 10_000).map(|v| v.len()).unwrap_or(0);
    let mut tick: u64 = 0;
    loop {
        tick = tick.wrapping_add(1);
        let cursor_on = (tick / 4).is_multiple_of(2);
        terminal.draw(|f| ui(f, &log, &input, scroll, &model, fact_count, cursor_on))?;

        if event::poll(Duration::from_millis(120))? {
            if let CtEvent::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Char('d'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Esc, _) => break,
                    (KeyCode::Enter, _) => {
                        let text = std::mem::take(&mut input).trim().to_string();
                        if text.is_empty() { continue; }
                        log.push(LogEntry::User(text.clone()));
                        scroll = u16::MAX;
                        let handled = handle_command(&text, &runtime, &features, &user_id, &mut log).await;
                        if handled == HandleResult::Quit { break; }
                    }
                    (KeyCode::Char(c), _) => input.push(c),
                    (KeyCode::Backspace, _) => { input.pop(); }
                    (KeyCode::Up, _) => scroll = scroll.saturating_sub(2),
                    (KeyCode::Down, _) => scroll = scroll.saturating_add(2),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

#[derive(PartialEq, Eq)]
enum HandleResult { Continue, Quit }

async fn handle_command(
    text: &str,
    runtime: &Arc<Runtime>,
    features: &Arc<Features>,
    user_id: &str,
    log: &mut Vec<LogEntry>,
) -> HandleResult {
    if text == "/q" || text == "/quit" || text == "/exit" {
        return HandleResult::Quit;
    }

    // Jarvis layer: natural-language intent router runs first so plain
    // `clear` / `reset` / `brief` / `status` work without any slash prefix.
    // Only when detect() returns None do we fall through to the explicit
    // `/research <topic>` style parsers below (which take structured args
    // and need their own handlers).
    if let Some(intent) = flaco_core::intent::detect(text) {
        match flaco_core::intent::dispatch(
            intent.clone(),
            runtime,
            features,
            &Surface::Tui,
            user_id,
        )
        .await
        {
            Ok(reply) => {
                if matches!(intent, flaco_core::intent::Intent::Reset) {
                    log.clear();
                    log.push(LogEntry::System("conversation reset. fresh brain, same memory.".into()));
                } else {
                    log.push(LogEntry::Assistant(reply));
                }
            }
            Err(e) => log.push(LogEntry::Error(format!("{e}"))),
        }
        return HandleResult::Continue;
    }

    if let Some(topic) = text.strip_prefix("/research ") {
        match features.research(topic).await {
            Ok(r) => log.push(LogEntry::Assistant(r.to_markdown())),
            Err(e) => log.push(LogEntry::Error(format!("research: {e}"))),
        }
        return HandleResult::Continue;
    }
    if let Some(rest) = text.strip_prefix("/shortcut ") {
        let (name, desc) = if let Some(idx) = rest.find(':') {
            (rest[..idx].trim().to_string(), rest[idx+1..].trim().to_string())
        } else {
            ("Flaco Shortcut".into(), rest.trim().to_string())
        };
        match features.create_shortcut(&name, &desc).await {
            Ok(r) => log.push(LogEntry::Tool("create_shortcut".into(), r.output, r.ok)),
            Err(e) => log.push(LogEntry::Error(format!("shortcut: {e}"))),
        }
        return HandleResult::Continue;
    }
    if let Some(idea) = text.strip_prefix("/scaffold ") {
        match features.scaffold(idea, "FLACO", None).await {
            Ok(r) => log.push(LogEntry::Tool("scaffold_idea".into(), r.output, r.ok)),
            Err(e) => log.push(LogEntry::Error(format!("scaffold: {e}"))),
        }
        return HandleResult::Continue;
    }
    // /brief, /clear, /reset, /new, /help, /memories, /tools, /status all
    // handled by the intent router above — no per-surface duplication.
    if let Some(fact) = text.strip_prefix("/remember ") {
        match features.remember(user_id, fact, "fact") {
            Ok(id) => log.push(LogEntry::System(format!("remembered #{id}: {fact}"))),
            Err(e) => log.push(LogEntry::Error(format!("{e}"))),
        }
        return HandleResult::Continue;
    }
    // Default: chat
    match runtime.session(&Surface::Tui, user_id) {
        Ok(session) => match runtime.handle_turn(&session, text, None).await {
            Ok(reply) => log.push(LogEntry::Assistant(reply)),
            Err(e) => log.push(LogEntry::Error(format!("{e}"))),
        },
        Err(e) => log.push(LogEntry::Error(format!("{e}"))),
    }
    HandleResult::Continue
}

fn ui(
    f: &mut Frame,
    log: &[LogEntry],
    input: &str,
    scroll: u16,
    model: &str,
    fact_count: usize,
    cursor_on: bool,
) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(size);

    render_header(f, chunks[0], model, fact_count);
    render_log(f, chunks[1], log, scroll);
    render_input(f, chunks[2], input, cursor_on);
}

fn render_header(f: &mut Frame<'_>, area: Rect, model: &str, fact_count: usize) {
    // A two-half header: the brand on the left, runtime chips on the right.
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(46)])
        .split(area);

    let brand = Line::from(vec![
        Span::styled(
            "  flacoAi ",
            Style::default()
                .fg(Color::Rgb(180, 155, 255))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "· ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            "powered by ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            "Roura.io",
            Style::default()
                .fg(Color::Rgb(62, 207, 142))
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let left = Paragraph::new(brand).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(44, 51, 66))),
    );
    f.render_widget(left, halves[0]);

    let right_line = Line::from(vec![
        Span::styled(" ● ", Style::default().fg(Color::Rgb(62, 207, 142)).add_modifier(Modifier::BOLD)),
        Span::styled("online ", Style::default().fg(Color::Gray)),
        Span::styled("· ", Style::default().fg(Color::DarkGray)),
        Span::styled(model, Style::default().fg(Color::Rgb(180, 155, 255)).add_modifier(Modifier::BOLD)),
        Span::styled("  · ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{fact_count} memories"), Style::default().fg(Color::Gray)),
    ]);
    let right = Paragraph::new(right_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(44, 51, 66))),
    );
    f.render_widget(right, halves[1]);
}

fn render_log(f: &mut Frame<'_>, area: Rect, log: &[LogEntry], _scroll: u16) {
    let mut lines: Vec<Line> = Vec::new();
    for entry in log {
        match entry {
            LogEntry::User(t) => {
                lines.push(Line::from(vec![
                    Span::styled("you  ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::raw(t.clone()),
                ]));
                lines.push(Line::from(""));
            }
            LogEntry::Assistant(t) => {
                lines.push(Line::from(Span::styled(
                    "flaco",
                    Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                )));
                for raw in t.lines() {
                    lines.push(Line::from(Span::raw(format!("  {raw}"))));
                }
                lines.push(Line::from(""));
            }
            LogEntry::Tool(name, out, ok) => {
                let color = if *ok { Color::Cyan } else { Color::Red };
                lines.push(Line::from(vec![
                    Span::styled(format!("⚙ {name}"), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                ]));
                for raw in out.lines().take(12) {
                    lines.push(Line::from(Span::styled(format!("  {raw}"), Style::default().fg(Color::DarkGray))));
                }
                lines.push(Line::from(""));
            }
            LogEntry::System(t) => {
                lines.push(Line::from(Span::styled(
                    format!("· {t}"),
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                )));
                lines.push(Line::from(""));
            }
            LogEntry::Error(t) => {
                lines.push(Line::from(Span::styled(
                    format!("! {t}"),
                    Style::default().fg(Color::Red),
                )));
                lines.push(Line::from(""));
            }
        }
    }
    // auto-scroll: start at the bottom
    let total = lines.len() as u16;
    let visible = area.height.saturating_sub(2);
    let start = total.saturating_sub(visible);
    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((start, 0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" chat ")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    f.render_widget(p, area);
}

fn render_input(f: &mut Frame<'_>, area: Rect, input: &str, cursor_on: bool) {
    let caret = if cursor_on { "▏" } else { " " };
    let p = Paragraph::new(Line::from(vec![
        Span::styled(
            "❯ ",
            Style::default()
                .fg(Color::Rgb(180, 155, 255))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(input.to_string()),
        Span::styled(
            caret,
            Style::default()
                .fg(Color::Rgb(180, 155, 255))
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Line::from(vec![
                Span::styled(" input ", Style::default().fg(Color::Rgb(180, 155, 255)).add_modifier(Modifier::BOLD)),
                Span::styled(
                    "· /brief · /research · /shortcut · /scaffold · /memories · /clear · /q ",
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
            .border_style(Style::default().fg(Color::Rgb(44, 51, 66))),
    );
    f.render_widget(p, area);
}
