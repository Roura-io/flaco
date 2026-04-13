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
    let mut log: Vec<LogEntry> = vec![LogEntry::System(
        "welcome to flaco v2. memory is shared with Slack + Web. type /q to quit.".into(),
    )];
    let mut scroll: u16 = 0;
    let user_id = "chris".to_string();

    loop {
        terminal.draw(|f| ui(f, &log, &input, scroll))?;

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
    if text == "/memories" {
        let mems = runtime.memory.all_facts(user_id, 50).unwrap_or_default();
        let text = if mems.is_empty() {
            "(no memories yet)".into()
        } else {
            mems.iter()
                .map(|m| format!("#{} [{}] {}", m.id, m.kind, m.content))
                .collect::<Vec<_>>()
                .join("\n")
        };
        log.push(LogEntry::Assistant(text));
        return HandleResult::Continue;
    }
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

fn ui(f: &mut Frame, log: &[LogEntry], input: &str, scroll: u16) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(size);

    render_header(f, chunks[0]);
    render_log(f, chunks[1], log, scroll);
    render_input(f, chunks[2], input);
}

fn render_header(f: &mut Frame<'_>, area: Rect) {
    let line = Line::from(vec![
        Span::styled("flaco", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled("v2", Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled(
            "unified brain · one memory · three surfaces",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let p = Paragraph::new(line).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(p, area);
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

fn render_input(f: &mut Frame<'_>, area: Rect, input: &str) {
    let p = Paragraph::new(Line::from(vec![
        Span::styled("❯ ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::raw(input),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" input · /q quit · /research · /shortcut · /scaffold · /memories · /remember ")
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(p, area);
}
