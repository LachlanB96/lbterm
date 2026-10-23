# Compositor and motion

LBTerm draws through Windows' compositor so that panes can move. This file is the design: why it's built this way, how a frame reaches the screen, how motion is described, and what's done. The `lbterm-motion` skill (`.claude/skills/lbterm-motion/SKILL.md`) holds the rules for designing a new animation.

## Why the compositor

WezTerm draws everything (window background, every pane, the split lines, the tab bar) in one pass into one swapchain, on its one GUI thread. Animating inside that loop means every frame of motion needs that thread, and the thread blocks: `wezterm cli list` takes 200 to 290 ms with ~18 panes, and the summary pane's split arrives just as a Claude session starts, which is exactly when panes move.

With DirectComposition each pane is its own visual with its own swapchain, and DWM moves, clips and fades the visuals at the monitor's refresh rate (160 Hz on the main monitor) on its own thread. LBTerm only redraws a pane when its content changes. A stalled GUI thread freezes the text, not the motion.

DirectComposition rather than Windows.UI.Composition (the WinRT layer over it): it's plain Win32 COM with no DispatcherQueue in WezTerm's message loop, and wgpu 25 already creates swapchains for DComp visuals (`wgpu::SurfaceTargetUnsafe::CompositionVisual`). DComp has no springs, so LBTerm samples each spring itself and hands DWM the curve as cubic segments (`IDCompositionAnimation::AddCubic`). The motion on screen is then exactly the spec, and the sampling is unit-tested.

