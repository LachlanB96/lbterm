//! LBTerm: the window strip's words (lbterm/DESIGN.md, "The window strip"). With the tab panel
//! listing the tabs, the strip shows the active tab's title where the tabs would be, then
//! dimmer the summary Terminal's summary worker last wrote for the tab's Claude session, and by
//! the window's buttons when ("22:22 · 2m ago"). Resting the pointer on the words drops the
//! session's detail down in a card, and Alt+S (Terminal's `expanded.flag`) pins it open. What
//! fits and where it goes are lbterm_extras::strip's; the summaries come from
//! crate::summaries, read off this thread.
//!
//! With `lbterm_prompt_history` the strip has a second row: an arrow, then the Claude pane's last
//! prompt on one line. Resting the pointer on a prompt cut short shows all of it in the same card;
//! the arrow and a click on the prompt are render/prompt.rs's.
//!
//! The words are part of the fancy tab bar, which is built again when what they say changes
//! ([`TermWindow::lbterm_strip_refresh`], every paint). They stay something the window is
//! dragged by. The card is drawn in z-layers of its own. With the compositor they're a visual
//! of their own, over the panes and the strip's hover slots and under the tab panel, which DWM
//! fades in on `pane_enter` as the card opens and out on `pane_exit` as it closes; the card
//! that's going is drawn as it was until it has faded. Without it the card comes and goes at
//! once, in the window's order of z-layers.

use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::{
    BorderColor, BoxDimension, Corners, DisplayType, Element, ElementColors, ElementContent,
    LayoutContext, SizedPoly, VerticalAlign,
};
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use crate::termwindow::{TermWindowNotif, UIItemType};
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, RectF, WindowOps};
use config::{ConfigHandle, Dimension, DimensionContext, SrgbaTuple};
use lbterm_extras::strip::{self, Card, CardDraw, Content, DetailRow};
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use lbterm_panel::Hover;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};
use wezterm_font::LoadedFont;
use window::color::LinearRgba;

/// The card's shadow, the card, and its words. With the compositor they're the card's visual's
/// alone (`Target::StripCard`), which DWM fades.
const SHADOW_Z: i8 = 30;
const CARD_Z: i8 = 31;
const TEXT_Z: i8 = 32;

/// Whether a z-layer is the card's
pub fn owns_zindex(zindex: i8) -> bool {
    (SHADOW_Z..=TEXT_Z).contains(&zindex)
}

/// The pointer rests on the words this long before the card drops, so one passing over them on
/// the way to the buttons doesn't fling it open; and it closes this long after it leaves
const OPEN_DELAY: Duration = Duration::from_millis(300);
const CLOSE_DELAY: Duration = Duration::from_millis(200);

/// White over the strip, as the eye sees it (the pill's way, restart.rs)
const TITLE_ALPHA: f32 = 0.92;
const SUMMARY_ALPHA: f32 = 0.60;
const SUMMARY_OPEN_ALPHA: f32 = 0.85;
const SEPARATOR_ALPHA: f32 = 0.35;
const CAPTION_ALPHA: f32 = 0.50;
const LABEL_ALPHA: f32 = 0.50;
const DETAIL_ALPHA: f32 = 0.85;
/// The prompt at rest and under the pointer, and a note in its place
const PROMPT_ALPHA: f32 = 0.80;
const PROMPT_HOVER_ALPHA: f32 = 1.0;
const NOTE_ALPHA: f32 = 0.55;
/// The arrow's chip: its fill at rest and under the pointer, and the arrow itself
const CHIP_ALPHA: f32 = 0.07;
const CHIP_HOVER_ALPHA: f32 = 0.18;
const ARROW_ALPHA: f32 = 0.70;
/// The card's hairline: the outline on hover
const RING_ALPHA: f32 = 0.14;
/// A notice in the caption's place: brighter than the time it stands in for
const NOTICE_ALPHA: f32 = 0.85;
/// A badge after the title: its words, on the arrow chip's fill
const BADGE_ALPHA: f32 = 0.85;

/// What the card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardFor {
    /// The session's detail, under the first row's words
    Summary,
    /// All of the last prompt, cut short on the second row
    Prompt,
}

