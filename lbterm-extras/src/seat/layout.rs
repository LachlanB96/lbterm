//! Where everything in the Conductor's view goes (lbterm/TAB-PANEL.md, "The Conductor's view"):
//! from what the seat said, what's typed and what the pointer is on, a [`Scene`] of rounded
//! rectangles, words and the places a click lands on, for the GUI to draw and route clicks
//! through. Nothing here draws; the GUI's termwindow/render/conductor_view.rs does.
//!
//! The view is a card in the panes' room, in Material's dark look (Lachlan, 2026-10-28: "like
//! something Google would've released"): Roboto in its type roles, code in the panes' font. Down
//! its main column: a header (the Conductor's state in chips, its modes as a button group, its
//! controls), the conversation (newest at the bottom, scrolling, each day under its name and every
//! message with its time), and the box he types in. Down its side, a column of cards: what the
//! Conductor asks to do (Allow, Deny), what sessions wait on him for (their options as buttons),
//! every session, the hub's events, the RSI report's suggestions and the Conductor's notes.
//!
//! Sizes are at 96 DPI, times `scale`. Words are measured with the GUI's widths for each font, in
//! [`UNIT`]s of a sixteenth of a pixel, which the wrapping in text.rs and input.rs counts in.

use super::input::{self, Input, Visual};
use super::protocol::{Shown, View, Waiting, Who};
use super::text::{self, Kind, Line, Style};
use super::words;
use crate::state;
use crate::tabs::Rect;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// The status colours (lbterm/DESIGN.md, "Colour"), and the Conductor's modes' (read green,
/// most yellow, all red)
pub const GREEN: u32 = 0x3DDC84;
pub const ORANGE: u32 = 0xFF8A3D;
pub const BLUE: u32 = 0x5B7CFF;
pub const RED: u32 = 0xFF5449;
pub const YELLOW: u32 = 0xF2E86D;
/// Another session's words, and the Conductor's own thinking about one
pub const LAVENDER: u32 = 0xB4B4CC;
/// Under a bar's fill
pub const TRACK: u32 = 0x2A2A36;
pub const TITLE: u32 = 0xECECF4;
/// Material's blue on dark: links, the box's focus, a picked option, the Conductor's avatar
pub const PRIMARY: u32 = 0xA8C7FA;
/// Words on a filled button
const ON_FILL: u32 = 0x0D1017;
/// Code's words
const CODE: u32 = 0xE6DCC0;
const WHITE: u32 = 0xFFFFFF;

/// Units a pixel: widths are counted in sixteenths
pub const UNIT: f64 = 16.0;
/// The most lines the box shows before it scrolls
pub const BOX_LINES: usize = 8;
/// A question in text shows at most this many lines on its card
const QUESTION_LINES: usize = 18;
/// The events shown, newest first
const EVENTS: usize = 12;
/// A message from another session shows this many lines until "Show more"
const PEER_LINES: usize = 8;

/// Icons, from the Symbols Nerd Font every font falls back on (nerdfonts_data.rs's names)
const ICON_CHECK: &str = "\u{f012c}"; // md_check
const ICON_CLOSE: &str = "\u{f0156}"; // md_close
const ICON_CANCEL: &str = "\u{f073a}"; // md_cancel
const ICON_LOADING: &str = "\u{f0772}"; // md_loading
const ICON_BELL: &str = "\u{f009c}"; // md_bell_outline
const ICON_ALERT: &str = "\u{f0028}"; // md_alert_circle
const ICON_SEND: &str = "\u{f005d}"; // md_arrow_up
const ICON_STOP: &str = "\u{f04db}"; // md_stop
const ICON_DOWN: &str = "\u{f0045}"; // md_arrow_down
/// The Conductor's own, as on its pad (md_music)
const ICON_CONDUCTOR: &str = "\u{f075a}";
const ICON_FOLD: &str = "\u{f013e}"; // md_chevron_double_right
const ICON_UNFOLD: &str = "\u{f013d}"; // md_chevron_double_left
const ICON_OPEN: &str = "\u{f0140}"; // md_chevron_down
const ICON_SHUT: &str = "\u{f0142}"; // md_chevron_right
const ICON_PREVIOUS: &str = "\u{f0141}"; // md_chevron_left
const ICON_QUESTION: &str = "\u{f0186}"; // md_comment_question_outline
const ICON_WORKING: &str = "\u{f0996}"; // md_progress_clock
const ICON_IDLE: &str = "\u{f04b2}"; // md_sleep
const ICON_IDEA: &str = "\u{f0336}"; // md_lightbulb_outline
const ICON_NOTE: &str = "\u{f11d7}"; // md_note_text_outline
const ICON_GENERATE: &str = "\u{f0674}"; // md_creation

/// `over` laid on `base` at `alpha`, as one solid colour (lbterm_panel's palette::blend)
pub fn blend(base: u32, over: u32, alpha: f64) -> u32 {
    let mix = |shift: u32| {
        let (b, o) = (((base >> shift) & 0xFF) as f64, ((over >> shift) & 0xFF) as f64);
        ((b + (o - b) * alpha.clamp(0.0, 1.0)).round() as u32).min(255) << shift
    };
    mix(16) | mix(8) | mix(0)
}

/// Which font words are drawn in: Material's type roles in Roboto, and code in the panes' font
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Font {
    /// Roboto 14 px: the conversation, the boxes
    Text,
    /// Roboto Medium 14 px: bold words, names, buttons
    Strong,
    /// Roboto 12 px: what's said about something: times, projects, summaries, events
    Small,
    /// Roboto Medium 12 px: section headings, chips, small buttons
    SmallStrong,
    /// Roboto Medium 16 px: the title, a reply's headings
    Title,
    /// The panes' font at 12 px: code, an ask's exact words
    Code,
}

impl Font {
    pub const ALL: [Font; 6] = [Font::Text, Font::Strong, Font::Small, Font::SmallStrong, Font::Title, Font::Code];

    /// Its bold
    fn bold(self) -> Font {
        match self {
            Font::Text => Font::Strong,
            Font::Small => Font::SmallStrong,
            f => f,
        }
    }

    /// The font a run of `style` takes in words of this font
    pub fn of(self, style: Style) -> Font {
        match style {
            Style::Plain => self,
            Style::Bold | Style::Link => self.bold(),
            Style::Heading => Font::Title,
            Style::Code => Font::Code,
        }
    }
}

/// What the GUI gives to size things: the scale over 96 DPI, each character's width in each
/// font, how tall each font's words are as it lays them out, and the tab rail's width, which the
/// side column folds to (all in pixels)
#[derive(Clone, Copy)]
pub struct Metrics<'a> {
    pub scale: f64,
    pub width: &'a dyn Fn(Font, char) -> f64,
    pub height: &'a dyn Fn(Font) -> f64,
    pub rail: f64,
}

/// A character's width in `font`, in units
pub fn units<'m>(m: &'m Metrics<'m>, font: Font) -> impl Fn(char) -> usize + Copy + 'm {
    move |c| ((m.width)(font, c) * UNIT).round().max(0.0) as usize
}

/// A character's width in units in a run of words of `base` in its style
fn styled<'m>(m: &'m Metrics<'m>, base: Font) -> impl Fn(char, Style) -> usize + Copy + 'm {
    move |c, style| ((m.width)(base.of(style), c) * UNIT).round().max(0.0) as usize
}

/// Which of the view's z-layers a thing is drawn in: back to front
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    /// The view's card and the cards on it
    Back,
    /// Buttons, chips, bars, a selection
    Mid,
    Text,
    /// The caret
    Top,
}

/// One thing to draw
#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    /// A rounded rectangle filled with one colour
    Fill { rect: Rect, radius: f64, rgb: u32, layer: Layer },
    /// A hairline round a rounded rectangle
    Outline { rect: Rect, radius: f64, rgb: u32, layer: Layer },
    /// Words from `x`, `y` (the top of their words as the GUI lays them out), at most `w` wide
    Text {
        x: f64,
        y: f64,
        w: f64,
        text: String,
        font: Font,
        rgb: u32,
        layer: Layer,
    },
}

/// A box that's typed in
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BoxKey {
    /// The box at the bottom: his words to the Conductor
    Main,
    /// A waiting session's answer: the box for a question in text (`question` none), or the
    /// words of his own for a picker's question
    Answer { waiting: String, question: Option<usize> },
}

impl Default for BoxKey {
    fn default() -> Self {
        BoxKey::Main
    }
}

impl BoxKey {
    pub fn name(&self) -> String {
        match self {
            BoxKey::Main => "main".to_string(),
            BoxKey::Answer { waiting, question: None } => format!("answer:{waiting}"),
            BoxKey::Answer {
                waiting,
                question: Some(q),
            } => format!("other:{waiting}:{q}"),
        }
    }
}

/// What a place on the view does
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    /// `seat.control`: `on`, `off`, `restart`, `mode read`, `listen off`, ...
    Control(String),
    Allow(String),
    Deny(String),
    /// His words in the box to the Conductor
    Send,
    /// `seat.interrupt`
    Stop,
    /// A picker's option, counting from 0
    Pick { waiting: String, question: usize, option: usize },
    /// Words of his own for a picker's question
    Other { waiting: String, question: usize },
    /// A `1a`-style option of a question in text, into its box
    Chip { waiting: String, code: String },
    /// Send a waiting session its answer
    Answer { waiting: String },
    /// Open that session's tab: a session's card, or its name in the conversation
    Session { session_id: String, pane: Option<String> },
    /// Show to Conductor, on a session in the Hidden group: `lbterm-conductor-hide` with
    /// `hide = false` for it, as the tab menu's Show to Conductor fires (lbterm/NEW-TAB.md,
    /// section 8); `label` is what the view calls it
    Show { session_id: String, label: String },
    /// `send`, `dismiss`, `useful` or `not`
    Rsi { suggestion: String, action: String },
    /// An item of the conversation shown whole, or cut short again; a run of alerts opened or
    /// folded, by its first's id
    More(String),
    /// The side column folded to the rail's width, or opened again
    FoldSide,
    /// A section of the side folded to its heading, or opened: "suggestions", "sessions:idle"
    Section(String),
    /// On the folded side: open it at that section
    Reveal(String),
    /// Reviewing the suggestions one at a time: `start`, `next`, `previous` or `done`
    Review(String),
    /// Back down to the newest
    Newest,
    /// A box: the caret to where it's clicked
    Box(BoxKey),
    /// The conversation: a drag selects
    Conversation,
    /// The side column, for the wheel
    Side,
}

impl Target {
    /// Its name in the state file and for a button waiting on the seat
    pub fn name(&self) -> String {
        match self {
            Target::Control(action) => format!("control:{action}"),
            Target::Allow(id) => format!("allow:{id}"),
            Target::Deny(id) => format!("deny:{id}"),
            Target::Send => "send".to_string(),
            Target::Stop => "stop".to_string(),
            Target::Pick {
                waiting,
                question,
                option,
            } => format!("pick:{waiting}:{question}:{option}"),
            Target::Other { waiting, question } => format!("other:{waiting}:{question}"),
            Target::Chip { waiting, code } => format!("chip:{waiting}:{code}"),
            Target::Answer { waiting } => format!("answer:{waiting}"),
            Target::Session { session_id, .. } => format!("session:{session_id}"),
            Target::Show { session_id, .. } => format!("show:{session_id}"),
            Target::Rsi { suggestion, action } => format!("rsi:{suggestion}:{action}"),
            Target::More(id) => format!("more:{id}"),
            Target::FoldSide => "fold-side".to_string(),
            Target::Section(name) => format!("section:{name}"),
            Target::Reveal(name) => format!("reveal:{name}"),
            Target::Review(step) => format!("review:{step}"),
            Target::Newest => "newest".to_string(),
            Target::Box(key) => format!("box:{}", key.name()),
            Target::Conversation => "conversation".to_string(),
            Target::Side => "side".to_string(),
        }
    }

    /// Whether it's a button: the pointer's hand over it, and lit under it
    pub fn is_button(&self) -> bool {
        !matches!(self, Target::Box(_) | Target::Conversation | Target::Side)
    }
}

/// The session a link opens: `lbterm://session/<id>`
fn session_link(link: &str) -> Option<Target> {
    let id = link.strip_prefix("lbterm://session/")?.trim_end_matches('/');
    (!id.is_empty()).then(|| Target::Session {
        session_id: id.to_string(),
        pane: None,
    })
}

/// A place in the conversation's lines
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Spot {
    pub line: usize,
    pub col: usize,
}

/// The conversation's items wrapped, kept while their words, font and width stay the same
#[derive(Debug, Default)]
pub struct LineCache {
    lines: HashMap<(String, u64, usize), std::rc::Rc<Vec<Line>>>,
}

impl LineCache {
    fn get(&mut self, id: &str, text: &str, how: &str, width: usize, make: impl FnOnce() -> Vec<Line>) -> std::rc::Rc<Vec<Line>> {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hash);
        how.hash(&mut hash);
        let key = (id.to_string(), hash.finish(), width);
        if self.lines.len() > 2000 {
            self.lines.clear();
        }
        self.lines.entry(key).or_insert_with(|| std::rc::Rc::new(make())).clone()
    }
}

/// Plain words wrapped (the side's cards, a question's options, the notes), kept while their
/// words, font and width stay the same: a layout wraps every card down the side, scrolled away
/// or not, and the suggestions alone are thousands of words
#[derive(Debug, Default)]
pub struct WrapCache {
    lines: HashMap<(u64, Font, usize), std::rc::Rc<Vec<Line>>>,
}

impl WrapCache {
    fn get(&mut self, words: &str, font: Font, cols: usize, make: impl FnOnce() -> Vec<Line>) -> std::rc::Rc<Vec<Line>> {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        words.hash(&mut hash);
        if self.lines.len() > 2000 {
            self.lines.clear();
        }
        self.lines.entry((hash.finish(), font, cols)).or_insert_with(|| std::rc::Rc::new(make())).clone()
    }
}

/// What the window keeps of the view between frames: what's typed, what's picked, where it's
/// scrolled, what the pointer is on
#[derive(Debug, Default)]
pub struct Ui {
    /// The box at the bottom
    pub main: Input,
    /// The answer boxes on waiting sessions' cards
    pub boxes: HashMap<BoxKey, Input>,
    /// Where typing goes
    pub focus: BoxKey,
    /// Pickers' questions he chose to answer in words of his own: (waiting, question)
    pub others: HashSet<(String, usize)>,
    /// Pickers' options picked so far, by waiting question, per question, counting from 1
    pub picks: HashMap<String, Vec<Vec<u32>>>,
    /// Buttons waiting on the seat's answer, by name: they say so
    pub pending: HashSet<String>,
    /// How far the conversation is scrolled up from the newest, in pixels
    pub scroll: f64,
    /// How far the side column is scrolled down, in pixels
    pub side_scroll: f64,
    pub hovered: Option<Target>,
    /// A selection in the conversation: where it started and where it reaches
    pub selection: Option<(Spot, Spot)>,
    /// Under the box: what the seat said to a command (true: it refused)
    pub notice: Option<(String, bool)>,
    /// A dead key or IME composition under way, shown at the caret
    pub composing: Option<String>,
    /// Items of the conversation shown whole ("Show more"), by id; a run of alerts opened, by
    /// its first's
    pub expanded: HashSet<String>,
    /// The side column folded to the rail's width
    pub side_folded: bool,
    /// The side's sections turned from how they start, by name: folded to their headings
    /// ("suggestions", "sessions:idle"), or for one that starts folded ([`FOLDED_AT_FIRST`], the
    /// Hidden group), opened; [`Ui::is_folded`] says which way each is now
    pub folded: HashSet<String>,
    /// The suggestion being reviewed, one at a time, while reviewing them
    pub review: Option<String>,
    /// Where each side section's heading was, down the column from its top, when it was last
    /// laid out: for the folded side's buttons to open it there
    pub sections: Vec<(String, f64)>,
    pub cache: LineCache,
    /// The side's words wrapped, lent to the layout while it runs
    pub wraps: WrapCache,
    /// How tall the conversation was last frame, so new words below don't move what he's
    /// reading while he's scrolled up
    pub last_height: f64,
    /// How far the side column could scroll last frame
    pub side_max: f64,
}

/// The side's sections that start folded: the sessions hidden from the Conductor, which he
/// handles himself (lbterm/NEW-TAB.md, section 8)
pub const FOLDED_AT_FIRST: [&str; 1] = ["sessions:hidden"];

impl Ui {
    /// Whether the side's section `key` is folded to its heading: as it starts
    /// ([`FOLDED_AT_FIRST`]) unless a press has turned it ([`Ui::folded`])
    pub fn is_folded(&self, key: &str) -> bool {
        self.folded.contains(key) != FOLDED_AT_FIRST.contains(&key)
    }

    /// The box `key`, made when it's first typed in
    pub fn input(&mut self, key: &BoxKey) -> &mut Input {
        match key {
            BoxKey::Main => &mut self.main,
            key => self.boxes.entry(key.clone()).or_default(),
        }
    }

    pub fn input_of(&self, key: &BoxKey) -> Option<&Input> {
        match key {
            BoxKey::Main => Some(&self.main),
            key => self.boxes.get(key),
        }
    }

    /// A picker's option picked or unpicked: one question's only pick, or one of several
    pub fn pick(&mut self, waiting: &Waiting, question: usize, option: usize) {
        let picks = self
            .picks
            .entry(waiting.id.clone())
            .or_insert_with(|| vec![vec![]; waiting.questions.len()]);
        picks.resize(waiting.questions.len(), vec![]);
        let Some(chosen) = picks.get_mut(question) else {
            return;
        };
        let n = option as u32 + 1;
        let multi = waiting.questions.get(question).map_or(false, |q| q.multi_select);
        if chosen.contains(&n) {
            chosen.retain(|p| *p != n);
        } else if multi {
            chosen.push(n);
            chosen.sort_unstable();
        } else {
            *chosen = vec![n];
        }
    }

    /// The answer as it stands: each question's picks and words of his own, and a question in
    /// text's words; `None` while a question has neither
    pub fn answer(&self, waiting: &Waiting) -> Option<(Vec<Vec<u32>>, Vec<Option<String>>, Option<String>)> {
        if !waiting.questions.is_empty() {
            let picks = self.picks.get(&waiting.id);
            let mut all_picks = vec![];
            let mut others = vec![];
            for q in 0..waiting.questions.len() {
                let p = picks.and_then(|p| p.get(q)).cloned().unwrap_or_default();
                let key = BoxKey::Answer {
                    waiting: waiting.id.clone(),
                    question: Some(q),
                };
                let other = self
                    .others
                    .contains(&(waiting.id.clone(), q))
                    .then(|| self.boxes.get(&key).map(|b| b.text().trim().to_string()))
                    .flatten()
                    .filter(|t| !t.is_empty());
                if p.is_empty() && other.is_none() {
                    return None;
                }
                all_picks.push(p);
                others.push(other);
            }
            return Some((all_picks, others, None));
        }
        let key = BoxKey::Answer {
            waiting: waiting.id.clone(),
            question: None,
        };
        let words = self.boxes.get(&key).map(|b| b.text().trim().to_string()).filter(|t| !t.is_empty())?;
        Some((vec![], vec![], Some(words)))
    }

    /// The fonts changed (the DPI, the config): words wrapped in the old ones go
    pub fn forget_measures(&mut self) {
        self.cache = LineCache::default();
        self.wraps = WrapCache::default();
    }

    /// What's kept for waiting questions that are gone goes too
    pub fn forget_gone(&mut self, view: &View) {
        let here = |id: &str| view.waiting.iter().any(|w| w.id == id);
        self.picks.retain(|id, _| here(id));
        self.others.retain(|(id, _)| here(id));
        self.boxes.retain(|key, _| match key {
            BoxKey::Main => true,
            BoxKey::Answer { waiting, .. } => here(waiting),
        });
        if let BoxKey::Answer { waiting, .. } = &self.focus {
            if !here(waiting) {
                self.focus = BoxKey::Main;
            }
        }
    }
}

/// A line of the conversation where it was laid out, for a selection and its copy
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub text: String,
    /// Where each character starts, and the last one ends: one more than its characters
    pub xs: Vec<f64>,
    /// Copied, it runs on into the next without a line break
    pub wraps: bool,
}

/// A box where it was laid out, for a click's caret and the up and down keys
#[derive(Debug, Clone, PartialEq)]
pub struct BoxPlace {
    pub key: BoxKey,
    /// Where its words go
    pub text: Rect,
    pub lines: Vec<Visual>,
    /// The first line it shows
    pub first: usize,
    /// Pixels a unit of its lines' widths
    pub cw: f64,
    pub lh: f64,
}

/// A place a click lands on
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub rect: Rect,
    pub target: Target,
}

/// The view, laid out
#[derive(Debug, Default)]
pub struct Scene {
    pub prims: Vec<Prim>,
    /// Later ones are over earlier ones
    pub hits: Vec<Hit>,
    /// Every line of the conversation, shown or scrolled away
    pub lines: Vec<TextLine>,
    pub card: Rect,
    pub conversation: Rect,
    pub side: Option<Rect>,
    pub scroll_max: f64,
    pub side_max: f64,
    pub boxes: Vec<BoxPlace>,
    /// The caret of the box typing goes to, for the IME's window
    pub caret: Option<Rect>,
    /// When what it says about time next changes (`changes_at`), epoch milliseconds
    pub clock: Option<u64>,
    /// Laid out again with what this layout kept of itself in the `Ui` (how far it scrolled,
    /// how far the side could), it would come out otherwise: `lay_out_settled` lays it out again
    pub again: bool,
    /// For the state file (the GUI fills in what's the window's)
    pub said: state::ConductorView,
}

impl Scene {
    /// What's at `x`, `y`: the last place laid out there
    pub fn hit(&self, x: f64, y: f64) -> Option<&Target> {
        self.hits.iter().rev().find(|h| h.rect.contains(x, y)).map(|h| &h.target)
    }

    pub fn box_place(&self, key: &BoxKey) -> Option<&BoxPlace> {
        self.boxes.iter().find(|b| b.key == *key)
    }

    /// The place in the conversation's lines nearest `x`, `y`
    pub fn spot(&self, x: f64, y: f64) -> Option<Spot> {
        if self.lines.is_empty() {
            return None;
        }
        let line = self
            .lines
            .iter()
            .position(|l| y < l.y + l.h)
            .unwrap_or(self.lines.len() - 1);
        let xs = &self.lines[line].xs;
        let col = (0..xs.len())
            .min_by(|a, b| (xs[*a] - x).abs().total_cmp(&(xs[*b] - x).abs()))
            .unwrap_or(0);
        Some(Spot { line, col })
    }

