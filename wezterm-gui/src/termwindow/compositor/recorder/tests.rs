//! The stand-in DirectComposition device's own tests: what it records and how it reads back,
//! each called through the interfaces as compositor.rs calls DirectComposition's
//! (lbterm/COMPOSITOR.md, "Checking it without a window")
#![deny(non_fmt_panics)]

use super::*;

const NONE: Option<&IDCompositionVisual> = None;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// A device and a target with a root visual, as `Compositor::new` makes them
fn window(recorder: &Recorder) -> (IDCompositionDesktopDevice, IDCompositionTarget, IDCompositionVisual2) {
    let device = recorder.device();
    unsafe {
        let target = device
            .CreateTargetForHwnd(HWND(std::ptr::null_mut()), true)
            .unwrap();
        let root = device.CreateVisual().unwrap();
        target.SetRoot(&root).unwrap();
        (device, target, root)
    }
}

fn visual(device: &IDCompositionDesktopDevice) -> IDCompositionVisual2 {
    unsafe { device.CreateVisual().unwrap() }
}

/// An animation of `cubics` and an end, as `play` builds one
fn animation(device: &IDCompositionDesktopDevice, segments: &Segments) -> IDCompositionAnimation {
    unsafe {
        let animation = device.CreateAnimation().unwrap();
        for cubic in &segments.cubics {
            let [c0, c1, c2, c3] = cubic.coefficients;
            animation.AddCubic(cubic.begin, c0, c1, c2, c3).unwrap();
        }
        animation.End(segments.end, segments.end_value).unwrap();
        animation
    }
}

/// Two cubics and an end: rising, then past where it ends (1.32 at 0.18 s) and back, as a
/// spring's segments do
fn springy() -> Segments {
    Segments {
        cubics: vec![
            Cubic {
                begin: 0.0,
                coefficients: [0.0, 10.0, 0.0, 0.0],
            },
            Cubic {
                begin: 0.1,
                coefficients: [1.0, 8.0, -50.0, 0.0],
            },
        ],
        end: 0.2,
        end_value: 1.3,
    }
}

/// The device makes each stand-in compositor.rs asks for, casts between its interfaces as
/// DirectComposition's does, and fails what it doesn't record
#[test]
fn the_device_makes_each_stand_in_and_casts_as_directcompositions_does() {
    let recorder = Recorder::default();
    let device = recorder.device();
    unsafe {
        // compositor.rs casts its desktop device for the effects
        let device3: IDCompositionDevice3 = device.cast().unwrap();
        let again: IDCompositionDesktopDevice = device3.cast().unwrap();
        assert_eq!(recorder.object(&again), Some(Object::Ours(1)));
        let _target = device
            .CreateTargetForHwnd(HWND(std::ptr::null_mut()), true)
            .unwrap();
        let visual = device.CreateVisual().unwrap();
        // and the visuals it makes, for the opacity
        let visual3: IDCompositionVisual3 = visual.cast().unwrap();
        assert_eq!(recorder.object(&visual3), Some(Object::Ours(3)));
        let _clip = device.CreateRectangleClip().unwrap();
        let _animation = device.CreateAnimation().unwrap();
        let _rotate = device.CreateRotateTransform().unwrap();
        let _scale = device.CreateScaleTransform().unwrap();
        let _matrix = device3.CreateColorMatrixEffect().unwrap();
        let _hue = device3.CreateHueRotationEffect().unwrap();
        assert_eq!(device.CreateTranslateTransform().unwrap_err().code(), E_NOTIMPL);
        assert_eq!(device3.CreateGaussianBlurEffect().unwrap_err().code(), E_NOTIMPL);
    }
    let made: Vec<(Id, &str)> = recorder
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Made { id, kind } => Some((id, kind)),
            _ => None,
        })
        .collect();
    assert_eq!(
        made,
        vec![
            (1, "device"),
            (2, "target"),
            (3, "visual"),
            (4, "clip"),
            (5, "animation"),
            (6, "rotate"),
            (7, "scale"),
            (8, "colour matrix"),
            (9, "hue rotation"),
        ]
    );
    assert_eq!(
        recorder.calls().last(),
        Some(&Call::Unrecorded {
            id: 1,
            method: "CreateGaussianBlurEffect"
        })
    );
}

