# When the GPU goes away

A graphics driver update, a TDR (Windows resetting a hung GPU) or a GPU reset removes the D3D12 device out from under LBTerm. Everything made on that device is dead at once: swapchains, the glyph atlas, buffers, the compositor's visuals.

What WezTerm did:

- **OpenGL**: `do_paint` saw the context was lost, logged "should reinit", and closed the window. Every pane in it went too. This is what happened on this PC before `front_end = 'WebGpu'`.
- **WebGpu**: wgpu's default handler for errors nobody captures panics, and a present on a removed device panics in wgpu 25 (`wgpu_core.rs`), so the GUI process died with all its panes. A failed submit deadlocks in `Device::handle_hal_error` instead (terminal-73 found it while fixing the panel's swapchain limit).

What LBTerm does, WebGpu only:

1. **Notice before using the device.** At the start of every frame, before anything is submitted or presented, LBTerm asks D3D12 directly whether the device is still there (`GetDeviceRemovedReason`, a cheap call). It also checks a flag that wgpu's device-lost callback and LBTerm's own error handler set. That handler logs errors instead of panicking, and sets the flag for out-of-memory or for an error whose text says the device went (DXGI's DEVICE_REMOVED, HUNG or RESET).
2. **Drop the GPU state.** The window's render state goes first (glyph atlas, vertex buffers, both holding the WebGpu state too), then its WebGpu state (device, surfaces, the compositor's visuals and DirectComposition target). Everything laid out against the old atlas is forgotten: the shape caches, the fancy tab bar, any modal, and the tab panel's words.
3. **Build it again.** Half a second later (for the driver to come back), the same constructor that set the window up when it opened makes a new device, surfaces and compositor, and the window repaints. If the device can't be made yet, it tries again every second until it can: a reset is over in seconds, a driver install takes longer, and a window that stopped trying would never draw again. The first 30 tries are logged one by one, then one in 30. Each failed try forgets the process's instance and adapter (`gpu_warm::forget`), so the next makes them anew: after a driver install the old adapter never answers again. That costs the enumeration (wgpu makes a device on every adapter to ask what it can do, about 380 ms on the GUI thread) once a try, only while the GPU is gone. Making the WebGpu state asks the window for its handle, which panics inside the window's own callbacks, so it's made in a task of its own on the GUI thread and handed to the window once it's ready.
   - **Never on Windows' software renderer.** While the GPU is still resetting, or its driver being replaced, the hardware adapter doesn't answer wgpu's probe (DXGI's DEVICE_HUNG), and wgpu hands back the one that does: the Microsoft Basic Render Driver (WARP, `DeviceType::Cpu`), which draws on the CPU. Until 2026-11-07 the rebuild took any state it got, so the window stayed on WARP, drawing on about 30 cores at a time, until Lachlan restarted it, twice: after the driver install of 2026-11-04 and the TDR of 2026-11-07 at 18:21, when the RTX 4080 came back six seconds after the window's first try (ITMan's incidents of those days). Now a state built on the software renderer is dropped, the adapter forgotten, and the next try made, unless the config asks for that adapter (`webgpu_force_fallback_adapter`, or a `webgpu_preferred_adapter` whose type is Cpu). The rule is `lbterm_extras::gpu::takes`. A window that opens while only WARP answers still opens on it (only the rebuild refuses it).
4. **A present that panics anyway** (the device went between the check and the present) is caught, and the window rebuilds as above. The panic is wgpu's own, from `Surface::present`'s `handle_error_fatal` (wgpu 25.0.2's `src/backend/wgpu_core.rs:3626`), and comes from inside a window message, so its stack shows `CallWindowProcW`: seen in the daily window's log in the TDR of 2026-11-07 at 18:21:33, a moment before the device lost was logged, and the window lived. Only when the device really is gone: any other panic carries on as before. Windows still shows WezTerm's "panic" toast for it, from the panic hook, though the window lives.

The panes live in the mux, not on the GPU, so nothing in them notices: Claude Code keeps running, and the screen comes back as it was. With the mux domain (lbterm/MUX.md) they'd outlive even a GUI that didn't survive.

Each window has a device of its own and rebuilds on its own.

## Checking it

`pwsh -File lbterm\perf\gpu-lost.ps1` opens a test window (Start-Unfocused, on the right-most screen) with the compositor and a pane that counts. `LBTERM_TEST_REMOVE_DEVICE=3` makes the window remove its own D3D12 device three seconds after it opens (`ID3D12Device5::RemoveDevice`, which is what a driver reset does to an app). The script then checks:

- the process and the window are still there;
- the window is painting again: two captures a second apart differ, with text in both (`gpu-after.png`, `gpu-later.png`, against `gpu-before.png`);
- the count went on;
- the log says the device was lost and the GPU state built again;
- the window never had the foreground.

On this PC the state is back 0.9 s after the device goes, on the first try, compositor and all.

`-SoftwareTries N` (`LBTERM_TEST_REBUILD_SOFTWARE=N`) hands the first N tries Windows' software renderer, as wgpu hands it while the GPU is still resetting (`gpu_warm::software_next_for_test` asks wgpu for its fallback adapter for the next instance made): the log must refuse each, and the state must be built again on try N+1, on the hardware adapter.

A real TDR can be forced with `dxcap -forcetdr` (Graphics Tools), but it resets the GPU for every app on the PC, so it's not part of any script.

## Where it is

- `wezterm-gui/src/termwindow/gpu_lost.rs`: the flag, wgpu's callbacks, the D3D12 check, dropping and rebuilding, the test seam.
- `wezterm-gui/src/termwindow/mod.rs`: `do_paint_webgpu` checks first and catches a present that panics. `webgpu.rs` gives each WebGpu state its flag.
- `lbterm-extras/src/gpu.rs`: which errors mean the device went, the retry timing, which tries are logged, and whether a state built again may be kept (`takes`), all tested.
- `wezterm-gui/src/termwindow/gpu_warm.rs`: the process's instance and adapter, forgotten for each try, and the test seam that makes the next one the software renderer.
