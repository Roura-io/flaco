use std::fmt::Write as FmtWrite;
use std::io::{self, IsTerminal, Write};

use crossterm::execute;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor, Stylize};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

/// Minimum usable width — under this the wrapper no-ops rather than
/// mangling the text into one-char-per-line output.
const MIN_WRAP_WIDTH: usize = 20;

/// Width used when stdout isn't a TTY (piped to a file, subshell, …).
const NON_TTY_WIDTH: usize = 0;

/// Probe the current terminal width. Returns `0` when stdout isn't a
/// TTY or when the probe fails — callers treat `0` as "don't wrap".
#[must_use]
pub fn current_terminal_width() -> usize {
    if !io::stdout().is_terminal() {
        return NON_TTY_WIDTH;
    }
    crossterm::terminal::size().map_or(NON_TTY_WIDTH, |(cols, _)| cols as usize)
}

/// Word-wrap a string that may contain ANSI escape sequences to the given
/// visible column width. Existing newlines are respected (they reset the
/// column counter). Words longer than `width` overflow rather than being
/// broken — better to wrap at the next boundary than to hyphenate.
///
/// ANSI CSI sequences (`ESC [ ... <letter>`) and SGR sequences contribute
/// zero to visible width; any other ESC-prefixed sequence we encounter is
/// treated the same, which matches terminal behaviour for styling codes.
#[must_use]
pub fn wrap_ansi_to_width(text: &str, width: usize) -> String {
    if width < MIN_WRAP_WIDTH {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + text.len() / width);
    let mut visible_col: usize = 0;
    let mut current_word = String::new();
    let mut current_word_vis: usize = 0;
    let mut in_esc = false;
    let mut esc_starts_csi = false;

    let flush_word = |out: &mut String,
                      visible_col: &mut usize,
                      current_word: &mut String,
                      current_word_vis: &mut usize,
                      width: usize| {
        if current_word.is_empty() {
            return;
        }
        if *visible_col > 0 && *visible_col + *current_word_vis > width {
            out.push('\n');
            *visible_col = 0;
        }
        out.push_str(current_word);
        *visible_col += *current_word_vis;
        current_word.clear();
        *current_word_vis = 0;
    };

    for ch in text.chars() {
        if in_esc {
            current_word.push(ch);
            // CSI sequences end on a byte in range 0x40..=0x7E
            // (letters/symbols); SGR "m" is the common case. Any other
            // escape (e.g. "ESC ] 0 ; title BEL") is terminated by an
            // alphabetic byte in practice for the styles we emit.
            let terminator = if esc_starts_csi {
                (0x40..=0x7E).contains(&(ch as u32))
            } else {
                ch.is_ascii_alphabetic() || ch == '\x07'
            };
            if terminator {
                in_esc = false;
                esc_starts_csi = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_esc = true;
            esc_starts_csi = false;
            current_word.push(ch);
            continue;
        }
        // Mark CSI when we see the '[' immediately after ESC. Because
        // we're already past ESC (in_esc was true briefly above) this
        // branch actually runs the char after ESC — but we handled that
        // inside the `in_esc` block. So here we only see the '[' when it
        // follows a prior ESC that already closed. In practice the SGR
        // sequences we emit are all `ESC [ ... m`; the flag prevents an
        // isolated `]` from tripping the non-CSI terminator rule above.
        // Keeping the branch here as a safety net for future styles.
        if ch == '[' && !current_word.is_empty() && current_word.ends_with('\x1b') {
            esc_starts_csi = true;
        }

        if ch == '\n' {
            flush_word(
                &mut out,
                &mut visible_col,
                &mut current_word,
                &mut current_word_vis,
                width,
            );
            out.push('\n');
            visible_col = 0;
            continue;
        }
        if ch == ' ' || ch == '\t' {
            flush_word(
                &mut out,
                &mut visible_col,
                &mut current_word,
                &mut current_word_vis,
                width,
            );
            if visible_col + 1 > width {
                out.push('\n');
                visible_col = 0;
            } else {
                out.push(ch);
                visible_col += 1;
            }
            continue;
        }
        current_word.push(ch);
        current_word_vis += 1;
    }
    flush_word(
        &mut out,
        &mut visible_col,
        &mut current_word,
        &mut current_word_vis,
        width,
    );
    out
}

/// Convenience: wrap against the current terminal width. No-op when
/// stdout isn't a TTY or the terminal is too narrow to wrap sanely.
#[must_use]
pub fn wrap_ansi_to_terminal(text: &str) -> String {
    let width = current_terminal_width();
    if width == 0 {
        return text.to_string();
    }
    // Leave one column of slack so wrapped output doesn't hug the right
    // edge (matches how most CLIs render prose).
    let target = width.saturating_sub(1).max(MIN_WRAP_WIDTH);
    justify_ansi_to_width(&wrap_ansi_to_width(text, target), target)
}

/// Assistant-turn framing: soft-wrap to the terminal, justify interior
/// lines, then prefix the first non-empty line of the turn with `● ` and
/// every continuation line with a 2-space indent. The caller threads the
/// same `bullet_emitted` flag through every chunk of a single turn so the
/// bullet paints exactly once even when `MarkdownStreamState` flushes in
/// multiple pieces.
#[must_use]
pub fn wrap_assistant_body(text: &str, bullet_emitted: &mut bool) -> String {
    let width = current_terminal_width();
    wrap_assistant_body_to_width(text, width, bullet_emitted)
}

/// Testable core. `terminal_width` of 0 means "no TTY" — skip wrapping
/// and justification but still emit the gutter affordance so downstream
/// consumers (and unit tests) see consistent structure.
#[must_use]
pub fn wrap_assistant_body_to_width(
    text: &str,
    terminal_width: usize,
    bullet_emitted: &mut bool,
) -> String {
    let body = if terminal_width == 0 {
        text.to_string()
    } else {
        // 1 col for right-edge slack + 2 cols reserved for indent/bullet.
        let target = terminal_width.saturating_sub(3).max(MIN_WRAP_WIDTH);
        justify_ansi_to_width(&wrap_ansi_to_width(text, target), target)
    };

    let mut out = String::with_capacity(body.len() + 16);
    let mut first_line_in_chunk = true;
    for line in body.split('\n') {
        if !first_line_in_chunk {
            out.push('\n');
        }
        if !*bullet_emitted && !strip_ansi(line).trim().is_empty() {
            // Bold amber filled-circle matches how Claude Code frames a
            // message turn — the eye scans down the left gutter to see
            // where each reply begins.
            out.push_str("\u{1b}[1;38;5;215m●\u{1b}[0m ");
            *bullet_emitted = true;
        } else if !line.is_empty() {
            out.push_str("  ");
        }
        out.push_str(line);
        first_line_in_chunk = false;
    }
    out
}

/// Justify already word-wrapped prose by padding interior word gaps so
/// each line lands at `width`. The last line of every paragraph (before
/// a blank line or the end of input) is left ragged — a justified last
/// line with huge gaps looks broken. Non-prose lines (code blocks,
/// tables, headings, quotes, list bullets) pass through untouched.
#[must_use]
pub fn justify_ansi_to_width(text: &str, width: usize) -> String {
    if width < MIN_WRAP_WIDTH {
        return text.to_string();
    }

    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = String::with_capacity(text.len() + text.len() / 16);
    for (index, line) in lines.iter().enumerate() {
        let next = lines.get(index + 1).copied();
        out.push_str(&justify_line(line, next, width));
        if index + 1 < lines.len() {
            out.push('\n');
        }
    }
    out
}

fn justify_line(line: &str, next: Option<&str>, width: usize) -> String {
    if should_skip_line(line) {
        return line.to_string();
    }
    // A paragraph's final line is the one followed by a blank line, a
    // non-prose line, or the end of the buffer — leave those ragged.
    let is_paragraph_tail = match next {
        None => true,
        Some(next_line) => next_line.trim().is_empty() || should_skip_line(next_line),
    };
    if is_paragraph_tail {
        return line.to_string();
    }

    let visible = visible_width(line);
    // Padding only helps lines that already fill most of the column —
    // a half-filled line with expanded gaps reads as broken typography.
    if visible == 0 || visible >= width || visible * 5 < width * 3 {
        return line.to_string();
    }

    let leading_end = line
        .bytes()
        .position(|byte| byte != b' ')
        .unwrap_or(line.len());
    let body = &line[leading_end..];

    // ANSI escape sequences contain no literal spaces, so every ' ' in
    // the body is a real word separator. We pad the first space of each
    // run (consecutive spaces are preserved verbatim after the first).
    let mut gap_positions: Vec<usize> = Vec::new();
    let mut prev_was_space = false;
    for (idx, ch) in body.char_indices() {
        if ch == ' ' {
            if !prev_was_space {
                gap_positions.push(idx);
            }
            prev_was_space = true;
        } else {
            prev_was_space = false;
        }
    }
    if gap_positions.is_empty() {
        return line.to_string();
    }

    let padding_total = width - visible;
    let base = padding_total / gap_positions.len();
    let extra = padding_total % gap_positions.len();

    let mut justified = String::with_capacity(line.len() + padding_total);
    justified.push_str(&line[..leading_end]);
    let mut cursor = 0;
    for (rank, &gap_byte) in gap_positions.iter().enumerate() {
        justified.push_str(&body[cursor..=gap_byte]);
        let extra_spaces = base + usize::from(rank < extra);
        for _ in 0..extra_spaces {
            justified.push(' ');
        }
        cursor = gap_byte + 1;
    }
    justified.push_str(&body[cursor..]);
    justified
}

/// Line prefixes that should never be justified — tables, quotes,
/// rules, code-block frames, headings. Justification would stretch the
/// glyphs and break the alignment that makes these readable.
const NON_PROSE_PREFIXES: &[&str] = &["│", "╭", "╰", "├", "┼", "─", "#", "> ", "│ "];

fn should_skip_line(line: &str) -> bool {
    // Code-block body: the highlighter wraps every line in its 256-color
    // background, so identify those by the literal escape prefix.
    if line.starts_with("\u{1b}[48;5;236m") {
        return true;
    }
    let stripped = strip_ansi(line);
    let trimmed = stripped.trim_start();
    if trimmed.is_empty() {
        return true;
    }

    if NON_PROSE_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return true;
    }

    // Bullet or dash list marker.
    if trimmed.starts_with("• ") || trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return true;
    }

    // Ordered list marker like "1. " or "12. ".
    let bytes = trimmed.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx > 0 && idx + 1 < bytes.len() && bytes[idx] == b'.' && bytes[idx + 1] == b' ' {
        return true;
    }

    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorTheme {
    heading: Color,
    emphasis: Color,
    strong: Color,
    inline_code: Color,
    link: Color,
    quote: Color,
    table_border: Color,
    code_block_border: Color,
    spinner_active: Color,
    spinner_done: Color,
    spinner_failed: Color,
}

