//! Animated "thinking" spinner that actually animates.
//!
//! The old one-shot `render::Spinner::tick` painted a single frame before
//! the turn's blocking `run_turn` call, so it looked frozen. This module
//! spawns a dedicated thread that repaints the frame every ~80 ms until
//! the run_turn finishes *or* the first streamed text token arrives.
//!
//! ## Coordination
//!
//! A process-wide `OnceLock<Mutex<Option<Arc<AtomicBool>>>>` holds the
//! stop flag of the currently-active spinner. The CLI's `run_turn`
//! registers a spinner at start and clears it at end; the streaming
//! client pokes `stop_active()` on its first text delta so the spinner
//! never fights with streamed output.
//!
//! ## Safety
//!
//! - Writes to stdout are already mutex-guarded by `std::io::Stdout` so
//!   the spinner's paint interleaves safely with streamed output.
//! - `SavePosition`/`RestorePosition` brackets the frame so the cursor
//!   returns to wherever the stream writer had it.
//! - On stop, we clear the current line and flush — the ✔ Done marker
//!   printed next lands on a clean row.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossterm::cursor::{MoveToColumn, RestorePosition, SavePosition};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use crossterm::{execute, queue};

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const FRAME_INTERVAL: Duration = Duration::from_millis(80);

/// A running spinner. Dropping it is a best-effort stop + join.
pub struct SpinnerHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SpinnerHandle {
    fn spawn(label: String) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let thread = thread::spawn(move || run_spinner(label, &stop_clone));
        Self {
            stop,
            thread: Some(thread),
        }
    }

    /// Stop the spinner and wait for its thread to finish painting.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SpinnerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_spinner(label: String, stop: &AtomicBool) {
    let mut stdout = io::stdout();
    let mut idx: usize = 0;
    // Paint once immediately so the user sees the spinner on the first
    // frame, not after the initial 80 ms sleep.
    paint(&mut stdout, FRAMES[idx % FRAMES.len()], &label);

    while !stop.load(Ordering::SeqCst) {
        thread::sleep(FRAME_INTERVAL);
        if stop.load(Ordering::SeqCst) {
            break;
        }
        idx = idx.wrapping_add(1);
        paint(&mut stdout, FRAMES[idx % FRAMES.len()], &label);
    }

    // Clear the line on stop so whatever prints next starts clean.
    let _ = execute!(
        stdout,
        SavePosition,
        MoveToColumn(0),
        Clear(ClearType::CurrentLine),
        RestorePosition
    );
    let _ = stdout.flush();
}

fn paint<W: Write>(out: &mut W, frame: &str, label: &str) {
    let _ = queue!(
        out,
        SavePosition,
        MoveToColumn(0),
        Clear(ClearType::CurrentLine),
        SetForegroundColor(Color::Blue),
        Print(format!("{frame} {label}")),
        ResetColor,
        RestorePosition
    );
    let _ = out.flush();
}

// =====================================================================
// Process-wide "active spinner" handle
// =====================================================================

fn active_slot() -> &'static Mutex<Option<Arc<AtomicBool>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Stop whatever spinner is currently running in this process (if any).
/// Safe to call from any thread; called from the streaming client when
/// the first text token lands so the spinner doesn't overwrite output.
pub fn stop_active() {
    if let Ok(mut guard) = active_slot().lock() {
        if let Some(flag) = guard.take() {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

/// Start an animated spinner for the duration of a turn. The returned
/// handle is registered as the process-wide active spinner; `stop_active`
/// (called from the streaming client on first text token) will stop it,
/// or dropping the handle will stop it.
pub fn start_turn(label: impl Into<String>) -> SpinnerHandle {
    let handle = SpinnerHandle::spawn(label.into());
    if let Ok(mut guard) = active_slot().lock() {
        // Replace any prior registration — there should not be one, but
        // if there is, clearing it prevents a leak.
        if let Some(prior) = guard.replace(Arc::clone(&handle.stop)) {
            prior.store(true, Ordering::SeqCst);
        }
    }
    handle
}
