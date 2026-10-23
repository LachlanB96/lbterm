# Tabs with the mouse

Everything done to a tab can be done with the mouse, in the tab panel or the tab bar when it shows tabs.

- **Move a tab:** press on it and drag along the panel (or the bar). It moves to the slot under the pointer as it goes, as the tab bar's own moves do. A press that wobbles less than 6 px is still a click. On the panel the row is lifted (built 2026-10-21): it follows the pointer with a soft shadow, over the others, which slide out of its way, and slides into its slot when it's let go (`lbterm/TAB-PANEL.md`).
- **Give it a window of its own:** drag it out of the window and let go at least 24 px away from it. The new window opens with its top-left where you let go, and takes the keyboard. Let go nearer than that and the tab stays where the drag left it. Once the pointer is past the window the tab stops moving along the panel, so it leaves from its own slot. The only tab in a window has nowhere to go and stays. Past that distance a small card with the tab's name follows the pointer, where the window would open (`wezterm-gui/src/tab_ghost.rs`, built 2026-10-21): a layered popup of its own that never activates or takes the keyboard, and that the mouse goes through, so the drag keeps the pointer. It goes when the pointer comes back near the window or the tab is let go.
- **Move it into another LBTerm window:** let go over that window. The tab becomes its active tab and the window comes forward. While the pointer is over it, that window's rail rings an empty slot after its last tab in the accent, where the tab will land (`lbterm_panel_drop_hint`).
- **The tab menu:** right-click a tab (in the panel or the bar, where upstream opened the tab navigator). A card in the panel's style opens at the pointer: dark, ringed in the panel's red light, in the panel's font.
  - **Rename tab:** the name is typed in place on the row. It starts as the name the tab shows, all of it selected (highlighted in the panel's red), as Windows' rename boxes do: typing replaces it, Backspace or Delete clears it, and an arrow, Home or End keeps it to edit from its end. Enter keeps the new name, Esc goes back to the menu. Enter on an untouched name changes nothing, and an empty name gives the tab back the title its program sets.
  - **Colour:** a row of swatches (none, then red, orange, yellow, green, teal, blue, purple, pink). The colour shows as a ring round the tab's status pad, in the closed rail and the open panel, beside its Claude status rather than over it.
  - **Duplicate tab:** a new tab in the same folder and domain.
  - **Move to new window:** as dragging it out, with the window where Windows puts new ones.
  - **Close other tabs:** asks once if any of them is running something besides a shell.
  - **Close tab:** as the tab's close, asking first if it's running something.
  - Either brings its tab forward first, so the question shows where you'll see it.
  - The arrow keys move through the rows (left and right along the swatches), Enter picks, Esc or a click outside closes it. While it's open it takes every key and mouse event, but the pointer's moves still reach the tab panel's hover, the menu's card counting as the panel, so the panel behind it closes once the pointer is off both (the Conductor's menu too: `tab_panel_hover_under_menu`).

## Through a mux server

Tabs in a mux server's domain (`lbterm-mux-server`, lbterm/MUX.md) belong to the server's windows, so tearing one off or moving it asks the server to move its active pane (`MovePaneToNewTab`, as `lbterm cli move-pane-to-new-tab` does): a tab with splits leaves them behind. That makes a new tab on the server, so the GUI gives it the old one's name and colour, and the window the server makes for a torn-off tab opens where it was let go, as in the GUI's own domain. A renamed tab's title goes to the server too, so another GUI attached to it sees the new name. Moving along the panel is the GUI's own order and works the same in either.

Two things only a mux server's tabs do:

- Upstream's client domain, after a move, adds the new tab but leaves the pane in its old local tab as well, so the panel would show it twice. LBTerm takes the pane out of the old tab (without killing it) and drops the tab once it's empty.
- A pane in a server can't say what it's running (`ClientPane` always answers "can't close without asking"), so Close tab and Close other tabs always ask there, as Ctrl+Shift+W does, and Close other tabs asks only "Close the N other tabs?".

## What's kept where

- A tab's colour is kept by the GUI for its life, by tab id, so it survives dragging the tab to another window but not a restart (crash restore brings back folders and Claude sessions, not colours). Closed tabs' colours are forgotten the next time one is given.
- A tab's name is its mux title, as `tab:set_title()` sets it from Lua.

## Where it is

- `lbterm-extras/src/tabs.rs`: the colours, when a press becomes a drag, the slot under the pointer, where a let-go tab goes (tested). `lbterm-extras/src/menu.rs`: the menu's rows, layout and keyboard steps (tested).
- `wezterm-gui/src/termwindow/tab_mouse.rs`: dragging, tearing off, moving between windows.
- `wezterm-gui/src/termwindow/render/tab_menu.rs`: the menu, drawn in z-layers 60-69, which the compositor gives a DirectComposition visual of its own (`top`) over the tab panel's. `wezterm-gui/src/tabcolors.rs`: the colours given.
- The hooks: `mouseevent.rs` (the tab bar), `render/tab_panel.rs` (the panel, the pad's ring), `keyevent.rs` (the menu's keys before any overlay's), `paint.rs` (the menu after modals).
- `lbterm\perf\tab-mouse.ps1` (`-Mux` for a test mux server): every one of these in a test window, with no real input. It posts mouse and key messages to the window, starts only once the keyboard and mouse have been idle 3 s, and fails if the window is ever the foreground one. Test windows never take focus, and under `LBTERM_NO_ACTIVATE` a window's `focus()` does nothing, since upstream's types an Alt with SendInput to get the foreground.
