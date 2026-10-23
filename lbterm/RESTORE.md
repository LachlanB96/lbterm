# Crash restore

When LBTerm ends without closing cleanly (a crash, the process killed, Windows restarting for an update), the next LBTerm asks in its first window which of the old tabs to open again: all of them (Enter, or "Open all"), the ones picked (Space, A, N, a click on a row, then Enter or "Open picked"), or none (Esc, or "Don't open").

A tab comes back as one pane in its folder. A tab that was running Claude Code comes back as `claude --resume <session>` in a pwsh that stays open afterwards, the same command AI Dashboard's resume runs, so the conversation carries on where it stopped and its hooks (summary pane, status light) start as usual. Splits aren't rebuilt: a tab's other panes (the summary pane under a Claude session) come back on their own or not at all.

`config.lbterm_crash_restore = false` turns it off.

## The note

LBTerm keeps a note of its tabs in `%APPDATA%\lbterm\session-<window class>.json` (its data folder), rewritten a couple of seconds after the tabs change: a tab opens or closes, a pane's folder changes, a Claude session starts or ends in one. It's written on a thread of its own, beside the old one and renamed over it, and only when it differs from the last. Each tab is noted from its Claude Code pane if it has one, else from its active pane: title (without Claude's status glyph), folder, and session id.

The session id is the `claude_session` user var that Terminal's `claudehook.exe` sets in the pane: at SessionStart (`claudehook session start`), and with every working or waiting status, emptied at SessionEnd (`claudehook session end`). So a session that ended normally isn't resumed, only reopened as a shell in its folder.

Closing LBTerm normally (its last window, or Quit) marks the note clean, and nothing is offered next time. The note also holds the pid and start time of the LBTerm that wrote it, so a second LBTerm with the same window class running at the same time isn't taken for a crash.

With panes in a mux server (a unix domain, `lbterm-mux-server`), a GUI crash leaves them running and the next GUI reattaches to them, so they aren't offered again: a noted tab is left out if a pane that outlived the GUI has its Claude session, or (for a tab without one) its folder, each such pane standing for one tab. Nothing is offered when they're all still open. Reattached panes' user vars arrive a moment after the panes, so with any of them the check waits two seconds. Closing the GUI then marks the note clean only if no panes live on in such a domain: if the server dies later (a restart), the next start still offers them.

Until the prompt is answered, the new LBTerm writes no note of its own: closing it without answering leaves the old note, and the next start asks again. Test windows use classes of their own (`com.lachlanb.lbterm.perf…`), so they never see or touch the real note.

## Where it is

- `lbterm-extras/src/restore.rs`: the note, when to offer it, the command a tab runs, a row's words, the picks (tested).
- `wezterm-gui/src/restore.rs`: start-up, writing the note, the clean exit, opening the tabs. `overlay/restore.rs`: the prompt.
- `lbterm\perf\restore.ps1`: the whole thing in test windows. `LBTERM_RESTORE_CLAUDE` names a stub `claude.cmd` that only echoes its arguments, so a test never starts a real Claude Code.