pub struct StripState {
    /// What the strip was last built with
    shown: Option<Content>,
    /// What the card showed when it was
    shown_card: Option<CardFor>,
    hover: Hover,
    /// The pointer on the second row's prompt
    prompt_hover: Hover,
    /// The second row had to cut the prompt short, as last built
    prompt_cut: Cell<bool>,
    /// The card while it's open, as last drawn: what the pointer can be on
    card: Option<Card>,
    /// The card last drawn, which a closing card fades out showing
    last_card: Option<Card>,
    /// Where the card's visual goes this frame, and whether it's opening
    visual: Option<(PixelRect, bool)>,
    /// When the caption next says something else
    caption_due: Option<Instant>,
    /// When a repaint is already on its way
    wake: Option<Instant>,
    /// A notice in the caption's place, and when it goes
    notice: Option<(String, Instant)>,
    /// The tab the strip was last built for: words that change with the tab swap at once,
    /// words that change in it dissolve (render/strip_swap.rs)
    tab: Option<mux::tab::TabId>,
    pub(crate) swap: super::strip_swap::SwapState,
}

impl Default for StripState {
    fn default() -> Self {
        Self {
            shown: None,
            shown_card: None,
            hover: Hover::new(OPEN_DELAY, CLOSE_DELAY),
            prompt_hover: Hover::new(OPEN_DELAY, CLOSE_DELAY),
            prompt_cut: Cell::new(false),
            card: None,
            last_card: None,
            visual: None,
            caption_due: None,
            wake: None,
            notice: None,
            tab: None,
            swap: Default::default(),
        }
    }
}

/// The strip's words, ready for the fancy tab bar
pub struct StripElements {
    /// Title and summary, left, where the tabs would be
    pub words: Element,
    /// When the summary was written, right, by the buttons
    pub caption: Option<Element>,
    /// The second row: the arrow and the last prompt, a line of its own under the first
    pub prompt_row: Option<Element>,
    /// How far in from the bar's left the words start, to line up with the panes' text
    pub indent: f32,
}

/// How tall the strip's second row is, when it has one: `lbterm_prompt_history` set, and the
/// fancy tab bar holding only the window's buttons for the panel. Every tab has it, with or
/// without a prompt, so switching tabs never resizes the panes.
pub fn prompt_row_height(
    config: &ConfigHandle,
    fontconfig: &wezterm_font::FontConfiguration,
) -> anyhow::Result<f32> {
    let on = config.lbterm_prompt_history.as_deref().map_or(false, |p| !p.is_empty())
        && config.use_fancy_tab_bar
        && config::lbterm::hides_tabs(config);
    if !on {
        return Ok(0.);
    }
    let font = fontconfig.title_font()?;
    Ok((font.metrics().cell_height.get() as f32 * 1.4).ceil())
}

fn over(surface: SrgbaTuple, alpha: f32) -> LinearRgba {
    let c = |v: f32| v + (1. - v) * alpha;
    SrgbaTuple(c(surface.0), c(surface.1), c(surface.2), 1.).to_linear()
}

fn text_colors(color: LinearRgba) -> ElementColors {
    ElementColors {
        border: BorderColor::default(),
        bg: LinearRgba::TRANSPARENT.into(),
        text: color.into(),
    }
}

fn local(time: SystemTime) -> chrono::NaiveDateTime {
    chrono::DateTime::<chrono::Local>::from(time).naive_local()
}

