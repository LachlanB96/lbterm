//! LBTerm: the mic mark on the window strip (lbterm/CONTROL.md, "The mic mark"; the logic is
//! lbterm_extras::mic): what Earshot last said of its listening, over the control pipe, shown
//! left of the window's buttons in every window until its time runs out.

use crate::termwindow::box_model::{BorderColor, Element, ElementColors, ElementContent};
use crate::termwindow::TermWindowNotif;
use ::window::WindowOps;
use config::{Dimension, SrgbaTuple};
use lbterm_extras::mic::{Mic, MicState};
use lbterm_panel::palette;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static MIC: Mutex<Mic> = Mutex::new(Mic::new());

/// What the mark shows now
pub fn shown() -> Option<MicState> {
    MIC.lock().unwrap().shown(Instant::now())
}

/// Earshot's `mic`: `state` for `ttl`; every window's strip is built again now, and once its
/// time's run out
pub fn set(state: Option<MicState>, ttl: Duration) -> Option<MicState> {
    let now = Instant::now();
    let (before, until) = {
        let mut mic = MIC.lock().unwrap();
        let before = mic.shown(now);
        mic.set(state, ttl, now);
        (before, mic.until())
    };
    if before != state {
        redraw_strips();
    }
    if let Some(until) = until {
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(until + Duration::from_millis(20))).await;
            // Only when this is still the time it runs out at: a state said again since holds
            if MIC.lock().unwrap().until() == Some(until) {
                redraw_strips();
            }
        })
        .detach();
    }
    shown()
}

/// Rebuilds every window's strip, to show the mark as it is, as restart.rs does for its pill
fn redraw_strips() {
    let Some(fe) = crate::frontend::try_front_end() else {
        return;
    };
    for win in fe.gui_windows() {
        win.window.notify(TermWindowNotif::Apply(Box::new(|tw| {
            tw.invalidate_fancy_tab_bar();
            if let Some(window) = tw.window.as_ref() {
                window.invalidate();
            }
        })));
    }
}

/// White over the strip, as the eye sees it: dim while it listens, as the buttons rest
const LISTENING_ALPHA: f32 = 0.45;
const MUTED_ALPHA: f32 = 0.30;

fn over(surface: SrgbaTuple, alpha: f32) -> window::color::LinearRgba {
    let c = |v: f32| v + (1. - v) * alpha;
    SrgbaTuple(c(surface.0), c(surface.1), c(surface.2), 1.).to_linear()
}

/// The mark, for the strip's right, left of the restart pill and the buttons: lit in the tab
/// rail's active red while it hears him
pub fn element(
    font: &std::rc::Rc<wezterm_font::LoadedFont>,
    surface: SrgbaTuple,
    state: MicState,
) -> Element {
    use crate::tabbar::TabBarItem;
    use crate::termwindow::box_model::{BoxDimension, VerticalAlign};
    use crate::termwindow::UIItemType;

    let text = match state {
        MicState::Listening => over(surface, LISTENING_ALPHA),
        MicState::Muted => over(surface, MUTED_ALPHA),
        MicState::Hearing => {
            let (r, g, b) = palette::bytes(palette::ACTIVE);
            window::color::LinearRgba::with_srgba(r, g, b, 255)
        }
    };
    Element::new(font, ElementContent::Text(state.glyph().to_string()))
        .item_type(UIItemType::TabBar(TabBarItem::None))
        .vertical_align(VerticalAlign::Middle)
        .padding(BoxDimension {
            left: Dimension::Cells(0.5),
            right: Dimension::Cells(0.75),
            top: Dimension::Cells(0.),
            bottom: Dimension::Cells(0.),
        })
        .colors(ElementColors {
            border: BorderColor::default(),
            bg: window::color::LinearRgba::TRANSPARENT.into(),
            text: text.into(),
        })
}
