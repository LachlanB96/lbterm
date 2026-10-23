# When the GPU goes away

A graphics driver update, a TDR (Windows resetting a hung GPU) or a GPU reset removes the D3D12 device out from under LBTerm. Everything made on that device is dead at once: swapchains, the glyph atlas, buffers, the compositor's visuals.

What WezTerm did:

- **OpenGL**: `do_paint` saw the context was lost, logged "should reinit", and closed the window. Every pane in it went too. This is what happened on this PC before `front_end = 'WebGpu'`.
- **WebGpu**: wgpu's default handler for errors nobody captures panics, and a present on a removed device panics in wgpu 25 (`wgpu_core.rs`), so the GUI process died with all its panes. A failed submit deadlocks in `Device::handle_hal_error` instead (terminal-73 found it while fixing the panel's swapchain limit).

What LBTerm does, WebGpu only:

1. **Notice before using the device.** At the start of every frame, before anything is submitted or presented, LBTerm asks D3D12 directly whether the device is still there (`GetDeviceRemovedReason`, a cheap call). It also checks a flag that wgpu's device-lost callback and LBTerm's own error handler set. That handler logs errors instead of panicking, and sets the flag for out-of-memory or for an error whose text says the device went (DXGI's DEVICE_REMOVED, HUNG or RESET).
2. **Drop the GPU state.** The window's render state goes first (glyph atlas, vertex buffers, both holding the WebGpu state too), then its WebGpu state (device, surfaces, the compositor's visuals and DirectComposition target). Everything laid out against the old atlas is forgotten: the shape caches, the fancy tab bar, any modal, and the tab panel's words.
3. **Build it again.** Half a second later (for the driver to come back), the same constructor that set the window up when it opened makes a new device, surfaces and compositor, and the window repaints. If the device can't be made yet, it tries again every second, 30 times. Making the WebGpu state asks the window for its handle, which panics inside the window's own callbacks, so it's made in a task of its own on the GUI thread and handed to the window once it's ready.
4. **A present that panics anyway** (the device went between the check and the present) is caught, and the window rebuilds as above. Only when the device really is gone: any other panic carries on as before. Windows still shows WezTerm's "panic" toast for it, from the panic hook, though the window lives.

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

A real TDR can be forced with `dxcap -forcetdr` (Graphics Tools), but it resets the GPU for every app on the PC, so it's not part of any script.

## Where it is

- `wezterm-gui/src/termwindow/gpu_lost.rs`: the flag, wgpu's callbacks, the D3D12 check, dropping and rebuilding, the test seam.
- `wezterm-gui/src/termwindow/mod.rs`: `do_paint_webgpu` checks first and catches a present that panics. `webgpu.rs` gives each WebGpu state its flag.
- `lbterm-extras/src/gpu.rs`: which errors mean the device went, and the retry timing (tested).
