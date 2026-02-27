//! Launch command — start INFINITAS and apply borderless window mode.

use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use infst::ProcessHandle;
use infst::input::window;

const LOGIN_PAGE_URL: &str = "https://p.eagate.573.jp/game/2dx/infinitas/top/index.html";
const WINDOW_POLL_INTERVAL: Duration = Duration::from_millis(500);
const WINDOW_POLL_TIMEOUT: Duration = Duration::from_secs(60);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_secs(2);

pub fn run(url: Option<&str>, pid: Option<u32>, timeout_secs: u64) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    eprintln!("infst {} - Launch (Borderless)", current_version);

    let process = find_or_launch_process(url, pid, timeout_secs)?;
    eprintln!("Game process found (PID: {})", process.pid);

    wait_and_apply_borderless(&process)?;

    eprintln!("Done!");
    Ok(())
}

/// Find a running game process, or launch one and wait for it.
fn find_or_launch_process(
    url: Option<&str>,
    pid: Option<u32>,
    timeout_secs: u64,
) -> Result<ProcessHandle> {
    // Explicit PID — just open it
    if let Some(pid) = pid {
        return Ok(ProcessHandle::open(pid)?);
    }

    // Already running — use it
    if let Ok(process) = ProcessHandle::find_and_open() {
        eprintln!("Game is already running");
        return Ok(process);
    }

    // Not running — launch or instruct
    match url {
        Some(uri) => {
            eprintln!("Launching game via URI...");
            open::that(uri)?;
        }
        None => {
            eprintln!("Game is not running. Opening login page...");
            open::that(LOGIN_PAGE_URL)?;
            eprintln!("Please log in and launch the game from the browser.");
        }
    }

    // Wait for the process to appear
    eprintln!("Waiting for game process (timeout: {}s)...", timeout_secs);
    let timeout = Duration::from_secs(timeout_secs);
    let start = Instant::now();

    loop {
        if start.elapsed() > timeout {
            bail!("Timed out waiting for game process after {}s", timeout_secs);
        }

        if let Ok(process) = ProcessHandle::find_and_open() {
            return Ok(process);
        }

        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

/// Poll until a visible window appears for the process, then apply borderless mode.
#[cfg(target_os = "windows")]
fn wait_and_apply_borderless(process: &ProcessHandle) -> Result<()> {
    eprintln!("Waiting for game window...");
    let start = Instant::now();

    let hwnd = loop {
        if start.elapsed() > WINDOW_POLL_TIMEOUT {
            bail!("Timed out waiting for game window");
        }

        if !process.is_alive() {
            bail!("Game process exited before a window appeared");
        }

        if let Ok(hwnd) = window::find_window_by_pid(process.pid) {
            break hwnd;
        }

        thread::sleep(WINDOW_POLL_INTERVAL);
    };

    eprintln!("Game window found");
    eprintln!("Applying borderless window mode...");
    window::apply_borderless(hwnd)
}

#[cfg(not(target_os = "windows"))]
fn wait_and_apply_borderless(_process: &ProcessHandle) -> Result<()> {
    bail!("Borderless window mode is only supported on Windows");
}