impl crate::TermWindow {
    /// What the strip says for the active tab, with the panel hiding the tabs from the fancy
    /// tab bar (the retro one doesn't know the panel)
    fn lbterm_strip_content(&mut self) -> Option<Content> {
        if !config::lbterm::hides_tabs(&self.config)
            || !self.config.use_fancy_tab_bar
            || !self.show_tab_bar
        {
            return None;
        }
        let (idx, tab) = self.lbterm_active_tab()?;
        let now = SystemTime::now();
        let unix = |t: SystemTime| {
            t.duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64())
        };
        let panel_tab = super::tab_panel::panel_tab(&tab, idx, true);
        let (row, _) = lbterm_panel::row(
            &panel_tab,
            unix(now),
            self.config.lbterm_tab_panel.idle_full_minutes * 60.0,
        );
        let mut content = Content {
            title: row.title,
            ..Content::default()
        };
        self.lbterm_strip.caption_due = None;
        let target = super::prompt::target(&tab);
        let summary = target
            .as_ref()
            .and_then(|t| crate::summaries::get(&t.sid));
        if let Some(summary) = summary {
            content.summary = summary.short;
            if let Some(written) = summary.written.filter(|_| content.summary.is_some()) {
                let (written, now) = (local(written), local(now));
                content.caption = Some(strip::caption(written, now));
                self.lbterm_strip.caption_due =
                    Some(Instant::now() + strip::caption_changes_in(written, now));
            }
            content.detail = summary.detail.iter().map(|l| strip::detail_row(l)).collect();
        }
        if self.lbterm_prompt_row_on() {
            content.prompt = target.and_then(|t| t.prompt);
            content.note = self.lbterm_prompt_note();
        }
        // A notice in the caption's place for a moment
        content.notice = self
            .lbterm_strip
            .notice
            .as_ref()
            .filter(|(_, until)| Instant::now() < *until)
            .map(|(text, _)| text.clone());
        // Zoomed, the tab's other panes are out of sight: say so, and which pane this is
        if let Some(zoomed) = tab.get_zoomed_pane() {
            let panes = tab.iter_panes_ignoring_zoom();
            if let Some(i) = panes.iter().position(|p| p.pane.pane_id() == zoomed.pane_id()) {
                content.badge = Some(strip::zoom_badge(i, panes.len()));
            }
        }
        Some(content)
    }

    /// The first window, as it opens: what it came back as after a restart, or how many tabs it
    /// found still running in the mux server after a crash (strip::arrival_notice)
    pub(crate) fn lbterm_arrival_notice(&mut self) {
        let mux = mux::Mux::get();
        // Tabs this GUI didn't make: in a mux server it found running, not one it started
        let tabs = if wezterm_client::client::lbterm_started_server() {
            0
        } else {
            mux.get_window(self.mux_window_id).map_or(0, |w| {
                w.iter_tabs()
                    .filter(|tab| {
                        tab.get_active_pane()
                            .and_then(|p| mux.get_domain(p.domain_id()))
                            .map_or(false, |d| d.domain_name() != "local")
                    })
                    .count()
            })
        };
        let from = crate::restart::restarted_from();
        let running = config::wezterm_version();
        if let Some(text) = strip::arrival_notice(from.as_deref(), running, tabs) {
            self.lbterm_strip_notice(text);
        }
    }

    /// Say `text` in the caption's place for a moment (strip::NOTICE_FOR): what happened to the
    /// window, such as the config reloading or a restart
    pub(crate) fn lbterm_strip_notice(&mut self, text: impl Into<String>) {
        let until = Instant::now() + strip::NOTICE_FOR;
        self.lbterm_strip.notice = Some((text.into(), until));
        self.lbterm_strip_wake(Some(until));
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// The strip has its second row
    pub(crate) fn lbterm_prompt_row_on(&self) -> bool {
        prompt_row_height(&self.config, &self.fonts).map_or(false, |h| h > 0.)
    }

    /// What the card shows: all of the prompt while the pointer rests on it cut short; else the
    /// detail, while the pointer rests on the words or the card, or Alt+S pins it
    fn lbterm_strip_card_for(&self, content: Option<&Content>) -> Option<CardFor> {
        let content = content?;
        if self.lbterm_strip.prompt_hover.is_open()
            && self.lbterm_strip.prompt_cut.get()
            && content.prompt.is_some()
            && content.note.is_none()
        {
            return Some(CardFor::Prompt);
        }
        let pinned = crate::summaries::store().map_or(false, |s| s.expanded());
        (!content.detail.is_empty() && (self.lbterm_strip.hover.is_open() || pinned))
            .then_some(CardFor::Summary)
    }

    /// Before the strip is built, each paint: build it again when what it says has changed
    /// (a tab switch, a new title, summary or prompt, the caption's minute, the card opening)
    pub(crate) fn lbterm_strip_refresh(&mut self) {
        // Coming back while the card fades out opens it again at once (lbterm_panel::hover).
        // Leaving while it fades in still waits out the grace: the pointer crosses the prompt
        // row on its way down to the card
        let motion = crate::termwindow::compositor::MotionSettings::from_config(&self.config.lbterm_motion);
        let closing = if self.lbterm_composited() && motion.enabled {
            Duration::from_secs_f64(motion.exit.duration())
        } else {
            Duration::ZERO
        };
        self.lbterm_strip.hover.set_motion(Duration::ZERO, closing);
        self.lbterm_strip.prompt_hover.set_motion(Duration::ZERO, closing);

        let content = self.lbterm_strip_content();
        let card = self.lbterm_strip_card_for(content.as_ref());
        if content != self.lbterm_strip.shown || card != self.lbterm_strip.shown_card {
            // Words that change in the same tab dissolve into the new ones; a tab switch swaps
            // them at once (render/strip_swap.rs)
            let tab = self.lbterm_active_tab().map(|(_, tab)| tab.tab_id());
            if let (Some(old), Some(new)) = (self.lbterm_strip.shown.clone(), content.as_ref()) {
                if tab.is_some() && tab == self.lbterm_strip.tab {
                    self.lbterm_strip_swap_begin(&old, new);
                }
            }
            self.lbterm_strip.tab = tab;
            self.lbterm_strip.shown = content;
            self.lbterm_strip.shown_card = card;
            self.invalidate_fancy_tab_bar();
        }
        let notice_due = self
            .lbterm_strip
            .notice
            .as_ref()
            .map(|(_, until)| *until)
            .filter(|until| *until > Instant::now());
        let due = [
            self.lbterm_strip.caption_due,
            self.lbterm_strip.hover.deadline(),
            self.lbterm_strip.prompt_hover.deadline(),
            self.lbterm_prompt_note_due(),
            notice_due,
        ]
        .iter()
        .flatten()
        .min()
        .copied();
        self.lbterm_strip_wake(due);
    }

    /// Where the strip's words start in the window: where the panes' text does
    pub(crate) fn lbterm_strip_text_x(&self) -> f32 {
        self.get_os_border().left.get() as f32 + self.padding_left_top().0
    }

    /// The strip's words for the fancy tab bar, fitted beside `right` (the buttons and the
    /// restart pill) in the first row, `bar`, and the prompt row under it when there's one, or
    /// nothing when it has none
    pub(crate) fn lbterm_strip_elements(
        &self,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        surface: SrgbaTuple,
        right: &[Element],
        bar: RectF,
    ) -> anyhow::Result<Option<StripElements>> {
        let Some(content) = self.lbterm_strip.shown.as_ref() else {
            return Ok(None);
        };
        let cell = font.metrics().cell_width.get() as f32;
        if cell <= 0. {
            return Ok(None);
        }
        // How wide the buttons and the pill are, laid out alone
        let right_width = if right.is_empty() {
            0.
        } else {
            let computed = self.compute_element(
                &LayoutContext {
                    height: DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.dimensions.pixel_height as f32,
                        pixel_cell: metrics.cell_size.height as f32,
                    },
                    width: DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.dimensions.pixel_width as f32,
                        pixel_cell: metrics.cell_size.width as f32,
                    },
                    bounds: bar,
                    metrics,
                    gl_state: self.render_state.as_ref().unwrap(),
                    zindex: 10,
                },
                &Element::new(font, ElementContent::Children(right.to_vec())),
            )?;
            computed.bounds.width()
        };
        let indent = (self.lbterm_strip_text_x() - bar.min_x()).max(0.);
        // The caption's margin to the pill or the buttons, half a column
        let margin = metrics.cell_size.width as f32 * 0.5;
        let room = bar.width() - indent - right_width - margin;
        let fitted = strip::fit(content, (room / cell).floor().max(0.) as usize);

        let piece = |text: String, alpha: f32| {
            Element::new(font, ElementContent::Text(text))
                .colors(text_colors(over(surface, alpha)))
                .vertical_align(VerticalAlign::Middle)
        };
        let mut kids = vec![piece(fitted.title, TITLE_ALPHA)];
        // A zoomed tab's badge, in a chip of the arrow's shape after the title
        if let Some(badge) = fitted.badge {
            let radius = Dimension::Pixels((metrics.cell_size.height as f32 * 0.3).round());
            let corner = |poly| SizedPoly {
                width: radius,
                height: radius,
                poly,
            };
            let fill = over(surface, CHIP_ALPHA);
            kids.push(
                Element::new(font, ElementContent::Text(badge))
                    .vertical_align(VerticalAlign::Middle)
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.5),
                        top: Dimension::Cells(0.05),
                        bottom: Dimension::Cells(0.05),
                    })
                    .border(BoxDimension::new(Dimension::Pixels(1.)))
                    .border_corners(Some(Corners {
                        top_left: corner(TOP_LEFT_ROUNDED_CORNER),
                        top_right: corner(TOP_RIGHT_ROUNDED_CORNER),
                        bottom_left: corner(BOTTOM_LEFT_ROUNDED_CORNER),
                        bottom_right: corner(BOTTOM_RIGHT_ROUNDED_CORNER),
                    }))
                    .margin(BoxDimension {
                        left: Dimension::Cells(1.),
                        right: Dimension::Cells(0.),
                        top: Dimension::Cells(0.),
                        bottom: Dimension::Cells(0.),
                    })
                    .colors(ElementColors {
                        border: BorderColor::new(fill),
                        bg: fill.into(),
                        text: over(surface, BADGE_ALPHA).into(),
                    }),
            );
        }
        // The summary brightens the moment the pointer reaches the words, before the card waits
        // out its rest: with the compositor it fades in on a hover slot (render/hover_fade.rs)
        let mut answers = None;
        if let Some(summary) = fitted.summary {
            kids.push(piece(strip::SEPARATOR.to_string(), SEPARATOR_ALPHA));
            if self.lbterm_strip.shown_card == Some(CardFor::Summary) {
                kids.push(piece(summary, SUMMARY_OPEN_ALPHA));
            } else {
                // On the strip's own colour, so on its slot it covers the dim summary and the
                // two cross-fade: bright text over dim text reads heavier than either
                let lit = ElementColors {
                    bg: over(surface, 0.).into(),
                    ..text_colors(over(surface, SUMMARY_OPEN_ALPHA))
                };
                kids.push(piece(summary, SUMMARY_ALPHA).hover_colors(Some(lit)));
                answers = Some(text_colors(over(surface, TITLE_ALPHA)));
            }
        }
        let words = Element::new(font, ElementContent::Children(kids))
            .item_type(UIItemType::TabBar(TabBarItem::LbtermSummary))
            .colors(text_colors(over(surface, TITLE_ALPHA)))
            .hover_colors(answers)
            .min_height(Some(Dimension::Pixels(bar.height())))
            .vertical_align(VerticalAlign::Middle);
        let caption_alpha = if fitted.notice { NOTICE_ALPHA } else { CAPTION_ALPHA };
        let caption = fitted.caption.map(|caption| {
            piece(caption, caption_alpha)
                .item_type(UIItemType::TabBar(TabBarItem::None))
                .margin(BoxDimension {
                    left: Dimension::Cells(0.),
                    right: Dimension::Pixels(margin),
                    top: Dimension::Cells(0.),
                    bottom: Dimension::Cells(0.),
                })
        });
        let prompt_row = self.lbterm_prompt_row(font, metrics, surface, content, bar, indent)?;
        Ok(Some(StripElements {
            words,
            caption,
            prompt_row,
            indent,
        }))
    }

    /// The second row: the arrow in a chip, which opens the list of prompts, then the last prompt,
    /// which jumps to it, or a note in its place for a moment (render/prompt.rs)
    fn lbterm_prompt_row(
        &self,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        surface: SrgbaTuple,
        content: &Content,
        bar: RectF,
        indent: f32,
    ) -> anyhow::Result<Option<Element>> {
        let height = prompt_row_height(&self.config, &self.fonts)?;
        if height <= 0. {
            return Ok(None);
        }
        let cell = font.metrics().cell_width.get() as f32;
        let mut kids = vec![];
        if content.prompt.is_some() || content.note.is_some() {
            // The chip's corners: filled in its colour, which the border is too
            let radius = Dimension::Pixels((metrics.cell_size.height as f32 * 0.3).round());
            let corner = |poly| SizedPoly {
                width: radius,
                height: radius,
                poly,
            };
            let chip = |fill: f32, arrow: f32| ElementColors {
                border: BorderColor::new(over(surface, fill)),
                bg: over(surface, fill).into(),
                text: over(surface, arrow).into(),
            };
            kids.push(
                Element::new(font, ElementContent::Text(lbterm_extras::prompts::MARK.to_string()))
                    .item_type(UIItemType::TabBar(TabBarItem::LbtermPromptList))
                    .vertical_align(VerticalAlign::Middle)
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.35),
                        right: Dimension::Cells(0.35),
                        top: Dimension::Cells(0.05),
                        bottom: Dimension::Cells(0.05),
                    })
                    .border(BoxDimension::new(Dimension::Pixels(1.)))
                    .border_corners(Some(Corners {
                        top_left: corner(TOP_LEFT_ROUNDED_CORNER),
                        top_right: corner(TOP_RIGHT_ROUNDED_CORNER),
                        bottom_left: corner(BOTTOM_LEFT_ROUNDED_CORNER),
                        bottom_right: corner(BOTTOM_RIGHT_ROUNDED_CORNER),
                    }))
                    .margin(BoxDimension {
                        left: Dimension::Cells(0.),
                        right: Dimension::Cells(0.75),
                        top: Dimension::Cells(0.),
                        bottom: Dimension::Cells(0.),
                    })
                    .colors(chip(CHIP_ALPHA, ARROW_ALPHA))
                    .hover_colors(Some(chip(CHIP_HOVER_ALPHA, 1.0))),
            );
            // What's left of the row after the chip, a column of it spare at the right
            let room = bar.width() - indent - 3.5 * cell;
            let width = (room / cell).floor().max(0.) as usize;
            let text = match (&content.note, &content.prompt) {
                (Some(note), _) => {
                    self.lbterm_strip.prompt_cut.set(false);
                    Element::new(font, ElementContent::Text(strip::elide(note, width)))
                        .colors(text_colors(over(surface, NOTE_ALPHA)))
                        .item_type(UIItemType::TabBar(TabBarItem::None))
                }
                (None, Some(prompt)) => {
                    let (shown, cut) = strip::fit_prompt(prompt, width);
                    self.lbterm_strip.prompt_cut.set(cut);
                    Element::new(font, ElementContent::Text(shown))
                        .colors(text_colors(over(surface, PROMPT_ALPHA)))
                        .hover_colors(Some(text_colors(over(surface, PROMPT_HOVER_ALPHA))))
                        .item_type(UIItemType::TabBar(TabBarItem::LbtermPrompt))
                }
                (None, None) => unreachable!(),
            };
            kids.push(text.vertical_align(VerticalAlign::Middle));
        }
        Ok(Some(
            Element::new(font, ElementContent::Children(kids))
                .display(DisplayType::Block)
                .item_type(UIItemType::TabBar(TabBarItem::None))
                .colors(text_colors(over(surface, PROMPT_ALPHA)))
                .min_width(Some(Dimension::Pixels(bar.width())))
                .min_height(Some(Dimension::Pixels(height)))
                .padding(BoxDimension {
                    left: Dimension::Pixels(indent),
                    right: Dimension::Cells(0.),
                    top: Dimension::Cells(0.),
                    bottom: Dimension::Cells(0.),
                })
                .vertical_align(VerticalAlign::Top),
        ))
    }

    /// The pointer moved, pressed or left: the card opens once it has rested on the words, or on
    /// a prompt cut short, and stays while it's on them or the card. True when the card took the
    /// event: a click on it doesn't reach the pane under it.
    pub(crate) fn lbterm_strip_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        if self.lbterm_strip.shown.is_none() {
            return false;
        }
        let (x, y) = (event.coords.x, event.coords.y);
        let on = |item: TabBarItem| {
            self.ui_items
                .iter()
                .any(|i| i.item_type == UIItemType::TabBar(item.clone()) && i.hit_test(x, y))
        };
        let on_words = on(TabBarItem::LbtermSummary);
        let on_prompt = on(TabBarItem::LbtermPrompt);
        let on_card = self
            .lbterm_strip
            .card
            .as_ref()
            .map_or(false, |card| card.rect.contains(x as f64, y as f64));
        let showing = self.lbterm_strip.shown_card;
        let now = Instant::now();
        let mut changed = self
            .lbterm_strip
            .hover
            .pointer(on_words || (on_card && showing == Some(CardFor::Summary)), now);
        changed |= self
            .lbterm_strip
            .prompt_hover
            .pointer(on_prompt || (on_card && showing == Some(CardFor::Prompt)), now);
        if changed {
            context.invalidate();
        }
        let due = [
            self.lbterm_strip.hover.deadline(),
            self.lbterm_strip.prompt_hover.deadline(),
        ]
        .iter()
        .flatten()
        .min()
        .copied();
        self.lbterm_strip_wake(due);
        if on_card && showing.is_some() {
            if matches!(event.kind, WMEK::Move) {
                context.set_cursor(Some(CursorIcon::Default));
            }
            return true;
        }
        false
    }

    /// The pointer left the window
    pub(crate) fn lbterm_strip_mouse_leave(&mut self) {
        let now = Instant::now();
        let changed = self.lbterm_strip.hover.left_window(now)
            | self.lbterm_strip.prompt_hover.left_window(now);
        if changed {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        let due = [
            self.lbterm_strip.hover.deadline(),
            self.lbterm_strip.prompt_hover.deadline(),
        ]
        .iter()
        .flatten()
        .min()
        .copied();
        self.lbterm_strip_wake(due);
    }

    /// Repaint at `at`: when a hover's wait is up, the caption moves on, or a note goes
    pub(crate) fn lbterm_strip_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at else {
            return;
        };
        // One already due sooner repaints, and the paint asks again
        if matches!(self.lbterm_strip.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_strip.wake = Some(at);
        promise::spawn::spawn(async move {
            smol::Timer::at(at).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_strip.wake == Some(at) {
                    tw.lbterm_strip.wake = None;
                }
                let now = Instant::now();
                tw.lbterm_strip.hover.tick(now);
                tw.lbterm_strip.prompt_hover.tick(now);
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    /// The card under the strip: laid out afresh while it's open, and with the compositor drawn
    /// as it was while its visual fades out
    pub(crate) fn paint_lbterm_strip_card(&mut self) -> anyhow::Result<()> {
        let open = self.lbterm_strip.shown_card.is_some() && !self.config.tab_bar_at_bottom;
        let fading = self.lbterm_composited()
            && self
                .webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .map_or(false, |c| c.strip_card_showing(Instant::now()));
        let card = match strip::card_draw(open, fading) {
            CardDraw::Fresh => self.lbterm_strip_lay_out_card()?,
            CardDraw::Last => self.lbterm_strip.last_card.clone(),
            CardDraw::None => None,
        };
        let scale = self.dimensions.dpi as f64 / 96.0;
        let window = (
            self.dimensions.pixel_width as u32,
            self.dimensions.pixel_height as u32,
        );
        self.lbterm_strip.visual = card.as_ref().and_then(|c| {
            strip::visual_bounds(&c.rect, scale, window)
                .map(|(x, y, w, h)| (PixelRect::new(x, y, w, h), open))
        });
        // Only an open card takes the pointer
        self.lbterm_strip.card = if open { card.clone() } else { None };
        if let Some(card) = card {
            let surface = self.palette().background;
            self.lbterm_strip_draw_card(&card, surface)?;
            self.lbterm_strip.last_card = Some(card);
        }
        if !open && fading {
            // Drawn until it has faded, then not
            let done = self
                .webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .and_then(|c| c.strip_card_settles());
            self.lbterm_strip_wake(done);
        }
        Ok(())
    }

    /// Where the card's visual goes this frame and whether it's opening (true) or closing, for
    /// the compositor; `None` while there's no card to show
    pub(crate) fn lbterm_strip_card_visual(&self) -> Option<(PixelRect, bool)> {
        self.lbterm_strip.visual
    }

    /// The card for what it shows now, under the strip
    fn lbterm_strip_lay_out_card(&self) -> anyhow::Result<Option<Card>> {
        let Some(content) = self.lbterm_strip.shown.as_ref() else {
            return Ok(None);
        };
        let prompt;
        let rows: &[DetailRow] = match self.lbterm_strip.shown_card {
            Some(CardFor::Prompt) => {
                prompt = content
                    .prompt
                    .iter()
                    .map(|text| DetailRow {
                        label: None,
                        text: text.clone(),
                    })
                    .collect::<Vec<_>>();
                &prompt
            }
            _ => &content.detail,
        };
        let scale = self.dimensions.dpi as f64 / 96.0;
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let cell = (
            font.metrics().cell_width.get(),
            metrics.cell_size.height as f64,
        );
        let border = self.get_os_border();
        let top = border.top.get() as f64 + self.tab_bar_pixel_height()? as f64 + 2.0 * scale;
        // Between the panes' room's edges: a gutter clear of the tab panel on the left
        let room = (
            border.left.get() as f64 + self.lbterm_shape().gutter() as f64,
            self.dimensions.pixel_width as f64 - border.right.get() as f64 - 4.0 * scale,
        );
        Ok(strip::card(
            rows,
            self.lbterm_strip_text_x() as f64,
            top,
            room,
            cell,
            scale,
        ))
    }

    /// Draw `card` over `surface`: its shadow, its hairline and fill, its words
    fn lbterm_strip_draw_card(&self, card: &Card, surface: SrgbaTuple) -> anyhow::Result<()> {
        let scale = self.dimensions.dpi as f64 / 96.0;
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let cell = (
            font.metrics().cell_width.get(),
            metrics.cell_size.height as f64,
        );
        let shape = self.lbterm_shape();
        let radius = shape.radius() as f64;
        let stroke = shape.hairline() as f64;
        let gl_state = self.render_state.as_ref().unwrap();
        let grow = |r: Rect, by: f64, down: f64| Rect {
            x: r.x - by,
            y: r.y - by + down,
            w: r.w + 2.0 * by,
            h: r.h + 2.0 * by,
        };

        // A soft shadow under it, so it reads over the pane's text
        {
            let layer = gl_state.layer_for_zindex(SHADOW_Z)?;
            let mut quads = layer.quad_allocator();
            // Out to strip::shadow_reach, which the card's visual holds
            let (reach, down) = strip::shadow_reach(scale);
            for (i, alpha) in [0.30f32, 0.18, 0.10, 0.05].iter().enumerate() {
                let by = reach * (2.0 + 3.0 * i as f64) / 11.0;
                let shadow = LinearRgba::with_components(0., 0., 0., *alpha);
                self.lbterm_menu_rounded(&mut quads, grow(card.rect, by, down), radius + by, shadow)?;
            }
        }
        // The hairline, then the card inside it
        {
            let layer = gl_state.layer_for_zindex(CARD_Z)?;
            let mut quads = layer.quad_allocator();
            self.lbterm_menu_rounded(&mut quads, grow(card.rect, stroke, 0.), radius + stroke, over(surface, RING_ALPHA))?;
            self.lbterm_menu_rounded(&mut quads, card.rect, radius, surface.to_linear())?;
        }

        let mut words = vec![];
        for line in &card.lines {
            let mut place = |text: &str, x: f64, w: f64, alpha: f32| -> anyhow::Result<()> {
                let element = Element::new(&font, ElementContent::Text(text.to_string()))
                    .colors(text_colors(over(surface, alpha)));
                words.push(self.compute_element(
                    &LayoutContext {
                        width: DimensionContext {
                            dpi: self.dimensions.dpi as f32,
                            pixel_max: self.dimensions.pixel_width as f32,
                            pixel_cell: metrics.cell_size.width as f32,
                        },
                        height: DimensionContext {
                            dpi: self.dimensions.dpi as f32,
                            pixel_max: self.dimensions.pixel_height as f32,
                            pixel_cell: metrics.cell_size.height as f32,
                        },
                        bounds: euclid::rect(x as f32, line.y as f32, w as f32, cell.1 as f32),
                        metrics: &metrics,
                        gl_state,
                        zindex: TEXT_Z,
                    },
                    &element,
                )?);
                Ok(())
            };
            if let Some(label) = &line.label {
                place(label, card.label_x, card.text_x - card.label_x, LABEL_ALPHA)?;
            }
            let text_w = card.rect.x + card.rect.w - card.text_x;
            place(&line.text, card.text_x, text_w, DETAIL_ALPHA)?;
        }
        for computed in &words {
            self.render_element(computed, gl_state, None)?;
        }
        Ok(())
    }
}

/// LBTerm: what the strip says, for the state file (lbterm/STATE.md)
impl crate::TermWindow {
    /// The strip's words and card as this frame left them, with `lit` what's lit under the
    /// pointer and `items` where the strip's parts are; `None` while the strip has no words (the
    /// tabs in the tab bar)
    pub(crate) fn lbterm_state_strip(
        &self,
        lit: Vec<String>,
        items: Vec<lbterm_extras::state::Item>,
    ) -> Option<lbterm_extras::state::Strip> {
        let content = self.lbterm_strip.shown.as_ref()?;
        Some(lbterm_extras::state::Strip {
            title: content.title.clone(),
            summary: content.summary.clone(),
            caption: content.caption.clone(),
            notice: content.notice.clone(),
            prompt: content.prompt.clone(),
            note: content.note.clone(),
            badge: content.badge.clone(),
            card: self.lbterm_strip.card.as_ref().map(|_| {
                match self.lbterm_strip.shown_card {
                    Some(CardFor::Prompt) => "prompt",
                    _ => "summary",
                }
                .to_string()
            }),
            restart: crate::restart::other_build_installed(),
            jumping: self.lbterm_prompt_jumping(),
            lit,
            items,
        })
    }

    /// Whether `x`, `y` is on the strip's open card
    pub(crate) fn lbterm_strip_card_at(&self, x: f64, y: f64) -> bool {
        self.lbterm_strip.card.as_ref().map_or(false, |card| card.rect.contains(x, y))
    }

    /// The strip's waits under way at `now`: the card's rest before it opens and grace before it
    /// closes (for the words and for the prompt), and a notice's time
    pub(crate) fn lbterm_strip_waits(&self, now: Instant) -> Vec<lbterm_extras::state::Wait> {
        let wait = |name: &str, due: Instant| lbterm_extras::state::Wait {
            name: name.to_string(),
            left_ms: lbterm_extras::state::millis(due.saturating_duration_since(now)),
        };
        let mut waits = vec![];
        for (hover, open, close) in [
            (&self.lbterm_strip.hover, "strip.card.open", "strip.card.close"),
            (&self.lbterm_strip.prompt_hover, "strip.prompt.open", "strip.prompt.close"),
        ] {
            if let Some(due) = hover.deadline() {
                match hover.phase() {
                    lbterm_panel::Phase::Opening => waits.push(wait(open, due)),
                    lbterm_panel::Phase::Closing => waits.push(wait(close, due)),
                    _ => {}
                }
            }
        }
        if let Some((_, until)) = self.lbterm_strip.notice.as_ref().filter(|(_, until)| *until > now) {
            waits.push(wait("strip.notice", *until));
        }
        waits
    }
}
