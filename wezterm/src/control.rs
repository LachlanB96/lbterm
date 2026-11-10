//! LBTerm: `lbterm control`, the GUI's control pipe (lbterm/CONTROL.md) from a shell. One
//! request; its reply printed as the GUI wrote it, one JSON line. Exits 1 when the GUI says no
//! (the reply's `error` on stderr too), when no LBTerm of that class is running, or when it
//! doesn't answer in [`WAIT`].

use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

/// The longest the reply is waited for: the GUI gives up on its own thread after 5 s
const WAIT: Duration = Duration::from_secs(10);
/// A busy pipe (every instance taken) is tried this many times, this far apart
const BUSY_TRIES: u32 = 10;
const BUSY_WAIT: Duration = Duration::from_millis(30);

#[derive(Debug, Parser, Clone)]
pub struct ControlCommand {
    /// The window class of the LBTerm to ask: the daily one unless given (a try window's is
    /// com.lachlanb.lbterm.try)
    #[arg(long)]
    class: Option<String>,

    #[command(subcommand)]
    op: ControlOp,
}

#[derive(Debug, Subcommand, Clone)]
enum ControlOp {
    /// Where he is: the window last used, its active pane (the mux server's id, as `lbterm cli`
    /// takes it), the Claude session in it, its title, and whether LBTerm has the keyboard
    Focus,
    /// Run a Lua event in that window with its active pane, as a key bound to EmitEvent would
    /// (lbterm-tab-forward is the mouse's Forward button in Lachlan's config)
    Emit {
        /// The event's name, as the config's wezterm.on() gives it
        event: String,
    },
    /// The mic mark on the window strip: off, listening, hearing or muted, for --ttl seconds
    /// (30 unless given), with what was heard said on the strip for a moment
    Mic {
        state: String,
        #[arg(long)]
        ttl: Option<f64>,
        #[arg(long)]
        heard: Option<String>,
    },
    /// What the GUI has done and what it cost since it started, or since the last --reset
    /// (lbterm/PERF.md): frames and who asked for them, messages, tasks, timers, the mux
    /// client's PDUs, the process's CPU and memory, upstream's histograms
    Metrics {
        /// Start the counts again after this reply. The daily LBTerm's counts are shared by
        /// everything that reads them, so it refuses unless --force is given too
        #[arg(long)]
        reset: bool,
        /// Reset the daily LBTerm's counts all the same
        #[arg(long)]
        force: bool,
        /// Time its frames to the screen and walk its motion (on), or stop (off), from now on
        /// (lbterm/PERF.md, "What's hard to time")
        #[arg(long, value_parser = ["on", "off"])]
        present: Option<String>,
    },
    /// A voice draft over a pane: held there, drawn above Claude Code's prompt, and typed only
    /// when it's sent
    Draft {
        /// The pane, by the id `lbterm cli` takes; the open one when left out
        #[arg(long)]
        pane: Option<u64>,
        #[command(subcommand)]
        act: DraftCmd,
    },
}

#[derive(Debug, Subcommand, Clone)]
enum DraftCmd {
    /// What the draft says now
    Get,
    /// Add a piece of an answer (\n for a new line)
    Add {
        text: String,
        /// Start the draft again with it
        #[arg(long)]
        fresh: bool,
    },
    /// Picks for an AskUserQuestion picker, in place of what's there
    Picker {
        /// What they read as: "1a 2b"
        #[arg(long)]
        label: String,
        /// The keys that make them, in order: --key 1 --key 2
        #[arg(long = "key")]
        keys: Vec<String>,
        /// The picker's questions, the first by which it's known on screen
        #[arg(long = "question")]
        questions: Vec<String>,
        /// Questions without a pick yet
        #[arg(long)]
        missing: Vec<String>,
    },
    /// Take the newest piece back
    Scratch,
    /// Let the draft go
    Clear,
    /// Type it and send it (only into Claude Code's prompt, or the picker it's for)
    Send,
    /// Type it into the prompt without sending it, to finish by hand
    Edit,
}

impl ControlCommand {
    pub fn run(&self) -> anyhow::Result<()> {
        let class = self
            .class
            .as_deref()
            .unwrap_or(wezterm_gui_subcommands::DEFAULT_WINDOW_CLASS);
        let pipe = config::lbterm::control_pipe(class);
        let request = match &self.op {
            ControlOp::Focus => json!({ "id": 1, "op": "focus" }),
            ControlOp::Emit { event } => json!({ "id": 1, "op": "emit", "event": event }),
            ControlOp::Mic { state, ttl, heard } => {
                json!({ "id": 1, "op": "mic", "state": state, "ttl": ttl, "heard": heard })
            }
            ControlOp::Metrics { reset, force, present } => {
                json!({ "id": 1, "op": "metrics", "reset": reset, "force": force, "present": present })
            }
            ControlOp::Draft { pane, act } => {
                let mut request = match act {
                    DraftCmd::Get => json!({ "op": "draft.get" }),
                    DraftCmd::Add { text, fresh } => json!({
                        "op": "draft.add",
                        "text": text.replace("\\n", "\n"),
                        "fresh": fresh,
                    }),
                    DraftCmd::Picker {
                        label,
                        keys,
                        questions,
                        missing,
                    } => json!({
                        "op": "draft.picker",
                        "label": label,
                        "keys": keys,
                        "questions": questions,
                        "missing": missing,
                    }),
                    DraftCmd::Scratch => json!({ "op": "draft.scratch" }),
                    DraftCmd::Clear => json!({ "op": "draft.clear" }),
                    DraftCmd::Send => json!({ "op": "draft.send" }),
                    DraftCmd::Edit => json!({ "op": "draft.edit" }),
                };
                request["id"] = json!(1);
                request["pane"] = json!(pane);
                request
            }
        };
        let mut file = match open(&pipe) {
            Ok(file) => file,
            // Expected whenever LBTerm is closed: said plainly, not as a crash
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("no LBTerm of class {class} is running (nothing listens on {pipe})");
                std::process::exit(1);
            }
            Err(err) => anyhow::bail!("can't open {pipe}: {err}"),
        };
        file.write_all(format!("{request}\n").as_bytes())?;

        // A GUI that's stopped answering would leave the read blocked for good
        std::thread::spawn(|| {
            std::thread::sleep(WAIT);
            eprintln!("LBTerm didn't answer in {} s", WAIT.as_secs());
            std::process::exit(1);
        });
        let mut line = String::new();
        BufReader::new(file).read_line(&mut line)?;
        if line.trim().is_empty() {
            anyhow::bail!("LBTerm closed the pipe without answering");
        }
        println!("{}", line.trim_end());
        let reply: Value = serde_json::from_str(&line)?;
        if reply.get("ok") != Some(&Value::Bool(true)) {
            let error = reply.get("error").and_then(Value::as_str).unwrap_or("it said no");
            eprintln!("{error}");
            std::process::exit(1);
        }
        Ok(())
    }
}

fn open(pipe: &str) -> std::io::Result<std::fs::File> {
    let mut tries = 0;
    loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(pipe) {
            // ERROR_PIPE_BUSY: every instance is taken, and the GUI is making the next
            Err(err) if err.raw_os_error() == Some(231) && tries < BUSY_TRIES => {
                tries += 1;
                std::thread::sleep(BUSY_WAIT);
            }
            result => return result,
        }
    }
}