/// AddVisual puts a child on top or at the bottom with no reference, just above or below one
/// with it, and refuses what DirectComposition refuses; RemoveVisual takes it out
#[test]
fn add_visual_above_and_below_a_reference_and_remove_it() {
    let recorder = Recorder::default();
    let (device, _target, root) = window(&recorder);
    let [a, b, c, d, e, loose] = [(); 6].map(|_| visual(&device));
    let o = |v: &IDCompositionVisual2| recorder.object(v).unwrap();
    unsafe {
        // FALSE with no reference puts each on top, TRUE at the bottom
        root.AddVisual(&a, false, NONE).unwrap();
        root.AddVisual(&b, false, NONE).unwrap();
        root.AddVisual(&c, true, NONE).unwrap();
        // Just above a, and just below b
        root.AddVisual(&d, true, &a).unwrap();
        root.AddVisual(&e, false, &b).unwrap();
        assert_eq!(recorder.live().children(o(&root)), vec![o(&c), o(&a), o(&d), o(&e), o(&b)]);

        let refused = |r: windows58::core::Result<()>| assert_eq!(r.unwrap_err().code(), E_INVALIDARG);
        // A visual has one parent
        refused(root.AddVisual(&a, false, NONE));
        // A reference that isn't a child
        refused(root.AddVisual(&loose, true, &loose));
        refused(a.AddVisual(&loose, true, &b));
        // Itself, its own ancestor, and a target's root
        refused(loose.AddVisual(&loose, false, NONE));
        let free = visual(&device);
        loose.AddVisual(&free, false, NONE).unwrap();
        refused(free.AddVisual(&loose, false, NONE));
        refused(a.AddVisual(&root, false, NONE));
        // Not a child
        refused(root.RemoveVisual(&loose));
        // Another device's visual
        let other = Recorder::default().device().CreateVisual().unwrap();
        refused(root.AddVisual(&other, false, NONE));

        root.RemoveVisual(&a).unwrap();
        d.AddVisual(&a, false, NONE).unwrap();
        refused(a.AddVisual(&d, false, NONE));
    }
    let live = recorder.live();
    assert_eq!(live.children(o(&root)), vec![o(&c), o(&d), o(&e), o(&b)]);
    assert_eq!(live.parent(o(&a)), Some(o(&d)));
    assert!(live.shown(o(&a)));
    assert!(!live.shown(o(&loose)));
    assert_eq!(
        live.walk(),
        vec![
            (0, o(&root)),
            (1, o(&c)),
            (1, o(&d)),
            (2, o(&a)),
            (1, o(&e)),
            (1, o(&b)),
        ]
    );
    // What was refused left no call behind
    let adds = recorder
        .calls()
        .iter()
        .filter(|c| matches!(c, Call::Add { .. }))
        .count();
    assert_eq!(adds, 7);
}

