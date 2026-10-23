# Notifications (OSC 99)

LBTerm shows kitty's desktop notifications ([the protocol](https://sw.kovidgoyal.net/kitty/desktop-notifications/)) as Windows toasts. A program sends `ESC ] 99 ; key=value:key=value ; payload ESC \`; LBTerm answers what the protocol asks of a terminal. WezTerm has only OSC 9 and 777: a title and a body, and a click that does nothing.

Claude Code speaks it on its `kitty` notification channel, which Lachlan turned on on 2026-10-20 (`"preferredNotifChannel": "kitty"` in `~/.claude/settings.json`; it was `terminal_bell`). It writes three chunks: the title (`d=0`), the body (done), then an empty `a=focus` chunk after the notification is complete, which LBTerm drops rather than showing an empty toast (`claude_codes_three_chunks_make_one_toast` in `lbterm-notify`). A click on the toast brings the session's pane forward. WezTerm ignores OSC 99, so WezTerm sessions lost only the system beep the bell gave; claudehook's ding on a permission prompt plays in both terminals.

## What's supported

| | |
|---|---|
| Title and body (`p=title`, `p=body`), in chunks (`d=0` until the last), plain or base64 (`e=1`) | yes; control characters are dropped, newlines become spaces, 8 KB each |
| Click (`a=`) | `focus` (the default) brings the window, tab and pane forward; `report` writes `ESC ] 99 ; i=<id> ; ESC \` to the program; both at once work |
| Buttons (`p=buttons`, labels split by U+2028) | up to five, Windows' limit; a click reports `ESC ] 99 ; i=<id> ; <n> ESC \` with `a=report` |
| When (`o=`) | `always`; `unfocused` (only while the window hasn't the keyboard); `invisible` (only while unfocused or the pane isn't showing) |
| Urgency (`u=`) | 0 is silent and short, 1 short, 2 stays up longer |
| Sound (`s=`) | `silent` is silent; anything else is Windows' sound |
| Expiry (`w=`) | above 0, LBTerm takes the toast down itself after that many ms |
| Update and close (`i=`, `p=close`) | a toast with the same id replaces the one up; `p=close` takes it down |
| Close events (`c=1`) | `ESC ] 99 ; i=<id>:p=close ; ESC \` when it goes, however it goes |
| Alive (`p=alive`) | the ids still up, from this pane |
| Query (`p=?`) | answered by the terminal itself, at once |
| Icons (`n=`, `p=icon`, `g=`), types (`t=`) | taken and ignored: Windows shows LBTerm's own icon |

Ids are stripped to `[A-Za-z0-9_+.-]` before they're echoed, so nothing a program sends can come back as an escape sequence.

## How it's built

- `lbterm-notify`: parsing a sequence, putting a pane's chunks together (`Assembler`, at most 32 in progress), and every reply. `cargo test -p lbterm-notify`.
- The terminal (`term/src/terminalstate/performer.rs`): OSC 99 arrives as an OSC the parser doesn't know. A query is answered there; the rest becomes `Alert::KittyNotification`. `term/src/test/notify.rs` tests it through the whole terminal.
- The window holding the pane (`wezterm-gui/src/termwindow/notify.rs`) decides on `o=`, shows the toast and acts on what's done with it. Toasts (`wezterm-gui/src/toasts.rs`) carry a tag of pane and id, so programs in two panes never collide, and each has a serial number, so a replaced toast's late Dismissed isn't taken for its successor's.
- Toasts go out under LBTerm's app id, `com.lachlanb.lbterm`. `lbterm\perf\notify.ps1` sends one from a test window and reads the log: `Windows says Ok(0)` means Windows shows them.
