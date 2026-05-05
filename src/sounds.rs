// Retro hacker sound effects via Windows Beep API.
// All sounds are non-blocking (spawned on background threads).

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
    std::thread::spawn(f);
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