/// A value lets go of an animation and an animation of a value; the animation is copied as
/// it's handed over, and what's added to it after is recorded as late
#[test]
fn properties_set_and_animated_replace_each_other() {
    let recorder = Recorder::default();
    let device = recorder.device();
    let v = visual(&device);
    let id = 2;
    let visual3: IDCompositionVisual3 = v.cast().unwrap();
    let rising = animation(&device, &springy());
    unsafe {
        visual3.SetOpacity2(0.5).unwrap();
        assert_eq!(recorder.live().value(Object::Ours(id), "Opacity"), Some(0.5));
        visual3.SetOpacity(&rising).unwrap();
        assert!(recorder.live().curve(Object::Ours(id), "Opacity").is_some());
        assert_eq!(recorder.live().value(Object::Ours(id), "Opacity"), None);
        // The same animation on another property, then a cubic too late for either
        v.SetOffsetX(&rising).unwrap();
        assert_eq!(rising.AddCubic(0.3, 2.0, 0.0, 0.0, 0.0).unwrap_err().code(), E_INVALIDARG);
        let open = device.CreateAnimation().unwrap();
        open.AddCubic(0.0, 4.0, 1.0, 0.0, 0.0).unwrap();
        v.SetOffsetY(&open).unwrap();
        let mark = recorder.mark();
        open.AddCubic(1.0, 5.0, 0.0, 0.0, 0.0).unwrap();
        assert_eq!(
            recorder.since(mark),
            vec![Call::Segment {
                id: 4,
                segment: Segment::Cubic(Cubic {
                    begin: 1.0,
                    coefficients: [5.0, 0.0, 0.0, 0.0]
                }),
                late: true
            }]
        );
        visual3.SetOpacity2(1.0).unwrap();
        // None, a NaN, and an animation another device made are refused
        assert_eq!(v.SetOffsetX(None::<&IDCompositionAnimation>).unwrap_err().code(), E_INVALIDARG);
        assert_eq!(v.SetOffsetX2(f32::NAN).unwrap_err().code(), E_INVALIDARG);
        let foreign = Recorder::default().device().CreateAnimation().unwrap();
        assert_eq!(v.SetOffsetX(&foreign).unwrap_err().code(), E_INVALIDARG);
        // and an object of the wrong kind
        let not_an_animation: IDCompositionAnimation = std::mem::transmute(device.CreateRectangleClip().unwrap());
        assert_eq!(v.SetOffsetX(&not_an_animation).unwrap_err().code(), E_INVALIDARG);
        std::mem::forget(not_an_animation);
    }
    let live = recorder.live();
    let me = Object::Ours(id);
    assert_eq!(live.prop(me, "Opacity"), Some(&Prop::Value(1.0)));
    assert_eq!(live.curve(me, "OffsetX").unwrap().segments(), Some(springy()));
    // The copy holds the one cubic it had when handed over
    let offset_y = live.curve(me, "OffsetY").unwrap();
    assert_eq!(offset_y.segments.len(), 1);
    assert_eq!(offset_y.ends(), None);
    assert_eq!(offset_y.played(), None);
    assert!(close(offset_y.value(2.0), 6.0));
    assert_eq!(
        live.props(me).iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        vec!["OffsetX", "OffsetY", "Opacity"]
    );
}

/// An animation's value at a time is what lbterm_motion's own Segments say, and its played
/// numbers are what the state file would say of them
#[test]
fn an_animations_value_at_a_time_is_what_dwm_plays() {
    let recorder = Recorder::default();
    let device = recorder.device();
    let v = visual(&device);
    let segments = springy();
    let mark = recorder.mark();
    unsafe {
        v.SetOffsetX(&animation(&device, &segments)).unwrap();
    }
    let handed = recorder.animated(mark);
    assert_eq!(handed.len(), 1);
    let Handed { id, property, curve } = &handed[0];
    assert_eq!((*id, *property), (2, "OffsetX"));
    for t in [-0.05, 0.0, 0.05, 0.1, 0.15, 0.199, 0.2, 0.5] {
        assert_eq!(curve.value(t), segments.value(t), "at {t}");
    }
    assert!(close(curve.value(0.05), 0.5));
    assert!(close(curve.value(0.15), 1.275));
    assert_eq!(curve.value(0.2), 1.3f32 as f64);
    let span = segments.span();
    assert_eq!(
        curve.played(),
        Some(Played {
            from: span.from,
            to: span.to,
            low: span.low,
            high: span.high,
            ms: 200
        })
    );
    // It goes past where it ends on the way
    assert!(span.high > 1.3);
    assert_eq!(Prop::Animation(curve.clone()).at(0.05), curve.value(0.05));
    assert_eq!(Prop::Value(3.0).at(0.05), 3.0);
}