Rejected: animating inside the wgpu renderer (stalls with the GUI thread), and a WebView2 layer for CSS/JS (can't hold terminal text; two input systems; 100+ MB per window). CSS survives as the syntax for curves.

## How a frame reaches the screen

`config.lbterm_compositor = true` with `front_end = 'WebGpu'`. Otherwise LBTerm draws the way WezTerm does.

- The window is created with `WS_EX_NOREDIRECTIONBITMAP`, so DWM keeps no GDI surface for it. A DComp target on the HWND holds the visual tree. wgpu is pinned to DX12 (Vulkan can't present to a DComp visual).
- `WebGpuState` keeps the wgpu `Instance`, so it can make a surface per visual. Every surface shares one device and queue.
- The paint pass is unchanged except that it records which quads each pane emitted (`[start, end)` per sub-layer of z-layer 0). The scrollbar thumb is painted after the panes so it isn't counted as part of the active pane.
- `call_draw` runs the same vertex buffers once per surface, each with a projection that maps that surface's rectangle of the window onto its own swapchain, and a subset of the quad ranges. Pixels outside the surface fall off its edges, which is the clipping.
- Every visual with content of its own is a swapchain, and D3D12 lets one command list, and one `ExecuteCommandLists` call, write to at most 8 (`COMMAND_LIST_TOO_MANY_SWAPCHAIN_REFERENCES`, `..._QUEUE_...`). With the tab panel's five visuals, `under` and `over`, a tab's second pane made nine. So a frame records into one encoder per 8 visuals (`lbterm_motion::command_list_for`) and submits each on its own. Past the limit the list fails to close with `E_INVALIDARG`, and wgpu 25.0.2 then deadlocks in `Device::handle_hal_error` on the GUI thread; one submit past it removes the device instead.
- After presenting, LBTerm commits the visual tree only when it changed (a layout change, a new or closed pane, an animation).

Visual tree, bottom to top:

| Visual | Draws | Alpha |
|---|---|---|
| `cards` | the card templates, z-layers 70, 71, 74 and 75: the mask (what's outside a card) and the line (the outline alone), each as wide as the window and as tall; never seen, clipped to nothing; their swapchains are what the pieces below show | premultiplied |
| `under` | z-layers below 0, and layer 0's quads before the first pane (window background) | opaque |
| one per visible pane | that pane's quads (the window background, its card's fill, its text), clipped to the pane's rectangle; over them, as children, the nine pieces of the mask round its card; its container at 85% while it hasn't the keyboard | opaque |
| `focus` | the outline of the pane with the keyboard: nine pieces of the line, in a group that follows that pane and glides to the next, inside a lamp whose opacity is the outline's light. The line template is drawn at its brightest (white 35%) and the lamp shows it at rest (6/35), out while the window hasn't the keyboard, and pulsing to full as the keyboard arrives from outside (`arrive_in`, `arrive_out`, one `lbterm_motion::Chain`) | premultiplied |
| `over` | layer 0's quads after the panes minus the splits (tab bar, borders, scrollbar), and z-layers above 0 (fancy tab bar, modals) | premultiplied |
| `swap` | the window strip as it was (z-layers 36 and 37, render/strip_swap.rs), over the rows whose words just changed, on the strip's own colour: there at once, and faded out by DWM on `swap`, so the old words dissolve into the new | premultiplied |
| `lights` | brief light (render/lights.rs): a right-click's ripple, copied or opened text lighting up. Each is a visual showing a white swatch (in `cards`, never seen itself, cleared to white only when it's new or a new size) through a rounded rectangle clip; only the clip and the opacity move. Over `over` and the strip, so a strip row can brighten too. Put away once done, at the next frame | premultiplied |
| `hover` | the strip's two hover slots, the size of the strip (z-layers 72 and 73): a window button's, a pill's or the strip's words' look under the pointer, which `over` draws only at rest. An element made of parts (the words: title, " · ", summary) puts on the slot only the parts with a look of their own, the summary at 85%, so nothing is drawn twice. DWM fades each slot's opacity on `button_hover`, so the button the pointer left fades out on one while the one it reached fades in on the other (`render/hover_fade.rs`, `lbterm_extras::hover`). Made the first time a hover fades | premultiplied |
| `strip_card` | the strip's detail card (z-layers 30 to 32), a visual its size with its shadow: DWM fades its opacity in on `pane_enter` as the card opens and out on `pane_exit` as it closes, from wherever it is, and the card that's going is drawn as it was until it has faded (`render/strip.rs`, `lbterm_extras::strip`). Made the first time the card opens | premultiplied |
| `readout` | the readout of the active pane's size in cells (z-layers 33 to 35, render/lights.rs), the card's size with its shadow: there at once while the window is resized or the font changes, and faded out by DWM on `pane_exit` a moment after, drawn as it was until it has | premultiplied |
| `panel` | the tab panel's own z-layers (45 to 59): the panel drawn open, a body group clipped at a seam that moves with its edge and a cap group that rides on it, each with its glow; over it the light that runs round the ring as it opens (`lbterm/TAB-PANEL.md`). In the body, over the panel itself: a visual per tab row showing its slot of the rows' surface (z-layer 46), and in the words' visual a second one per row showing its open part (48), the open tab's ring and bar (47) and each row's progress fill (45, in its row's second visual), so rows slide, grow in, fade out and dissolve whole, open or closed, the marker glides and progress grows, all in DWM | premultiplied |
| `panel_lights` | a pad's glint (render/lights.rs, as `lights`), over the panel | premultiplied |
| `top` | the tab menu's z-layers (60 to 69), only while it's open (`lbterm/TAB-MOUSE.md`) | premultiplied |

While the tab panel is open, each pane's visual shows somewhere other than where its surface was drawn from: `PaneLayer` keeps the pane's rectangle (its surface and projection) and where it's `shown` (its offset and clip) apart, and the planner works on where panes are shown.

