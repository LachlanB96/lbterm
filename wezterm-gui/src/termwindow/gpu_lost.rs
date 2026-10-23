//! LBTerm: surviving a lost GPU device (lbterm/GPU-LOST.md). A driver update, a TDR or a GPU
//! reset removes the D3D12 device out from under wgpu. WezTerm then panicked in wgpu at present,
//! or (with OpenGL) closed the window, and every pane in it went with the process or the window.
//!
//! Here: noticing the device has gone before anything is submitted to it (wgpu 25 deadlocks
//! when a submit fails, and panics when a present does), so the window can drop its GPU state
//! and build it again. The panes, which live in the mux, never notice.

use crate::renderstate::RenderContext;
use crate::termwindow::webgpu::WebGpuState;
use crate::termwindow::{TermWindow, TermWindowNotif};
use mux::window::WindowId as MuxWindowId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use lbterm_extras::gpu::{removed_message, wait_before, TRIES};
use std::time::Duration;
use window::WindowOps;

thread_local! {
    /// A window's new GPU state, made outside its callbacks (making it asks the window for its
    /// handle, which panics inside them) and picked up in one
    static REBUILT: RefCell<HashMap<MuxWindowId, Rc<WebGpuState>>> = RefCell::new(HashMap::new());
}

/// A window's part in it
#[derive(Default)]
pub struct GpuLostState {
    /// Its GPU state is gone and a new one is on the way
    rebuilding: bool,
}

impl TermWindow {
    /// Whether this window's device has gone: wgpu said so, or D3D12 does
    pub(crate) fn lbterm_gpu_gone(&self) -> bool {
        match self.webgpu.as_ref() {
            Some(webgpu) => webgpu.lost.is_set() || removed(&webgpu.device),
            None => false,
        }
    }

    /// Drop everything made on the lost device and start making it again. The panes live in
    /// the mux, so nothing in them notices.
    pub(crate) fn lbterm_drop_gpu(&mut self) {
        if self.lbterm_gpu.rebuilding {
            return;
        }
        self.lbterm_gpu.rebuilding = true;
        log::error!("GPU device lost: dropping this window's GPU state to build it again");
        // The render state first: its buffers and atlas hold the WebGpuState too
        self.render_state = None;
        self.webgpu = None;
        self.lbterm_forget_gpu_caches();

        let Some(window) = self.window.clone() else {
            return;
        };
        let mux_window_id = self.mux_window_id;
        let config = self.config.clone();
        // The size it has now; a paint resizes the surfaces to whatever it has by then
        let dimensions = self.dimensions;
        promise::spawn::spawn(async move {
            for attempt in 1..=TRIES {
                smol::Timer::after(wait_before(attempt)).await;
                match WebGpuState::new(&window, dimensions, &config).await {
                    Ok(state) => {
                        log::warn!("GPU state built again (try {attempt})");
                        REBUILT.with(|r| r.borrow_mut().insert(mux_window_id, Rc::new(state)));
                        window.notify(TermWindowNotif::Apply(Box::new(|tw| tw.lbterm_gpu_rebuilt())));
                        return;
                    }
                    Err(err) => log::warn!("GPU state not back yet (try {attempt} of {TRIES}): {err:#}"),
                }
            }
            log::error!("gave up building the GPU state again after {TRIES} tries");
        })
        .detach();
    }

    /// The new GPU state is ready: take it, and paint
    fn lbterm_gpu_rebuilt(&mut self) {
        let Some(webgpu) = REBUILT.with(|r| r.borrow_mut().remove(&self.mux_window_id)) else {
            return;
        };
        self.webgpu.replace(Rc::clone(&webgpu));
        if let Err(err) = self.created(RenderContext::WebGpu(webgpu)) {
            log::error!("GPU state built again, but not its render state: {err:#}");
        }
        self.lbterm_forget_gpu_caches();
        self.lbterm_gpu.rebuilding = false;
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Anything laid out against the old glyph atlas
    fn lbterm_forget_gpu_caches(&mut self) {
        self.shape_generation += 1;
        self.shape_cache.borrow_mut().clear();
        self.line_to_ele_shape_cache.borrow_mut().clear();
        self.invalidate_fancy_tab_bar();
        self.invalidate_modal();
        self.lbterm_panel.forget_laid_out();
    }
}

/// Set once the device is known to be gone; read at the start of every frame
#[derive(Clone, Default)]
pub struct Lost(Arc<AtomicBool>);

impl Lost {
    pub fn set(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Have wgpu report a lost device and its errors to us instead of panicking: its default
/// handler for uncaptured errors panics, which took the whole GUI down
pub fn watch(device: &wgpu::Device, lost: &Lost) {
    let on_lost = lost.clone();
    device.set_device_lost_callback(move |reason, message| {
        log::error!("GPU device lost ({reason:?}): {message}");
        on_lost.set();
    });
    let on_error = lost.clone();
    device.on_uncaptured_error(Box::new(move |error| {
        log::error!("GPU error: {error}");
        if matches!(error, wgpu::Error::OutOfMemory { .. }) || removed_message(&error.to_string()) {
            on_error.set();
        }
    }));
}

/// Ask D3D12 directly whether the device has been removed: cheap, and true before wgpu has
/// noticed anything
#[cfg(windows)]
pub fn removed(device: &wgpu::Device) -> bool {
    unsafe {
        device.as_hal::<wgpu::hal::api::Dx12, _, _>(|hal| match hal {
            Some(hal) => hal.raw_device().GetDeviceRemovedReason().is_err(),
            None => false,
        })
    }
}

#[cfg(not(windows))]
pub fn removed(_: &wgpu::Device) -> bool {
    false
}

/// For tests only: with `LBTERM_TEST_REMOVE_DEVICE=<seconds>`, the window's device is removed
/// that long after it opens (lbterm\perf\gpu-lost.ps1)
pub fn schedule_test_removal(window: &window::Window) {
    let Some(seconds) = std::env::var("LBTERM_TEST_REMOVE_DEVICE")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
    else {
        return;
    };
    let window = window.clone();
    promise::spawn::spawn(async move {
        smol::Timer::after(Duration::from_secs(seconds)).await;
        window.notify(TermWindowNotif::Apply(Box::new(|tw| {
            if let Some(webgpu) = tw.webgpu.as_ref() {
                remove_for_test(&webgpu.device);
            }
            if let Some(window) = tw.window.as_ref() {
                window.invalidate();
            }
        })));
    })
    .detach();
}

/// For tests only (`LBTERM_TEST_REMOVE_DEVICE`): remove the device as a driver reset would,
/// through ID3D12Device5::RemoveDevice
#[cfg(windows)]
pub fn remove_for_test(device: &wgpu::Device) {
    use windows58::core::Interface;
    use windows58::Win32::Graphics::Direct3D12::ID3D12Device5;
    unsafe {
        device.as_hal::<wgpu::hal::api::Dx12, _, _>(|hal| {
            if let Some(hal) = hal {
                match hal.raw_device().cast::<ID3D12Device5>() {
                    Ok(device5) => {
                        log::warn!("LBTERM_TEST_REMOVE_DEVICE: removing the GPU device");
                        device5.RemoveDevice();
                    }
                    Err(err) => log::error!("LBTERM_TEST_REMOVE_DEVICE: no ID3D12Device5: {err:#}"),
                }
            }
        })
    }
}

#[cfg(not(windows))]
pub fn remove_for_test(_: &wgpu::Device) {}
