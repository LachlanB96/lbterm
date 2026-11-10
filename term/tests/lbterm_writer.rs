//! LBTerm: what a pane's `writer()` writes keeps its place behind the keys the terminal was given
//! (TerminalState::lbterm_writer, which mux's localpane.rs maps a pane's writer to).
//!
//! The terminal writes its program's input (a key, a paste, a reply) on a thread of its own
//! (terminalstate/mod.rs, ThreadedWriter). Upstream's pane writer wrote to the pty on the caller's
//! thread instead, so a binding's SendString, Lachlan's Shift+Enter, pressed just after a letter
//! reached the program first whenever that thread hadn't run yet: on a busy mux server a letter
//! and F5's SendString swapped in 3 to 24 of 40 pairs (perf/mux-client, `perf/mux-gui.ps1`'s
//! write part). These hold the writer thread on its first write, as a busy PC does.

use std::io::Write;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wezterm_term::color::ColorPalette;
use wezterm_term::{KeyCode, KeyModifiers, Terminal, TerminalConfiguration, TerminalSize};

#[derive(Debug)]
struct Config;

impl TerminalConfiguration for Config {
    fn scrollback_size(&self) -> usize {
        100
    }
    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

/// The pty's input: what reached it, its first write held until `open` says so
struct Pty {
    got: Arc<Mutex<Vec<u8>>>,
    open: Option<Receiver<()>>,
}

impl Write for Pty {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(open) = self.open.take() {
            open.recv().ok();
        }
        self.got.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn terminal(pty: Pty) -> Terminal {
    Terminal::new(
        TerminalSize {
            rows: 24,
            cols: 80,
            pixel_width: 640,
            pixel_height: 384,
            dpi: 0,
        },
        Arc::new(Config),
        "WezTerm",
        "lbterm-test",
        Box::new(pty),
    )
}

/// What reached the pty once it holds `len` bytes, or after 10 s
fn reached(got: &Arc<Mutex<Vec<u8>>>, len: usize) -> String {
    let start = Instant::now();
    loop {
        let now = got.lock().unwrap().clone();
        if now.len() >= len || start.elapsed() > Duration::from_secs(10) {
            return String::from_utf8_lossy(&now).into_owned();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_write_keeps_its_place_behind_a_key_its_thread_hasnt_written_yet() {
    let got = Arc::new(Mutex::new(Vec::new()));
    let (open, held) = channel();
    let mut term = terminal(Pty {
        got: Arc::clone(&got),
        open: Some(held),
    });
    term.key_down(KeyCode::Char('a'), KeyModifiers::NONE).unwrap();
    term.lbterm_writer().write_all(b"b").unwrap();
    term.lbterm_writer().flush().unwrap();
    // Nothing reaches the pty while its thread is held on the key
    std::thread::sleep(Duration::from_millis(50));
    assert!(got.lock().unwrap().is_empty(), "the hold didn't hold");
    open.send(()).unwrap();
    assert_eq!(reached(&got, 2), "ab");
}

/// Upstream's way, for contrast: the same write made to the pty itself goes ahead of the key
/// still on the terminal's thread, which is why the test above holds anything
#[test]
fn a_write_to_the_pty_itself_goes_ahead_of_a_key_its_thread_hasnt_written_yet() {
    let got = Arc::new(Mutex::new(Vec::new()));
    let (open, held) = channel();
    let mut term = terminal(Pty {
        got: Arc::clone(&got),
        open: Some(held),
    });
    let mut pty = Pty {
        got: Arc::clone(&got),
        open: None,
    };
    term.key_down(KeyCode::Char('a'), KeyModifiers::NONE).unwrap();
    pty.write_all(b"b").unwrap();
    open.send(()).unwrap();
    assert_eq!(reached(&got, 2), "ba");
}