A pane is a card (`lbterm/DESIGN.md`, "Shape"; `render/card.rs`): the surface colour with 12 px corners, a gutter in from the edges of its room (the window, the tab bar, the tab panel's strip) and half a gutter short of the middle of a split's gap, so neighbouring cards are a gutter apart and nothing is drawn between them. The pane's visual is its card with half a gutter round it, so neighbouring visuals meet in the middle of the gap. The middle snaps to a pixel boundary the way the GPU fills quads (a pixel centre exactly on an edge belongs to the quad on its right or below, so `ceil(edge - 0.5)`, not `round`), the card's edges are whole pixels from there, so each visual holds exactly the pixels its quads fill and text stays 1:1. Each pane draws the window background first, which keeps its visual opaque to its corners.

**A card's edges ride on its visual.** With the compositor a card's edges are cut from template cards drawn every frame into visuals of their own and never shown, each drawn twice: as wide as the window and the template's height (its corners, top and bottom edges), and as tall (its left and right edges). The mask is the window background outside the card and clear inside; the line is the outline alone. Every pane shows nine pieces of the mask as children of its visual: the four corners, and the four edges cut to length, all sharing the templates' swapchains (`IDCompositionVisual::SetContent` with one swapchain on many visuals; `wgpu::hal`'s D3D12 surface hands it over). The top-left corner sits still; the right-hand pieces sit a fixed distance from the visual's width and the bottom row from its height, so each is driven by the pane's own motion of its clip, shifted (`compositor.rs`, `Edges`). The same samples, shifted, land the pieces where the clip is in every DWM frame, so a card stays whole and rounded at every size it passes through, while the content inside keeps its size and is cut off at its edge.

**One outline, on the pane with the keyboard** (`lbterm/DESIGN.md`, "Panes"). The outline is nine pieces of the line in a group of its own over the panes (`FocusOutline`), its offset and its pieces driven like a pane's. It follows the pane with the keyboard on whatever motion moves that pane (the same curve, from the same place, so the two move as one), and when the keyboard moves it glides to the next pane on `focus_move`, from wherever it is and keeping its speed; a tab switch places it at once (`lbterm_motion::focus`). A pane without the keyboard is its container at 85% opacity (`palette::INACTIVE`), going there and back on `dim`, so it's drawn undimmed and upstream's `inactive_pane_hsb` is only for drawing without the compositor; DWM blends the pane over `under`, the window background, so its card keeps the surface colour and its text sits at 85%. The keyboard reaches the new pane at once: only the light moves. Without the compositor the pane with the keyboard draws its own outline and the others are dimmed by `inactive_pane_hsb`.

Things learned building it:

- Upstream hands WGSL its projection with `to_arrays_transposed()`. That only works because a window-centred orthographic projection has no translation. A pane's projection has one, and transposed it lands in `w`: the perspective divide then shreds the frame into horizontal streaks. The compositor's draw uses `to_arrays()`.
- The visible differences from drawing without the compositor: the panes without the keyboard (85% opacity against `inactive_pane_hsb`), and the soft pixel either side of the outline, a few levels apart (at most 11 in `compare.ps1`'s three panes). Without it the GPU blends that pixel in linear light; with it DWM lays the template over the pane in its stored, gamma-encoded values.

Surfaces are pooled, not owned by panes: a pane keeps its surface while it stays visible (for continuity during motion), and a tab switch reassigns free surfaces, resizing them if needed. The pool is as large as the most panes on screen at once, plus any outgoing visuals still fading.

## Motion

Written as data in `lbterm.lua`, never as per-frame Lua (Lua runs on the GUI thread):

```lua
config.lbterm_motion = {
  enabled = 'on',                         -- 'on' | 'off' | 'system' (follow Windows' Animation effects)
  slowdown = 1.0,                         -- stretches every animation, for judging by eye
  pane_layout = 'spring(220ms, 0)',       -- split, close, swap, rotate, zoom: one shared spring
  pane_enter  = 'spring(160ms, 0)',
  pane_exit   = '120ms cubic-bezier(0.23, 1, 0.32, 1)',
}
```

- Curves use CSS syntax: `cubic-bezier()`, `linear()`, `ease`, `ease-out`, `ease-in-out`, `linear`. Springs use Apple's and Motion's `spring(duration, bounce)`: with mass 1, stiffness = (2π/duration)² and damping ratio = 1 − bounce. `spring(stiffness=800, damping=1.0)` is accepted too (niri's and Material's form).
- A retarget mid-flight starts from the visual's current value and carries its velocity. LBTerm knows both, because it computed the curve.
- `enabled = 'system'` follows Windows' Animation effects (`SPI_GETCLIENTAREAANIMATION`). It's the default. Lachlan's config sets `'on'`, because that switch is off on his PC.

What moves:

- Panes that only change position (swap, rotate) animate their offset.
- Panes that change size (a split shrinking a pane, a close growing its neighbours) resize their swapchain at once, so the PTY gets one resize, never one per frame. The visual's clip animates from the old size to the new while the content stays anchored at its top-left.
- A new pane rides in beside the moving split: its offset animates from the edge it was split from, inside a container visual clipped to the old rectangle of the pane it came out of (so it never draws over a neighbour), and it fades in on `pane_enter`.
- A closing pane's visual stays alive with its last frame, moves to the bottom of the pane group so its growing neighbours cover it, fades out on `pane_exit`, then returns to the pool. A pane that comes back before the fade ends is picked up where it is.
- The window buttons, the pills and the strip's words fade their look under the pointer in and out on `button_hover` (`120ms cubic-bezier(0.23, 1, 0.32, 1)`): the slot's opacity from wherever it is, so moving across the buttons fades one out as the next fades in, and coming back picks a fade up where it is. Motion off, or the window being resized, sets it at once. Without the compositor the strip swaps colours at once, as WezTerm does.
- A focus change moves only light and opacity: the outline glides to the pane with the keyboard on `focus_move` and the panes dim and brighten on `dim`. A tab switch doesn't move anything yet (`lbterm/DESIGN.md` cross-fades it in a step still to build). Nor does a window being resized by its frame: any change of window size places everything at once and ends any fade.
- A card's edges move with every one of these, and the outline with its pane: on a split the shrinking card's right edge glides in with the split while the new card rides in a gutter beside it, and the outline glides onto it as it takes the keyboard; on a close the growing card's edge rides out with its clip.

How it's built (`wezterm-gui/src/termwindow/compositor.rs`): each pane is a container visual holding the pane's visual, whose offset, `IDCompositionRectangleClip` and opacity (`IDCompositionVisual3`) carry the animations. Every animated number is kept with its curve and start time, so a retarget reads where it is and how fast it's going. The planner runs against where panes were going, not where they are mid-flight, so a frame painted during motion (the shell printing, say) changes nothing. Everything a layout change sets is committed together after the frame is presented, so the animations start in one DWM frame.

Measured (2026-10-19, `slowdown = 5`, captures below): a split's line follows the layout spring with 0.7 px RMS error, through a second split 450 ms in that retargets it; a tab switch, motion `off` or `system` with Windows' Animation effects off, and a window resize all go straight to the final layout. The compositor's own work for a layout change stays under 8 ms on the GUI thread (9 ms the first time, when the pane surfaces are made); a `PrintWindow` capture once blocked ~100 ms during a split, from elsewhere in the GUI thread, and the motion kept running through it.

Measured with the cards (2026-10-20, 96 DPI): a split's edge (the shrinking card's outline) follows the layout spring with 2.2 px RMS error at `slowdown = 5`, all of it from one frame where `PrintWindow` stalled; opening the tab panel over three panes keeps every gutter 4 px wide and the two cards in a column together in every frame, at real speed and at `slowdown = 5`. That last needed the fancy tab bar laid out before the panes (`paint.rs`): built when it was painted, after them, a pane painted while it was invalidated saw no bar, sat 2 px higher than in the frame before, and was retargeted nearly every frame, falling behind its neighbour.

Known details:

- While a split slides in, the shrinking pane's content is already its new size, so the strip between its edge and its outline gliding in after it shows the window background, which differs by a few levels from a dimmed inactive pane's card.

## Checking it

- `cargo test -p lbterm-motion`: spring maths, CSS curve parsing, the sampling into cubic segments (error under half a pixel), the planner that turns two layouts into animations, and which quads go to which visual.
- `LBTERM_DUMP_FRAMES=<dir>` saves every visual's texture of the first six frames as `frame<n>-<visual>.png`: what LBTerm drew, apart from what DWM shows. `WEZTERM_LOG=lbterm_gui::termwindow::render::composited=debug` logs each visual's rectangle and quad ranges per frame.
- `pwsh -File lbterm\motion\compare.ps1`: opens the build with the compositor on and off, gives both the same three panes, captures each with `PrintWindow(PW_RENDERFULLCONTENT)` (it renders DComp content whatever covers the window; a screen grab doesn't) and diffs them. Expect the two panes without the keyboard (opacity against `inactive_pane_hsb`), and the soft pixels beside the outline a few levels apart (above); before the outline was the active pane's alone, the panes with no differences but those soft pixels showed 7128 of them, none more than 11 levels (2026-10-20).
- `pwsh -File lbterm\motion\capture.ps1 -Name split -Cli split-pane,--pane-id,0,--right -Row 300 -From 150 -Expect 1170,612,220,0`: opens the build looking like Lachlan's LBTerm (`New-TestConfig` in `offscreen.ps1`: his `lbterm.lua` without its summon key, which a background window would take from him, or its mux domain) with motion on at `slowdown = 5`, starts the change, records ~60 frames a second, then `sheet.py` writes `target\motion\<name>\contact.png` (the frames tiled with their times), `edge.png` (the split's x over time against the spring: the first outline on `-Row` from `-From` on, past the panel and the first card's own edge) and prints the fit. `-Then ... -ThenMs 450` interrupts it. Both tools need Python with Pillow and numpy, and capture's timestamps come after each `PrintWindow` returns, because it can block while LBTerm's GUI thread is busy.
- Since step 2 only the pane with the keyboard has an outline, so on a split the row sees the outline gliding onto the new pane, not the split line: `capture.ps1 -Name split -Cli split-pane,--pane-id,0,--right -Row 300 -From 74 -Still -Expect 76,613,300,0` follows its left edge on `focus_move` (1.6 and 3.2 px RMS in two runs on 2026-10-21, the difference being capture timing).
- `pwsh -File lbterm\motion\focus-demo.ps1`: three panes, the keyboard moved between them with `lbterm cli activate-pane` (right, below, left, right), recorded to `target\motion\focus-x1\demo.gif`. `-Slowdown 5 -Measure` makes one move and fits the outline's top edge to `focus_move`: 0.58 px RMS over 112 frames on 2026-10-21, and the panes' text peaking at 255 with the keyboard and 220 without, 85% of the way from the background.
- `slowdown = 5` to watch it by eye.
- `WGPU_VALIDATION=1` turns on the D3D12 debug layer. wgpu-hal 25.0.2 is meant to log its messages but decodes them wrongly and drops every one, so read them where Windows sends them instead: any `OutputDebugString` listener (DebugView, or the DBWIN shared-memory protocol it uses) shows `D3D12 ERROR: ...` with the call that failed. That is how the swapchain limit above was found.
- A hung GUI thread: write a minidump (`MiniDumpWriteDump`), make Breakpad symbols with `dump_syms` from `target\release\lbterm-gui.exe` (the exe, so the unwind tables come with the PDB) and from the system DLLs (`--symbol-server` with Microsoft's), and walk it with `minidump-stackwalk` (both `cargo install`).
- The motion scripts open their windows on the right-most screen (`lbterm/motion/offscreen.ps1`) with `Start-Unfocused` (`lbterm/motion/start-unfocused.ps1`): no console, and `LBTERM_NO_ACTIVATE`, which makes LBTerm show its window without taking focus and at the bottom of the z-order, so a test never lands where Lachlan is typing. Never `explorer.exe launch.cmd`: Explorer runs the .cmd in a console, a Windows Terminal window that comes up in front and swallows his typing for the second it runs (it did, from three sessions at once, on 2026-10-20).

## Milestones

1. **Composition host.** The whole frame in one DComp visual instead of an HWND swapchain. Looks and resizes the same as before. Done 2026-10-19.
2. **Panes as visuals.** `under`, per-pane and `over` visuals, static. Looks the same as before. Done 2026-10-19.
3. **Motion.** The `lbterm-motion` crate, `lbterm_motion` config, the animations above. Done 2026-10-19.
4. **Capture tool and contact sheets.** `lbterm/motion/`. Done 2026-10-19.

Turned on in Lachlan's `lbterm.lua` 2026-10-20. The tab panel (`lbterm/TAB-PANEL.md`) is the next thing DWM moves: its own visual, opened and closed by its clip.

Next: swap and rotate (they go through the same planner, untested so far), zoom, and pane drag with release velocity (`drag_release` in the skill).
