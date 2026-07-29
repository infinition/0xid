// Retro hacker sound effects via Windows Beep API.
// All sounds are non-blocking (spawned on background threads).

use std::sync::atomic::{AtomicBool, Ordering};

/// Global mute flag. Persisted to `<data_dir>/settings.json` so it survives
/// restarts. Toggled with the [M] key.
static MUTED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
extern "system" {
    fn Beep(dwFreq: u32, dwDuration: u32) -> i32;
}

#[cfg(windows)]
fn tone(freq: u32, dur: u32) {
    unsafe {
        Beep(freq, dur);
    }
}

#[cfg(not(windows))]
fn tone(_freq: u32, _dur: u32) {}

fn play(f: impl FnOnce() + Send + 'static) {
    if MUTED.load(Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(f);
}

// ── Mute state + persistence (backed by `settings`) ─────────────────────────

/// Sync the in-memory mute flag from persisted settings. Call once at startup,
/// before any sound is played.
pub fn init_muted() {
    MUTED.store(crate::settings::get().muted, Ordering::Relaxed);
}

pub fn is_muted() -> bool {
    MUTED.load(Ordering::Relaxed)
}

/// Force a specific mute state, persist it.
pub fn set_muted(muted: bool) {
    let mut s = crate::settings::get();
    s.muted = muted;
    crate::settings::save(&s);
    MUTED.store(muted, Ordering::Relaxed);
}

/// Flip mute on/off, persist it, and return the new state.
pub fn toggle_muted() -> bool {
    let muted = !is_muted();
    set_muted(muted);
    muted
}

/// Quick tick when navigating up/down
pub fn nav() {
    play(|| tone(1800, 15));
}

/// Ascending sweep when entering a folder
pub fn enter() {
    play(|| {
        tone(600, 25);
        tone(900, 25);
        tone(1200, 35);
    });
}

/// Descending sweep when going back
pub fn back() {
    play(|| {
        tone(1200, 25);
        tone(900, 25);
        tone(600, 35);
    });
}

/// Dramatic sequence when launching a plugin
pub fn launch() {
    play(|| {
        tone(800, 35);
        tone(1200, 35);
        tone(1600, 50);
        tone(2200, 70);
    });
}

/// Low buzz on error
pub fn error() {
    play(|| tone(200, 120));
}

/// Boot-up sequence
pub fn boot() {
    play(|| {
        tone(400, 40);
        tone(600, 40);
        tone(800, 40);
        tone(1000, 40);
        tone(1400, 60);
        tone(1800, 80);
    });
}