    /// The words from `a` to `b` (either way round): a wrapped line runs on into the next, any
    /// other ends with a line break
    pub fn selected(&self, a: Spot, b: Spot) -> String {
        let (from, to) = (a.min(b), a.max(b));
        let mut out = String::new();
        for (i, l) in self.lines.iter().enumerate().take(to.line + 1).skip(from.line) {
            let chars: Vec<char> = l.text.chars().collect();
            let start = if i == from.line { from.col.min(chars.len()) } else { 0 };
            let end = if i == to.line { to.col.min(chars.len()) } else { chars.len() };
            out.extend(&chars[start..end.max(start)]);
            if i != to.line && !l.wraps {
                out.push('\n');
            }
        }
        out
    }
}

/// What the view shows: whether the seat answers, what it said, and the conversation
pub struct Model<'a> {
    pub connected: bool,
    /// The seat's pipe, to say what it's waiting for
    pub pipe: &'a str,
    pub view: Option<&'a View>,
    pub items: &'a [Shown<'a>],
    pub now_ms: u64,
    /// Epoch milliseconds as a time of day, "14:02"
    pub clock: fn(u64) -> String,
    /// The day epoch milliseconds fall on, as he'd say it then (`now_ms` second): "Today",
    /// "Yesterday", "Monday 26 October"
    pub day: fn(u64, u64) -> String,
}