impl Default for ColorTheme {
    fn default() -> Self {
        Self {
            heading: Color::Cyan,
            emphasis: Color::Magenta,
            strong: Color::Yellow,
            inline_code: Color::Green,
            link: Color::Blue,
            quote: Color::DarkGrey,
            table_border: Color::DarkCyan,
            code_block_border: Color::DarkGrey,
            spinner_active: Color::Blue,
            spinner_done: Color::Green,
            spinner_failed: Color::Red,
        }
    }
}

/// Completion marker for a turn. Animated frames are handled by the
/// dedicated `spinner` module; this struct only paints the final ✔/✘
/// line once `run_turn` returns.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Spinner;

impl Spinner {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn finish(
        &mut self,
        label: &str,
        theme: &ColorTheme,
        out: &mut impl Write,
    ) -> io::Result<()> {
        // Always start the marker on a fresh line. Any cursor-manipulation
        // scheme would only wipe the last physical row of wrapped output
        // and risk overlaying the tail of the assistant's message — a
        // plain leading newline guarantees a clean row for the marker.
        execute!(
            out,
            Print("\n"),
            SetForegroundColor(theme.spinner_done),
            Print(format!("✔ {label}\n")),
            ResetColor
        )?;
        out.flush()
    }

    pub fn fail(
        &mut self,
        label: &str,
        theme: &ColorTheme,
        out: &mut impl Write,
    ) -> io::Result<()> {
        execute!(
            out,
            Print("\n"),
            SetForegroundColor(theme.spinner_failed),
            Print(format!("✘ {label}\n")),
            ResetColor
        )?;
        out.flush()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ListKind {
    Unordered,
    Ordered { next_index: u64 },
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct TableState {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    in_head: bool,
}

impl TableState {
    fn push_cell(&mut self) {
        let cell = self.current_cell.trim().to_string();
        self.current_row.push(cell);
        self.current_cell.clear();
    }

    fn finish_row(&mut self) {
        if self.current_row.is_empty() {
            return;
        }
        let row = std::mem::take(&mut self.current_row);
        if self.in_head {
            self.headers = row;
        } else {
            self.rows.push(row);
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct RenderState {
    emphasis: usize,
    strong: usize,
    heading_level: Option<u8>,
    quote: usize,
    list_stack: Vec<ListKind>,
    link_stack: Vec<LinkState>,
    table: Option<TableState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LinkState {
    destination: String,
    text: String,
}

impl RenderState {
    fn style_text(&self, text: &str, theme: &ColorTheme) -> String {
        let mut style = text.stylize();

        if matches!(self.heading_level, Some(1 | 2)) || self.strong > 0 {
            style = style.bold();
        }
        if self.emphasis > 0 {
            style = style.italic();
        }

        if let Some(level) = self.heading_level {
            style = match level {
                1 => style.with(theme.heading),
                2 => style.white(),
                3 => style.with(Color::Blue),
                _ => style.with(Color::Grey),
            };
        } else if self.strong > 0 {
            style = style.with(theme.strong);
        } else if self.emphasis > 0 {
            style = style.with(theme.emphasis);
        }

        if self.quote > 0 {
            style = style.with(theme.quote);
        }

        format!("{style}")
    }

    fn append_raw(&mut self, output: &mut String, text: &str) {
        if let Some(link) = self.link_stack.last_mut() {
            link.text.push_str(text);
        } else if let Some(table) = self.table.as_mut() {
            table.current_cell.push_str(text);
        } else {
            output.push_str(text);
        }
    }

    fn append_styled(&mut self, output: &mut String, text: &str, theme: &ColorTheme) {
        let styled = self.style_text(text, theme);
        self.append_raw(output, &styled);
    }
}

#[derive(Debug)]
pub struct TerminalRenderer {
    syntax_set: SyntaxSet,
    syntax_theme: Theme,
    color_theme: ColorTheme,
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let syntax_theme = ThemeSet::load_defaults()
            .themes
            .remove("base16-ocean.dark")
            .unwrap_or_default();
        Self {
            syntax_set,
            syntax_theme,
            color_theme: ColorTheme::default(),
        }
    }
}

impl TerminalRenderer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn color_theme(&self) -> &ColorTheme {
        &self.color_theme
    }

    #[must_use]
    pub fn render_markdown(&self, markdown: &str) -> String {
        let mut output = String::new();
        let mut state = RenderState::default();
        let mut code_language = String::new();
        let mut code_buffer = String::new();
        let mut in_code_block = false;

        for event in Parser::new_ext(markdown, Options::all()) {
            self.render_event(
                event,
                &mut state,
                &mut output,
                &mut code_buffer,
                &mut code_language,
                &mut in_code_block,
            );
        }

        output.trim_end().to_string()
    }

    #[must_use]
    pub fn markdown_to_ansi(&self, markdown: &str) -> String {
        self.render_markdown(markdown)
    }

    #[allow(clippy::too_many_lines)]
    fn render_event(
        &self,
        event: Event<'_>,
        state: &mut RenderState,
        output: &mut String,
        code_buffer: &mut String,
        code_language: &mut String,
        in_code_block: &mut bool,
    ) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                self.start_heading(state, level as u8, output);
            }
            Event::End(TagEnd::Paragraph) => output.push_str("\n\n"),
            Event::Start(Tag::BlockQuote(..)) => self.start_quote(state, output),
            Event::End(TagEnd::BlockQuote(..)) => {
                state.quote = state.quote.saturating_sub(1);
                output.push('\n');
            }
            Event::End(TagEnd::Heading(..)) => {
                state.heading_level = None;
                output.push_str("\n\n");
            }
            Event::End(TagEnd::Item) | Event::SoftBreak | Event::HardBreak => {
                state.append_raw(output, "\n");
            }
            Event::Start(Tag::List(first_item)) => {
                let kind = match first_item {
                    Some(index) => ListKind::Ordered { next_index: index },
                    None => ListKind::Unordered,
                };
                state.list_stack.push(kind);
            }
            Event::End(TagEnd::List(..)) => {
                state.list_stack.pop();
                output.push('\n');
            }
            Event::Start(Tag::Item) => Self::start_item(state, output),
            Event::Start(Tag::CodeBlock(kind)) => {
                *in_code_block = true;
                *code_language = match kind {
                    CodeBlockKind::Indented => String::from("text"),
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                };
                code_buffer.clear();
                self.start_code_block(code_language, output);
            }
            Event::End(TagEnd::CodeBlock) => {
                self.finish_code_block(code_buffer, code_language, output);
                *in_code_block = false;
                code_language.clear();
                code_buffer.clear();
            }
            Event::Start(Tag::Emphasis) => state.emphasis += 1,
            Event::End(TagEnd::Emphasis) => state.emphasis = state.emphasis.saturating_sub(1),
            Event::Start(Tag::Strong) => state.strong += 1,
            Event::End(TagEnd::Strong) => state.strong = state.strong.saturating_sub(1),
            Event::Code(code) => {
                let rendered =
                    format!("{}", format!("`{code}`").with(self.color_theme.inline_code));
                state.append_raw(output, &rendered);
            }
            Event::Rule => output.push_str("---\n"),
            Event::Text(text) => {
                self.push_text(text.as_ref(), state, output, code_buffer, *in_code_block);
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                state.append_raw(output, &html);
            }
            Event::FootnoteReference(reference) => {
                state.append_raw(output, &format!("[{reference}]"));
            }
            Event::TaskListMarker(done) => {
                state.append_raw(output, if done { "[x] " } else { "[ ] " });
            }
            Event::InlineMath(math) | Event::DisplayMath(math) => {
                state.append_raw(output, &math);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                state.link_stack.push(LinkState {
                    destination: dest_url.to_string(),
                    text: String::new(),
                });
            }
            Event::End(TagEnd::Link) => {
                if let Some(link) = state.link_stack.pop() {
                    let label = if link.text.is_empty() {
                        link.destination.clone()
                    } else {
                        link.text
                    };
                    let rendered = format!(
                        "{}",
                        format!("[{label}]({})", link.destination)
                            .underlined()
                            .with(self.color_theme.link)
                    );
                    state.append_raw(output, &rendered);
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                let rendered = format!(
                    "{}",
                    format!("[image:{dest_url}]").with(self.color_theme.link)
                );
                state.append_raw(output, &rendered);
            }
            Event::Start(Tag::Table(..)) => state.table = Some(TableState::default()),
            Event::End(TagEnd::Table) => {
                if let Some(table) = state.table.take() {
                    output.push_str(&self.render_table(&table));
                    output.push_str("\n\n");
                }
            }
            Event::Start(Tag::TableHead) => {
                if let Some(table) = state.table.as_mut() {
                    table.in_head = true;
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(table) = state.table.as_mut() {
                    table.finish_row();
                    table.in_head = false;
                }
            }
            Event::Start(Tag::TableRow) => {
                if let Some(table) = state.table.as_mut() {
                    table.current_row.clear();
                    table.current_cell.clear();
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(table) = state.table.as_mut() {
                    table.finish_row();
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(table) = state.table.as_mut() {
                    table.current_cell.clear();
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(table) = state.table.as_mut() {
                    table.push_cell();
                }
            }
            Event::Start(Tag::Paragraph | Tag::MetadataBlock(..) | _)
            | Event::End(TagEnd::Image | TagEnd::MetadataBlock(..) | _) => {}
        }
    }

    #[allow(clippy::unused_self)]
    fn start_heading(&self, state: &mut RenderState, level: u8, output: &mut String) {
        state.heading_level = Some(level);
        if !output.is_empty() {
            output.push('\n');
        }
    }

    fn start_quote(&self, state: &mut RenderState, output: &mut String) {
        state.quote += 1;
        let _ = write!(output, "{}", "│ ".with(self.color_theme.quote));
    }

    fn start_item(state: &mut RenderState, output: &mut String) {
        let depth = state.list_stack.len().saturating_sub(1);
        output.push_str(&"  ".repeat(depth));

        let marker = match state.list_stack.last_mut() {
            Some(ListKind::Ordered { next_index }) => {
                let value = *next_index;
                *next_index += 1;
                format!("{value}. ")
            }
            _ => "• ".to_string(),
        };
        output.push_str(&marker);
    }

    fn start_code_block(&self, code_language: &str, output: &mut String) {
        let label = if code_language.is_empty() {
            "code".to_string()
        } else {
            code_language.to_string()
        };
        let _ = writeln!(
            output,
            "{}",
            format!("╭─ {label}")
                .bold()
                .with(self.color_theme.code_block_border)
        );
    }

    fn finish_code_block(&self, code_buffer: &str, code_language: &str, output: &mut String) {
        output.push_str(&self.highlight_code(code_buffer, code_language));
        let _ = write!(
            output,
            "{}",
            "╰─".bold().with(self.color_theme.code_block_border)
        );
        output.push_str("\n\n");
    }

    fn push_text(
        &self,
        text: &str,
        state: &mut RenderState,
        output: &mut String,
        code_buffer: &mut String,
        in_code_block: bool,
    ) {
        if in_code_block {
            code_buffer.push_str(text);
        } else {
            state.append_styled(output, text, &self.color_theme);
        }
    }

    fn render_table(&self, table: &TableState) -> String {
        let mut rows = Vec::new();
        if !table.headers.is_empty() {
            rows.push(table.headers.clone());
        }
        rows.extend(table.rows.iter().cloned());

        if rows.is_empty() {
            return String::new();
        }

        let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
        let widths = (0..column_count)
            .map(|column| {
                rows.iter()
                    .filter_map(|row| row.get(column))
                    .map(|cell| visible_width(cell))
                    .max()
                    .unwrap_or(0)
            })
            .collect::<Vec<_>>();

        let border = format!("{}", "│".with(self.color_theme.table_border));
        let separator = widths
            .iter()
            .map(|width| "─".repeat(*width + 2))
            .collect::<Vec<_>>()
            .join(&format!("{}", "┼".with(self.color_theme.table_border)));
        let separator = format!("{border}{separator}{border}");

        let mut output = String::new();
        if !table.headers.is_empty() {
            output.push_str(&self.render_table_row(&table.headers, &widths, true));
            output.push('\n');
            output.push_str(&separator);
            if !table.rows.is_empty() {
                output.push('\n');
            }
        }

        for (index, row) in table.rows.iter().enumerate() {
            output.push_str(&self.render_table_row(row, &widths, false));
            if index + 1 < table.rows.len() {
                output.push('\n');
            }
        }

        output
    }

    fn render_table_row(&self, row: &[String], widths: &[usize], is_header: bool) -> String {
        let border = format!("{}", "│".with(self.color_theme.table_border));
        let mut line = String::new();
        line.push_str(&border);

        for (index, width) in widths.iter().enumerate() {
            let cell = row.get(index).map_or("", String::as_str);
            line.push(' ');
            if is_header {
                let _ = write!(line, "{}", cell.bold().with(self.color_theme.heading));
            } else {
                line.push_str(cell);
            }
            let padding = width.saturating_sub(visible_width(cell));
            line.push_str(&" ".repeat(padding + 1));
            line.push_str(&border);
        }

        line
    }

    #[must_use]
    pub fn highlight_code(&self, code: &str, language: &str) -> String {
        let syntax = self
            .syntax_set
            .find_syntax_by_token(language)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        let mut syntax_highlighter = HighlightLines::new(syntax, &self.syntax_theme);
        let mut colored_output = String::new();

        for line in LinesWithEndings::from(code) {
            match syntax_highlighter.highlight_line(line, &self.syntax_set) {
                Ok(ranges) => {
                    let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
                    colored_output.push_str(&apply_code_block_background(&escaped));
                }
                Err(_) => colored_output.push_str(&apply_code_block_background(line)),
            }
        }

        colored_output
    }

    pub fn stream_markdown(&self, markdown: &str, out: &mut impl Write) -> io::Result<()> {
        let rendered_markdown = wrap_ansi_to_terminal(&self.markdown_to_ansi(markdown));
        write!(out, "{rendered_markdown}")?;
        if !rendered_markdown.ends_with('\n') {
            writeln!(out)?;
        }
        out.flush()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MarkdownStreamState {
    pending: String,
}

impl MarkdownStreamState {
    #[must_use]
    pub fn push(&mut self, renderer: &TerminalRenderer, delta: &str) -> Option<String> {
        self.pending.push_str(delta);
        let split = find_stream_safe_boundary(&self.pending)?;
        let ready = self.pending[..split].to_string();
        self.pending.drain(..split);
        Some(renderer.markdown_to_ansi(&ready))
    }

    #[must_use]
    pub fn flush(&mut self, renderer: &TerminalRenderer) -> Option<String> {
        if self.pending.trim().is_empty() {
            self.pending.clear();
            None
        } else {
            let pending = std::mem::take(&mut self.pending);
            Some(renderer.markdown_to_ansi(&pending))
        }
    }
}

fn apply_code_block_background(line: &str) -> String {
    let trimmed = line.trim_end_matches('\n');
    let trailing_newline = if trimmed.len() == line.len() {
        ""
    } else {
        "\n"
    };
    let with_background = trimmed.replace("\u{1b}[0m", "\u{1b}[0;48;5;236m");
    format!("\u{1b}[48;5;236m{with_background}\u{1b}[0m{trailing_newline}")
}

fn find_stream_safe_boundary(markdown: &str) -> Option<usize> {
    let mut in_fence = false;
    let mut last_boundary = None;

    for (offset, line) in markdown.split_inclusive('\n').scan(0usize, |cursor, line| {
        let start = *cursor;
        *cursor += line.len();
        Some((start, line))
    }) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            if !in_fence {
                last_boundary = Some(offset + line.len());
            }
            continue;
        }

        if in_fence {
            continue;
        }

        if trimmed.is_empty() {
            last_boundary = Some(offset + line.len());
        }
    }

    last_boundary
}

fn visible_width(input: &str) -> usize {
    strip_ansi(input).chars().count()
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            output.push(ch);
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::{strip_ansi, MarkdownStreamState, Spinner, TerminalRenderer};

    #[test]
    fn renders_markdown_with_styling_and_lists() {
        let terminal_renderer = TerminalRenderer::new();
        let markdown_output = terminal_renderer
            .render_markdown("# Heading\n\nThis is **bold** and *italic*.\n\n- item\n\n`code`");

        assert!(markdown_output.contains("Heading"));
        assert!(markdown_output.contains("• item"));
        assert!(markdown_output.contains("code"));
        assert!(markdown_output.contains('\u{1b}'));
    }

    #[test]
    fn renders_links_as_colored_markdown_labels() {
        let terminal_renderer = TerminalRenderer::new();
        let markdown_output =
            terminal_renderer.render_markdown("See [Claw](https://example.com/docs) now.");
        let plain_text = strip_ansi(&markdown_output);

        assert!(plain_text.contains("[Claw](https://example.com/docs)"));
        assert!(markdown_output.contains('\u{1b}'));
    }

    #[test]
    fn highlights_fenced_code_blocks() {
        let terminal_renderer = TerminalRenderer::new();
        let markdown_output =
            terminal_renderer.markdown_to_ansi("```rust\nfn hi() { println!(\"hi\"); }\n```");
        let plain_text = strip_ansi(&markdown_output);

        assert!(plain_text.contains("╭─ rust"));
        assert!(plain_text.contains("fn hi"));
        assert!(markdown_output.contains('\u{1b}'));
        assert!(markdown_output.contains("[48;5;236m"));
    }

    #[test]
    fn renders_ordered_and_nested_lists() {
        let terminal_renderer = TerminalRenderer::new();
        let markdown_output =
            terminal_renderer.render_markdown("1. first\n2. second\n   - nested\n   - child");
        let plain_text = strip_ansi(&markdown_output);

        assert!(plain_text.contains("1. first"));
        assert!(plain_text.contains("2. second"));
        assert!(plain_text.contains("  • nested"));
        assert!(plain_text.contains("  • child"));
    }

    #[test]
    fn renders_tables_with_alignment() {
        let terminal_renderer = TerminalRenderer::new();
        let markdown_output = terminal_renderer
            .render_markdown("| Name | Value |\n| ---- | ----- |\n| alpha | 1 |\n| beta | 22 |");
        let plain_text = strip_ansi(&markdown_output);
        let lines = plain_text.lines().collect::<Vec<_>>();

        assert_eq!(lines[0], "│ Name  │ Value │");
        assert_eq!(lines[1], "│───────┼───────│");
        assert_eq!(lines[2], "│ alpha │ 1     │");
        assert_eq!(lines[3], "│ beta  │ 22    │");
        assert!(markdown_output.contains('\u{1b}'));
    }

    #[test]
    fn streaming_state_waits_for_complete_blocks() {
        let renderer = TerminalRenderer::new();
        let mut state = MarkdownStreamState::default();

        assert_eq!(state.push(&renderer, "# Heading"), None);
        let flushed = state
            .push(&renderer, "\n\nParagraph\n\n")
            .expect("completed block");
        let plain_text = strip_ansi(&flushed);
        assert!(plain_text.contains("Heading"));
        assert!(plain_text.contains("Paragraph"));

        assert_eq!(state.push(&renderer, "```rust\nfn main() {}\n"), None);
        let code = state
            .push(&renderer, "```\n")
            .expect("closed code fence flushes");
        assert!(strip_ansi(&code).contains("fn main()"));
    }

    #[test]
    fn wrap_leaves_short_text_alone() {
        assert_eq!(super::wrap_ansi_to_width("hello world", 40), "hello world");
    }

    #[test]
    fn wrap_breaks_on_word_boundary_not_midword() {
        let input = "the quick brown fox jumps over the lazy dog";
        let wrapped = super::wrap_ansi_to_width(input, 20);
        // Every line must end at a word boundary and be ≤ 20 visible chars.
        for line in wrapped.split('\n') {
            assert!(line.len() <= 20, "line too long ({}): {line:?}", line.len());
            // The wrap never breaks mid-word, so each word is contiguous.
            // Check: no word from the input is split across a line break.
        }
        // Original whitespace-separated words survive intact.
        for word in input.split_whitespace() {
            assert!(
                wrapped.contains(word),
                "word {word:?} got chopped: {wrapped:?}"
            );
        }
    }

    #[test]
    fn wrap_respects_existing_newlines() {
        let input = "paragraph one\n\nparagraph two goes here";
        let wrapped = super::wrap_ansi_to_width(input, 30);
        assert!(wrapped.contains("paragraph one\n\nparagraph two"));
    }

    #[test]
    fn wrap_does_not_count_ansi_toward_width() {
        // 20-char visible word wrapped with SGR around it — the whole
        // styled word fits within width 30 despite the escape bytes.
        let input = "\x1b[1;32mbold-green-word\x1b[0m and plain text";
        let wrapped = super::wrap_ansi_to_width(input, 30);
        assert!(wrapped.starts_with("\x1b[1;32mbold-green-word\x1b[0m"));
    }

    #[test]
    fn wrap_noops_for_absurdly_narrow_widths() {
        // Widths below MIN_WRAP_WIDTH return the input unchanged rather
        // than produce one-char-per-line garbage.
        let input = "the quick brown fox";
        assert_eq!(super::wrap_ansi_to_width(input, 3), input);
    }

    #[test]
    fn wrap_overflows_words_longer_than_width() {
        // A single word bigger than the width still emits in full — we
        // prefer an over-width line to mid-word hyphenation.
        let input = "supercalifragilisticexpialidocious is long";
        let wrapped = super::wrap_ansi_to_width(input, 20);
        assert!(wrapped.contains("supercalifragilisticexpialidocious"));
    }

    #[test]
    fn spinner_finish_prefixes_newline_then_marker() {
        let terminal_renderer = TerminalRenderer::new();
        let mut spinner = Spinner::new();
        let mut out = Vec::new();
        spinner
            .finish("Done", terminal_renderer.color_theme(), &mut out)
            .expect("finish succeeds");
        let output = String::from_utf8_lossy(&out);
        // Leading newline guarantees the marker never clobbers streamed
        // output left on the same terminal row.
        assert!(output.starts_with('\n'));
        assert!(output.contains("✔ Done"));
    }

    #[test]
    fn spinner_fail_prefixes_newline_then_marker() {
        let terminal_renderer = TerminalRenderer::new();
        let mut spinner = Spinner::new();
        let mut out = Vec::new();
        spinner
            .fail("Boom", terminal_renderer.color_theme(), &mut out)
            .expect("fail succeeds");
        let output = String::from_utf8_lossy(&out);
        assert!(output.starts_with('\n'));
        assert!(output.contains("✘ Boom"));
    }

    #[test]
    fn justify_pads_interior_lines_and_leaves_paragraph_tails_ragged() {
        // Two prose lines then a paragraph break — the first line is
        // interior and should be padded to exactly `width`. The second
        // closes the paragraph (next line is blank) so it stays ragged.
        let wrapped = "alpha beta gamma\ndelta epsilon\n\nnext paragraph";
        let out = super::justify_ansi_to_width(wrapped, 24);

        let mut lines = out.split('\n');
        let first = lines.next().expect("first");
        assert_eq!(super::visible_width(first), 24, "interior line padded");
        assert!(first.starts_with("alpha"), "first word untouched");

        let second = lines.next().expect("second");
        assert_eq!(
            super::visible_width(second),
            "delta epsilon".len(),
            "paragraph-tail line stays ragged"
        );
        assert_eq!(lines.next(), Some(""), "blank separator preserved");
    }

    #[test]
    fn justify_skips_tables_code_and_lists() {
        // Each line below is a non-prose line — justification must pass
        // them through verbatim even if they sit in the interior of the
        // buffer.
        let inputs = [
            "│ header │ value │\nbody",
            "• bullet one\nbody",
            "1. ordered one\nbody",
            "# heading\nbody",
        ];
        for input in inputs {
            let out = super::justify_ansi_to_width(input, 80);
            let first = out.split('\n').next().expect("first line");
            let original_first = input.split('\n').next().expect("first line input");
            assert_eq!(first, original_first, "non-prose line preserved: {input:?}");
        }
    }

    #[test]
    fn assistant_body_emits_bullet_once_and_indents_continuations() {
        let mut flag = false;
        let first = super::wrap_assistant_body_to_width("first paragraph", 80, &mut flag);
        assert!(flag, "bullet state flipped after first emission");
        assert!(
            first.contains('●'),
            "first chunk carries the gutter bullet: {first:?}"
        );

        let second = super::wrap_assistant_body_to_width("second paragraph", 80, &mut flag);
        assert!(
            !second.contains('●'),
            "second chunk must not repaint the bullet: {second:?}"
        );
        // Continuation chunks indent by 2 spaces to sit under the bullet.
        assert!(
            second.starts_with("  second"),
            "continuation chunk indents: {second:?}"
        );
    }
}
