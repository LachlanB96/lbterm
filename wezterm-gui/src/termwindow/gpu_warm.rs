//! LBTerm: the GPU's instance and adapter, made once a process and shared by its windows, and
//! begun on a thread of their own as the GUI starts (lbterm/PERF.md, "What font setup costs": a
//! window's start). Choosing the adapter is the dearest step of a window's start: wgpu makes a
//! D3D12 device on every adapter the PC has to ask what it can do, 383 ms of a window's 576 ms of
//! GPU setup in a test window here (2026-10-31), and upstream did it again for every window. Now
//! the first window's is under way while the GUI reads its config, starts its mux and makes its
//! window, and every later window takes it as it is. Each window still makes its own device
//! (gpu_lost.rs rebuilds one window's).

use config::{ConfigHandle, WebGpuPowerPreference};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// For tests only: the next adapter made is Windows' software renderer, as wgpu hands back while
/// the GPU is still resetting (`LBTERM_TEST_REBUILD_SOFTWARE`, gpu_lost.rs, perf/gpu-lost.ps1)
static TEST_SOFTWARE: AtomicBool = AtomicBool::new(false);

/// For tests only: make the next adapter Windows' software renderer
pub fn software_next_for_test() {
    TEST_SOFTWARE.store(true, Ordering::Release);
}

/// What a window's instance and adapter are made for
#[derive(Debug, Clone, PartialEq)]
struct Key {
    backends: wgpu::Backends,
    power: bool,
    force_fallback: bool,
    preferred: Option<String>,
}

impl Key {
    fn of(backends: wgpu::Backends, config: &ConfigHandle) -> Self {
        Self {
            backends,
            power: matches!(config.webgpu_power_preference, WebGpuPowerPreference::HighPerformance),
            force_fallback: config.webgpu_force_fallback_adapter,
            preferred: config.webgpu_preferred_adapter.as_ref().map(|p| p.to_string()),
        }
    }
}

/// An instance, and the adapter chosen on it
pub struct Gpu {
    pub instance: wgpu::Instance,
    /// The config's preferred adapter when there is one by that description, else the one wgpu
    /// picks for the config's power preference; `None` when wgpu found none
    pub adapter: Option<wgpu::Adapter>,
}

type Cell = Arc<OnceLock<Result<Arc<Gpu>, String>>>;

static MADE: Mutex<Vec<(Key, Cell)>> = Mutex::new(Vec::new());

/// The backends a window of `config` draws with: only DX12 takes a DirectComposition visual
pub fn backends_for(config: &ConfigHandle, use_compositor: bool) -> wgpu::Backends {
    let _ = config;
    if use_compositor {
        wgpu::Backends::DX12
    } else {
        wgpu::Backends::all()
    }
}

/// The instance and adapter for `backends` and `config`'s adapter settings: made the first time
/// they're asked for, by whichever thread asks first, the others waiting for it
pub fn shared(backends: wgpu::Backends, config: &ConfigHandle) -> anyhow::Result<Arc<Gpu>> {
    let key = Key::of(backends, config);
    let cell = {
        let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
        match made.iter().find(|(k, _)| *k == key) {
            Some((_, cell)) => Arc::clone(cell),
            None => {
                let cell = Cell::default();
                made.push((key.clone(), Arc::clone(&cell)));
                cell
            }
        }
    };
    cell.get_or_init(|| make(backends, config).map(Arc::new).map_err(|e| format!("{e:#}")))
        .clone()
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Begun as the GUI starts, on a thread of its own, for the windows `config` makes
pub fn warm_up(config: &ConfigHandle) {
    if config.front_end != config::FrontEndSelection::WebGpu {
        return;
    }
    let config = config.clone();
    let backends = backends_for(&config, config::lbterm::uses_compositor(&config));
    let started = std::thread::Builder::new()
        .name("lbterm-gpu-warm".into())
        .spawn(move || {
            if let Err(err) = shared(backends, &config) {
                log::error!("the GPU's instance and adapter weren't made: {err:#}");
            }
        });
    if let Err(err) = started {
        log::error!("no thread to make the GPU's instance and adapter on: {err:#}");
    }
}

/// Forgotten, so the next window made (or a lost one built again) makes them anew: after a device
/// is lost the adapter may have gone with it (a driver update)
pub fn forget() {
    MADE.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

fn make(backends: wgpu::Backends, config: &ConfigHandle) -> anyhow::Result<Gpu> {
    let mut steps = lbterm_extras::start::Steps::new("gpu-adapter");
    // LBTerm: WGPU_VALIDATION=1 and WGPU_DEBUG=1 turn on the D3D12 debug layer and validation,
    // which log the call behind a failed submit. Not wgpu's checks of indirect draws and
    // dispatches, on in its release builds, which LBTerm never makes: each device compiled two
    // compute pipelines for them as it was made (wgpu-core's indirect_validation), through FXC
    // on the window's start (WGPU_VALIDATION_INDIRECT_CALL=1 turns them back on)
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends,
        flags: wgpu::InstanceFlags::from_build_config()
            .difference(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL)
            .with_env(),
        ..Default::default()
    });
    steps.step("instance");
    let mut adapter = None;
    if let Some(preference) = &config.webgpu_preferred_adapter {
        for a in instance.enumerate_adapters(backends) {
            let info = a.get_info();
            if preference.name != info.name
                || preference.device_type != format!("{:?}", info.device_type)
                || preference.backend != format!("{:?}", info.backend)
                || preference.driver.as_ref().map_or(false, |d| *d != info.driver)
                || preference.vendor.map_or(false, |v| v != info.vendor)
                || preference.device.map_or(false, |d| d != info.device)
            {
                continue;
            }
            adapter = Some(a);
            break;
        }
        if adapter.is_none() {
            log::warn!(
                "Your webgpu preferred adapter '{}' was not found. Available:\n{}",
                preference.to_string(),
                instance
                    .enumerate_adapters(backends)
                    .into_iter()
                    .map(|a| crate::termwindow::webgpu::adapter_info_to_gpu_info(a.get_info()).to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }
    if adapter.is_none() {
        adapter = smol::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: match config.webgpu_power_preference {
                WebGpuPowerPreference::HighPerformance => wgpu::PowerPreference::HighPerformance,
                WebGpuPowerPreference::LowPower => wgpu::PowerPreference::LowPower,
            },
            compatible_surface: None,
            force_fallback_adapter: config.webgpu_force_fallback_adapter
                || TEST_SOFTWARE.swap(false, Ordering::AcqRel),
        }))
        .ok();
    }
    steps.step("adapter");
    steps.done();
    Ok(Gpu { instance, adapter })
}