/// How long ago `at` was, shortly: "now", "2m", "3h", "2d"
pub fn ago(now_ms: u64, at: u64) -> String {
    if at == 0 {
        return String::new();
    }
    let s = now_ms.saturating_sub(at) / 1000;
    match s {
        0..=44 => "now".to_string(),
        45..=3599 => format!("{}m", (s + 30) / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

/// How long ago `at` was, in words: "just now", "6 min ago", "3 h ago", "2 d ago"
pub fn ago_words(now_ms: u64, at: u64) -> String {
    match ago(now_ms, at).as_str() {
        "" => String::new(),
        "now" => "just now".to_string(),
        short => {
            let (n, unit) = short.split_at(short.len() - 1);
            let unit = match unit {
                "m" => "min",
                "h" => "h",
                _ => "d",
            };
            format!("{n} {unit} ago")
        }
    }
}

/// When what `ago(now_ms, at)` says next changes, in epoch milliseconds: "now" turns to "1m" at
/// 45 s, each minute after it half a minute either side of the minute, the hours on the hour, the
/// days on the day. None without a time
pub fn ago_changes(now_ms: u64, at: u64) -> Option<u64> {
    if at == 0 {
        return None;
    }
    let s = now_ms.saturating_sub(at) / 1000;
    let next = match s {
        0..=44 => 45,
        45..=3599 => (((s + 30) / 60 + 1) * 60 - 30).min(3600),
        3600..=86_399 => (s / 3600 + 1) * 3600,
        _ => (s / 86_400 + 1) * 86_400,
    };
    Some(at + next * 1000)
}

/// When what the view says about time next changes, in epoch milliseconds: an ask's or a waiting
/// question's age (`ago_words`) turning from "just now" to "1 min ago", and on. The GUI lays the
/// view out again then, and not before (render/conductor_view.rs), so every age the view shows
/// goes in here; the conversation's times of day never change, and its days' names change at
/// midnight, which the GUI adds (it knows the time zone). `clock_is_when_its_words_change` holds
/// the layout to it.
pub fn changes_at(model: &Model) -> Option<u64> {
    let view = model.view?;
    view.asks
        .iter()
        .map(|a| a.at)
        .chain(view.waiting.iter().filter(|w| !w.extras).map(|w| w.at))
        .filter_map(|at| ago_changes(model.now_ms, at))
        .min()
}

/// "~4 min" for seconds left, as the rail says it
pub fn left(seconds: u32) -> String {
    match seconds {
        0..=89 => "~1 min".to_string(),
        90..=5399 => format!("~{} min", (seconds + 30) / 60),
        _ => format!("~{} h", (seconds + 1800) / 3600),
    }
}

/// What a session is called to him: its label, or "new session" before its first step, never
/// its messaging name (Conductor's SEAT.md, "Names")
pub fn called(label: Option<&str>) -> &str {
    label.map(str::trim).filter(|l| !l.is_empty()).unwrap_or("new session")
}

/// A session as he knows it, its project after it: "Publishing the résumé · Website"
fn called_in(label: Option<&str>, project: &str) -> String {
    let name = called(label);
    if project.is_empty() {
        name.to_string()
    } else {
        format!("{name} · {project}")
    }
}

/// Who a message from another session is from, as he knows it: "From Publishing the résumé ·
/// Website", "From a new session · Website" before its first step, "From another session" when
/// the seat can't tell which sent it. Never its messaging name (SEAT.md, `peer`).
fn peer_label(label: Option<&str>, project: Option<&str>) -> String {
    let project = project.map(str::trim).filter(|p| !p.is_empty());
    match (label.map(str::trim).filter(|l| !l.is_empty()), project) {
        (Some(_), _) => format!("From {}", called_in(label, project.unwrap_or(""))),
        (None, Some(p)) => format!("From a new session · {p}"),
        (None, None) => "From another session".to_string(),
    }
}

/// The colour a session's status shows in
pub fn status_colour(status: &str) -> u32 {
    match status {
        "working" => GREEN,
        "waiting" | "blocked" => ORANGE,
        "idle" | "done" => BLUE,
        "failed" => RED,
        _ => 0x8A8A9E,
    }
}

fn event_colour(kind: &str) -> u32 {
    match kind {
        "waiting" | "asking" | "needsYou" => ORANGE,
        "outward" => RED,
        "finished" => GREEN,
        "started" | "ended" => 0x8A8A9E,
        _ => LAVENDER,
    }
}

/// A word with its first letter capital: "working" is "Working"
fn capital(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// `words` with each session's messaging name (a tool call's "read: website-a6") as its label,
/// the name he knows it by (Conductor's SEAT.md, "Names")
fn as_he_knows_them(view: Option<&View>, words: &str) -> String {
    let mut out = words.to_string();
    for s in view.map_or(&[][..], |v| &v.sessions[..]) {
        let label = s.label.trim();
        if !s.name.is_empty() && !label.is_empty() && s.name != label && out.contains(&s.name) {
            out = out.replace(&s.name, label);
        }
    }
    out
}

/// The session a name and project are, among the view's: to open its tab
fn session_named(view: Option<&View>, label: &str, project: &str) -> Option<Target> {
    let s = view?.sessions.iter().find(|s| s.label.trim() == label.trim() && (project.is_empty() || s.project == project))?;
    Some(Target::Session {
        session_id: s.session_id.clone(),
        pane: s.pane.clone(),
    })
}

/// How a button looks (Material's buttons)
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tone {
    /// Filled in its colour, dark words: the one thing to press (Allow, Send answer, Turn on)
    Filled(u32),
    /// Its colour faint behind its words in it (a picked mode, Send on a suggestion)
    Tonal(u32),
    /// A hairline round it, its words in its colour (Deny, a mode not picked)
    Outlined(u32),
    /// Words only, a faint fill under the pointer (Restart, Dismiss, Other…)
    Plain(u32),
    /// A filter chip: a hairline, filled faintly in its colour with a check while it's on
    Chip(u32),
}

struct Builder<'a> {
    m: &'a Metrics<'a>,
    surface: u32,
    prims: Vec<Prim>,
    hits: Vec<Hit>,
    buttons: Vec<state::ViewButton>,
    /// What's drawn now is cut to this: a scrolled column's room
    clip: Rect,
    /// The `Ui`'s, while the layout runs
    wraps: WrapCache,
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    (x1 > x0 && y1 > y0).then(|| Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

fn said_rect(r: Rect) -> state::Rect {
    state::Rect::round(r.x, r.y, r.w, r.h)
}

impl<'a> Builder<'a> {
    fn px(&self, v: f64) -> f64 {
        (v * self.m.scale).round()
    }

    /// A line of `font`'s words, as tall as Material's type roles set them
    fn line(&self, font: Font) -> f64 {
        self.px(match font {
            Font::Text | Font::Strong => 22.0,
            Font::Small | Font::SmallStrong => 18.0,
            Font::Title => 24.0,
            Font::Code => 20.0,
        })
    }

    /// How wide `s` is in `font`
    fn w(&self, s: &str, font: Font) -> f64 {
        let u = units(self.m, font);
        s.chars().map(u).sum::<usize>() as f64 / UNIT
    }

    /// `s` cut to `width` pixels in `font`, an ellipsis where it was cut
    fn fit(&self, s: &str, width: f64, font: Font) -> String {
        text::fit(s, (width.max(0.0) * UNIT) as usize, units(self.m, font))
    }

    /// White at `alpha` over the view, as the eye sees it
    fn white(&self, alpha: f64) -> u32 {
        blend(self.surface, WHITE, alpha)
    }

    fn fill(&mut self, r: Rect, radius: f64, rgb: u32, layer: Layer) {
        if let Some(rect) = intersect(r, self.clip) {
            self.prims.push(Prim::Fill { rect, radius, rgb, layer });
        }
    }

    /// A hairline round `r`: a pill's a hair short of round, as the GUI's stroke draws nothing
    /// with a radius of half its height
    fn outline(&mut self, r: Rect, radius: f64, rgb: u32, layer: Layer) {
        let radius = radius.min(r.h / 2.0 - 1.0).min(r.w / 2.0 - 1.0).max(0.0);
        if intersect(r, self.clip) == Some(r) {
            self.prims.push(Prim::Outline { rect: r, radius, rgb, layer });
        }
    }

    /// A circle `d` across from `x`, `y`
    fn dot(&mut self, x: f64, y: f64, d: f64, rgb: u32, layer: Layer) {
        self.fill(Rect { x, y, w: d, h: d }, d / 2.0, rgb, layer);
    }

    /// Words on a line of `font` whose top is `y`, drawn only where the whole line is in the room
    fn text(&mut self, x: f64, y: f64, text: &str, font: Font, rgb: u32, layer: Layer) -> bool {
        let lh = self.line(font);
        if text.is_empty() || y < self.clip.y - 0.5 || y + lh > self.clip.y + self.clip.h + 0.5 {
            return false;
        }
        let ty = y + ((lh - (self.m.height)(font)) / 2.0).round();
        let w = self.w(text, font) + self.px(4.0);
        self.prims.push(Prim::Text {
            x: x.round(),
            y: ty,
            w,
            text: text.to_string(),
            font,
            rgb,
            layer,
        });
        true
    }

    fn hit(&mut self, r: Rect, target: Target) {
        if let Some(rect) = intersect(r, self.clip) {
            self.hits.push(Hit { rect, target });
        }
    }

    /// A button's height for words in `font`
    fn pill_h(&self, font: Font) -> f64 {
        self.px(if font == Font::Strong { 32.0 } else { 28.0 })
    }

    /// A button's width for its words
    fn pill_w(&self, label: &str, font: Font) -> f64 {
        (self.w(label, font) + self.px(if font == Font::Strong { 32.0 } else { 24.0 })).round()
    }

    /// A button: its fill in its tone, a state layer under the pointer, its words in the middle,
    /// "…" after them while it waits on the seat; where it is goes in the state file
    #[allow(clippy::too_many_arguments)]
    fn button(&mut self, r: Rect, label: &str, tone: Tone, target: Target, ui: &Ui, under: u32, enabled: bool, on: bool, font: Font) {
        let name = target.name();
        let pending = ui.pending.contains(&name);
        let hovered = enabled && !pending && ui.hovered.as_ref() == Some(&target);
        let lift = if hovered { 0.08 } else { 0.0 };
        let shown = match tone {
            Tone::Chip(_) if on => format!("{ICON_CHECK} {label}"),
            _ => label.to_string(),
        };
        let shown = if pending { format!("{shown}…") } else { shown };
        let words = if !enabled {
            self.fill(r, r.h / 2.0, blend(under, WHITE, 0.06), Layer::Mid);
            blend(under, WHITE, 0.34)
        } else {
            match tone {
                Tone::Filled(c) => {
                    self.fill(r, r.h / 2.0, blend(c, WHITE, lift * 1.5), Layer::Mid);
                    ON_FILL
                }
                Tone::Tonal(c) => {
                    self.fill(r, r.h / 2.0, blend(under, c, 0.20 + lift), Layer::Mid);
                    c
                }
                Tone::Outlined(c) => {
                    if hovered {
                        self.fill(r, r.h / 2.0, blend(under, c, 0.10), Layer::Mid);
                    }
                    self.outline(r, r.h / 2.0, blend(under, c, 0.6), Layer::Mid);
                    c
                }
                Tone::Plain(c) => {
                    if hovered {
                        self.fill(r, r.h / 2.0, blend(under, c, 0.10), Layer::Mid);
                    }
                    c
                }
                Tone::Chip(c) if on => {
                    self.fill(r, self.px(8.0), blend(under, c, 0.20 + lift), Layer::Mid);
                    c
                }
                Tone::Chip(_) => {
                    if hovered {
                        self.fill(r, self.px(8.0), blend(under, WHITE, 0.06), Layer::Mid);
                    }
                    self.outline(r, self.px(8.0), blend(under, WHITE, 0.24), Layer::Mid);
                    blend(under, WHITE, 0.78)
                }
            }
        };
        let words = if pending { blend(under, words, 0.6) } else { words };
        let tx = r.x + ((r.w - self.w(&shown, font)) / 2.0).max(0.0).round();
        let ty = r.y + ((r.h - self.line(font)) / 2.0).round();
        self.text(tx, ty, &shown, font, words, Layer::Text);
        if enabled && !pending {
            self.hit(r, target);
        }
        self.buttons.push(state::ViewButton {
            name,
            label: label.to_string(),
            rect: intersect(r, self.clip).map(said_rect),
            on,
            enabled,
            pending,
        });
    }

    /// Buttons from `x` on `y`, going on to a new row from `left` where one doesn't fit before
    /// `right`; where the next would go, and the last row's top
    #[allow(clippy::too_many_arguments)]
    fn pills(
        &mut self,
        left: f64,
        x: f64,
        y: f64,
        right: f64,
        pills: &[(String, Tone, Target, bool, bool)],
        ui: &Ui,
        under: u32,
        font: Font,
    ) -> (f64, f64) {
        let (mut x, mut y) = (x, y);
        let gap = self.px(8.0);
        let h = self.pill_h(font);
        for (label, tone, target, enabled, on) in pills {
            let shown = match tone {
                Tone::Chip(_) if *on => format!("{ICON_CHECK} {label}"),
                _ => label.clone(),
            };
            let w = self.pill_w(&shown, font);
            if x + w > right && x > left {
                x = left;
                y += h + gap;
            }
            self.button(Rect { x, y, w, h }, label, *tone, target.clone(), ui, under, *enabled, *on, font);
            x += w + gap;
        }
        (x, y)
    }

    /// Words wrapped to `width` pixels from `x`, from `y`, in `font`: the y after them. At most
    /// `max` lines, the last cut with an ellipsis when there were more.
    #[allow(clippy::too_many_arguments)]
    fn wrapped(&mut self, x: f64, y: f64, width: f64, words: &str, font: Font, rgb: u32, max: usize) -> f64 {
        let u = units(self.m, font);
        let cols = ((width * UNIT).floor() as usize).max(4);
        let lines = self.wraps.get(words, font, cols, || text::plain(words, cols, u));
        let lh = self.line(font);
        let mut y = y;
        let many = lines.len() > max;
        for (i, line) in lines.iter().take(max).enumerate() {
            if line.kind == Kind::Blank {
                y += (lh * 0.5).round();
                continue;
            }
            let shown = if many && i + 1 == max {
                text::fit(&format!("{} …", line.text), cols, u)
            } else {
                line.text.clone()
            };
            self.text(x, y, &shown, font, rgb, Layer::Text);
            y += lh;
        }
        y
    }

    /// A small chip of words in `rgb` on a faint fill of it: the header's state, a count
    fn chip(&mut self, x: f64, y: f64, words: &str, rgb: u32, dot: bool) -> f64 {
        let h = self.px(22.0);
        let dot_w = if dot { self.px(14.0) } else { 0.0 };
        let w = (self.w(words, Font::SmallStrong) + self.px(16.0) + dot_w).round();
        self.fill(Rect { x, y, w, h }, h / 2.0, blend(self.surface, rgb, 0.16), Layer::Mid);
        if dot {
            let d = self.px(6.0);
            self.dot(x + self.px(9.0), y + ((h - d) / 2.0).round(), d, rgb, Layer::Mid);
        }
        let ty = y + ((h - self.line(Font::SmallStrong)) / 2.0).round();
        self.text(x + self.px(8.0) + dot_w, ty, words, Font::SmallStrong, rgb, Layer::Text);
        w
    }
}

/// A box being typed in, from `x`, `y`, `w` wide: its fill (a hairline in Material's blue with
/// the typing going to it), its words, a selection, the caret, and what's being composed at it;
/// at most `max` lines, scrolled to the caret. Its height. `round` makes it a pill while it's
/// one line, the box at the bottom.
#[allow(clippy::too_many_arguments)]
fn text_box(
    b: &mut Builder,
    ui: &Ui,
    key: &BoxKey,
    x: f64,
    y: f64,
    w: f64,
    right_room: f64,
    max: usize,
    placeholder: &str,
    under: u32,
    round: bool,
    boxes: &mut Vec<BoxPlace>,
    caret: &mut Option<Rect>,
    said: &mut Vec<state::ViewBox>,
) -> f64 {
    let m = b.m;
    let u = units(m, Font::Text);
    let lh = b.line(Font::Text);
    let pad_x = b.px(if round { 20.0 } else { 12.0 });
    let pad_y = b.px(if round { 13.0 } else { 9.0 });
    let text_w = (w - 2.0 * pad_x - right_room).max(b.px(40.0));
    let cols = ((text_w * UNIT).floor() as usize).max(4);
    let empty = Input::default();
    let input = ui.input_of(key).unwrap_or(&empty);
    let lines = input::wrap(input.text(), cols, u);
    let (caret_line, caret_col) = input::place_of(input.text(), &lines, input.caret(), u);
    let shown = lines.len().clamp(1, max);
    let first = if caret_line >= shown { caret_line + 1 - shown } else { 0 };
    let h = shown as f64 * lh + 2.0 * pad_y;
    let r = Rect { x, y, w, h };
    let focused = ui.focus == *key;
    let radius = if round { (b.px(48.0) / 2.0).min(h / 2.0).max(b.px(16.0)) } else { b.px(12.0) };
    b.fill(r, radius, blend(under, WHITE, if round { 0.07 } else { 0.05 }), Layer::Mid);
    let edge = if focused { blend(under, PRIMARY, 0.55) } else { blend(under, WHITE, if round { 0.0 } else { 0.12 }) };
    if focused || !round {
        b.outline(r, radius, edge, Layer::Mid);
    }
    let tx = x + pad_x;
    let ty = y + pad_y;
    b.hit(r, Target::Box(key.clone()));
    if input.is_empty() {
        let hint = b.fit(placeholder, text_w, Font::Text);
        b.text(tx, ty, &hint, Font::Text, blend(under, WHITE, 0.42), Layer::Text);
    }
    let at = |s: &str| s.chars().map(u).sum::<usize>() as f64 / UNIT;
    let selection = input.selection();
    for (i, line) in lines.iter().enumerate().skip(first).take(shown) {
        let ly = ty + (i - first) as f64 * lh;
        let words = &input.text()[line.start..line.end];
        if let Some((s0, s1)) = selection {
            let a = s0.clamp(line.start, line.end);
            let z = s1.clamp(line.start, line.end);
            if z > a || (s1 > line.end && a < line.end) {
                let x0 = at(&input.text()[line.start..a]);
                let x1 = at(&input.text()[line.start..z]) + if s1 > line.end && !line.soft { b.px(4.0) } else { 0.0 };
                b.fill(
                    Rect {
                        x: tx + x0,
                        y: ly,
                        w: (x1 - x0).max(1.0),
                        h: lh,
                    },
                    0.0,
                    blend(under, PRIMARY, 0.30),
                    Layer::Mid,
                );
            }
        }
        b.text(tx, ly, words, Font::Text, blend(under, WHITE, 0.92), Layer::Text);
    }
    if focused {
        let cx = tx + caret_col as f64 / UNIT;
        let cy = ty + (caret_line.saturating_sub(first)) as f64 * lh;
        if let Some(composing) = ui.composing.as_deref().filter(|c| !c.is_empty()) {
            b.text(cx, cy, composing, Font::Text, YELLOW, Layer::Top);
            let cw = b.w(composing, Font::Text);
            b.fill(
                Rect {
                    x: cx,
                    y: cy + lh - b.px(3.0),
                    w: cw,
                    h: b.px(1.0).max(1.0),
                },
                0.0,
                YELLOW,
                Layer::Top,
            );
        }
        let rect = Rect {
            x: cx,
            y: cy + b.px(3.0),
            w: b.px(2.0).max(1.0),
            h: lh - b.px(6.0),
        };
        b.fill(rect, 0.0, PRIMARY, Layer::Top);
        *caret = Some(rect);
    }
    boxes.push(BoxPlace {
        key: key.clone(),
        text: Rect {
            x: tx,
            y: ty,
            w: cols as f64 / UNIT,
            h: shown as f64 * lh,
        },
        lines,
        first,
        cw: 1.0 / UNIT,
        lh,
    });
    said.push(state::ViewBox {
        name: key.name(),
        rect: intersect(r, b.clip).map(said_rect),
        text: input.text().to_string(),
        caret: input.text()[..input.caret()].chars().count(),
        focused,
    });
    h
}

/// Something drawn in a block of the conversation, from the block's top left
enum Draw {
    Fill(Rect, f64, u32),
    Words(f64, f64, String, Font, u32),
    Hit(Rect, Target),
    Button(Rect, String, Tone, Target, u32, Font),
}

/// A line of words placed in a block: from its top left, on a line `h` tall
struct Placed {
    line: Line,
    x: f64,
    y: f64,
    h: f64,
    base: Font,
    rgb: u32,
}

/// A block of the conversation, laid out but not placed
struct Block<'s> {
    item: Option<&'s Shown<'s>>,
    height: f64,
    /// The space above it
    gap: f64,
    draws: Vec<Draw>,
    lines: Vec<Placed>,
}

impl<'s> Block<'s> {
    fn new(item: Option<&'s Shown<'s>>, gap: f64) -> Self {
        Self {
            item,
            height: 0.0,
            gap,
            draws: vec![],
            lines: vec![],
        }
    }
}

/// Where a block's lines go from `x`, `y` in `base`: their heights (a heading's taller, a blank
/// half a line), code blocks on a fill of their own. The y after them.
fn place(b: &Builder, block: &mut Block, lines: &[Line], x: f64, y: f64, width: f64, base: Font, rgb: u32) -> f64 {
    let lh = b.line(base);
    let mut y = y;
    let mut code_from: Option<f64> = None;
    for (i, line) in lines.iter().enumerate() {
        let h = match line.kind {
            Kind::Blank => (lh * 0.5).round(),
            Kind::Code => b.line(Font::Code),
            Kind::Text if line.runs.iter().any(|r| r.style == Style::Heading) => b.line(Font::Title) + b.px(4.0),
            Kind::Text => lh,
        };
        if line.kind == Kind::Code && code_from.is_none() {
            code_from = Some(y);
            y += b.px(8.0);
        }
        block.lines.push(Placed {
            line: line.clone(),
            x: if line.kind == Kind::Code { x + b.px(12.0) } else { x },
            y,
            h,
            base,
            rgb,
        });
        y += h;
        let code_ends = line.kind == Kind::Code && lines.get(i + 1).map_or(true, |n| n.kind != Kind::Code);
        if code_ends {
            if let Some(from) = code_from.take() {
                y += b.px(8.0);
                block.draws.push(Draw::Fill(
                    Rect {
                        x,
                        y: from,
                        w: width,
                        h: y - from,
                    },
                    b.px(12.0),
                    b.white(0.05),
                ));
            }
        }
    }
    y
}

/// An avatar in the gutter: a circle 24 across, its glyph or letter in the middle, level with a
/// line of `font` whose top is `y`
fn avatar(b: &Builder, block: &mut Block, y: f64, line: Font, glyph: &str, glyph_font: Font, fill: u32, rgb: u32) {
    let d = b.px(24.0);
    let top = y + ((b.line(line) - d) / 2.0).round();
    block.draws.push(Draw::Fill(Rect { x: 0.0, y: top, w: d, h: d }, d / 2.0, fill));
    let gx = ((d - b.w(glyph, glyph_font)) / 2.0).round();
    let gy = top + ((d - b.line(glyph_font)) / 2.0).round();
    block.draws.push(Draw::Words(gx, gy, glyph.to_string(), glyph_font, rgb));
}

/// The whole view, laid out in `area` (the panes' room, its gutter taken off) for this frame
pub fn lay_out(area: Rect, m: &Metrics, surface: u32, model: &Model, ui: &mut Ui) -> Scene {
    let mut b = Builder {
        m,
        surface,
        prims: vec![],
        hits: vec![],
        buttons: vec![],
        clip: area,
        wraps: std::mem::take(&mut ui.wraps),
    };
    let s = m.scale;
    let pad = b.px(20.0);
    let gap = b.px(20.0);

    // The card itself, the panes' colour, with the active pane's outline
    b.fill(area, b.px(12.0), surface, Layer::Back);
    b.outline(area, b.px(12.0), b.white(0.10), Layer::Back);

    let inner = Rect {
        x: area.x + pad,
        y: area.y + pad,
        w: (area.w - 2.0 * pad).max(0.0),
        h: (area.h - 2.0 * pad).max(0.0),
    };
    // The side column, or folded to a rail as wide as the tab rail: by his press, or always
    // when there isn't room for both columns
    let narrow = inner.w < 600.0 * s;
    let folded = narrow || ui.side_folded;
    let (side_w, gap) = if folded {
        (m.rail.max(b.px(64.0)).round(), b.px(12.0))
    } else {
        ((inner.w * 0.36).clamp(280.0 * s, 440.0 * s).round(), gap)
    };
    let main = Rect {
        w: (inner.w - side_w - gap).max(0.0),
        ..inner
    };
    let view = model.view;
    let conductor = view.map(|v| v.conductor.clone()).unwrap_or_default();
    let connected = model.connected;
    let mut said = state::ConductorView {
        connected,
        state: if connected { conductor.state.clone() } else { "unknown".to_string() },
        mode: conductor.mode.clone(),
        listening: conductor.listening,
        busy: conductor.busy,
        queued: conductor.queued,
        error: conductor.error.clone(),
        rect: said_rect(area),
        ..Default::default()
    };

    // The header: the title and its state in chips, then its modes and controls
    let mut y = inner.y;
    {
        let right = main.x + main.w;
        let title_h = b.px(28.0);
        let ty = y + ((title_h - b.line(Font::Title)) / 2.0).round();
        b.text(main.x, ty, "Conductor", Font::Title, TITLE, Layer::Text);
        let mut x = main.x + b.w("Conductor", Font::Title) + b.px(12.0);
        let chip_y = y + ((title_h - b.px(22.0)) / 2.0).round();
        let mut chips: Vec<(String, u32, bool)> = vec![];
        if !connected {
            chips.push(("Seat isn't running".to_string(), ORANGE, true));
        } else {
            let (word, colour) = match conductor.state.as_str() {
                "on" => ("On", GREEN),
                "starting" => ("Starting", YELLOW),
                "stopping" => ("Stopping", YELLOW),
                "error" => ("Error", RED),
                _ => ("Off", b.white(0.6)),
            };
            chips.push((word.to_string(), colour, true));
            if matches!(conductor.state.as_str(), "on" | "starting") {
                if !conductor.listening {
                    let since = conductor.paused_at.map(|t| format!(" since {}", (model.clock)(t))).unwrap_or_default();
                    chips.push((format!("Listening paused{since}"), ORANGE, false));
                } else if conductor.catch_up {
                    chips.push(("Catching up".to_string(), b.white(0.7), false));
                }
                if conductor.busy {
                    chips.push(("Working".to_string(), GREEN, false));
                }
                if conductor.queued > 0 {
                    chips.push((format!("{} queued", conductor.queued), YELLOW, false));
                }
            }
        }
        let model_name = conductor.model.as_deref().filter(|_| connected).map(|m| m.trim_start_matches("claude-").to_string());
        let model_w = model_name.as_deref().map_or(0.0, |n| b.w(n, Font::Small) + b.px(12.0));
        for (words, colour, dot) in &chips {
            let need = b.w(words, Font::SmallStrong) + b.px(16.0) + if *dot { b.px(14.0) } else { 0.0 };
            if x + need > right - model_w {
                break;
            }
            x += b.chip(x, chip_y, words, *colour, *dot) + b.px(6.0);
        }
        if let Some(name) = model_name.as_deref() {
            let nx = right - b.w(name, Font::Small);
            if nx > x {
                b.text(nx, y + ((title_h - b.line(Font::Small)) / 2.0).round(), name, Font::Small, b.white(0.42), Layer::Text);
            }
        }
        y += title_h + b.px(12.0);

        if connected {
            let on = conductor.state == "on";
            let mut pills: Vec<(String, Tone, Target, bool, bool)> = vec![];
            match conductor.state.as_str() {
                "on" => {
                    for (mode, label, colour) in [("read", "Read only", GREEN), ("most", "Accept most", YELLOW), ("all", "Accept all", RED)] {
                        let here = conductor.mode == mode;
                        pills.push((label.to_string(), Tone::Chip(colour), Target::Control(format!("mode {mode}")), true, here));
                    }
                }
                "starting" | "stopping" => {}
                _ => pills.push(("Turn on".to_string(), Tone::Filled(GREEN), Target::Control("on".to_string()), true, false)),
            }
            let (x, row_y) = b.pills(main.x, main.x, y, right, &pills, ui, surface, Font::SmallStrong);
            let mut more: Vec<(String, Tone, Target, bool, bool)> = vec![];
            if on {
                if conductor.listening {
                    more.push(("Stop listening".to_string(), Tone::Plain(PRIMARY), Target::Control("listen off".to_string()), true, false));
                } else {
                    more.push(("Listen, catch up".to_string(), Tone::Plain(PRIMARY), Target::Control("listen catch up".to_string()), true, false));
                    more.push(("Listen, from now".to_string(), Tone::Plain(PRIMARY), Target::Control("listen fresh".to_string()), true, false));
                }
                more.push(("Restart".to_string(), Tone::Plain(PRIMARY), Target::Control("restart".to_string()), true, false));
            }
            if matches!(conductor.state.as_str(), "on" | "starting" | "error") {
                more.push(("Turn off".to_string(), Tone::Plain(RED), Target::Control("off".to_string()), true, false));
            }
            // The controls sit at the row's right end when they fit there, else wrap under
            let more_w: f64 = more.iter().map(|(l, ..)| b.pill_w(l, Font::SmallStrong)).sum::<f64>() + b.px(8.0) * more.len().saturating_sub(1) as f64;
            let start = if pills.is_empty() {
                main.x
            } else if x + b.px(8.0) + more_w <= right {
                right - more_w
            } else {
                x + b.px(8.0)
            };
            let (_, row_y) = b.pills(main.x, start, row_y, right, &more, ui, surface, Font::SmallStrong);
            y = row_y + b.pill_h(Font::SmallStrong) + b.px(10.0);
        }
        if let Some(error) = conductor.error.as_deref().filter(|_| connected) {
            y = b.wrapped(main.x, y, main.w, error, Font::Small, RED, 3) + b.px(6.0);
        }
        y += b.px(2.0);
        b.fill(
            Rect {
                x: main.x,
                y,
                w: main.w,
                h: s.round().max(1.0),
            },
            0.0,
            b.white(0.08),
            Layer::Back,
        );
        y += b.px(1.0);
    }
    let header_bottom = y;

    // The box at the bottom, and under it what the seat last said or how to use it
    let mut boxes = vec![];
    let mut caret = None;
    let mut said_boxes = vec![];
    let hint_y = inner.y + inner.h - b.line(Font::Small);
    let (hint, hint_colour) = match &ui.notice {
        Some((words, true)) => (words.clone(), ORANGE),
        Some((words, false)) => (words.clone(), b.white(0.65)),
        None => (
            "Enter sends · Shift+Enter for a new line · Esc goes back to the tab".to_string(),
            b.white(0.36),
        ),
    };
    let hint = b.fit(&hint, main.w, Font::Small);
    let hint_x = main.x + ((main.w - b.w(&hint, Font::Small)) / 2.0).max(0.0).round();
    b.text(hint_x, hint_y, &hint, Font::Small, hint_colour, Layer::Text);
    said.notice = ui.notice.as_ref().map(|(w, _)| w.clone());

    let busy = connected && conductor.busy;
    let round_d = b.px(36.0);
    let buttons_w = round_d + if busy { round_d + b.px(8.0) } else { 0.0 };
    let placeholder = if !connected {
        "The seat isn't running"
    } else if conductor.takes_words() {
        "Message the Conductor"
    } else {
        "The Conductor is off: turn it on to talk to it"
    };
    let right_room = buttons_w + b.px(12.0);
    // Its height first, then where it goes: its bottom over the hint
    let box_h = {
        let text_w = (main.w - 2.0 * b.px(20.0) - right_room).max(b.px(40.0));
        let cols = ((text_w * UNIT).floor() as usize).max(4);
        let n = input::wrap(ui.main.text(), cols, units(m, Font::Text)).len().clamp(1, BOX_LINES);
        n as f64 * b.line(Font::Text) + 2.0 * b.px(13.0)
    };
    let box_y = hint_y - b.px(8.0) - box_h;
    let box_h = text_box(
        &mut b,
        ui,
        &BoxKey::Main,
        main.x,
        box_y,
        main.w,
        right_room,
        BOX_LINES,
        placeholder,
        surface,
        true,
        &mut boxes,
        &mut caret,
        &mut said_boxes,
    );
    {
        // Send and Stop: round icon buttons in the box's right end, at its last line
        let by = box_y + box_h - b.px(6.0) - round_d;
        let mut bx = main.x + main.w - b.px(6.0) - round_d;
        let can_send = connected && conductor.takes_words() && !ui.main.text().trim().is_empty();
        let under = blend(surface, WHITE, 0.07);
        icon_button(&mut b, Rect { x: bx, y: by, w: round_d, h: round_d }, ICON_SEND, "Send", PRIMARY, true, Target::Send, ui, under, can_send);
        if busy {
            bx -= round_d + b.px(8.0);
            icon_button(&mut b, Rect { x: bx, y: by, w: round_d, h: round_d }, ICON_STOP, "Stop", RED, false, Target::Stop, ui, under, true);
        }
    }

    // The conversation, between the header and the box
    // Scrolled up, "↓ Newest" has a band of its own at the conversation's foot, so no words
    // run under it
    let newest_h = b.pill_h(Font::SmallStrong) + b.px(4.0);
    let scrolled_up = ui.scroll > 1.0;
    let band = if scrolled_up { newest_h + b.px(8.0) } else { 0.0 };
    let conv = Rect {
        x: main.x,
        y: header_bottom,
        w: main.w,
        h: (box_y - b.px(12.0) - header_bottom - band).max(0.0),
    };
    let blocks = conversation_blocks(&b, model, ui, conv.w);
    let lead = b.px(16.0);
    let content_h: f64 = lead + blocks.iter().enumerate().map(|(i, bl)| bl.height + if i == 0 { 0.0 } else { bl.gap }).sum::<f64>() + b.px(8.0);
    // Words arriving below while he's scrolled up don't move what he's reading
    if ui.scroll > 0.0 && content_h > ui.last_height {
        ui.scroll += content_h - ui.last_height;
    }
    ui.last_height = content_h;
    let scroll_max = (content_h - conv.h).max(0.0);
    ui.scroll = ui.scroll.clamp(0.0, scroll_max);
    // The band went by how far it was scrolled before that: clamped past it, it goes next time
    let mut again = (ui.scroll > 1.0) != scrolled_up;
    let top = if content_h <= conv.h { conv.y } else { conv.y + conv.h - content_h + ui.scroll };

    b.clip = conv;
    b.hit(conv, Target::Conversation);
    let mut lines_out: Vec<TextLine> = vec![];
    let selection = ui.selection.map(|(a, z)| (a.min(z), a.max(z)));
    let mut y = top + lead;
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            y += block.gap;
        }
        let bx = conv.x;
        for draw in &block.draws {
            match draw {
                Draw::Fill(r, radius, rgb) => b.fill(Rect { x: bx + r.x, y: y + r.y, ..*r }, *radius, *rgb, Layer::Back),
                Draw::Words(x, wy, words, font, rgb) => {
                    b.text(bx + x, y + wy, words, *font, *rgb, Layer::Text);
                }
                Draw::Hit(..) | Draw::Button(..) => {}
            }
        }
        for placed in &block.lines {
            let (x, line_y) = (bx + placed.x, y + placed.y);
            let line = &placed.line;
            let width = styled(m, placed.base);
            let xs = {
                let mut xs = vec![];
                let mut end = x;
                for run in &line.runs {
                    let mut cx = x + run.col as f64 / UNIT;
                    for c in run.text.chars() {
                        xs.push(cx);
                        cx += width(c, run.style) as f64 / UNIT;
                    }
                    end = cx;
                }
                xs.push(end);
                xs
            };
            let n = lines_out.len();
            // The selection behind its words
            if let Some((a, z)) = selection {
                if n >= a.line && n <= z.line {
                    let total = xs.len() - 1;
                    let c0 = if n == a.line { a.col.min(total) } else { 0 };
                    let c1 = if n == z.line { z.col.min(total) } else { total };
                    let tail = if n != z.line && !line.wraps { b.px(4.0) } else { 0.0 };
                    let (x0, x1) = (xs[c0], xs[c1] + tail);
                    if x1 > x0 {
                        b.fill(Rect { x: x0, y: line_y, w: (x1 - x0).max(1.0), h: placed.h }, 0.0, blend(surface, PRIMARY, 0.32), Layer::Mid);
                    }
                }
            }
            for run in &line.runs {
                let rx = x + run.col as f64 / UNIT;
                let font = placed.base.of(run.style);
                let run_w = run.text.chars().map(|c| width(c, run.style)).sum::<usize>() as f64 / UNIT;
                let ry = line_y + ((placed.h - b.line(font)) / 2.0).round();
                let rgb = match run.style {
                    Style::Plain => placed.rgb,
                    Style::Bold => blend(placed.rgb, WHITE, 0.6),
                    Style::Heading => TITLE,
                    Style::Code => {
                        if line.kind == Kind::Text {
                            // Into the half space of room text.rs left each side of it
                            let room = (width(' ', Style::Code) / 2) as f64 / UNIT;
                            let chip_h = b.line(Font::Code) - b.px(2.0);
                            b.fill(
                                Rect {
                                    x: rx - room,
                                    y: line_y + ((placed.h - chip_h) / 2.0).round(),
                                    w: run_w + 2.0 * room,
                                    h: chip_h,
                                },
                                b.px(5.0),
                                blend(surface, WHITE, 0.09),
                                Layer::Mid,
                            );
                        }
                        CODE
                    }
                    Style::Link => {
                        if let Some(target) = run.link.as_deref().and_then(session_link) {
                            if ui.hovered.as_ref() == Some(&target) {
                                b.fill(Rect { x: rx, y: line_y + placed.h - b.px(3.0), w: run_w, h: b.px(1.0).max(1.0) }, 0.0, PRIMARY, Layer::Mid);
                            }
                            b.hit(Rect { x: rx, y: line_y, w: run_w, h: placed.h }, target);
                        }
                        PRIMARY
                    }
                };
                b.text(rx, ry, &run.text, font, rgb, Layer::Text);
            }
            lines_out.push(TextLine {
                x,
                y: line_y,
                h: placed.h,
                text: line.text.clone(),
                xs,
                wraps: line.wraps,
            });
        }
        for draw in &block.draws {
            match draw {
                Draw::Hit(r, target) => b.hit(Rect { x: bx + r.x, y: y + r.y, ..*r }, target.clone()),
                Draw::Button(r, label, tone, target, under, font) => {
                    b.button(Rect { x: bx + r.x, y: y + r.y, ..*r }, label, *tone, target.clone(), ui, *under, true, false, *font)
                }
                _ => {}
            }
        }
        if let Some(item) = block.item {
            let rect = Rect {
                x: conv.x,
                y,
                w: conv.w,
                h: block.height,
            };
            said.items.push(state::ViewItem {
                id: item.id.to_string(),
                who: item.who.name().to_string(),
                text: item.text.chars().take(200).collect(),
                queued: item.queued,
                streaming: item.streaming,
                tool: item.tool.map(|t| t.status.clone()),
                rect: intersect(rect, conv).map(said_rect),
            });
        }
        y += block.height;
    }
    if blocks.is_empty() {
        let words = if !connected {
            format!("Waiting for the seat on {}…", model.pipe)
        } else if conductor.takes_words() {
            "Nothing said yet. Type below to talk to the Conductor.".to_string()
        } else {
            "The Conductor is off. Turn it on above to talk to it.".to_string()
        };
        let words = b.fit(&words, conv.w, Font::Text);
        let x = conv.x + ((conv.w - b.w(&words, Font::Text)) / 2.0).max(0.0).round();
        b.text(x, conv.y + (conv.h / 2.0 - b.line(Font::Text)).max(0.0).round(), &words, Font::Text, b.white(0.45), Layer::Text);
    }
    if band > 0.0 {
        let label = format!("{ICON_DOWN} Newest");
        let w = b.pill_w(&label, Font::SmallStrong);
        let r = Rect {
            x: conv.x + ((conv.w - w) / 2.0).round(),
            y: conv.y + conv.h + b.px(4.0),
            w,
            h: newest_h,
        };
        b.clip = area;
        b.button(r, &label, Tone::Tonal(PRIMARY), Target::Newest, ui, surface, true, false, Font::SmallStrong);
    }
    said.conversation = said_rect(conv);
    said.scroll = ui.scroll.round() as u32;
    said.scroll_max = scroll_max.round() as u32;
    said.streaming = model.items.iter().find(|i| i.streaming).map(|i| i.id.to_string());
    if let Some((a, z)) = ui.selection {
        let scene = Scene {
            lines: lines_out.clone(),
            ..Default::default()
        };
        let words = scene.selected(a, z);
        said.selection = (!words.is_empty()).then_some(words);
    }

    // The side column: its own header, then its cards scrolling under it; or folded, a rail
    let col = Rect {
        x: main.x + main.w + gap,
        y: inner.y,
        w: side_w,
        h: inner.h,
    };
    let side = Some(col);
    let mut side_max = 0.0;
    b.clip = area;
    if folded {
        side_rail(&mut b, ui, model, col, narrow);
    } else {
        let head_h = b.px(28.0);
        side_head(&mut b, ui, model, Rect { h: head_h, ..col });
        let cards = Rect {
            y: col.y + head_h + b.px(12.0),
            h: (col.h - head_h - b.px(12.0)).max(0.0),
            ..col
        };
        b.clip = cards;
        b.hit(cards, Target::Side);
        ui.side_scroll = ui.side_scroll.clamp(0.0, ui.side_max);
        let top = cards.y - ui.side_scroll;
        let (end, sections) = side_cards(&mut b, ui, model, cards, top, &mut boxes, &mut caret, &mut said_boxes, &mut said);
        ui.sections = sections;
        let total = end - top;
        side_max = (total - cards.h).max(0.0);
        ui.side_max = side_max;
        // It was clamped to how far it could scroll last time: past how far it can now, it
        // moves up next time
        again |= ui.side_scroll > side_max;
    }

    said.buttons = std::mem::take(&mut b.buttons);
    said.boxes = said_boxes;
    said.focus = ui.focus.name();
    said.hovered = ui.hovered.as_ref().map(Target::name);
    ui.wraps = std::mem::take(&mut b.wraps);
    let clock = changes_at(model);
    said.clock = clock;
    Scene {
        prims: b.prims,
        hits: b.hits,
        lines: lines_out,
        card: area,
        conversation: conv,
        side,
        scroll_max,
        side_max,
        boxes,
        caret,
        clock,
        again,
        said,
    }
}

/// The whole view laid out as `lay_out` does, and again while that leaves what it kept of itself
/// a step behind (`Scene::again`): a layout uses how far the side could scroll and whether the
/// conversation was scrolled up as the last one left them, so one run after a change can leave
/// the side scrolled past its end or the "Newest" band where it's gone. While the GUI laid the
/// view out every frame, the next frame put that right; it lays it out only when something it's
/// made from changes now (render/conductor_view.rs), so it settles here, in two runs at most.
pub fn lay_out_settled(area: Rect, m: &Metrics, surface: u32, model: &Model, ui: &mut Ui) -> Scene {
    let mut scene = lay_out(area, m, surface, model, ui);
    for _ in 0..2 {
        if !scene.again {
            break;
        }
        scene = lay_out(area, m, surface, model, ui);
    }
    scene
}

/// A round button with an icon (the box's Send and Stop): filled in its colour while it can be
/// pressed (`filled`), else its colour faint behind its icon in it
#[allow(clippy::too_many_arguments)]
fn icon_button(b: &mut Builder, r: Rect, icon: &str, label: &str, colour: u32, filled: bool, target: Target, ui: &Ui, under: u32, enabled: bool) {
    let name = target.name();
    let pending = ui.pending.contains(&name);
    let hovered = enabled && !pending && ui.hovered.as_ref() == Some(&target);
    let (fill, words) = if enabled && filled {
        (blend(colour, WHITE, if hovered { 0.14 } else { 0.0 }), ON_FILL)
    } else if enabled {
        (blend(under, colour, if hovered { 0.30 } else { 0.20 }), colour)
    } else {
        (blend(under, WHITE, 0.08), blend(under, WHITE, 0.38))
    };
    b.fill(r, r.w / 2.0, fill, Layer::Mid);
    let gx = r.x + ((r.w - b.w(icon, Font::Title)) / 2.0).round();
    let gy = r.y + ((r.h - b.line(Font::Title)) / 2.0).round();
    b.text(gx, gy, icon, Font::Title, words, Layer::Text);
    if enabled && !pending {
        b.hit(r, target);
    }
    b.buttons.push(state::ViewButton {
        name,
        label: label.to_string(),
        rect: intersect(r, b.clip).map(said_rect),
        on: false,
        enabled,
        pending,
    });
}

/// What came before a block, for its gap and whether a reply needs its header
#[derive(Debug, Clone, Copy, PartialEq)]
enum Prev {
    Nothing,
    Day,
    You,
    /// The Conductor's turn: a reply (false) or a tool call (true)
    Turn(bool),
    Hub,
    Peer,
    Notice,
}

/// The conversation's blocks, each laid out from its own top left, `width` wide: a day's name
/// where the day changes, his words in a bubble on the right with their time under them, the
/// Conductor's turns under its avatar and time with their tool calls as quiet rows, the hub's
/// wakes as cards of what happened, other sessions' messages under who sent them, and the seat's
/// notices: a session needing him as an alert, the rest a line in the middle
fn conversation_blocks<'s>(b: &Builder, model: &'s Model, ui: &mut Ui, width: f64) -> Vec<Block<'s>> {
    let m = b.m;
    let indent = b.px(36.0);
    let body_w = (width - indent).max(b.px(80.0));
    let muted = b.white(0.46);
    let mut blocks: Vec<Block> = vec![];
    let mut prev = Prev::Nothing;
    let mut last_day: Option<String> = None;
    let items = model.items;
    // Three or more alerts that a session needs him, back to back (the seat's other notices
    // between them count in), fold to one line until he opens them: by the first's index, where
    // the run ends and its alerts
    let alert = |item: &Shown| *item.who == Who::Notice && words::needs(item.text).is_some();
    let mut runs: HashMap<usize, (usize, Vec<words::Needs>)> = HashMap::new();
    let mut i = 0;
    while i < items.len() {
        if !alert(&items[i]) {
            i += 1;
            continue;
        }
        let mut needs = vec![];
        let (mut j, mut end) = (i, i);
        while j < items.len() && matches!(items[j].who, Who::Notice | Who::Other(_)) {
            if let Some(n) = words::needs(items[j].text) {
                needs.push(n);
                end = j + 1;
            }
            j += 1;
        }
        if needs.len() >= 3 {
            runs.insert(i, (end, needs));
        }
        i = end;
    }
    let mut i = 0;
    while i < items.len() {
        let item = &items[i];
        i += 1;
        if item.at > 0 {
            let day = (model.day)(item.at, model.now_ms);
            if last_day.as_deref() != Some(day.as_str()) {
                let mut block = Block::new(None, b.px(24.0));
                let lh = b.line(Font::SmallStrong);
                let words_w = b.w(&day, Font::SmallStrong);
                let x = ((width - words_w) / 2.0).round();
                block.draws.push(Draw::Words(x, 0.0, day.clone(), Font::SmallStrong, b.white(0.55)));
                let rule_y = (lh / 2.0).round();
                for (from, to) in [(indent, x - b.px(12.0)), (x + words_w + b.px(12.0), width - indent)] {
                    if to > from {
                        block.draws.push(Draw::Fill(Rect { x: from, y: rule_y, w: to - from, h: m.scale.round().max(1.0) }, 0.0, b.white(0.08)));
                    }
                }
                block.height = lh;
                blocks.push(block);
                last_day = Some(day);
                prev = Prev::Day;
            }
        }
        let time = if item.at > 0 { (model.clock)(item.at) } else { String::new() };
        let after = |gap: f64, prev: Prev| if prev == Prev::Day { b.px(16.0) } else { gap };
        if let Some((end, needs)) = runs.get(&(i - 1)) {
            // The run on one line: how many, from whom, when, and a button to open it; opened,
            // the same line with a button to fold it, its alerts under it
            let open = ui.expanded.contains(item.id);
            let mut block = Block::new(if open { None } else { Some(item) }, after(b.px(20.0), prev));
            avatar(b, &mut block, 0.0, Font::SmallStrong, ICON_ALERT, Font::Title, blend(b.surface, ORANGE, 0.16), ORANGE);
            let mut sessions: Vec<&str> = vec![];
            for n in needs {
                if !sessions.contains(&n.label.as_str()) {
                    sessions.push(&n.label);
                }
            }
            let headline = if sessions.len() == needs.len() {
                format!("{} sessions needed you", needs.len())
            } else {
                format!("{} alerts from {} sessions", needs.len(), sessions.len())
            };
            let last_at = items[*end - 1].at;
            let span = if item.at > 0 && last_at > 0 { format!("{} – {}", time, (model.clock)(last_at)) } else { String::new() };
            block.draws.push(Draw::Words(indent, 0.0, headline.clone(), Font::SmallStrong, blend(b.surface, ORANGE, 0.95)));
            let mut x = indent + b.w(&headline, Font::SmallStrong) + b.px(8.0);
            if !span.is_empty() {
                block.draws.push(Draw::Words(x, 0.0, span.clone(), Font::Small, muted));
                x += b.w(&span, Font::Small);
            }
            let label = if open { "Fold" } else { "Show all" };
            let bw = b.pill_w(label, Font::SmallStrong);
            let bh = b.pill_h(Font::SmallStrong);
            let bx = (width - bw).max(x + b.px(8.0));
            block.draws.push(Draw::Button(Rect { x: bx, y: ((b.line(Font::SmallStrong) - bh) / 2.0).round(), w: bw, h: bh }, label.to_string(), Tone::Plain(PRIMARY), Target::More(item.id.to_string()), b.surface, Font::SmallStrong));
            let names = if sessions.len() > 3 {
                format!("{} and {} more", sessions[..3].join(", "), sessions.len() - 3)
            } else {
                sessions.join(", ")
            };
            let names = b.fit(&names, body_w, Font::Small);
            block.draws.push(Draw::Words(indent, b.line(Font::SmallStrong) + b.px(2.0), names, Font::Small, muted));
            block.height = b.line(Font::SmallStrong) + b.px(2.0) + b.line(Font::Small);
            blocks.push(block);
            prev = Prev::Notice;
            if !open {
                i = *end;
                continue;
            }
        }
        match item.who {
            Who::You => {
                let mut block = Block::new(Some(item), after(b.px(24.0), prev));
                let pad_x = b.px(16.0);
                let pad_y = b.px(10.0);
                let max_w = (width * 0.78).round();
                let cols = (((max_w - 2.0 * pad_x) * UNIT).floor() as usize).max(8);
                let lines = ui.cache.get(item.id, item.text, "you", cols, || text::plain(item.text, cols, units(m, Font::Text)));
                let used = lines.iter().map(|l| l.columns(styled(m, Font::Text))).max().unwrap_or(0) as f64 / UNIT;
                let bubble_w = (used + 2.0 * pad_x).max(b.px(40.0)).round();
                let bx = width - bubble_w;
                let colour = if item.queued { b.white(0.6) } else { b.white(0.94) };
                let fill = blend(b.surface, WHITE, if item.queued { 0.05 } else { 0.10 });
                let end = place(b, &mut block, &lines, bx + pad_x, pad_y, max_w, Font::Text, colour);
                let h = end + pad_y;
                block.draws.insert(0, Draw::Fill(Rect { x: bx, y: 0.0, w: bubble_w, h }, b.px(20.0).min(h / 2.0), fill));
                let mut meta = time.clone();
                if let Some(from) = item.from.filter(|f| *f != "lbterm") {
                    meta = if meta.is_empty() { format!("From {from}") } else { format!("From {from} · {meta}") };
                }
                let (meta, meta_rgb) = if item.queued {
                    (format!("Queued, for after this turn{}", if meta.is_empty() { String::new() } else { format!(" · {meta}") }), blend(b.surface, YELLOW, 0.8))
                } else {
                    (meta, muted)
                };
                let mut height = h;
                if !meta.is_empty() {
                    let my = h + b.px(4.0);
                    block.draws.push(Draw::Words(width - b.w(&meta, Font::Small) - b.px(6.0), my, meta, Font::Small, meta_rgb));
                    height = my + b.line(Font::Small);
                }
                block.height = height;
                blocks.push(block);
                prev = Prev::You;
            }
            Who::Conductor | Who::Tool => {
                let tool = *item.who == Who::Tool;
                let head = !matches!(prev, Prev::Turn(_));
                let gap = match prev {
                    _ if head => after(b.px(24.0), prev),
                    Prev::Turn(true) if tool => b.px(2.0),
                    Prev::Turn(false) if !tool => b.px(12.0),
                    _ => b.px(10.0),
                };
                let mut block = Block::new(Some(item), gap);
                let mut top = 0.0;
                if head {
                    let lh = b.line(Font::SmallStrong);
                    avatar(b, &mut block, 0.0, Font::SmallStrong, ICON_CONDUCTOR, Font::Small, blend(b.surface, PRIMARY, 0.22), PRIMARY);
                    block.draws.push(Draw::Words(indent, 0.0, "Conductor".to_string(), Font::SmallStrong, b.white(0.9)));
                    if !time.is_empty() {
                        let tx = indent + b.w("Conductor", Font::SmallStrong) + b.px(8.0);
                        block.draws.push(Draw::Words(tx, 0.0, time.clone(), Font::Small, muted));
                    }
                    top = lh + b.px(8.0);
                }
                if tool {
                    let (icon, colour) = match item.tool.map(|t| t.status.as_str()) {
                        Some("running") => (ICON_LOADING, GREEN),
                        Some("error") => (ICON_CLOSE, RED),
                        Some("denied") => (ICON_CANCEL, ORANGE),
                        _ => (ICON_CHECK, b.white(0.42)),
                    };
                    let (name, detail) = match item.tool {
                        Some(t) => {
                            let name = if t.name.is_empty() { t.text.split(':').next().unwrap_or("").trim() } else { t.name.as_str() };
                            let rest = t.text.trim();
                            let rest = rest.strip_prefix(name).map(|r| r.trim_start_matches(':').trim()).unwrap_or(rest);
                            let rest = rest.trim_end_matches(':').trim_end();
                            (name.to_string(), as_he_knows_them(model.view, rest))
                        }
                        None => (String::new(), item.text.to_string()),
                    };
                    let mut x = indent;
                    block.draws.push(Draw::Words(x, top, icon.to_string(), Font::Small, colour));
                    x += b.w(icon, Font::Small) + b.px(8.0);
                    if !name.is_empty() {
                        block.draws.push(Draw::Words(x, top, name.clone(), Font::SmallStrong, b.white(0.66)));
                        x += b.w(&name, Font::SmallStrong) + b.px(6.0);
                    }
                    let shown = b.fit(&detail, width - x, Font::Small);
                    block.draws.push(Draw::Words(x, top, shown, Font::Small, muted));
                    block.height = top + b.line(Font::Small);
                } else {
                    let cols = ((body_w * UNIT).floor() as usize).max(8);
                    let lines = ui.cache.get(item.id, item.text, "md", cols, || text::markdown(item.text, cols, styled(m, Font::Text)));
                    let mut end = place(b, &mut block, &lines, indent, top, body_w, Font::Text, b.white(0.88));
                    if item.streaming {
                        let lh = b.line(Font::Text);
                        let (cx, cy) = match block.lines.last() {
                            Some(p) => (p.x + p.line.columns(styled(m, Font::Text)) as f64 / UNIT + b.px(2.0), p.y),
                            None => (indent, top),
                        };
                        block.draws.push(Draw::Fill(Rect { x: cx, y: cy + b.px(5.0), w: b.px(2.0).max(1.0), h: lh - b.px(10.0) }, 0.0, PRIMARY));
                        if lines.is_empty() {
                            end = top + lh;
                        }
                    }
                    block.height = end;
                }
                blocks.push(block);
                prev = Prev::Turn(tool);
            }
            Who::Hub => {
                let mut block = Block::new(Some(item), after(b.px(20.0), prev));
                let wake = words::wake(item.text);
                let ipad = b.px(14.0);
                let lh = b.line(Font::Small);
                let x0 = indent;
                let inner_w = body_w - 2.0 * ipad;
                avatar(b, &mut block, ipad, Font::Small, ICON_BELL, Font::Small, b.white(0.07), b.white(0.6));
                let mut y = ipad;
                block.draws.push(Draw::Words(x0 + ipad, y, "Hub".to_string(), Font::SmallStrong, b.white(0.8)));
                let hx = x0 + ipad + b.w("Hub", Font::SmallStrong) + b.px(8.0);
                let time_w = if time.is_empty() { 0.0 } else { b.w(&time, Font::Small) + b.px(8.0) };
                let headline = b.fit(&wake.headline, x0 + body_w - ipad - time_w - hx, Font::Small);
                block.draws.push(Draw::Words(hx, y, headline, Font::Small, b.white(0.6)));
                if !time.is_empty() {
                    block.draws.push(Draw::Words(x0 + body_w - ipad - b.w(&time, Font::Small), y, time.clone(), Font::Small, muted));
                }
                y += lh + b.px(6.0);
                let clock_w = b.w("00:00", Font::Small) + b.px(10.0);
                for e in &wake.events {
                    if let Some(t) = &e.time {
                        block.draws.push(Draw::Words(x0 + ipad, y, t.clone(), Font::Small, muted));
                    }
                    let tw = inner_w - clock_w;
                    let cols = ((tw * UNIT).floor() as usize).max(8);
                    let lines = text::plain(&e.text, cols, units(m, Font::Small));
                    for (i, line) in lines.iter().take(2).enumerate() {
                        let shown = if i == 1 && lines.len() > 2 { b.fit(&format!("{} …", line.text), tw, Font::Small) } else { line.text.clone() };
                        block.draws.push(Draw::Words(x0 + ipad + clock_w, y, shown, Font::Small, b.white(0.74)));
                        y += lh;
                    }
                    y += b.px(4.0);
                }
                let height = if wake.events.is_empty() { 2.0 * ipad + lh } else { y - b.px(4.0) + ipad };
                block.draws.insert(0, Draw::Fill(Rect { x: x0, y: 0.0, w: body_w, h: height }, b.px(14.0), b.white(0.045)));
                block.height = height;
                blocks.push(block);
                prev = Prev::Hub;
            }
            Who::Peer => {
                let mut block = Block::new(Some(item), after(b.px(24.0), prev));
                let name = peer_label(item.label, item.project);
                let initial = item.label.and_then(|l| l.trim().chars().next()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".to_string());
                avatar(b, &mut block, 0.0, Font::SmallStrong, &initial, Font::SmallStrong, blend(b.surface, LAVENDER, 0.24), TITLE);
                // "From Site publish · Website": the name opens its tab when it's known
                let label = called(item.label).to_string();
                let from_w = b.w("From ", Font::Small);
                let mut x = indent;
                if item.label.is_some() {
                    block.draws.push(Draw::Words(x, 0.0, "From".to_string(), Font::Small, muted));
                    x += from_w;
                    let label_w = b.w(&label, Font::SmallStrong);
                    let target = session_named(model.view, &label, item.project.unwrap_or(""));
                    block.draws.push(Draw::Words(x, 0.0, label.clone(), Font::SmallStrong, if target.is_some() { LAVENDER } else { b.white(0.9) }));
                    if let Some(t) = target {
                        block.draws.push(Draw::Hit(Rect { x, y: 0.0, w: label_w, h: b.line(Font::SmallStrong) }, t));
                    }
                    x += label_w;
                    if let Some(p) = item.project.filter(|p| !p.trim().is_empty()) {
                        let p = format!(" · {p}");
                        block.draws.push(Draw::Words(x, 0.0, p.clone(), Font::Small, muted));
                        x += b.w(&p, Font::Small);
                    }
                } else {
                    block.draws.push(Draw::Words(x, 0.0, name.clone(), Font::SmallStrong, b.white(0.9)));
                    x += b.w(&name, Font::SmallStrong);
                }
                if !time.is_empty() {
                    block.draws.push(Draw::Words(x + b.px(8.0), 0.0, time.clone(), Font::Small, muted));
                }
                let top = b.line(Font::SmallStrong) + b.px(6.0);
                let cols = ((body_w * UNIT).floor() as usize).max(8);
                let lines = ui.cache.get(item.id, item.text, "md", cols, || text::markdown(item.text, cols, styled(m, Font::Text)));
                let open = ui.expanded.contains(item.id);
                let long = lines.len() > PEER_LINES;
                let shown: &[Line] = if long && !open { &lines[..PEER_LINES - 1] } else { &lines };
                let mut end = place(b, &mut block, shown, indent, top, body_w, Font::Text, b.white(0.8));
                if long {
                    let label = if open { "Show less" } else { "Show more" };
                    let w = b.pill_w(label, Font::SmallStrong);
                    let h = b.pill_h(Font::SmallStrong);
                    end += b.px(4.0);
                    block.draws.push(Draw::Button(Rect { x: indent - b.px(12.0), y: end, w, h }, label.to_string(), Tone::Plain(PRIMARY), Target::More(item.id.to_string()), b.surface, Font::SmallStrong));
                    end += h;
                }
                block.height = end;
                blocks.push(block);
                prev = Prev::Peer;
            }
            Who::Notice | Who::Other(_) => {
                let lh = b.line(Font::Small);
                if let Some(needs) = words::needs(item.text) {
                    let mut block = Block::new(Some(item), after(b.px(20.0), prev));
                    avatar(b, &mut block, 0.0, Font::SmallStrong, ICON_ALERT, Font::Title, blend(b.surface, ORANGE, 0.16), ORANGE);
                    let mut x = indent;
                    let label_w = b.w(&needs.label, Font::SmallStrong);
                    let target = session_named(model.view, &needs.label, &needs.project);
                    block.draws.push(Draw::Words(x, 0.0, needs.label.clone(), Font::SmallStrong, b.white(0.92)));
                    if let Some(t) = target {
                        block.draws.push(Draw::Hit(Rect { x, y: 0.0, w: label_w, h: b.line(Font::SmallStrong) }, t));
                    }
                    x += label_w;
                    let rest = if needs.project.is_empty() { " needs you".to_string() } else { format!(" · {} needs you", needs.project) };
                    block.draws.push(Draw::Words(x, 0.0, rest.clone(), Font::Small, blend(b.surface, ORANGE, 0.9)));
                    x += b.w(&rest, Font::Small);
                    if !time.is_empty() {
                        block.draws.push(Draw::Words(x + b.px(8.0), 0.0, time.clone(), Font::Small, muted));
                    }
                    let mut y = b.line(Font::SmallStrong) + b.px(2.0);
                    let cols = ((body_w * UNIT).floor() as usize).max(8);
                    let what = text::plain(&needs.what, cols, units(m, Font::Text));
                    for (i, line) in what.iter().take(2).enumerate() {
                        let shown = if i == 1 && what.len() > 2 { b.fit(&format!("{} …", line.text), body_w, Font::Text) } else { line.text.clone() };
                        block.draws.push(Draw::Words(indent, y, shown, Font::Text, b.white(0.86)));
                        y += b.line(Font::Text);
                    }
                    if !needs.detail.is_empty() {
                        let detail = b.fit(&needs.detail, body_w, Font::Small);
                        block.draws.push(Draw::Words(indent, y, detail, Font::Small, muted));
                        y += lh;
                    }
                    block.height = y;
                    blocks.push(block);
                } else {
                    let gap = if prev == Prev::Notice { b.px(4.0) } else { after(b.px(18.0), prev) };
                    let mut block = Block::new(Some(item), gap);
                    let words = if time.is_empty() { item.text.to_string() } else { format!("{} · {time}", item.text.trim().trim_end_matches('.')) };
                    let room = width - 2.0 * indent;
                    let cols = ((room * UNIT).floor() as usize).max(8);
                    let lines = text::plain(&words, cols, units(m, Font::Small));
                    let mut y = 0.0;
                    for (i, line) in lines.iter().filter(|l| l.kind != Kind::Blank).take(2).enumerate() {
                        let shown = if i == 1 && lines.len() > 2 { b.fit(&format!("{} …", line.text), room, Font::Small) } else { line.text.clone() };
                        let x = ((width - b.w(&shown, Font::Small)) / 2.0).max(0.0).round();
                        block.draws.push(Draw::Words(x, y, shown, Font::Small, b.white(0.5)));
                        y += lh;
                    }
                    block.height = y;
                    blocks.push(block);
                }
                prev = Prev::Notice;
            }
        }
    }
    blocks
}

/// A round button with only an icon, words-only in tone: a faint fill under the pointer (the
/// side's fold, the review's previous and next)
#[allow(clippy::too_many_arguments)]
fn round_icon(b: &mut Builder, r: Rect, icon: &str, label: &str, target: Target, ui: &Ui, under: u32, rgb: u32) {
    let name = target.name();
    if ui.hovered.as_ref() == Some(&target) {
        b.fill(r, r.w.min(r.h) / 2.0, blend(under, WHITE, 0.08), Layer::Mid);
    }
    let gx = r.x + ((r.w - b.w(icon, Font::Title)) / 2.0).round();
    let gy = r.y + ((r.h - b.line(Font::Title)) / 2.0).round();
    b.text(gx, gy, icon, Font::Title, rgb, Layer::Text);
    b.hit(r, target);
    b.buttons.push(state::ViewButton {
        name,
        label: label.to_string(),
        rect: intersect(r, b.clip).map(said_rect),
        on: false,
        enabled: true,
        pending: false,
    });
}

/// How many of the view's sessions are in one of `statuses`, those hidden from the Conductor
/// left out: they're in the Hidden group, not their status's (lbterm/NEW-TAB.md, section 8)
fn sessions_in(view: &View, statuses: &[&str]) -> usize {
    view.sessions.iter().filter(|s| !s.hidden && statuses.contains(&s.status.as_str())).count()
}

/// The Sessions card's group of the sessions hidden from the Conductor, at its foot and folded
/// at first ([`FOLDED_AT_FIRST`]), and its button's words (lbterm/NEW-TAB.md, section 8)
const HIDDEN_GROUP: &str = "sessions:hidden";
const SHOW: &str = "Show";

/// The groups the sessions list falls into, in order: their section's name, heading, statuses
/// and colour
const GROUPS: [(&str, &str, &[&str], u32); 3] = [
    ("sessions:waiting", "Waiting", &["waiting", "blocked"], ORANGE),
    ("sessions:working", "Working", &["working"], GREEN),
    ("sessions:idle", "Idle", &["idle", "done"], BLUE),
];

/// The side column's header, level with the title: how the sessions stand, and the button that
/// folds the side to a rail
fn side_head(b: &mut Builder, ui: &Ui, model: &Model, r: Rect) {
    let fold = Rect { x: r.x + r.w - r.h, w: r.h, ..r };
    round_icon(b, fold, ICON_FOLD, "Fold the side", Target::FoldSide, ui, b.surface, b.white(0.62));
    let Some(view) = model.view.filter(|_| model.connected) else {
        return;
    };
    let mut x = r.x + b.px(4.0);
    let ty = r.y + ((r.h - b.line(Font::Small)) / 2.0).round();
    for (_, word, statuses, colour) in GROUPS {
        let n = sessions_in(view, statuses);
        if n == 0 {
            continue;
        }
        let d = b.px(7.0);
        b.dot(x, r.y + ((r.h - d) / 2.0).round(), d, colour, Layer::Mid);
        x += d + b.px(6.0);
        let words = format!("{n} {}", word.to_lowercase());
        b.text(x, ty, &words, Font::Small, b.white(0.66), Layer::Text);
        x += b.w(&words, Font::Small) + b.px(14.0);
    }
}

/// The side folded to a rail as wide as the tab rail, Material's navigation rail: what needs him
/// and how the sessions stand, an icon and a count each, and a press opens the side at that
/// section. Too narrow for both columns, it stays folded and only shows.
fn side_rail(b: &mut Builder, ui: &Ui, model: &Model, col: Rect, narrow: bool) {
    let under = b.white(0.035);
    b.fill(col, b.px(16.0), under, Layer::Back);
    let cx = col.x + col.w / 2.0;
    let mut y = col.y + b.px(10.0);
    if !narrow {
        let d = b.px(36.0);
        round_icon(b, Rect { x: (cx - d / 2.0).round(), y, w: d, h: d }, ICON_UNFOLD, "Open the side", Target::FoldSide, ui, under, b.white(0.7));
        y += d + b.px(14.0);
    }
    let Some(view) = model.view.filter(|_| model.connected) else {
        return;
    };
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut items: Vec<(&str, &str, String, u32, bool)> = vec![];
    if !view.asks.is_empty() {
        items.push(("asks", ICON_ALERT, plural(view.asks.len(), "ask", "asks"), ORANGE, true));
    }
    if !view.waiting.is_empty() {
        items.push(("waiting", ICON_QUESTION, format!("{} waiting", view.waiting.len()), ORANGE, true));
    }
    let working = sessions_in(view, &["working"]);
    items.push(("sessions:working", ICON_WORKING, format!("{working} working"), GREEN, working > 0));
    items.push(("sessions:idle", ICON_IDLE, format!("{} idle", sessions_in(view, &["idle", "done"])), BLUE, false));
    if let Some(e) = view.events.last() {
        items.push(("events", ICON_BELL, (model.clock)(e.at), LAVENDER, false));
    }
    let open = view.suggestions.iter().filter(|s| s.status == "open").count();
    if open > 0 {
        items.push(("suggestions", ICON_IDEA, plural(open, "idea", "ideas"), PRIMARY, false));
    }
    if !view.notes.is_empty() {
        items.push(("notes", ICON_NOTE, plural(view.notes.len(), "note", "notes"), b.white(0.8), false));
    }
    let pill_w = (col.w - b.px(16.0)).min(b.px(56.0)).round();
    let pill_h = b.px(32.0);
    for (key, icon, label, colour, lit) in items {
        let start = y;
        let target = Target::Reveal(key.to_string());
        let hovered = !narrow && ui.hovered.as_ref() == Some(&target);
        let pill = Rect { x: (cx - pill_w / 2.0).round(), y, w: pill_w, h: pill_h };
        let fill = if lit { blend(under, colour, 0.22) } else { blend(under, WHITE, 0.05) };
        b.fill(pill, pill_h / 2.0, if hovered { blend(fill, WHITE, 0.08) } else { fill }, Layer::Mid);
        let gx = cx - b.w(icon, Font::Title) / 2.0;
        b.text(gx.round(), y + ((pill_h - b.line(Font::Title)) / 2.0).round(), icon, Font::Title, if lit { colour } else { blend(under, colour, 0.85) }, Layer::Text);
        y += pill_h + b.px(4.0);
        let label = b.fit(&label, col.w - b.px(4.0), Font::Small);
        b.text((cx - b.w(&label, Font::Small) / 2.0).round(), y, &label, Font::Small, if lit { colour } else { b.white(0.72) }, Layer::Text);
        y += b.line(Font::Small) + b.px(14.0);
        let r = Rect { x: col.x, y: start, w: col.w, h: y - start - b.px(6.0) };
        if !narrow {
            b.hit(r, target.clone());
        }
        b.buttons.push(state::ViewButton {
            name: target.name(),
            label,
            rect: intersect(r, b.clip).map(said_rect),
            on: lit,
            enabled: !narrow,
            pending: false,
        });
    }
}

/// The side column's cards from `top`: what the Conductor asks, who waits on him, every session,
/// the events, the suggestions and the notes, each section under a heading that folds it. Where
/// they end, and where each section's heading is down from `top`.
#[allow(clippy::too_many_arguments)]
fn side_cards(
    b: &mut Builder,
    ui: &Ui,
    model: &Model,
    col: Rect,
    top: f64,
    boxes: &mut Vec<BoxPlace>,
    caret: &mut Option<Rect>,
    said_boxes: &mut Vec<state::ViewBox>,
    said: &mut state::ConductorView,
) -> (f64, Vec<(String, f64)>) {
    let mut sections: Vec<(String, f64)> = vec![];
    let surface = b.surface;
    let card_rgb = blend(surface, WHITE, 0.045);
    let hover_rgb = blend(card_rgb, WHITE, 0.04);
    let ipad = b.px(16.0);
    let inner_x = col.x + ipad;
    let inner_w = col.w - 2.0 * ipad;
    let section_gap = b.px(24.0);
    let card_gap = b.px(10.0);
    let radius = b.px(16.0);
    let small_lh = b.line(Font::Small);
    let muted = blend(card_rgb, WHITE, 0.48);
    let mut y = top;
    let Some(view) = model.view.filter(|_| model.connected) else {
        b.text(col.x, y, "Nothing from the seat yet", Font::Small, b.white(0.45), Layer::Text);
        return (y + small_lh, sections);
    };
    // A section's heading: a chevron, its name and count, folding it at a press, and buttons of
    // its own at its right end. Whether the section is open.
    let heading = |b: &mut Builder, y: &mut f64, sections: &mut Vec<(String, f64)>, key: &str, words: &str, count: usize, actions: &[(String, Target, bool)]| -> bool {
        let open = !ui.is_folded(key);
        let row_h = b.px(28.0);
        let row = Rect { x: col.x, y: *y, w: col.w, h: row_h };
        let target = Target::Section(key.to_string());
        if ui.hovered.as_ref() == Some(&target) {
            b.fill(row, b.px(8.0), b.white(0.04), Layer::Mid);
        }
        b.hit(row, target.clone());
        b.buttons.push(state::ViewButton {
            name: target.name(),
            label: words.to_string(),
            rect: intersect(row, b.clip).map(said_rect),
            on: open,
            enabled: true,
            pending: false,
        });
        let ty = *y + ((row_h - b.line(Font::SmallStrong)) / 2.0).round();
        let chevron = if open { ICON_OPEN } else { ICON_SHUT };
        let mut x = col.x + b.px(4.0);
        b.text(x, ty, chevron, Font::Small, b.white(0.5), Layer::Text);
        x += b.w(chevron, Font::Small) + b.px(6.0);
        b.text(x, ty, words, Font::SmallStrong, b.white(0.75), Layer::Text);
        x += b.w(words, Font::SmallStrong) + b.px(8.0);
        if count > 0 {
            let n = count.to_string();
            let h = b.px(18.0);
            let w = (b.w(&n, Font::SmallStrong) + b.px(12.0)).max(h);
            let by = *y + ((row_h - h) / 2.0).round();
            b.fill(Rect { x, y: by, w, h }, h / 2.0, b.white(0.09), Layer::Mid);
            b.text(x + ((w - b.w(&n, Font::SmallStrong)) / 2.0).round(), by, &n, Font::SmallStrong, b.white(0.78), Layer::Text);
        }
        let mut right = col.x + col.w;
        for (label, target, enabled) in actions.iter().rev() {
            let w = b.pill_w(label, Font::SmallStrong);
            right -= w;
            b.button(Rect { x: right, y: *y, w, h: row_h }, label, Tone::Plain(PRIMARY), target.clone(), ui, b.surface, *enabled, false, Font::SmallStrong);
        }
        sections.push((key.to_string(), *y - top));
        *y += row_h + b.px(6.0);
        open
    };
    let card = |b: &mut Builder, said: &mut state::ConductorView, kind: &str, id: &str, title: &str, top: f64, bottom: f64, outline: Option<u32>, fill: u32| {
        let r = Rect {
            x: col.x,
            y: top,
            w: col.w,
            h: bottom - top,
        };
        b.fill(r, radius, fill, Layer::Back);
        if let Some(o) = outline {
            b.outline(r, radius, o, Layer::Back);
        }
        said.cards.push(state::ViewCard {
            kind: kind.to_string(),
            id: id.to_string(),
            title: title.to_string(),
            rect: intersect(r, b.clip).map(said_rect),
        });
    };
    // "19:18 · 6 min ago"
    let when = |at: u64| -> String {
        if at == 0 {
            return String::new();
        }
        format!("{} · {}", (model.clock)(at), ago_words(model.now_ms, at))
    };

    // What the Conductor wants to do, waiting on his Allow or Deny
    if !view.asks.is_empty() {
        let open = heading(b, &mut y, &mut sections, "asks", "Asks", view.asks.len(), &[]);
        for ask in view.asks.iter().filter(|_| open) {
            let start = y;
            let fill = blend(surface, ORANGE, 0.07);
            y += ipad;
            let icon_w = b.w(ICON_ALERT, Font::Title) + b.px(8.0);
            b.text(inner_x, y, ICON_ALERT, Font::Title, ORANGE, Layer::Text);
            y = b.wrapped(inner_x + icon_w, y + b.px(1.0), inner_w - icon_w, if ask.title.is_empty() { &ask.tool } else { &ask.title }, Font::Strong, TITLE, 3);
            // The tool, then the session and its project where the title doesn't name them
            let session = ask.session.as_ref().map(|_| called(ask.label.as_deref()));
            let mut meta: Vec<String> = [Some(ask.tool.as_str()), session, ask.project.as_deref()]
                .into_iter()
                .flatten()
                .enumerate()
                .filter(|(i, s)| !s.is_empty() && (*i == 0 || !ask.title.contains(*s)))
                .map(|(_, s)| s.to_string())
                .collect();
            let w = when(ask.at);
            if !w.is_empty() {
                meta.push(w);
            }
            b.text(inner_x + icon_w, y, &b.fit(&meta.join(" · "), inner_w - icon_w, Font::Small), Font::Small, blend(fill, WHITE, 0.5), Layer::Text);
            y += small_lh + b.px(10.0);
            if !ask.detail.is_empty() {
                let dstart = y;
                let end = b.wrapped(inner_x + b.px(12.0), y + b.px(10.0), inner_w - b.px(24.0), &ask.detail, Font::Code, blend(surface, WHITE, 0.84), usize::MAX);
                b.fill(
                    Rect {
                        x: inner_x,
                        y: dstart,
                        w: inner_w,
                        h: end - dstart + b.px(10.0),
                    },
                    b.px(10.0),
                    surface,
                    Layer::Mid,
                );
                y = end + b.px(22.0);
            }
            let ph = b.pill_h(Font::Strong);
            let aw = b.pill_w("Allow", Font::Strong);
            let dw = b.pill_w("Deny", Font::Strong);
            b.button(Rect { x: inner_x, y, w: aw, h: ph }, "Allow", Tone::Filled(GREEN), Target::Allow(ask.id.clone()), ui, fill, true, false, Font::Strong);
            b.button(Rect { x: inner_x + aw + b.px(8.0), y, w: dw, h: ph }, "Deny", Tone::Outlined(RED), Target::Deny(ask.id.clone()), ui, fill, true, false, Font::Strong);
            y += ph + ipad;
            card(b, said, "ask", &ask.id, &ask.title, start, y, Some(blend(surface, ORANGE, 0.45)), fill);
            y += card_gap;
        }
        y += if open { section_gap - card_gap } else { b.px(12.0) };
    }

    // A part the seat couldn't read, said where its list would be, in the warning colour, so
    // "nobody's waiting" and "can't tell who's waiting" look different (the Conductor's InMail
    // of 2026-11-09): what it is, what went wrong, and since when
    let warning = blend(surface, ORANGE, 0.85);
    let failed_line = |b: &mut Builder, y: f64, what: &str, f: &crate::seat::protocol::Failing| -> f64 {
        let since = when(f.since);
        let mut words = format!("{what}: {}", f.error.trim());
        if !since.is_empty() {
            words = format!("{words} · since {since}");
        }
        b.wrapped(col.x + b.px(4.0), y, col.w - b.px(8.0), &words, Font::Small, warning, 4) + card_gap
    };

    // What sessions wait on him for
    let waiting_failed = view.failed("waiting");
    if !view.waiting.is_empty() || waiting_failed.is_some() {
        let open = heading(b, &mut y, &mut sections, "waiting", "Waiting on you", view.waiting.len(), &[]);
        if let Some(f) = waiting_failed.filter(|_| open) {
            y = failed_line(b, y, "Can't read who's waiting on you, so nobody shows here", f);
        }
        for w in view.waiting.iter().filter(|_| open) {
            let start = y;
            y += ipad;
            let label = called(w.label.as_deref());
            let right = if w.extras { "Extras".to_string() } else { ago_words(model.now_ms, w.at) };
            let right_font = if w.extras { Font::SmallStrong } else { Font::Small };
            let right_w = b.w(&right, right_font);
            let room = inner_w - right_w - b.px(8.0);
            let name = b.fit(label, room, Font::Strong);
            let name_w = b.w(&name, Font::Strong);
            b.text(inner_x, y, &name, Font::Strong, TITLE, Layer::Text);
            if !w.project.is_empty() && name_w + b.px(36.0) < room {
                let p = b.fit(&w.project, room - name_w - b.px(8.0), Font::Small);
                b.text(inner_x + name_w + b.px(8.0), y + ((b.line(Font::Strong) - small_lh) / 2.0).round(), &p, Font::Small, muted, Layer::Text);
            }
            b.text(inner_x + inner_w - right_w, y + ((b.line(Font::Strong) - small_lh) / 2.0).round(), &right, right_font, if w.extras { BLUE } else { muted }, Layer::Text);
            y += b.line(Font::Strong) + b.px(8.0);
            if !w.answerable {
                let why = format!("Can't answer from here: {}", w.why.as_deref().unwrap_or("no pane to type into"));
                y = b.wrapped(inner_x, y, inner_w, &why, Font::Small, blend(card_rgb, ORANGE, 0.85), 4) + b.px(6.0);
            }
            if !w.questions.is_empty() {
                for (qi, q) in w.questions.iter().enumerate() {
                    if !q.header.is_empty() {
                        let hw = b.w(&q.header, Font::SmallStrong) + b.px(16.0);
                        let hh = b.px(22.0);
                        b.fill(Rect { x: inner_x, y, w: hw, h: hh }, hh / 2.0, blend(card_rgb, PRIMARY, 0.16), Layer::Mid);
                        b.text(inner_x + b.px(8.0), y + ((hh - small_lh) / 2.0).round(), &q.header, Font::SmallStrong, PRIMARY, Layer::Text);
                        y += hh + b.px(8.0);
                    }
                    y = b.wrapped(inner_x, y, inner_w, &q.question, Font::Text, blend(card_rgb, WHITE, 0.9), 8) + b.px(8.0);
                    let picked = ui.picks.get(&w.id).and_then(|p| p.get(qi)).cloned().unwrap_or_default();
                    for (oi, opt) in q.options.iter().enumerate() {
                        let on = picked.contains(&(oi as u32 + 1));
                        let ostart = y;
                        let label = if on { format!("{ICON_CHECK} {}", opt.label) } else { opt.label.clone() };
                        let mut oy = y + b.px(8.0);
                        // Laid out first to know its height, then its fill under it
                        let probe_prims = b.prims.len();
                        oy = b.wrapped(inner_x + b.px(14.0), oy, inner_w - b.px(28.0), &label, Font::Strong, if on { PRIMARY } else { blend(card_rgb, WHITE, 0.92) }, 4);
                        if !opt.description.is_empty() {
                            oy = b.wrapped(inner_x + b.px(14.0), oy, inner_w - b.px(28.0), &opt.description, Font::Small, blend(card_rgb, WHITE, 0.55), 4);
                        }
                        oy += b.px(8.0);
                        let r = Rect { x: inner_x, y: ostart, w: inner_w, h: oy - ostart };
                        let target = Target::Pick {
                            waiting: w.id.clone(),
                            question: qi,
                            option: oi,
                        };
                        let pending = ui.pending.contains(&Target::Answer { waiting: w.id.clone() }.name());
                        let hovered = ui.hovered.as_ref() == Some(&target);
                        let fill = if on {
                            blend(card_rgb, PRIMARY, 0.16)
                        } else if hovered && w.answerable {
                            blend(card_rgb, WHITE, 0.08)
                        } else {
                            card_rgb
                        };
                        let edge = if on { blend(card_rgb, PRIMARY, 0.6) } else { blend(card_rgb, WHITE, 0.16) };
                        // The fill goes under the words already laid out: put it before them
                        if let Some(rect) = intersect(r, b.clip) {
                            b.prims.insert(
                                probe_prims,
                                Prim::Fill {
                                    rect,
                                    radius: b.px(12.0),
                                    rgb: fill,
                                    layer: Layer::Mid,
                                },
                            );
                        }
                        b.outline(r, b.px(12.0), edge, Layer::Mid);
                        if w.answerable && !pending {
                            b.hit(r, target.clone());
                        }
                        b.buttons.push(state::ViewButton {
                            name: target.name(),
                            label: opt.label.clone(),
                            rect: intersect(r, b.clip).map(said_rect),
                            on,
                            enabled: w.answerable,
                            pending,
                        });
                        y = oy + b.px(6.0);
                    }
                    if w.answerable {
                        let other = (w.id.clone(), qi);
                        let open = ui.others.contains(&other);
                        let ph = b.pill_h(Font::SmallStrong);
                        let label = "Other…";
                        b.button(
                            Rect { x: inner_x - b.px(8.0), y, w: b.pill_w(label, Font::SmallStrong), h: ph },
                            label,
                            Tone::Plain(PRIMARY),
                            Target::Other { waiting: w.id.clone(), question: qi },
                            ui,
                            card_rgb,
                            true,
                            open,
                            Font::SmallStrong,
                        );
                        y += ph + b.px(6.0);
                        if open {
                            let key = BoxKey::Answer {
                                waiting: w.id.clone(),
                                question: Some(qi),
                            };
                            let h = text_box(b, ui, &key, inner_x, y, inner_w, 0.0, 4, "In your own words", card_rgb, false, boxes, caret, said_boxes);
                            y += h + b.px(8.0);
                        }
                    }
                }
                let ready = ui.answer(w).is_some();
                let any_other = (0..w.questions.len()).any(|q| ui.others.contains(&(w.id.clone(), q)));
                if w.answerable && (!w.one_click() || any_other) {
                    let ph = b.pill_h(Font::Strong);
                    let label = "Send answer";
                    b.button(
                        Rect { x: inner_x, y, w: b.pill_w(label, Font::Strong), h: ph },
                        label,
                        Tone::Filled(PRIMARY),
                        Target::Answer { waiting: w.id.clone() },
                        ui,
                        card_rgb,
                        ready,
                        false,
                        Font::Strong,
                    );
                    y += ph + b.px(4.0);
                }
            } else {
                let words = w.text.as_deref().unwrap_or(&w.gist);
                y = b.wrapped(inner_x, y, inner_w, words, Font::Text, blend(card_rgb, WHITE, 0.84), QUESTION_LINES) + b.px(8.0);
                if w.answerable {
                    let key = BoxKey::Answer {
                        waiting: w.id.clone(),
                        question: None,
                    };
                    // What's typed: a chip whose code is in it is lit
                    let typed = ui.input_of(&key).map(|i| i.text().to_string()).unwrap_or_default();
                    let mut x = inner_x;
                    let ph = b.pill_h(Font::SmallStrong);
                    for (code, label) in text::options(words) {
                        let chip = if label.is_empty() { code.clone() } else { format!("{code} {}", b.fit(&label, b.px(130.0), Font::SmallStrong)) };
                        let on = input::has_code(&typed, &code);
                        let pw = b.pill_w(&if on { format!("{ICON_CHECK} {chip}") } else { chip.clone() }, Font::SmallStrong);
                        if x + pw > inner_x + inner_w && x > inner_x {
                            x = inner_x;
                            y += ph + b.px(6.0);
                        }
                        b.button(
                            Rect { x, y, w: pw.min(inner_w), h: ph },
                            &chip,
                            Tone::Chip(PRIMARY),
                            Target::Chip { waiting: w.id.clone(), code: code.clone() },
                            ui,
                            card_rgb,
                            true,
                            on,
                            Font::SmallStrong,
                        );
                        x += pw + b.px(6.0);
                    }
                    if x > inner_x {
                        y += ph + b.px(10.0);
                    }
                    // Each extra on a row of its own, with a yes and a no ("E2+", "E2-")
                    for (code, label) in text::extras(words) {
                        let yes = format!("{code}+");
                        let no = format!("{code}-");
                        let chips: Vec<(&str, String, u32, bool)> = vec![
                            ("Yes", yes.clone(), GREEN, input::has_code(&typed, &yes)),
                            ("No", no.clone(), RED, input::has_code(&typed, &no)),
                        ];
                        let widths: Vec<f64> = chips.iter().map(|(l, ..)| b.pill_w(&format!("{ICON_CHECK} {l}"), Font::SmallStrong)).collect();
                        let chips_w: f64 = widths.iter().sum::<f64>() + b.px(6.0);
                        let ty = y + ((ph - small_lh) / 2.0).round();
                        b.text(inner_x, ty, &code, Font::SmallStrong, blend(card_rgb, WHITE, 0.85), Layer::Text);
                        let lx = inner_x + b.w(&code, Font::SmallStrong) + b.px(8.0);
                        let words = b.fit(&label, inner_x + inner_w - chips_w - b.px(8.0) - lx, Font::Small);
                        b.text(lx, ty, &words, Font::Small, muted, Layer::Text);
                        let mut cx = inner_x + inner_w - chips_w;
                        for ((word, code, colour, on), cw) in chips.into_iter().zip(widths) {
                            b.button(
                                Rect { x: cx, y, w: cw, h: ph },
                                word,
                                Tone::Chip(colour),
                                Target::Chip { waiting: w.id.clone(), code },
                                ui,
                                card_rgb,
                                true,
                                on,
                                Font::SmallStrong,
                            );
                            cx += cw + b.px(6.0);
                        }
                        y += ph + b.px(6.0);
                    }
                    if !text::extras(words).is_empty() {
                        y += b.px(4.0);
                    }
                    let send_w = b.pill_w("Send", Font::SmallStrong);
                    let room = send_w + b.px(4.0);
                    let h = text_box(b, ui, &key, inner_x, y, inner_w, room, 4, "Your answer", card_rgb, false, boxes, caret, said_boxes);
                    let ready = ui.answer(w).is_some();
                    let ph = b.pill_h(Font::SmallStrong);
                    b.button(
                        Rect { x: inner_x + inner_w - b.px(6.0) - send_w, y: y + h - b.px(6.0) - ph, w: send_w, h: ph },
                        "Send",
                        Tone::Filled(PRIMARY),
                        Target::Answer { waiting: w.id.clone() },
                        ui,
                        blend(card_rgb, WHITE, 0.05),
                        ready,
                        false,
                        Font::SmallStrong,
                    );
                    y += h + b.px(4.0);
                }
            }
            y += ipad - b.px(4.0);
            card(b, said, "waiting", &w.id, &w.gist, start, y, Some(blend(surface, ORANGE, 0.4)), card_rgb);
            y += card_gap;
        }
        y += if open { section_gap - card_gap } else { b.px(12.0) };
    }

    // Every session: one list on one card, under a heading for each status it's in (waiting,
    // working, idle, then the rest), each folding on its own; idle ones a line each
    let open = heading(b, &mut y, &mut sections, "sessions", "Sessions", view.sessions.len(), &[]);
    if open {
        if let Some(f) = view.failed("sessions") {
            y = failed_line(b, y, "Can't read the sessions, so these are as last read", f);
        } else if view.sessions.is_empty() {
            b.text(col.x + b.px(4.0), y, "No other sessions", Font::Small, b.white(0.42), Layer::Text);
            y += small_lh + card_gap;
        }
    }
    let list_top = y;
    let list_prims = b.prims.len();
    let grouped: HashSet<&str> = GROUPS.iter().flat_map(|(_, _, statuses, _)| statuses.iter().copied()).collect();
    // The sessions hidden from the Conductor in a group of their own at the foot, folded until
    // he opens it (FOLDED_AT_FIRST), whatever their status (lbterm/NEW-TAB.md, section 8)
    let shown = || view.sessions.iter().filter(|s| !s.hidden);
    let mut groups: Vec<(&str, &str, Vec<&crate::seat::protocol::Session>, u32)> = GROUPS
        .iter()
        .map(|(key, word, statuses, colour)| (*key, *word, shown().filter(|s| statuses.contains(&s.status.as_str())).collect(), *colour))
        .collect();
    groups.push(("sessions:other", "Other", shown().filter(|s| !grouped.contains(s.status.as_str())).collect(), b.white(0.6)));
    groups.push((HIDDEN_GROUP, "Hidden", view.sessions.iter().filter(|s| s.hidden).collect(), b.white(0.6)));
    let hairline = |b: &mut Builder, y: f64| {
        b.fill(Rect { x: inner_x, y, w: inner_w, h: b.m.scale.round().max(1.0) }, 0.0, blend(card_rgb, WHITE, 0.06), Layer::Mid);
    };
    let mut first_group = true;
    for (key, word, members, colour) in groups.into_iter().filter(|g| open && !g.2.is_empty()) {
        // The group's heading inside the card: a chevron, its word in its colour, its count
        if !first_group {
            hairline(b, y);
        }
        first_group = false;
        let group_open = !ui.is_folded(key);
        let hidden_group = key == HIDDEN_GROUP;
        let row_h = b.px(32.0);
        let row = Rect { x: col.x, y, w: col.w, h: row_h };
        let target = Target::Section(key.to_string());
        if ui.hovered.as_ref() == Some(&target) {
            b.fill(Rect { x: col.x + b.px(4.0), y: y + b.px(3.0), w: col.w - b.px(8.0), h: row_h - b.px(6.0) }, b.px(10.0), hover_rgb, Layer::Back);
        }
        b.hit(row, target.clone());
        b.buttons.push(state::ViewButton {
            name: target.name(),
            label: word.to_string(),
            rect: intersect(row, b.clip).map(said_rect),
            on: group_open,
            enabled: true,
            pending: false,
        });
        sections.push((key.to_string(), y - top));
        let ty = y + ((row_h - small_lh) / 2.0).round();
        let chevron = if group_open { ICON_OPEN } else { ICON_SHUT };
        b.text(inner_x, ty, chevron, Font::Small, b.white(0.5), Layer::Text);
        let wx = inner_x + b.w(chevron, Font::Small) + b.px(6.0);
        b.text(wx, ty, word, Font::SmallStrong, colour, Layer::Text);
        // Its count; the Hidden group's as "Hidden · 2", then in orange how many of them
        // wait on him, folded or open, since their questions are his to answer
        let n = if hidden_group { format!("\u{b7} {}", members.len()) } else { members.len().to_string() };
        let nx = wx + b.w(word, Font::SmallStrong) + b.px(6.0);
        b.text(nx, ty, &n, Font::SmallStrong, blend(card_rgb, WHITE, 0.55), Layer::Text);
        if hidden_group {
            let waiting = members.iter().filter(|s| GROUPS[0].2.contains(&s.status.as_str())).count();
            if waiting > 0 {
                let words = format!("{waiting} waiting on you");
                b.text(nx + b.w(&n, Font::SmallStrong) + b.px(10.0), ty, &words, Font::SmallStrong, ORANGE, Layer::Text);
            }
        }
        y += row_h;
        let compact = key == "sessions:idle";
        for sess in members.into_iter().filter(|_| group_open) {
            let start = y;
            hairline(b, y);
            y += b.px(if compact { 7.0 } else { 12.0 });
            let target = Target::Session {
                session_id: sess.session_id.clone(),
                pane: sess.pane.clone(),
            };
            let hovered = ui.hovered.as_ref() == Some(&target);
            let under = if hovered { hover_rgb } else { card_rgb };
            let strong_lh = b.line(Font::Strong);
            let dot = b.px(8.0);
            let status_rgb = status_colour(&sess.status);
            b.dot(inner_x, y + ((strong_lh - dot) / 2.0).round(), dot, status_rgb, Layer::Mid);
            let name_x = inner_x + dot + b.px(10.0);
            let line_w = inner_x + inner_w - name_x;
            let name = called(Some(&sess.label));
            let sy = y + ((strong_lh - small_lh) / 2.0).round();
            // Hidden from the Conductor: Show at the right end of its line, drawn once the row's
            // own place is taken, so a press on it is Show's and not the row's
            let mut show: Option<Rect> = None;
            if hidden_group {
                // A line: its name, its project, its status in its colour, and Show; the question
                // it asks under them in orange
                let bw = b.pill_w(SHOW, Font::SmallStrong);
                let bh = b.pill_h(Font::SmallStrong);
                let bx = inner_x + inner_w - bw;
                show = Some(Rect { x: bx, y: y + ((strong_lh - bh) / 2.0).round(), w: bw, h: bh });
                let status = capital(&sess.status);
                let status_w = b.w(&status, Font::SmallStrong);
                let room = bx - b.px(10.0) - status_w - b.px(10.0) - name_x;
                let shown = b.fit(name, room, Font::Strong);
                let name_w = b.w(&shown, Font::Strong);
                b.text(name_x, y, &shown, Font::Strong, blend(under, TITLE, 0.85), Layer::Text);
                if !sess.project.is_empty() && name_w + b.px(36.0) < room {
                    let project = b.fit(&sess.project, room - name_w - b.px(8.0), Font::Small);
                    b.text(name_x + name_w + b.px(8.0), sy, &project, Font::Small, blend(under, WHITE, 0.46), Layer::Text);
                }
                b.text(bx - b.px(10.0) - status_w, sy, &status, Font::SmallStrong, status_rgb, Layer::Text);
                y += strong_lh;
                if let Some(q) = sess.question.as_deref() {
                    b.text(name_x, y, &b.fit(&format!("Asks: {q}"), line_w, Font::Small), Font::Small, blend(under, ORANGE, 0.85), Layer::Text);
                    y += small_lh;
                }
                y += b.px(9.0);
            } else if compact {
                // A line: its name, its project, and what it last did as far as there's room
                let shown = b.fit(name, line_w * 0.6, Font::Strong);
                let name_w = b.w(&shown, Font::Strong);
                b.text(name_x, y, &shown, Font::Strong, blend(under, TITLE, 0.85), Layer::Text);
                let mut x = name_x + name_w + b.px(8.0);
                if !sess.project.is_empty() && x + b.px(30.0) < name_x + line_w {
                    let project = b.fit(&sess.project, name_x + line_w - x, Font::Small);
                    b.text(x, sy, &project, Font::Small, blend(under, WHITE, 0.46), Layer::Text);
                    x += b.w(&project, Font::Small) + b.px(8.0);
                }
                if !sess.summary.is_empty() && x + b.px(40.0) < name_x + line_w {
                    let about = b.fit(&sess.summary, name_x + line_w - x, Font::Small);
                    b.text(x, sy, &about, Font::Small, blend(under, WHITE, 0.36), Layer::Text);
                }
                y += strong_lh + b.px(7.0);
            } else {
                // Its status on the right, the Conductor's thinking or acting on it before that
                let status = capital(&sess.status);
                let tag = sess.conductor.clone().map(|c| capital(&c));
                let status_w = b.w(&status, Font::SmallStrong);
                let tag_w = tag.as_deref().map_or(0.0, |t| b.w(t, Font::SmallStrong) + b.px(8.0));
                let room = inner_x + inner_w - name_x - status_w - tag_w - b.px(10.0);
                let shown = b.fit(name, room, Font::Strong);
                let name_w = b.w(&shown, Font::Strong);
                b.text(name_x, y, &shown, Font::Strong, TITLE, Layer::Text);
                if !sess.project.is_empty() && name_w + b.px(36.0) < room {
                    let project = b.fit(&sess.project, room - name_w - b.px(8.0), Font::Small);
                    b.text(name_x + name_w + b.px(8.0), sy, &project, Font::Small, blend(under, WHITE, 0.46), Layer::Text);
                }
                let rx = inner_x + inner_w - status_w;
                if let Some(t) = tag.as_deref() {
                    b.text(rx - tag_w, sy, t, Font::SmallStrong, LAVENDER, Layer::Text);
                }
                b.text(rx, sy, &status, Font::SmallStrong, status_rgb, Layer::Text);
                y += strong_lh;
                if !sess.summary.is_empty() {
                    b.text(name_x, y, &b.fit(&sess.summary, line_w, Font::Small), Font::Small, blend(under, WHITE, 0.62), Layer::Text);
                    y += small_lh;
                }
                if let Some(q) = sess.question.as_deref() {
                    b.text(name_x, y, &b.fit(&format!("Asks: {q}"), line_w, Font::Small), Font::Small, blend(under, ORANGE, 0.85), Layer::Text);
                    y += small_lh;
                }
                if let Some(p) = sess.percent {
                    let mut words = sess.left.map(left).unwrap_or_default();
                    if !sess.agents.is_empty() {
                        let agents = format!("{} agent{}", sess.agents.len(), if sess.agents.len() == 1 { "" } else { "s" });
                        words = if words.is_empty() { agents } else { format!("{agents} · {words}") };
                    }
                    let ww = b.w(&words, Font::Small);
                    let track_w = (line_w - ww - b.px(10.0)).max(b.px(20.0));
                    let bar_h = b.px(4.0).max(2.0);
                    let by = y + ((small_lh - bar_h) / 2.0).round();
                    b.fill(Rect { x: name_x, y: by, w: track_w, h: bar_h }, bar_h / 2.0, blend(under, WHITE, 0.10), Layer::Mid);
                    if p > 0 {
                        b.fill(
                            Rect { x: name_x, y: by, w: (track_w * p as f64 / 100.0).max(bar_h), h: bar_h },
                            bar_h / 2.0,
                            blend(under, GREEN, 0.75),
                            Layer::Mid,
                        );
                    }
                    b.text(inner_x + inner_w - ww, y, &words, Font::Small, blend(under, WHITE, 0.5), Layer::Text);
                    y += small_lh;
                }
                y += b.px(12.0);
            }
            let r = Rect { x: col.x, y: start, w: col.w, h: y - start };
            if hovered {
                b.fill(Rect { x: col.x + b.px(4.0), y: start + b.px(3.0), w: col.w - b.px(8.0), h: y - start - b.px(6.0) }, b.px(12.0), hover_rgb, Layer::Back);
            }
            b.hit(r, target.clone());
            b.buttons.push(state::ViewButton {
                name: target.name(),
                label: name.to_string(),
                rect: intersect(r, b.clip).map(said_rect),
                on: false,
                enabled: true,
                pending: false,
            });
            said.cards.push(state::ViewCard {
                kind: if hidden_group { "hidden" } else { "session" }.to_string(),
                id: sess.session_id.clone(),
                title: name.to_string(),
                rect: intersect(r, b.clip).map(said_rect),
            });
            if let Some(r) = show {
                let target = Target::Show {
                    session_id: sess.session_id.clone(),
                    label: name.to_string(),
                };
                b.button(r, SHOW, Tone::Plain(PRIMARY), target, ui, under, true, false, Font::SmallStrong);
            }
        }
    }
    if open && !view.sessions.is_empty() {
        // The list's card under its rows
        y += b.px(4.0);
        let r = Rect { x: col.x, y: list_top, w: col.w, h: y - list_top };
        if let Some(rect) = intersect(r, b.clip) {
            b.prims.insert(list_prims, Prim::Fill { rect, radius, rgb: card_rgb, layer: Layer::Back });
        }
    }
    y += if open { section_gap } else { b.px(12.0) };

    // The hub's events, newest first
    if !view.events.is_empty() {
        let open = heading(b, &mut y, &mut sections, "events", "Events", 0, &[]);
        if open {
            let start = y;
            y += b.px(12.0);
            let clock_w = b.w("00:00", Font::Small) + b.px(10.0);
            let text_x = inner_x + clock_w;
            let text_w = inner_w - clock_w;
            for (n, e) in view.events.iter().rev().take(EVENTS).enumerate() {
                if n > 0 {
                    y += b.px(10.0);
                }
                b.text(inner_x, y, &(model.clock)(e.at), Font::Small, muted, Layer::Text);
                let kind = words::kind(&e.kind);
                b.text(text_x, y, &kind, Font::SmallStrong, event_colour(&e.kind), Layer::Text);
                let mut x = text_x + b.w(&kind, Font::SmallStrong);
                if e.wake {
                    let d = b.px(5.0);
                    b.dot(x + b.px(5.0), y + ((small_lh - d) / 2.0).round(), d, ORANGE, Layer::Mid);
                    x += b.px(10.0);
                }
                x += b.px(8.0);
                let who = if e.session.is_empty() && e.label.is_none() { String::new() } else { called_in(e.label.as_deref(), &e.project) };
                b.text(x, y, &b.fit(&who, inner_x + inner_w - x, Font::Small), Font::Small, blend(card_rgb, WHITE, 0.78), Layer::Text);
                y += small_lh;
                if let Some(words) = words::event(e) {
                    y = b.wrapped(text_x, y, text_w, &words, Font::Small, muted, 2);
                }
            }
            y += b.px(12.0);
            card(b, said, "events", "events", "Events", start, y, None, card_rgb);
        }
        y += if open { section_gap } else { b.px(12.0) };
    }

    // The RSI report's suggestions: its heading folds them, reviews them one at a time, or has
    // the Conductor make the report now (`seat.rsi` `run`, which spends Opus: his press)
    let open_ones: Vec<&crate::seat::protocol::Suggestion> = view.suggestions.iter().filter(|s| s.status == "open").collect();
    let reviewing = ui.review.as_deref().and_then(|id| view.suggestions.iter().find(|s| s.id == id));
    let mut actions: Vec<(String, Target, bool)> = vec![];
    if reviewing.is_none() {
        if !open_ones.is_empty() {
            actions.push(("Review".to_string(), Target::Review("start".to_string()), true));
        }
        let (label, enabled) = if view.rsi_running { (format!("{ICON_GENERATE} Generating"), false) } else { (format!("{ICON_GENERATE} Generate"), true) };
        actions.push((label, Target::Rsi { suggestion: String::new(), action: "run".to_string() }, enabled));
    }
    let open = heading(b, &mut y, &mut sections, "suggestions", "Suggestions", open_ones.len(), &actions);
    let suggestion = |b: &mut Builder, said: &mut state::ConductorView, y: &mut f64, sug: &crate::seat::protocol::Suggestion| {
        let start = *y;
        *y += ipad;
        *y = b.wrapped(inner_x, *y, inner_w, &sug.title, Font::Strong, TITLE, 3);
        let meta: Vec<&str> = [sug.project.as_str(), sug.kind.as_str(), sug.day.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
        b.text(inner_x, *y, &b.fit(&meta.join(" · "), inner_w, Font::Small), Font::Small, muted, Layer::Text);
        *y += small_lh + b.px(8.0);
        if !sug.change.is_empty() {
            *y = b.wrapped(inner_x, *y, inner_w, &sug.change, Font::Small, blend(card_rgb, WHITE, 0.82), 6) + b.px(6.0);
        }
        if !sug.saves.is_empty() {
            *y = b.wrapped(inner_x, *y, inner_w, &format!("Saves: {}", sug.saves), Font::Small, blend(card_rgb, GREEN, 0.75), 3) + b.px(6.0);
        }
        if !sug.evidence.is_empty() {
            *y = b.wrapped(inner_x, *y, inner_w, &sug.evidence, Font::Small, muted, 3) + b.px(6.0);
        }
        *y += b.px(4.0);
        let ph = b.pill_h(Font::SmallStrong);
        let mut x = inner_x;
        let rsi = |action: &str| Target::Rsi {
            suggestion: sug.id.clone(),
            action: action.to_string(),
        };
        match sug.status.as_str() {
            "open" => {
                for (label, action, tone) in [("Send", "send", Tone::Filled(PRIMARY)), ("Dismiss", "dismiss", Tone::Plain(PRIMARY))] {
                    let w = b.pill_w(label, Font::SmallStrong);
                    b.button(Rect { x, y: *y, w, h: ph }, label, tone, rsi(action), ui, card_rgb, true, false, Font::SmallStrong);
                    x += w + b.px(6.0);
                }
            }
            status => {
                let (word, colour) = if status == "sent" { ("Sent", GREEN) } else { ("Dismissed", muted) };
                b.text(x, *y + ((ph - small_lh) / 2.0).round(), word, Font::SmallStrong, colour, Layer::Text);
                x += b.w(word, Font::SmallStrong) + b.px(14.0);
            }
        }
        x += b.px(6.0);
        for (label, action, rating) in [("Useful", "useful", "useful"), ("Not useful", "not", "not")] {
            let on = sug.rating.as_deref() == Some(rating);
            let shown = if on { format!("{ICON_CHECK} {label}") } else { label.to_string() };
            let w = b.pill_w(&shown, Font::SmallStrong);
            if x + w > inner_x + inner_w {
                x = inner_x;
                *y += ph + b.px(6.0);
            }
            b.button(Rect { x, y: *y, w, h: ph }, label, Tone::Chip(if rating == "useful" { GREEN } else { ORANGE }), rsi(action), ui, card_rgb, true, on, Font::SmallStrong);
            x += w + b.px(6.0);
        }
        *y += ph + ipad;
        card(b, said, "suggestion", &sug.id, &sug.title, start, *y, None, card_rgb);
        *y += card_gap;
    };
    if open {
        match reviewing {
            Some(sug) => {
                // One at a time: where he is in the open ones, back, on, and done
                let row_h = b.px(32.0);
                let place = open_ones.iter().position(|s| s.id == sug.id).map_or_else(|| "Reviewed".to_string(), |i| format!("{} of {}", i + 1, open_ones.len()));
                let ty = y + ((row_h - small_lh) / 2.0).round();
                b.text(col.x + b.px(4.0), ty, &format!("Reviewing · {place}"), Font::SmallStrong, PRIMARY, Layer::Text);
                let done_w = b.pill_w("Done", Font::SmallStrong);
                let mut x = col.x + col.w - done_w;
                b.button(Rect { x, y, w: done_w, h: row_h }, "Done", Tone::Plain(PRIMARY), Target::Review("done".to_string()), ui, surface, true, false, Font::SmallStrong);
                for (icon, label, step) in [(ICON_SHUT, "Next", "next"), (ICON_PREVIOUS, "Previous", "previous")] {
                    x -= row_h + b.px(2.0);
                    round_icon(b, Rect { x, y, w: row_h, h: row_h }, icon, label, Target::Review(step.to_string()), ui, surface, b.white(0.75));
                }
                y += row_h + b.px(8.0);
                suggestion(b, said, &mut y, sug);
            }
            None if view.suggestions.is_empty() => match view.failed("suggestions") {
                Some(f) => y = failed_line(b, y, "Can't read the suggestions", f),
                None => {
                    let words = b.fit("None yet: Generate has the Conductor make the report now", col.w - b.px(8.0), Font::Small);
                    b.text(col.x + b.px(4.0), y, &words, Font::Small, b.white(0.42), Layer::Text);
                    y += small_lh + card_gap;
                }
            },
            None => {
                for sug in &view.suggestions {
                    suggestion(b, said, &mut y, sug);
                }
            }
        }
    }
    y += if open { section_gap - card_gap } else { b.px(12.0) };

    // What the Conductor told him, newest first
    if !view.notes.is_empty() {
        let open = heading(b, &mut y, &mut sections, "notes", "Notes", 0, &[]);
        if open {
            let start = y;
            y += b.px(12.0);
            let clock_w = b.w("00:00", Font::Small) + b.px(10.0);
            for note in view.notes.iter().rev() {
                b.text(inner_x, y, &(model.clock)(note.at), Font::Small, muted, Layer::Text);
                y = b.wrapped(inner_x + clock_w, y, inner_w - clock_w, &note.text, Font::Small, blend(card_rgb, WHITE, 0.84), 4) + b.px(8.0);
            }
            y += b.px(4.0);
            card(b, said, "notes", "notes", "Notes", start, y, None, card_rgb);
        }
        y += if open { section_gap } else { b.px(12.0) };
    }
    (y, sections)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::seat::protocol::{parse, Conversation, Incoming, Item};

    /// Roboto-like widths: the small sizes narrower, Medium a little wider, code monospaced, and
    /// a wide character twice its font's
    fn width(font: Font, c: char) -> f64 {
        let base = match font {
            Font::Text => 7.0,
            Font::Strong => 7.5,
            Font::Small => 6.0,
            Font::SmallStrong => 6.5,
            Font::Title => 8.5,
            Font::Code => 7.8,
        };
        if c == '界' {
            base * 2.0
        } else {
            base
        }
    }

    fn height(font: Font) -> f64 {
        match font {
            Font::Text | Font::Strong => 17.0,
            Font::Small | Font::SmallStrong => 14.0,
            Font::Title => 19.0,
            Font::Code => 16.0,
        }
    }

    fn metrics() -> Metrics<'static> {
        Metrics {
            scale: 1.0,
            width: &width,
            height: &height,
            rail: 72.0,
        }
    }

    fn clock(ms: u64) -> String {
        format!("t{}", ms / 1000)
    }

    fn day(ms: u64, _now: u64) -> String {
        format!("day {}", ms / 86_400_000)
    }

    const AREA: Rect = Rect {
        x: 72.0,
        y: 60.0,
        w: 1200.0,
        h: 800.0,
    };

    fn view(json: &str) -> View {
        match parse(json) {
            Some(Incoming::View(v)) => *v,
            other => panic!("{other:?}"),
        }
    }

    fn on_view() -> View {
        view(
            r#"{"type":"seat.view","conductor":{"state":"on","mode":"most","listening":true,"busy":true,"queued":1},
            "asks":[{"id":"ask-3","at":1000,"tool":"SendMessage","title":"Message Publishing in Website","detail":"Push the site now, please.","session":"website-a6","label":"Publishing","project":"Website"}],
            "waiting":[
              {"id":"s1:t1","sessionId":"s1","session":"website-a6","label":"Publishing","project":"Website","kind":"picker","gist":"Which layout?","at":1000,
               "questions":[{"question":"Which layout?","header":"Layout","multiSelect":false,"options":[{"label":"Two columns","description":"Side by side"},{"label":"One"}]}],"answerable":true},
              {"id":"s2:9","sessionId":"s2","session":"terminal-b1","label":null,"project":"Terminal","kind":"text","text":"Q1. Which?\n1a. Red\n1b. Blue","answerable":true}],
            "sessions":[{"sessionId":"s1","name":"website-a6","project":"Website","status":"waiting","label":"Publishing","summary":"Pushing the site","percent":40,"left":240,"agents":[{"id":"a","percent":10}],"conductor":"acting","pane":"17"}],
            "events":[{"seq":1,"at":2000,"kind":"waiting","session":"website-a6","label":"Publishing","project":"Website","text":"asks which layout","wake":true}],
            "suggestions":[{"id":"g1","day":"2026-10-23","project":"Terminal","kind":"tooling","title":"Cache the build","change":"Keep target","status":"open"}],
            "notes":[{"id":"n1","at":3000,"text":"Publishing in Website needs your OK to push"}]}"#,
        )
    }

    fn convo() -> Conversation {
        let mut c = Conversation::default();
        let items = |line: &str| match parse(line) {
            Some(Incoming::Items { items, .. }) => items,
            other => panic!("{other:?}"),
        };
        c.apply(
            true,
            items(
                r#"{"type":"seat.items","reset":true,"items":[
                {"id":"a","at":10000,"who":"you","from":"lbterm","text":"what's waiting on me?"},
                {"id":"t1","at":11000,"who":"tool","tool":{"name":"fleet","text":"fleet","status":"done"}},
                {"id":"t2","at":12000,"who":"tool","tool":{"name":"read","text":"read: website-a6","status":"running"}},
                {"id":"r","at":13000,"who":"conductor","text":"Two things:\n- **website-a6** wants `Allow`\n- [Publishing](lbterm://session/s1) in Website asks"},
                {"id":"n","at":14000,"who":"notice","text":"Accept most from now on (Lachlan, LBTerm)"},
                {"id":"q","at":15000,"who":"you","from":"phone","text":"and later","queued":true}]}"#,
            ),
        );
        c
    }

    fn model<'a>(v: Option<&'a View>, shown: &'a [Shown<'a>], connected: bool) -> Model<'a> {
        Model {
            connected,
            pipe: r"\\.\pipe\conductor-seat",
            view: v,
            items: shown,
            now_ms: 100_000,
            clock,
            day,
        }
    }

    fn scene(v: Option<&View>, c: &Conversation, ui: &mut Ui, connected: bool) -> Scene {
        let shown = c.shown();
        lay_out(AREA, &metrics(), 0x121212, &model(v, &shown, connected), ui)
    }

    fn button<'s>(s: &'s Scene, name: &str) -> &'s state::ViewButton {
        s.said.buttons.iter().find(|b| b.name == name).unwrap_or_else(|| {
            panic!("no {name}: {:?}", s.said.buttons.iter().map(|b| &b.name).collect::<Vec<_>>())
        })
    }

    fn centre(r: state::Rect) -> (f64, f64) {
        (r.x as f64 + r.w as f64 / 2.0, r.y as f64 + r.h as f64 / 2.0)
    }

    fn inside(outer: Rect, r: &Prim) -> bool {
        let rect = match r {
            Prim::Fill { rect, .. } | Prim::Outline { rect, .. } => *rect,
            Prim::Text { x, y, w, font, .. } => Rect { x: *x, y: *y, w: *w - 4.0, h: height(*font) },
        };
        rect.x >= outer.x - 0.5 && rect.y >= outer.y - 0.5 && rect.x + rect.w <= outer.x + outer.w + 0.5 && rect.y + rect.h <= outer.y + outer.h + 0.5
    }

    fn drawn(s: &Scene) -> Vec<&str> {
        s.prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn everything_is_inside_the_view_and_every_button_lands_on_itself() {
        let v = on_view();
        let c = convo();
        let mut ui = Ui::default();
        let s = scene(Some(&v), &c, &mut ui, true);
        assert!(s.prims.iter().all(|p| inside(AREA, p)), "nothing drawn outside the view");
        assert!(s.side.is_some(), "a wide view has its side column");
        // Each button's middle lands on it, not something laid over it
        for b in s.said.buttons.iter().filter(|b| b.enabled && !b.pending) {
            let Some(r) = b.rect else { continue };
            let (x, y) = centre(r);
            assert_eq!(s.hit(x, y).map(Target::name).as_deref(), Some(b.name.as_str()), "{}", b.name);
        }
        for name in ["control:mode read", "control:mode most", "control:mode all", "control:listen off", "control:restart", "control:off", "send", "stop", "allow:ask-3", "deny:ask-3", "pick:s1:t1:0:0", "other:s1:t1:0", "chip:s2:9:1a", "chip:s2:9:1b", "session:s1", "rsi:g1:send", "rsi:g1:dismiss", "rsi:g1:useful", "rsi:g1:not"] {
            button(&s, name);
        }
        assert!(button(&s, "control:mode most").on, "the mode in force is lit");
        assert!(!button(&s, "send").enabled, "nothing to send yet");
        assert!(!button(&s, "answer:s2:9").enabled, "an empty answer can't be sent");
        // One question, one pick: its options answer at once, no Send answer button
        assert!(s.said.buttons.iter().all(|b| b.name != "answer:s1:t1"));
        assert_eq!(s.said.state, "on");
        assert!(s.said.busy);
        let kinds: Vec<&str> = s.said.cards.iter().map(|c| c.kind.as_str()).collect();
        assert_eq!(kinds, ["ask", "waiting", "waiting", "session", "events", "suggestion", "notes"]);
    }

    #[test]
    fn sessions_are_named_by_their_labels_never_by_their_messaging_names() {
        let v = on_view();
        let c = Conversation::default();
        let mut ui = Ui::default();
        // Tall enough that every card is drawn
        let s = lay_out(Rect { h: 3000.0, ..AREA }, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        let drawn = drawn(&s);
        for name in ["website-a6", "terminal-b1"] {
            assert!(drawn.iter().all(|t| !t.contains(name)), "{name} is drawn: {drawn:?}");
        }
        let has = |words: &str| drawn.iter().any(|t| t.contains(words));
        assert!(drawn.contains(&"Message Publishing in Website"), "the ask's title: {drawn:?}");
        assert!(has("SendMessage · t1 · 2 min ago"), "the ask's meta, without the session its title names: {drawn:?}");
        assert!(has("Publishing · Website"), "the event's session");
        assert!(drawn.contains(&"new session"), "a label not yet given: {drawn:?}");
        assert!(drawn.contains(&"Publishing"), "the session's row leads with its label");
        assert!(has("Pushing the site"), "its summary under it");
        assert_eq!(s.said.cards.iter().find(|c| c.kind == "session").map(|c| c.title.as_str()), Some("Publishing"));
        assert_eq!(called(None), "new session");
        assert_eq!(called(Some("  ")), "new session");
    }

    #[test]
    fn a_part_the_seat_couldnt_read_says_so_in_the_warning_colour_where_its_list_would_be() {
        use crate::seat::protocol::Failing;
        let mut v = on_view();
        // As the seat sends it: waiting and suggestions empty, the sessions as last read
        v.waiting.clear();
        v.suggestions.clear();
        v.failing = vec![
            Failing { part: "waiting".into(), error: "Relay's openQuestions didn't load".into(), since: 40_000 },
            Failing { part: "sessions".into(), error: "the ledger is locked".into(), since: 0 },
            Failing { part: "suggestions".into(), error: "the report can't be read".into(), since: 70_000 },
        ];
        let c = Conversation::default();
        let mut ui = Ui::default();
        let s = lay_out(Rect { h: 3000.0, ..AREA }, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        let lines: Vec<(&str, u32)> = s
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, rgb, .. } => Some((text.as_str(), *rgb)),
                _ => None,
            })
            .collect();
        let warning = blend(0x121212, ORANGE, 0.85);
        let said = |start: &str| lines.iter().find(|(t, _)| t.starts_with(start)).copied();
        // The waiting section shows though nobody's in it, with what went wrong and since when
        assert!(lines.iter().any(|(t, _)| *t == "Waiting on you"), "{lines:?}");
        let (waiting, colour) = said("Can't read who's waiting on you, so nobody shows here").expect("the waiting line");
        assert_eq!(colour, warning);
        let all: String = lines.iter().map(|(t, _)| *t).collect::<Vec<_>>().join(" ");
        assert!(all.contains("Relay's openQuestions didn't load · since t40 · 1 min ago"), "{waiting}: {all}");
        // The sessions say they're as last read, and no time when the seat sent none
        // (Its words wrap, so they're read joined)
        let (_, colour) = said("Can't read the sessions, so these are as last read").expect("the sessions line");
        assert_eq!(colour, warning);
        assert!(all.contains("so these are as last read: the ledger is locked"), "{all}");
        assert!(!all.contains("the ledger is locked · since"), "no time when the seat sent none: {all}");
        assert!(lines.iter().all(|(t, _)| *t != "No other sessions"));
        assert!(said("Publishing").is_some(), "the sessions as last read are still drawn");
        // The suggestions' failure in place of "None yet"
        assert_eq!(said("Can't read the suggestions").map(|(_, c)| c), Some(warning));
        assert!(said("None yet").is_none());

        // Read again: the lines go and the lists say what they did before
        v.failing.clear();
        let s = lay_out(Rect { h: 3000.0, ..AREA }, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        let drawn = drawn(&s);
        assert!(drawn.iter().all(|t| !t.starts_with("Can't read")), "{drawn:?}");
        assert!(drawn.iter().all(|t| *t != "Waiting on you"), "an empty waiting list read whole has no section");
        assert!(drawn.iter().any(|t| t.starts_with("None yet")));
    }

    #[test]
    fn a_failing_part_reads_from_the_seats_view_and_is_found_by_its_part() {
        let v = parse(r#"{"type":"seat.view","seq":1,"at":1,"failing":[{"part":"waiting","error":"no module","since":5000},{"error":"no part"}]}"#);
        let Some(Incoming::View(v)) = v else { panic!("a view") };
        assert_eq!(v.failing.len(), 1, "an entry without its part is left out");
        assert_eq!(v.failed("waiting").map(|f| (f.error.as_str(), f.since)), Some(("no module", 5000)));
        assert!(v.failed("sessions").is_none());
    }

    #[test]
    fn a_message_from_another_session_names_it_as_he_knows_it_never_by_its_messaging_name() {
        assert_eq!(peer_label(Some("Site publish"), Some("Website")), "From Site publish · Website");
        assert_eq!(peer_label(Some("Site publish"), None), "From Site publish");
        assert_eq!(peer_label(None, Some("Website")), "From a new session · Website");
        assert_eq!(peer_label(Some(" "), Some("")), "From another session");
        assert_eq!(peer_label(None, None), "From another session");
    }

    #[test]
    fn the_conversation_reads_in_order_with_his_words_on_the_right() {
        let v = on_view();
        let c = convo();
        let mut ui = Ui::default();
        let s = scene(Some(&v), &c, &mut ui, true);
        let ids: Vec<&str> = s.said.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["a", "t1", "t2", "r", "n", "q"], "his queued words last");
        let rect = |id: &str| s.said.items.iter().find(|i| i.id == id).and_then(|i| i.rect).unwrap();
        for pair in ids.windows(2) {
            assert!(rect(pair[0]).y < rect(pair[1]).y, "{} above {}", pair[0], pair[1]);
        }
        // His bubble sits right, the Conductor's words from the left
        let bubble = s
            .prims
            .iter()
            .find_map(|p| match p {
                Prim::Text { text, x, .. } if text == "what's waiting on me?" => Some(*x),
                _ => None,
            })
            .unwrap();
        assert!(bubble > s.conversation.x + s.conversation.w / 2.0);
        let bold = s.prims.iter().any(|p| matches!(p, Prim::Text { text, font: Font::Strong, .. } if text == "website-a6"));
        assert!(bold, "**website-a6** in bold");
        let drawn = drawn(&s);
        assert!(drawn.iter().any(|t| t.starts_with("Queued") && t.ends_with("From phone · t15")), "queued, where he typed it, and when: {drawn:?}");
        assert!(drawn.iter().any(|t| *t == "t10"), "his words' time");
        assert!(drawn.contains(&ICON_LOADING), "a running tool call");
        assert!(drawn.contains(&"read"), "a tool call's name apart from its words");
        let tool_words = s.prims.iter().any(|p| matches!(p, Prim::Text { text, font: Font::Small, .. } if text == "Publishing"));
        assert!(tool_words, "\"read: website-a6\" names the session by its label: {drawn:?}");
    }

    #[test]
    fn every_message_says_when_and_each_day_its_name() {
        let mut c = Conversation::default();
        let item = |id: &str, at: u64, who: Who, text: &str| Item {
            id: id.into(),
            at,
            who,
            from: None,
            label: None,
            project: None,
            text: text.into(),
            queued: false,
            tool: None,
        };
        let day_ms = 86_400_000;
        c.apply(
            true,
            vec![
                item("y1", day_ms + 5_000, Who::You, "first"),
                item("r1", day_ms + 6_000, Who::Conductor, "one"),
                item("r2", day_ms + 7_000, Who::Conductor, "two"),
                item("y2", 2 * day_ms + 8_000, Who::You, "next day"),
                item("r3", 2 * day_ms + 9_000, Who::Conductor, "three"),
            ],
        );
        let mut ui = Ui::default();
        let s = scene(Some(&on_view()), &c, &mut ui, true);
        let drawn = drawn(&s);
        assert!(drawn.contains(&"day 1") && drawn.contains(&"day 2"), "a name for each day: {drawn:?}");
        // His words with their time; the Conductor's turn once under its name, with the time
        // it started
        assert!(drawn.contains(&"t86405") && drawn.contains(&"t172808"), "{drawn:?}");
        assert_eq!(drawn.iter().filter(|t| **t == "Conductor").count(), 3, "the title and two turns' headers: {drawn:?}");
        assert!(drawn.contains(&"t86406") && !drawn.contains(&"t86407"), "a reply in the same turn has no header of its own");
    }

    #[test]
    fn a_link_to_a_session_shows_its_name_and_opens_its_tab() {
        let v = on_view();
        let c = convo();
        let mut ui = Ui::default();
        let s = scene(Some(&v), &c, &mut ui, true);
        let drawn = drawn(&s);
        assert!(drawn.iter().all(|t| !t.contains("lbterm://")), "no link's target drawn: {drawn:?}");
        let (x, y) = s
            .prims
            .iter()
            .find_map(|p| match p {
                Prim::Text { text, x, y, rgb, .. } if text == "Publishing" && *rgb == PRIMARY => Some((*x + 3.0, *y + 4.0)),
                _ => None,
            })
            .expect("the link drawn in Material's blue");
        assert_eq!(s.hit(x, y).map(Target::name).as_deref(), Some("session:s1"));
    }

    #[test]
    fn a_hub_wake_shows_what_happened_without_the_conductors_instructions() {
        let mut c = Conversation::default();
        c.apply(
            true,
            vec![Item {
                id: "h".into(),
                at: 50_000,
                who: Who::Hub,
                from: None,
                label: None,
                project: None,
                text: "Conductor hub: 1 thing since you last looked.\n- 19:17 Publishing in Website is doing something outward (git push)\nUse read <session> for detail.\nYour mode: accept most.".into(),
                queued: false,
                tool: None,
            }],
        );
        let mut ui = Ui::default();
        let s = scene(Some(&on_view()), &c, &mut ui, true);
        let drawn = drawn(&s);
        assert!(drawn.contains(&"1 thing since you last looked") && drawn.contains(&"19:17"), "{drawn:?}");
        assert!(drawn.iter().any(|t| t.starts_with("Publishing in Website is doing")));
        assert!(drawn.iter().all(|t| !t.contains("Your mode") && !t.contains("Use read")), "{drawn:?}");
    }

    #[test]
    fn a_notice_that_a_session_needs_him_names_it_and_opens_its_tab() {
        let mut c = Conversation::default();
        c.apply(
            true,
            vec![Item {
                id: "n".into(),
                at: 50_000,
                who: Who::Notice,
                from: None,
                label: None,
                project: None,
                text: "Publishing in Website needs you: Pick a layout. It built both.".into(),
                queued: false,
                tool: None,
            }],
        );
        let mut ui = Ui::default();
        let s = scene(Some(&on_view()), &c, &mut ui, true);
        let drawn = drawn(&s);
        assert!(drawn.contains(&"Pick a layout") && drawn.contains(&"It built both."), "{drawn:?}");
        let (x, y) = s
            .prims
            .iter()
            .find_map(|p| match p {
                Prim::Text { text, x, y, font: Font::SmallStrong, .. } if text == "Publishing" => Some((*x + 3.0, *y + 4.0)),
                _ => None,
            })
            .expect("its name");
        assert_eq!(s.hit(x, y).map(Target::name).as_deref(), Some("session:s1"));
    }

    #[test]
    fn a_long_message_from_another_session_shows_more_when_asked() {
        let mut c = Conversation::default();
        let text = (0..20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        c.apply(
            true,
            vec![Item {
                id: "p".into(),
                at: 50_000,
                who: Who::Peer,
                from: Some("website-a6".into()),
                label: Some("Publishing".into()),
                project: Some("Website".into()),
                text,
                queued: false,
                tool: None,
            }],
        );
        let mut ui = Ui::default();
        let s = scene(Some(&on_view()), &c, &mut ui, true);
        assert!(button(&s, "more:p").enabled);
        assert!(!drawn(&s).contains(&"line 19"), "cut short");
        ui.expanded.insert("p".into());
        let s = lay_out(Rect { h: 3000.0, ..AREA }, &metrics(), 0x121212, &model(Some(&on_view()), &c.shown(), true), &mut ui);
        assert!(drawn(&s).contains(&"line 19"), "shown whole");
        assert!(drawn(&s).iter().all(|t| !t.contains("website-a6")), "never its messaging name");
    }

    #[test]
    fn a_long_conversation_scrolls_and_keeps_its_place() {
        let mut c = Conversation::default();
        let many: Vec<Item> = (0..80)
            .map(|i| Item {
                id: format!("i{i}"),
                at: 0,
                who: Who::Conductor,
                from: None,
                label: None,
                project: None,
                text: format!("line {i}"),
                queued: false,
                tool: None,
            })
            .collect();
        c.apply(true, many);
        let v = on_view();
        let mut ui = Ui::default();
        let s = scene(Some(&v), &c, &mut ui, true);
        assert!(s.scroll_max > 0.0);
        // At the newest: the last item shows, the first doesn't
        let shows = |s: &Scene, id: &str| s.said.items.iter().find(|i| i.id == id).and_then(|i| i.rect).is_some();
        assert!(shows(&s, "i79") && !shows(&s, "i0"));
        assert!(s.said.buttons.iter().all(|b| b.name != "newest"));
        ui.scroll = 1e9;
        let s = scene(Some(&v), &c, &mut ui, true);
        assert_eq!(ui.scroll, s.scroll_max, "scrolled no further than the top");
        assert!(shows(&s, "i0") && !shows(&s, "i79"));
        button(&s, "newest");
        // No words drawn outside the conversation's room
        let conv = s.conversation;
        for p in &s.prims {
            if let Prim::Text { text, y, .. } = p {
                if text.starts_with("line ") {
                    assert!(*y >= conv.y && *y + 17.0 <= conv.y + conv.h + 0.5, "{text} at {y}");
                }
            }
        }
        // New words below while scrolled up: what he reads stays put
        let before = s.said.items[0].rect;
        c.apply(false, vec![Item { id: "new".into(), at: 0, who: Who::Conductor, from: None, label: None, project: None, text: "more".into(), queued: false, tool: None }]);
        let s = scene(Some(&v), &c, &mut ui, true);
        assert_eq!(s.said.items[0].rect, before);
    }

    #[test]
    fn a_selection_copies_lines_and_runs_wrapped_ones_on() {
        let mut c = Conversation::default();
        c.apply(
            true,
            vec![Item {
                id: "r".into(),
                at: 0,
                who: Who::Conductor,
                from: None,
                label: None,
                project: None,
                text: "first line\nsecond".into(),
                queued: false,
                tool: None,
            }],
        );
        let mut ui = Ui::default();
        let s = scene(Some(&on_view()), &c, &mut ui, true);
        let first = &s.lines[0];
        assert_eq!(first.xs.len(), "first line".len() + 1);
        let a = s.spot(first.xs[6] + 1.0, first.y + 2.0).unwrap();
        assert_eq!(a, Spot { line: 0, col: 6 });
        let second = &s.lines[1];
        let z = s.spot(second.xs[3] - 1.0, second.y + 2.0).unwrap();
        assert_eq!(s.selected(a, z), "line\nsec");
        assert_eq!(s.selected(z, a), "line\nsec", "either way round");
        ui.selection = Some((a, z));
        let s = scene(Some(&on_view()), &c, &mut ui, true);
        assert_eq!(s.said.selection.as_deref(), Some("line\nsec"));
    }

    #[test]
    fn off_or_without_the_seat_it_says_so() {
        let c = Conversation::default();
        let mut ui = Ui::default();
        let s = scene(None, &c, &mut ui, false);
        assert_eq!(s.said.state, "unknown");
        assert!(s.said.buttons.iter().all(|b| !b.name.starts_with("control:")), "no controls without the seat");
        assert!(s.prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text.contains("conductor-seat"))));
        let off = view(r#"{"type":"seat.view","conductor":{"state":"off"}}"#);
        let s = scene(Some(&off), &c, &mut ui, true);
        assert!(button(&s, "control:on").enabled);
        assert!(s.said.buttons.iter().all(|b| b.name != "control:off" && b.name != "stop"));
        ui.main.insert("hello");
        let s = scene(Some(&off), &c, &mut ui, true);
        assert!(!button(&s, "send").enabled, "off, nothing can be sent");
    }

    #[test]
    fn picks_and_answers() {
        let v = on_view();
        let mut ui = Ui::default();
        let picker = &v.waiting[0];
        assert_eq!(ui.answer(picker), None);
        ui.pick(picker, 0, 1);
        assert_eq!(ui.answer(picker), Some((vec![vec![2]], vec![None], None)));
        ui.pick(picker, 0, 0);
        assert_eq!(ui.answer(picker).unwrap().0, vec![vec![1]], "one pick: the new one replaces it");
        ui.pick(picker, 0, 0);
        assert_eq!(ui.answer(picker), None, "picked again: unpicked");
        // Words of his own
        ui.others.insert((picker.id.clone(), 0));
        ui.input(&BoxKey::Answer { waiting: picker.id.clone(), question: Some(0) }).insert(" mine ");
        assert_eq!(ui.answer(picker), Some((vec![vec![]], vec![Some("mine".into())], None)));
        let text = &v.waiting[1];
        assert_eq!(ui.answer(text), None);
        ui.input(&BoxKey::Answer { waiting: text.id.clone(), question: None }).insert("1b");
        assert_eq!(ui.answer(text), Some((vec![], vec![], Some("1b".into()))));
        let mut c = Conversation::default();
        c.apply(true, vec![]);
        let s = lay_out(Rect { h: 3000.0, ..AREA }, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        assert!(button(&s, "answer:s2:9").enabled);
        assert!(button(&s, "answer:s1:t1").enabled, "with Other open, the picker sends with its button");
        // A question that's gone takes its picks and boxes with it
        let gone = view(r#"{"type":"seat.view","conductor":{"state":"on"}}"#);
        ui.focus = BoxKey::Answer { waiting: text.id.clone(), question: None };
        ui.forget_gone(&gone);
        assert!(ui.picks.is_empty() && ui.others.is_empty() && ui.boxes.is_empty());
        assert_eq!(ui.focus, BoxKey::Main);
    }

    #[test]
    fn the_box_grows_with_its_lines_and_shows_the_caret() {
        let v = on_view();
        let c = Conversation::default();
        let mut ui = Ui::default();
        let s = scene(Some(&v), &c, &mut ui, true);
        let one_line = s.box_place(&BoxKey::Main).unwrap().text;
        assert!(s.caret.is_some());
        ui.main.insert("a\nb\nc");
        let s = scene(Some(&v), &c, &mut ui, true);
        let three = s.box_place(&BoxKey::Main).unwrap().text;
        assert_eq!(three.h, 3.0 * one_line.h);
        assert!(three.y < one_line.y, "it grows upward");
        ui.main.insert(&"\nmore".repeat(20));
        let s = scene(Some(&v), &c, &mut ui, true);
        let place = s.box_place(&BoxKey::Main).unwrap();
        assert_eq!(place.text.h, BOX_LINES as f64 * 22.0, "at most eight lines");
        assert_eq!(place.first, place.lines.len() - BOX_LINES, "scrolled to the caret at the end");
        assert!(button(&s, "send").enabled);
        assert_eq!(s.said.boxes[0].name, "main");
        assert!(s.said.boxes[0].focused);
    }

    #[test]
    fn a_narrow_view_folds_its_side_to_the_rails_width_and_keeps_it_there() {
        let v = on_view();
        let c = convo();
        let mut ui = Ui::default();
        let shown = c.shown();
        let narrow = Rect { w: 600.0, ..AREA };
        let s = lay_out(narrow, &metrics(), 0x121212, &model(Some(&v), &shown, true), &mut ui);
        assert_eq!(s.side.map(|r| r.w), Some(72.0), "as wide as the tab rail");
        assert!(s.prims.iter().all(|p| inside(narrow, p)));
        assert!(s.said.buttons.iter().all(|b| b.name != "fold-side"), "no room to open it");
        let ask = button(&s, "reveal:asks");
        assert!(ask.on && !ask.enabled, "what needs him shows, lit, but opens nothing");
        assert!(drawn(&s).contains(&"1 ask"));
    }

    #[test]
    fn the_side_folds_to_a_rail_and_opens_at_a_section() {
        let v = on_view();
        let c = convo();
        let mut ui = Ui::default();
        let s = scene(Some(&v), &c, &mut ui, true);
        let wide = s.conversation.w;
        button(&s, "fold-side");
        let stats = |s: &Scene| drawn(s).iter().filter(|t| t.ends_with(" working") || t.ends_with(" waiting")).map(|t| t.to_string()).collect::<Vec<_>>();
        assert!(stats(&s).iter().any(|t| t == "1 waiting"), "its header says how the sessions stand: {:?}", stats(&s));
        ui.side_folded = true;
        let s = scene(Some(&v), &c, &mut ui, true);
        assert_eq!(s.side.map(|r| r.w), Some(72.0));
        assert!(s.conversation.w > wide + 200.0, "the conversation takes the room");
        for name in ["fold-side", "reveal:asks", "reveal:waiting", "reveal:sessions:working", "reveal:sessions:idle", "reveal:events", "reveal:suggestions", "reveal:notes"] {
            assert!(button(&s, name).enabled, "{name}");
        }
        assert!(drawn(&s).contains(&"2 waiting") && drawn(&s).contains(&"1 idea"), "{:?}", drawn(&s));
        assert!(s.prims.iter().all(|p| inside(AREA, p)));
    }

    #[test]
    fn a_sections_heading_folds_it_and_the_sessions_group_by_status() {
        let v = view(
            r#"{"type":"seat.view","conductor":{"state":"on"},
            "sessions":[{"sessionId":"w","project":"Website","status":"waiting","label":"Publishing","summary":"Asks about layout"},
                        {"sessionId":"k","project":"Terminal","status":"working","label":"Pad colours","summary":"Building"},
                        {"sessionId":"i","project":"Terminal","status":"idle","label":"Quiet one","summary":"Landed the fix"},
                        {"sessionId":"x","project":"Terminal","status":"failed","label":"Broke","summary":""}],
            "suggestions":[{"id":"g1","title":"Cache the build","status":"open"}]}"#,
        );
        let c = Conversation::default();
        let mut ui = Ui::default();
        let tall = Rect { h: 2000.0, ..AREA };
        let s = lay_out(tall, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        let d = drawn(&s);
        for word in ["Waiting", "Working", "Idle", "Other"] {
            assert!(d.contains(&word), "{word}: {d:?}");
        }
        assert!(d.contains(&"Landed the fix"), "an idle session's line carries what it did");
        let y = |s: &Scene, text: &str| s.prims.iter().find_map(|p| match p {
            Prim::Text { text: t, y, .. } if t == text => Some(*y),
            _ => None,
        });
        assert!(y(&s, "Publishing") < y(&s, "Pad colours") && y(&s, "Pad colours") < y(&s, "Quiet one") && y(&s, "Quiet one") < y(&s, "Broke"));
        assert!(ui.sections.iter().any(|(n, _)| n == "sessions:idle"), "where each section is, for the rail");
        ui.folded.insert("sessions:idle".into());
        ui.folded.insert("suggestions".into());
        let s = lay_out(tall, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        let d = drawn(&s);
        assert!(!d.contains(&"Quiet one") && d.contains(&"Pad colours"), "the idle group folded, the rest not");
        assert!(!d.contains(&"Cache the build"), "the suggestions folded to their heading");
        assert!(!button(&s, "section:suggestions").on && button(&s, "section:sessions").on);
    }

    #[test]
    fn sessions_hidden_from_the_conductor_fold_into_a_group_at_the_foot_with_show() {
        let v = view(
            r#"{"type":"seat.view","conductor":{"state":"on"},
            "sessions":[{"sessionId":"w","project":"Website","status":"waiting","label":"Publishing","summary":"Asks about layout"},
                        {"sessionId":"h1","project":"Terminal","status":"waiting","label":"Mine to answer","summary":"","question":"Which pad?","hidden":true},
                        {"sessionId":"h2","project":"Terminal","status":"working","label":"Left alone","summary":"Building","hidden":true},
                        {"sessionId":"k","project":"Terminal","status":"working","label":"Pad colours","summary":"Building"}]}"#,
        );
        let c = Conversation::default();
        let mut ui = Ui::default();
        let tall = Rect { h: 2000.0, ..AREA };
        let s = lay_out(tall, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        let d = drawn(&s);
        // Folded at first: its heading, its count, and in orange how many wait on him; its
        // sessions in no other group, and not counted with their statuses
        assert!(d.contains(&"Hidden") && d.contains(&"· 2") && d.contains(&"1 waiting on you"), "{d:?}");
        assert!(!d.contains(&"Mine to answer") && !d.contains(&"Left alone"), "folded: {d:?}");
        assert!(!button(&s, "section:sessions:hidden").on, "folded at first");
        let orange = s.prims.iter().any(|p| matches!(p, Prim::Text { text, rgb, .. } if text == "1 waiting on you" && *rgb == ORANGE));
        assert!(orange, "how many wait on him, in orange");
        let n = |word: &str| d.iter().position(|t| *t == word);
        assert!(n("Waiting") < n("Working") && n("Working") < n("Hidden"), "at the foot: {d:?}");
        assert!(d.contains(&"1 waiting") && d.contains(&"1 working"), "the side's head counts only what's not hidden: {d:?}");
        assert!(s.said.buttons.iter().all(|b| !b.name.starts_with("show:")), "no Show while folded");
        // A press opens it: each hidden session's line with Show, a press on which is Show's
        ui.folded.insert("sessions:hidden".into());
        assert!(!ui.is_folded("sessions:hidden"));
        let s = lay_out(tall, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        let d = drawn(&s);
        assert!(d.contains(&"Mine to answer") && d.contains(&"Left alone") && d.contains(&"Asks: Which pad?"), "{d:?}");
        assert!(button(&s, "section:sessions:hidden").on);
        for (sid, label) in [("h1", "Mine to answer"), ("h2", "Left alone")] {
            let b = button(&s, &format!("show:{sid}"));
            assert_eq!(b.label, "Show");
            let r = b.rect.unwrap();
            let target = s.hit(r.x as f64 + r.w as f64 / 2.0, r.y as f64 + r.h as f64 / 2.0);
            assert_eq!(target, Some(&Target::Show { session_id: sid.into(), label: label.into() }));
        }
        let kinds: Vec<&str> = s.said.cards.iter().map(|c| c.kind.as_str()).collect();
        assert_eq!(kinds.iter().filter(|k| **k == "hidden").count(), 2, "{kinds:?}");
        assert_eq!(kinds.iter().filter(|k| **k == "session").count(), 2, "{kinds:?}");
        // With none hidden, no group
        let none = view(r#"{"type":"seat.view","conductor":{"state":"on"},"sessions":[{"sessionId":"k","status":"working","label":"Pad colours"}]}"#);
        let s = lay_out(tall, &metrics(), 0x121212, &model(Some(&none), &c.shown(), true), &mut Ui::default());
        assert!(!drawn(&s).contains(&"Hidden"));
    }

    #[test]
    fn suggestions_are_reviewed_one_at_a_time_and_made_at_a_press() {
        let v = view(
            r#"{"type":"seat.view","conductor":{"state":"on"},
            "suggestions":[{"id":"g1","title":"Cache the build","status":"open"},{"id":"g2","title":"Warm the slot","status":"open"},{"id":"g3","title":"Old one","status":"sent"}]}"#,
        );
        let c = Conversation::default();
        let mut ui = Ui::default();
        let tall = Rect { h: 2000.0, ..AREA };
        let s = lay_out(tall, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        assert!(button(&s, "review:start").enabled);
        assert!(button(&s, "rsi::run").enabled, "Generate");
        assert!(drawn(&s).contains(&"Warm the slot") && drawn(&s).contains(&"Old one"));
        ui.review = Some("g2".into());
        let s = lay_out(tall, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        let d = drawn(&s);
        assert!(d.contains(&"Reviewing · 2 of 2"), "{d:?}");
        assert!(d.contains(&"Warm the slot") && !d.contains(&"Cache the build"), "only the one being reviewed");
        for name in ["review:next", "review:previous", "review:done", "rsi:g2:send"] {
            button(&s, name);
        }
        assert!(s.said.buttons.iter().all(|b| b.name != "rsi::run"), "no Generate while reviewing");
        // While the report's being made, Generate says so and can't be pressed again
        let running = view(r#"{"type":"seat.view","conductor":{"state":"on"},"rsi":{"running":true},"suggestions":[]}"#);
        ui.review = None;
        let s = lay_out(tall, &metrics(), 0x121212, &model(Some(&running), &c.shown(), true), &mut ui);
        assert!(!button(&s, "rsi::run").enabled);
        assert!(drawn(&s).iter().any(|t| t.contains("Generating")));
    }

    #[test]
    fn a_question_in_texts_extras_get_a_yes_and_a_no() {
        let mut v = on_view();
        v.waiting[1].text = Some("Q1. Which?\n1a. Red\n1b. Blue\n\nExtras, yes or no each:\nE2. A test for it".into());
        let c = Conversation::default();
        let mut ui = Ui::default();
        let tall = Rect { h: 3000.0, ..AREA };
        let s = lay_out(tall, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        assert!(!button(&s, "chip:s2:9:E2+").on && !button(&s, "chip:s2:9:E2-").on);
        ui.input(&BoxKey::Answer { waiting: "s2:9".into(), question: None }).set("1b E2+");
        let s = lay_out(tall, &metrics(), 0x121212, &model(Some(&v), &c.shown(), true), &mut ui);
        assert!(button(&s, "chip:s2:9:E2+").on, "its yes lit while the answer holds it");
        assert!(button(&s, "chip:s2:9:1b").on && !button(&s, "chip:s2:9:1a").on, "an option's chip too");
    }

    #[test]
    fn a_run_of_alerts_folds_to_one_line_until_opened() {
        let mut c = Conversation::default();
        let notice = |id: &str, at: u64, text: &str| Item {
            id: id.into(),
            at,
            who: Who::Notice,
            from: None,
            label: None,
            project: None,
            text: text.into(),
            queued: false,
            tool: None,
        };
        c.apply(
            true,
            vec![
                notice("a1", 60_000, "Publishing in Website needs you: Pick a layout"),
                notice("a2", 120_000, "Pad colours in Terminal needs you: Pick a colour"),
                notice("n", 130_000, "Accept most from now on (Lachlan, LBTerm)"),
                notice("a3", 180_000, "Publishing in Website needs you: Push it"),
                notice("a4", 240_000, "Quiet one in Terminal needs you: Restart"),
            ],
        );
        let mut ui = Ui::default();
        let s = scene(Some(&on_view()), &c, &mut ui, true);
        let d = drawn(&s);
        assert!(d.contains(&"4 alerts from 3 sessions"), "{d:?}");
        assert!(d.contains(&"Publishing, Pad colours, Quiet one") && d.contains(&"t60 – t240"), "{d:?}");
        assert!(!d.contains(&"Pick a layout"), "folded");
        assert_eq!(button(&s, "more:a1").label, "Show all");
        ui.expanded.insert("a1".into());
        let s = scene(Some(&on_view()), &c, &mut ui, true);
        let d = drawn(&s);
        assert!(d.contains(&"Pick a layout") && d.contains(&"Restart"), "opened: {d:?}");
        assert_eq!(button(&s, "more:a1").label, "Fold");
        // Two alerts aren't a run
        let mut c = Conversation::default();
        c.apply(true, vec![notice("b1", 60_000, "Publishing in Website needs you: Pick"), notice("b2", 120_000, "Pad colours in Terminal needs you: Pick")]);
        let s = scene(Some(&on_view()), &c, &mut Ui::default(), true);
        assert!(s.said.buttons.iter().all(|b| !b.name.starts_with("more:")));
    }

    #[test]
    fn the_controls_wrap_rather_than_go() {
        let v = on_view();
        let c = convo();
        let mut ui = Ui::default();
        let shown = c.shown();
        let narrow = Rect { w: 700.0, ..AREA };
        let s = lay_out(narrow, &metrics(), 0x121212, &model(Some(&v), &shown, true), &mut ui);
        assert!(s.side.is_some(), "700 px keeps its side column");
        let controls: Vec<_> = s.said.buttons.iter().filter(|b| b.name.starts_with("control:")).collect();
        assert_eq!(controls.len(), 6, "{:?}", controls.iter().map(|b| &b.name).collect::<Vec<_>>());
        let main_right = s.conversation.x + s.conversation.w;
        for b in &controls {
            let r = b.rect.unwrap_or_else(|| panic!("{} out of sight", b.name));
            assert!((r.x + r.w as i32) as f64 <= main_right + 1.0, "{} past the main column", b.name);
        }
        let rows: std::collections::BTreeSet<i32> = controls.iter().map(|b| b.rect.unwrap().y).collect();
        assert!(rows.len() > 1, "they took two rows");
        assert!(s.conversation.y > controls.iter().map(|b| b.rect.unwrap().y).max().unwrap() as f64, "the conversation under them");
    }

    #[test]
    fn times_and_ages_read_shortly() {
        assert_eq!(ago(100_000, 99_000), "now");
        assert_eq!(ago(10 * 60_000, 0), "", "no time, no age");
        assert_eq!(ago(10 * 60_000, 60_000), "9m");
        assert_eq!(ago(5 * 3_600_000, 1), "4h");
        assert_eq!(ago_words(10 * 60_000, 60_000), "9 min ago");
        assert_eq!(ago_words(100_000, 99_000), "just now");
        assert_eq!(ago_words(5 * 3_600_000, 1), "4 h ago");
        assert_eq!(left(30), "~1 min");
        assert_eq!(left(240), "~4 min");
        assert_eq!(left(7200), "~2 h");
        assert_eq!(blend(0x000000, 0xFFFFFF, 0.5), 0x808080);
    }

    #[test]
    fn ages_change_when_their_words_do() {
        // Each age's next change, in seconds after its time: "now" to 1m at 45 s, a minute on
        // half a minute either side of each, 60m to 1h on the hour, the days on the day
        let at = 1_000_000;
        for (s, next) in [
            (0, 45),
            (44, 45),
            (45, 90),
            (89, 90),
            (90, 150),
            (3569, 3570),
            (3570, 3600),
            (3600, 7200),
            (86_399, 86_400),
            (86_400, 172_800),
        ] {
            let now = at + s * 1000;
            let tick = ago_changes(now, at).unwrap();
            assert_eq!(tick, at + next * 1000, "{s} s on");
            assert_ne!(ago(tick, at), ago(now, at), "{s} s on, it changes at {next} s");
            assert_eq!(ago(tick - 1, at), ago(now, at), "{s} s on, and not before {next} s");
        }
        assert_eq!(ago_changes(5000, 0), None, "no time, no age");
        assert_eq!(ago_changes(1000, 5000), Some(50_000), "a time ahead of the clock is now until 45 s after it");
    }

    /// The view laid out afresh at `now`
    fn at_time<'a>(v: &'a View, shown: &'a [Shown<'a>], now: u64) -> Scene {
        let mut ui = Ui::default();
        let model = Model {
            now_ms: now,
            ..model(Some(v), shown, true)
        };
        lay_out_settled(AREA, &metrics(), 0x121212, &model, &mut ui)
    }

    #[test]
    fn clock_is_when_its_words_change() {
        // The GUI lays the view out again when its clock says, and not before: laid out a
        // second apart for six minutes, it changes only where its clock said it would, and the
        // clock then says when the next change is. The ask and the picker were asked at 1 s.
        let v = on_view();
        let c = convo();
        let shown = c.shown();
        let mut last = at_time(&v, &shown, 1_000);
        assert_eq!(last.clock, Some(46_000), "asked at 1 s, they're 1 min old at 46 s");
        assert_eq!(last.said.clock, last.clock, "the state file says it too");
        let mut changes = vec![];
        for now in (2_000..=361_000).step_by(1000) {
            let s = at_time(&v, &shown, now);
            let tick = last.clock.expect("an ask and a question with times: a clock");
            if s.prims != last.prims {
                assert!(tick <= now, "the view changed between {} and {now} ms, but its clock said {tick}", now - 1000);
                changes.push(now);
            }
            if let Some(next) = s.clock {
                assert!(next > now, "at {now} ms its clock says {next}, already gone");
            }
            last = s;
        }
        assert_eq!(changes, vec![46_000, 91_000, 151_000, 211_000, 271_000, 331_000], "just now, then 1 to 6 min ago");
    }

    /// A conversation `n` messages long
    fn long_convo(n: usize) -> Conversation {
        let mut c = Conversation::default();
        let line = format!(
            r#"{{"type":"seat.items","reset":true,"items":[{}]}}"#,
            (0..n)
                .map(|i| format!(r#"{{"id":"m{i}","at":{},"who":"{}","from":"lbterm","text":"Message {i}: what's waiting on me, and which of them needs me first?"}}"#, 10_000 + i * 1000, if i % 2 == 0 { "you" } else { "conductor" }))
                .collect::<Vec<_>>()
                .join(",")
        );
        match parse(&line) {
            Some(Incoming::Items { reset, items }) => c.apply(reset, items),
            other => panic!("{other:?}"),
        }
        c
    }

    #[test]
    fn a_settled_layout_comes_out_the_same_again() {
        // The GUI keeps the laid-out view until what it's made from changes, so a layout must
        // leave nothing for the next to put right: scrolled past the side's end or the
        // conversation's start, a hair off the newest, at the newest, and in a conversation
        // that scrolls
        let v = on_view();
        for c in [convo(), long_convo(60)] {
            let shown = c.shown();
            let m = model(Some(&v), &shown, true);
            for (scroll, side_scroll) in [(0.0, 0.0), (1e9, 0.0), (0.0, 1e9), (1e9, 1e9), (0.5, 0.0), (300.0, 40.0)] {
                let mut ui = Ui {
                    scroll,
                    side_scroll,
                    side_max: 1e9,
                    ..Default::default()
                };
                let first = lay_out_settled(AREA, &metrics(), 0x121212, &m, &mut ui);
                assert!(!first.again, "scrolled {scroll}, the side {side_scroll}: still unsettled");
                let second = lay_out(AREA, &metrics(), 0x121212, &m, &mut ui);
                assert!(first.prims == second.prims && first.said == second.said, "scrolled {scroll}, the side {side_scroll}: laid out again, it moved");
            }
        }
        // Words arriving while he's scrolled up keep what he's reading where it was, in one go
        let mut ui = Ui {
            scroll: 300.0,
            ..Default::default()
        };
        {
            let c = long_convo(60);
            let shown = c.shown();
            lay_out_settled(AREA, &metrics(), 0x121212, &model(Some(&v), &shown, true), &mut ui);
        }
        let c = long_convo(61);
        let shown = c.shown();
        let m = model(Some(&v), &shown, true);
        let first = lay_out_settled(AREA, &metrics(), 0x121212, &m, &mut ui);
        let second = lay_out(AREA, &metrics(), 0x121212, &m, &mut ui);
        assert!(first.prims == second.prims, "a message arriving while scrolled up: laid out again, it moved");
        assert!(ui.scroll > 300.0, "it scrolled on by the new message's height: {}", ui.scroll);
    }
}