/// AddRepeat plays the lap before it for ever: the spinners' turn and `lap`'s values
#[test]
fn add_repeat_plays_its_lap_for_ever() {
    let recorder = Recorder::default();
    let device = recorder.device();
    unsafe {
        // A spinner: 360° a lap, as compositor.rs's rail and menu spinners turn
        let period = 1.2;
        let turn = device.CreateAnimation().unwrap();
        turn.AddCubic(0.0, 0.0, (360.0 / period) as f32, 0.0, 0.0).unwrap();
        turn.AddRepeat(period, period).unwrap();
        let rotate = device.CreateRotateTransform().unwrap();
        rotate.SetAngle(&turn).unwrap();
        let curve = recorder.live().curve(recorder.object(&rotate).unwrap(), "Angle").unwrap().clone();
        assert!(close(curve.value(0.3), 90.0));
        assert!(close(curve.value(period + 0.3), 90.0));
        assert!(close(curve.value(2.0 * period + 0.6), 180.0));
        assert_eq!(curve.lap(), Some(period));
        assert_eq!(curve.ends(), None);
        let played = curve.played().unwrap();
        assert!(close(played.from, 0.0) && close(played.to, 360.0) && close(played.high, 360.0));
        assert_eq!(played.ms, 1200);
        assert_eq!(curve.to_string(), "0 to 360 every 1200 ms");

        // `lap`: straight from each value to the next and from the last back to the first
        let (values, lap) = ([0.0f32, 10.0, 4.0], 0.3);
        let dt = lap / 3.0;
        let laps = device.CreateAnimation().unwrap();
        for (k, &v) in values.iter().enumerate() {
            let next = values[(k + 1) % 3];
            laps.AddCubic(k as f64 * dt, v, ((next - v) as f64 / dt) as f32, 0.0, 0.0).unwrap();
        }
        laps.AddRepeat(lap, lap).unwrap();
        let v = visual(&device);
        v.SetOffsetY(&laps).unwrap();
        let curve = recorder.live().curve(recorder.object(&v).unwrap(), "OffsetY").unwrap().clone();
        for (t, want) in [(0.05, 5.0), (0.15, 7.0), (0.25, 2.0)] {
            for k in 0..4 {
                let t = t + k as f64 * lap;
                assert!(close(curve.value(t), want), "{} at {t}", curve.value(t));
            }
        }

        // What can't be played is refused
        let refused = |r: windows58::core::Result<()>| assert_eq!(r.unwrap_err().code(), E_INVALIDARG);
        let bad = device.CreateAnimation().unwrap();
        refused(bad.AddRepeat(1.0, 1.0));
        bad.AddCubic(0.5, 0.0, 1.0, 0.0, 0.0).unwrap();
        refused(bad.AddRepeat(1.0, 0.0));
        refused(bad.AddCubic(0.2, 0.0, 0.0, 0.0, 0.0));
        refused(bad.End(0.1, 0.0));
        bad.End(1.0, 0.0).unwrap();
        refused(bad.End(2.0, 0.0));
        refused(bad.AddCubic(1.5, 0.0, 0.0, 0.0, 0.0));
        // and what the stand-in doesn't play fails out loud
        assert_eq!(bad.AddSinusoidal(0.0, 0.0, 1.0, 1.0, 0.0).unwrap_err().code(), E_NOTIMPL);
    }
}

/// A rectangle clip's edges and corners, a value or an animation each, linked to a visual and
/// let go
#[test]
fn a_clip_with_corners() {
    let recorder = Recorder::default();
    let (device, _target, root) = window(&recorder);
    unsafe {
        let clip = device.CreateRectangleClip().unwrap();
        clip.SetLeft2(0.0).unwrap();
        clip.SetTop2(0.0).unwrap();
        clip.SetRight2(640.0).unwrap();
        clip.SetBottom2(480.0).unwrap();
        clip.SetTopLeftRadiusX2(8.0).unwrap();
        clip.SetTopLeftRadiusY2(8.0).unwrap();
        clip.SetTopRightRadiusX2(8.0).unwrap();
        clip.SetTopRightRadiusY2(8.0).unwrap();
        clip.SetBottomLeftRadiusX2(4.0).unwrap();
        clip.SetBottomLeftRadiusY2(4.0).unwrap();
        // One corner rounding as it plays
        let rounding = animation(&device, &springy());
        clip.SetBottomRightRadiusX(&rounding).unwrap();
        clip.SetBottomRightRadiusY(&rounding).unwrap();
        clip.SetRight(&rounding).unwrap();
        root.SetClip(&clip).unwrap();
        device.Commit().unwrap();

        let tree = recorder.tree();
        let clip_o = tree.link(tree.root().unwrap(), "Clip").unwrap();
        assert_eq!(Some(clip_o), recorder.object(&clip));
        assert_eq!(tree.kind(clip_o), "clip");
        assert_eq!(tree.value(clip_o, "Bottom"), Some(480.0));
        assert_eq!(tree.value(clip_o, "TopRightRadiusY"), Some(8.0));
        assert_eq!(tree.value(clip_o, "BottomLeftRadiusX"), Some(4.0));
        assert_eq!(tree.value(clip_o, "Right"), None);
        for corner in ["BottomRightRadiusX", "BottomRightRadiusY", "Right"] {
            assert_eq!(tree.curve(clip_o, corner).unwrap().segments(), Some(springy()));
        }
        assert_eq!(tree.props(clip_o).len(), 12);

        // A visual can't be a clip, and none lets it go
        let not_a_clip: IDCompositionClip = std::mem::transmute(visual(&device));
        assert_eq!(root.SetClip(&not_a_clip).unwrap_err().code(), E_INVALIDARG);
        std::mem::forget(not_a_clip);
        root.SetClip(None::<&IDCompositionClip>).unwrap();
        assert_eq!(recorder.live().link(tree.root().unwrap(), "Clip"), None);
        assert_eq!(root.SetClip2(std::ptr::null()).unwrap_err().code(), E_NOTIMPL);
    }
}

