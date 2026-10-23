# Tabs that outlive the GUI (the mux domain)

With the mux domain on, LBTerm's tabs don't live in the window's process. They live in `lbterm-mux-server`, and the GUI attaches to them over a socket, as `wezterm connect` does. When the GUI dies (a crash, a GPU driver update taking the window, the process killed, `lbterm.ps1 restart` after an install), every tab keeps running, Claude Code included, and the next LBTerm attaches to them where they were.

Closing a window, or quitting, still ends its tabs. That's LBTerm's own choice: WezTerm detaches instead, leaving everything running in the background. A reboot ends the server and its tabs; crash restore (`RESTORE.md`) offers them again then.

## Turning it on

It's in the config, not the build. Lachlan's `Terminal\lbterm\lbterm.lua` has `MUX = true`, which comes to:

```lua
config.unix_domains = { { name = 'unix' } }
config.default_gui_startup_args = { 'connect', 'unix' }
```

The GUI then starts by connecting to the `unix` domain, starting `lbterm-mux-server --daemonize` beside its own exe when nothing answers on the socket. The server reads the same config and listens on the domain's default socket, `~\.local\share\lbterm\sock`, which `lbterm.ps1 install` and `restart` assume. Changing it takes an LBTerm restart, and that ends the tabs it had.

## What LBTerm changes to make it work

Upstream WezTerm has the mux. These are the gaps that mattered for LBTerm's setup:

- **Closing ends the tabs.** `Mux::remove_window_internal` no longer detaches the window's domains, so each of its panes is killed, and `ClientPane::kill` sends the kill to the server. It holds an `Activity` until the server has it (at most 3 s), because the GUI exits once the mux is empty and no Activity is held. Quit (`frontend::quit_application`, from QuitApplication with or without its prompt) closes every window first, then ends the message loop when those kills are done. `DetachDomain` still detaches.
- **The server ends with its last tab.** `lbterm-mux-server` exits a second after its last pane has gone, so a closed LBTerm leaves nothing running, and the next one starts the server from the build installed now.
- **A GUI that attaches gets what the panes already have.** The server only sends changes, so a reattached pane used to have no user vars (claudehook's `claude_status` and `claude_session`), no OSC 9;4 progress and no OSC 7501 records until each next changed. The first time a client looks at a pane (`sent_initial_state` in `wezterm-mux-server-impl/src/sessionhandler.rs`), the server sends its user vars, progress and records ahead of anything else.
- **OSC 7501 crosses the socket.** `Alert::ProgramStatus` only says the records changed, and only a local pane can read them. The server now sends the records themselves in `SetProgramStatus` (`codec/src/lib.rs`, PDU 7501, numbered well clear of upstream's), and `ClientPane` keeps them for `get_program_status()`, so the tab panel and Lua see them. The protocol change is additive, so `CODEC_VERSION` stays upstream's: a peer that doesn't know the PDU decodes it as `Pdu::Invalid`.
- **The server's pane ids, in Lua.** A pane's `WEZTERM_PANE`, and the ids `lbterm cli` inside it uses, are the server's. `pane:pane_id()` in the GUI is the GUI's own number for the pane, and differs. `pane:get_metadata().remote_pane_id` is the server's (nil for a local pane). Terminal's summary pane names its files by it.

What needed nothing: OSC 99 notifications, the taskbar progress, triggers and the scrollbar marks all go through alerts, lines and seqnos, which the mux already carries.

## Living with it

- **Installing.** A running GUI and server keep their files open. `lbterm.ps1 install` renames each in-use file aside (`<name>.<stamp>.old`, deleted by the next install once nothing holds it) and copies the new one in. The running GUI and server go on with the old build. `lbterm.ps1 restart` kills the GUI and opens the installed one, which attaches to the same tabs. It refuses when the GUI has tabs of its own outside the server (started without the domain), since those would end with it. Install also writes the build's version to `version.txt` and keeps the build it replaces whole in `%LOCALAPPDATA%\Programs\LBTerm-previous`; `lbterm.ps1 rollback` swaps the two and restarts the GUI into the kept one, checking first that it speaks the running server's protocol.
- **The server keeps the old build** until its last tab closes: changes to the terminal itself (escape sequences, OSC 7501, triggers' matching) reach existing tabs only then. GUI changes (drawing, the tab panel, keys) take effect at `restart`.
- **Protocol changes.** An upstream merge that bumps `CODEC_VERSION` makes the new GUI unable to talk to a running older server. `install` checks for that with the new `lbterm cli` against the running server, and refuses: close LBTerm (which ends its tabs) and install again.
- **The cli.** Inside a pane, `WEZTERM_UNIX_SOCKET` is the server's socket, so `lbterm cli` there works on the server, with the server's pane ids. A `cli` call without `--no-auto-start` from where no server is running starts one, with a tab of its own, which the next LBTerm then shows.
- **Crash restore** waits 2 s for the attach when it finds panes in a mux domain, then offers only the noted tabs that aren't live (matched by `claude_session`, then by folder). After a GUI-only restart that's none.

## Checking it

`pwsh -File lbterm\perf\mux.ps1` runs all of it in test windows of their own class, on a domain and socket of their own (`target\perf\mux`), so the real server is never touched: a Claude-ish tab's user vars, OSC 7501 record and progress reach the GUI; the GUI is killed and the server keeps both tabs; a new GUI shows them with all of that again (`mux-reattached.png`) and offers no restore; WM_CLOSE ends the tabs and the server; a third GUI's QuitApplication does the same. The OSC 7501 PDU's encoding is tested in `codec` (`lbterm_program_status_round_trip`).
