//! LBTerm: the CLI is built as `lbterm` and says so. `CARGO_BIN_EXE_lbterm`
//! only exists if the `[[bin]]` in Cargo.toml is named `lbterm`, so a wrong
//! name fails to compile.
use std::process::Command;

fn run(arg: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_lbterm"))
        .arg(arg)
        .output()
        .expect("run lbterm");
    // Positional arguments: this crate is edition 2018, where a lone
    // assert message string isn't formatted.
    assert!(out.status.success(), "lbterm {} failed: {:?}", arg, out);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn version_names_lbterm() {
    let text = run("--version");
    assert!(text.starts_with("lbterm "), "--version printed {:?}", text);
}

#[test]
fn help_says_it_is_a_wezterm_fork() {
    let text = run("--help");
    assert!(
        text.contains("LBTerm, a fork of WezTerm"),
        "--help printed {:?}",
        text
    );
}

fn run_args(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lbterm"))
        .args(args)
        .output()
        .expect("run lbterm")
}

// Conductor (Life\Projects\Conductor, conductor.ts canSpawnQuietly) looks
// for the flag in this help before passing it
#[test]
fn spawn_help_lists_no_activate() {
    let out = run_args(&["cli", "spawn", "--help"]);
    assert!(out.status.success(), "spawn --help failed: {:?}", out);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("--no-activate"), "spawn --help printed {:?}", text);
}

// A new window has nothing to stay on. Refused before any connection
#[test]
fn spawn_no_activate_refuses_a_new_window() {
    let out = run_args(&["cli", "spawn", "--no-activate", "--new-window"]);
    assert!(!out.status.success(), "it ran: {:?}", out);
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("cannot be used with"), "it printed {:?}", text);
}