/// A colour matrix's elements by row and column, a value or an animation, and its clamp; a hue
/// rotation's angle; the transforms' angles, scales and centres; each linked to a visual
#[test]
fn a_colour_matrix_and_the_other_effects() {
    let recorder = Recorder::default();
    let (device, _target, root) = window(&recorder);
    let device3: IDCompositionDevice3 = device.cast().unwrap();
    unsafe {
        // The dimming tint compositor.rs gives a pane: the colours scaled, an offset animated
        let tint = device3.CreateColorMatrixEffect().unwrap();
        for i in 0..4 {
            tint.SetMatrixElement2(i, i, 1.0).unwrap();
        }
        let fading = animation(&device, &springy());
        tint.SetMatrixElement(4, 0, &fading).unwrap();
        tint.SetClampOutput(true).unwrap();
        assert_eq!(tint.SetMatrixElement2(5, 0, 1.0).unwrap_err().code(), E_INVALIDARG);
        assert_eq!(tint.SetMatrixElement2(0, 4, 1.0).unwrap_err().code(), E_INVALIDARG);
        assert_eq!(tint.SetMatrixElement2(-1, 0, 1.0).unwrap_err().code(), E_INVALIDARG);
        assert_eq!(tint.SetMatrix(std::ptr::null()).unwrap_err().code(), E_NOTIMPL);
        assert_eq!(tint.SetInput(0, None::<&IUnknown>, 0).unwrap_err().code(), E_NOTIMPL);
        root.SetEffect(&tint).unwrap();

        let a = visual(&device);
        let hue = device3.CreateHueRotationEffect().unwrap();
        hue.SetAngle2(30.0).unwrap();
        a.SetEffect(&hue).unwrap();
        let rotate = device.CreateRotateTransform().unwrap();
        rotate.SetAngle2(45.0).unwrap();
        rotate.SetCenterX2(8.0).unwrap();
        rotate.SetCenterY2(6.0).unwrap();
        a.SetTransform(&rotate).unwrap();
        let b = visual(&device);
        let scale = device.CreateScaleTransform().unwrap();
        scale.SetScaleX2(2.0).unwrap();
        scale.SetScaleY(&fading).unwrap();
        scale.SetCenterX2(4.0).unwrap();
        scale.SetCenterY2(4.0).unwrap();
        b.SetTransform(&scale).unwrap();
        b.SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR).unwrap();
        root.AddVisual(&a, false, NONE).unwrap();
        root.AddVisual(&b, false, NONE).unwrap();
        device.Commit().unwrap();
    }
    let tree = recorder.tree();
    let root_o = tree.root().unwrap();
    let tint = tree.link(root_o, "Effect").unwrap();
    assert_eq!(tree.kind(tint), "colour matrix");
    for i in 0..4 {
        assert_eq!(tree.value(tint, matrix(i, i)), Some(1.0));
    }
    assert_eq!(tree.value(tint, matrix(0, 1)), None);
    assert_eq!(tree.curve(tint, matrix(4, 0)).unwrap().segments(), Some(springy()));
    assert_eq!(tree.value(tint, "ClampOutput"), Some(1.0));
    let [a, b] = [0, 1].map(|k| tree.children(root_o)[k]);
    assert_eq!(tree.value(tree.link(a, "Effect").unwrap(), "Angle"), Some(30.0));
    let rotate = tree.link(a, "Transform").unwrap();
    assert_eq!(tree.kind(rotate), "rotate");
    assert_eq!(
        ["Angle", "CenterX", "CenterY"].map(|p| tree.value(rotate, p)),
        [Some(45.0), Some(8.0), Some(6.0)]
    );
    let scale = tree.link(b, "Transform").unwrap();
    assert_eq!(tree.value(scale, "ScaleX"), Some(2.0));
    assert!(tree.curve(scale, "ScaleY").is_some());
    assert_eq!(
        tree.value(b, "BitmapInterpolationMode"),
        Some(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR.0 as f32)
    );
}

/// Nothing shows until Commit; what's pending is the calls since it that change something;
/// the tree reads as text for a test's failure
#[test]
fn commit_shows_what_came_before_it() {
    let recorder = Recorder::default();
    let (device, _target, root) = window(&recorder);
    assert_eq!(recorder.tree(), Tree::default());
    assert_eq!(recorder.pending(), vec![Call::Linked { id: 2, property: "Root", object: Some(Object::Ours(3)) }]);
    let a = visual(&device);
    let clip = unsafe { device.CreateRectangleClip().unwrap() };
    unsafe {
        a.SetOffsetX2(10.0).unwrap();
        a.SetClip(&clip).unwrap();
        clip.SetRight2(20.0).unwrap();
        root.AddVisual(&a, false, NONE).unwrap();
        // A method it doesn't record changes nothing
        let a3: IDCompositionVisual3 = a.cast().unwrap();
        assert_eq!(a3.SetVisible(true).unwrap_err().code(), E_NOTIMPL);
    }
    assert_eq!(recorder.pending().len(), 5);
    assert_eq!(recorder.tree().root(), None);
    let mark = recorder.mark();
    unsafe { device.Commit().unwrap() };
    assert!(matches!(recorder.since(mark)[..], [Call::Commit { .. }]), "{:?}", recorder.since(mark));
    assert_eq!(recorder.pending(), vec![]);
    let tree = recorder.tree();
    assert_eq!(tree, recorder.live());
    assert_eq!(tree.root(), Some(Object::Ours(3)));
    assert_eq!(tree.value(Object::Ours(4), "OffsetX"), Some(10.0));

    // A change after it is pending, and the tree still shows what was committed
    let fade = animation(&device, &springy());
    unsafe {
        let a3: IDCompositionVisual3 = a.cast().unwrap();
        a3.SetOpacity(&fade).unwrap();
        a.SetContent(&gpu_free_content()).unwrap();
    }
    assert_eq!(recorder.pending().len(), 2);
    assert_eq!(recorder.tree(), tree);
    unsafe { device.Commit().unwrap() };
    assert_eq!(
        recorder.tree().to_string(),
        "target 2\n  visual 3\n    visual 4 (OffsetX 10, Opacity 0 to 1.3 in 200 ms (0 to 1.32))\n      Clip: clip 5 (Right 20)\n      Content: foreign 7\n"
    );
}

/// Something from outside the recorder to show, as a swapchain is
fn gpu_free_content() -> IUnknown {
    // Any COM object will do: another recorder's device, which this recorder didn't make
    Recorder::default().device().cast().unwrap()
}

/// A wgpu surface over a stand-in visual, on WARP: wgpu makes a real composition swapchain
/// and gives it to the visual with SetContent, the one DirectComposition call wgpu-hal makes
/// on it, so the compositor's surfaces need no seam (COMPOSITOR.md, "Checking it without a
/// window", Surfaces)
#[test]
fn a_wgpu_surface_over_a_stand_in_visual_gives_it_a_swapchain() {
    let recorder = Recorder::default();
    let device = recorder.device();
    let visual = visual(&device);
    // Its turn with the device, held until the surface is dropped (PITFALLS 303)
    let turn = gpu();
    let Gpu {
        instance,
        adapter,
        device,
        ..
    } = &*turn;
    let surface = unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CompositionVisual(visual.as_raw()))
            .unwrap()
    };
    assert!(adapter.is_surface_supported(&surface));
    let caps = surface.get_capabilities(&adapter);
    let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
        wgpu::CompositeAlphaMode::PreMultiplied
    } else {
        caps.alpha_modes[0]
    };
    surface.configure(
        device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: caps.formats[0],
            width: 64,
            height: 32,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        },
    );
    let content = recorder.live().link(Object::Ours(2), "Content");
    assert!(matches!(content, Some(Object::Foreign(_))), "{:?}", recorder.calls());
    // What it was given is the surface's own swapchain
    recorder
        .foreign(content.unwrap())
        .unwrap()
        .cast::<windows58::Win32::Graphics::Dxgi::IDXGISwapChain1>()
        .expect("the content is a DXGI swapchain");
}
