//! The tab menu's rows, layout and what's under the pointer or the keyboard (the GUI draws it in
//! the tab panel's style: lbterm/TAB-MOUSE.md, "The tab menu's model"). A card of rows in groups
//! under rules, which rows there are decided by what the GUI knows of the tab ([`Facts`]):
//!
//! - Rename tab (R), Colour (a row of swatches, 0 to 8), Copy link (L, a Claude tab's)
//! - New session in <Project> (N, when the tab's folder is in a project), Shell in its folder (S)
//! - Hide from Conductor or Show to Conductor (H: a Claude tab, while the Conductor's file has
//!   `hidden`: [`Facts::conductor`]; greyed, saying it's hiding or showing, while a pick for the
//!   tab waits: [`inert`]), Wait on… (W: a Claude tab, with `lbterm_links` and another Claude tab
//!   open)
//! - the tab's link rows (lbterm/LINKS.md)
//! - Move to new window (M)
//! - Close finished tabs (F, while one qualifies: [`finished`]), Close other tabs (O), Close tab
//!   (X, or Delete)
//!
//! A group with no rows takes no rule. Wait on… turns the card to a page of its own ([`Page`]):
//! ‹ Back, then the other Claude tabs by label. Hide from Conductor fires an event for Lachlan's
//! config to run the Conductor's `hide` or `show` (lbterm/NEW-TAB.md, section 8; the pick's wait
//! for the Conductor's word is `conductor::hide`).

use crate::conductor::{hide, Offer};
use crate::objectives::Mark;
use crate::tabs::{Rect, COLOURS};

/// What a row (or a swatch) does
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Rename,
    /// An index into COLOURS, or None for no colour
    Colour(Option<usize>),
    /// The session's link, `[label](lbterm://session/<id>) in <Project>`, on the clipboard
    /// ([`copy_link`])
    CopyLink,
    /// A new Claude session in the tab's project (`lbterm-project-action`, source 'tab menu')
    NewSession,
    /// PowerShell in the tab's folder and domain, in front: Duplicate tab until landing 2, and it
    /// does the same
    Shell,
    /// Hide the tab's Claude sessions from the Conductor, or show them to it again
    /// ([`Facts::conductor`] says which): `lbterm-conductor-hide`, for the config to run
    /// `conductor hide|show`
    Hide,
    /// The page of other Claude tabs to wait on ([`Page::WaitOn`])
    WaitOn,
    /// Park the tab, or unpark it ([`Facts::park`] says which): dimmed on the rail and passed over
    /// by the side buttons' hops (Lachlan's 8a, lbterm/NEW-TAB.md, "Landing 4")
    Park,
    /// One of the tab's link rows, by its place in [`Facts::links`]: what it does is the GUI's
    Link(usize),
    NewWindow,
    /// Close every tab [`finished`] picks, close marks written
    CloseFinished,
    CloseOthers,
    Close,
    /// Wait on…'s page: back to the menu
    Back,
    /// Wait on…'s page: another Claude tab, by its place in [`Facts::others`]
    On(usize),
}

/// Which list the card shows
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Main,
    /// Wait on…: ‹ Back, then the other Claude tabs
    WaitOn,
}

/// A tab as the rail knows it, for Close finished tabs and Wait on… (the GUI fills it from the
/// rail's row, `lbterm_panel::claude::row`, and the objectives' mark)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RailTab {
    /// Its id in the mux
    pub tab: usize,
    /// Its label, as under its pad
    pub label: String,
    /// It has a Claude session
    pub claude: bool,
    /// It's the Conductor's tab, which its pad stands for
    pub conductor: bool,
    /// Its row's status word: "Done", "Your turn", "Question", "Working", ...
    pub status: String,
    /// The white dot on its pad: a result it hasn't been shown
    pub unread: bool,
    /// Its parts still working: Claude Code's subagents and background tasks while it works
    pub agents: usize,
    /// Its tasks still running after it handed the turn back (the arc on its pad)
    pub waiting: usize,
    /// It waits on another tab through a tab link (its row says "Waiting on <tab>", lbterm/LINKS.md):
    /// a link the links file holds, or one the GUI has asked of it and draws already. A linked tab
    /// isn't finished, however its session left the turn: links.ts means to tell it to carry on
    /// (PITFALLS 153)
    pub linked: bool,
    /// Its objectives' mark, if it has one
    pub mark: Option<Mark>,
    /// Its pad's colour, as the rail draws it now (0xRRGGBB)
    pub colour: u32,
}

/// Another Claude tab, a row on Wait on…'s page
#[derive(Debug, Clone, PartialEq)]
pub struct Other {
    pub tab: usize,
    pub label: String,
    /// Its pad's colour (0xRRGGBB)
    pub colour: u32,
}

/// A tab Close finished tabs closes
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub tab: usize,
    pub label: String,
}

/// What decides the menu's rows, as the GUI knows it when the menu opens
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Facts {
    /// The tab has a Claude session: Copy link, and Wait on… when the rest allow
    pub claude: bool,
    /// The project the tab's folder is in, by the name the picker gives it: New session in it
    pub project: Option<String>,
    /// `lbterm_links` is set
    pub links_on: bool,
    /// The tab's link rows' words (tab_links.rs's), under a rule of their own
    pub links: Vec<String>,
    /// The other Claude tabs, in the rail's order ([`others`])
    pub others: Vec<Other>,
    /// The tabs Close finished tabs closes, in the rail's order ([`finished`])
    pub finished: Vec<Named>,
    /// What the Hide from Conductor row says, if it's there: none on a shell tab or while the
    /// Conductor's file has no `hidden` (`conductor::hide::Picks::offer`), the GUI's to set
    pub conductor: Option<Offer>,
    /// Whether the tab is parked, where it can be: none on the Conductor's tab, which its pad
    /// stands for, or where its domain's server can't park (an older build), the GUI's to set
    pub park: Option<bool>,
}

impl Facts {
    /// The facts for `tab`, the others and the finished taken from the rail's tabs
    pub fn from_rail(
        tab: usize,
        claude: bool,
        project: Option<String>,
        links_on: bool,
        links: Vec<String>,
        rail: &[RailTab],
    ) -> Self {
        Self {
            claude,
            project,
            links_on,
            links,
            conductor: None,
            park: None,
            others: others(rail, tab),
            finished: finished(rail)
                .into_iter()
                .map(|t| Named {
                    tab: t.tab,
                    label: t.label.clone(),
                })
                .collect(),
        }
    }
}

/// The words a row says when its session has finished and handed the turn back
/// (lbterm_panel::claude's `Row::status`): not Question, Extras, Permission, Login, Blocked,
/// Failed, Working or Stuck
const FINISHED_WORDS: [&str; 2] = ["Done", "Your turn"];

/// The tabs Close finished tabs closes, in the rail's order: every Claude tab but the Conductor's
/// whose row says Done or Your turn, with no unread dot, no agents or tasks running, no tab link
/// it waits through and its objectives' green dot (lbterm/NEW-TAB.md, section 7, row 10)
pub fn finished(rail: &[RailTab]) -> Vec<&RailTab> {
    rail.iter()
        .filter(|t| {
            t.claude
                && !t.conductor
                && !t.linked
                && FINISHED_WORDS.contains(&t.status.as_str())
                && !t.unread
                && t.agents == 0
                && t.waiting == 0
                && t.mark == Some(Mark::Done)
        })
        .collect()
}

/// The tabs Close finished tabs closes when it's picked: those its row listed as the menu opened
/// (`listed`, what Lachlan read) that still qualify on the rail now (`rail`, read again at the
/// pick), in the rail's order. A tab that started working again, got a question or a new result
/// since stays, and one that finished since the menu opened isn't taken unseen
pub fn to_close(listed: &[Named], rail: &[RailTab]) -> Vec<usize> {
    finished(rail)
        .into_iter()
        .filter(|t| listed.iter().any(|l| l.tab == t.tab))
        .map(|t| t.tab)
        .collect()
}

/// The Claude tabs `tab` could wait on, in the rail's order: every other one but the Conductor's
pub fn others(rail: &[RailTab], tab: usize) -> Vec<Other> {
    rail.iter()
        .filter(|t| t.claude && !t.conductor && t.tab != tab)
        .map(|t| Other {
            tab: t.tab,
            label: t.label.clone(),
            colour: t.colour,
        })
        .collect()
}

/// The rows top to bottom; `None` is a rule. A group with no rows takes no rule
pub fn rows(facts: &Facts, page: Page) -> Vec<Option<Item>> {
    if page == Page::WaitOn {
        let mut rows = vec![Some(Item::Back)];
        rows.extend((0..facts.others.len()).map(|i| Some(Item::On(i))));
        return rows;
    }
    let mut first = vec![Item::Rename, Item::Colour(None)];
    if facts.claude {
        first.push(Item::CopyLink);
    }
    let mut start = vec![];
    if facts.project.is_some() {
        start.push(Item::NewSession);
    }
    start.push(Item::Shell);
    let mut wait = vec![];
    // Park tab on every tab that can be parked, a shell's too
    if facts.park.is_some() {
        wait.push(Item::Park);
    }
    if facts.claude && facts.conductor.is_some() {
        wait.push(Item::Hide);
    }
    if facts.claude && facts.links_on && !facts.others.is_empty() {
        wait.push(Item::WaitOn);
    }
    let links: Vec<Item> = (0..facts.links.len()).map(Item::Link).collect();
    let mut close = vec![];
    if !facts.finished.is_empty() {
        close.push(Item::CloseFinished);
    }
    close.extend([Item::CloseOthers, Item::Close]);
    let mut rows = vec![];
    for group in [first, start, wait, links, vec![Item::NewWindow], close] {
        if group.is_empty() {
            continue;
        }
        if !rows.is_empty() {
            rows.push(None);
        }
        rows.extend(group.into_iter().map(Some));
    }
    rows
}

/// The rows that do something, top to bottom: every row but an [`inert`] one, which the keys
/// and the pointer pass over
pub fn items(facts: &Facts, page: Page) -> Vec<Item> {
    shown(facts, page).into_iter().filter(|&i| !inert(i, facts)).collect()
}

/// Every row with words, top to bottom, an [`inert`] one included: what the card shows
pub fn shown(facts: &Facts, page: Page) -> Vec<Item> {
    rows(facts, page).into_iter().flatten().collect()
}

/// Whether a row is shown greyed and does nothing: Hide from Conductor while a pick for the tab
/// waits for the Conductor's word ([`Offer::Waiting`], Lachlan's 11b of 2026-11-09). It has no
/// key, no highlight and no pick, so its letter is a key no row takes and the card's edge flares
pub fn inert(item: Item, facts: &Facts) -> bool {
    item == Item::Hide && matches!(facts.conductor, Some(Offer::Waiting { .. }))
}

/// A row's words
pub fn label(item: Item, facts: &Facts) -> String {
    match item {
        Item::Rename => "Rename tab".into(),
        Item::Colour(_) => "Colour".into(),
        Item::CopyLink => "Copy link".into(),
        Item::NewSession => format!("New session in {}", facts.project.as_deref().unwrap_or_default()),
        Item::Shell => "Shell in its folder".into(),
        Item::Hide => hide::words(facts.conductor.unwrap_or(Offer::Hide)).into(),
        Item::WaitOn => "Wait on\u{2026}".into(),
        Item::Park => park_words(facts.park.unwrap_or(false)).into(),
        Item::Link(i) => facts.links.get(i).cloned().unwrap_or_default(),
        Item::NewWindow => "Move to new window".into(),
        Item::CloseFinished => format!("Close finished tabs ({})", facts.finished.len()),
        Item::CloseOthers => "Close other tabs".into(),
        Item::Close => "Close tab".into(),
        Item::Back => "\u{2039} Back".into(),
        Item::On(i) => facts.others.get(i).map(|o| o.label.clone()).unwrap_or_default(),
    }
}

/// Park tab's words: "Unpark tab" on a parked tab
pub fn park_words(parked: bool) -> &'static str {
    if parked {
        "Unpark tab"
    } else {
        "Park tab"
    }
}

/// A row's key, shown dim at its right; Close tab takes Delete too, and the swatches 0 to 8
/// ([`by_key`])
pub fn key(item: Item) -> Option<char> {
    match item {
        Item::Rename => Some('R'),
        Item::CopyLink => Some('L'),
        Item::NewSession => Some('N'),
        Item::Shell => Some('S'),
        Item::Hide => Some('H'),
        Item::WaitOn => Some('W'),
        Item::Park => Some('P'),
        Item::NewWindow => Some('M'),
        Item::CloseFinished => Some('F'),
        Item::CloseOthers => Some('O'),
        Item::Close => Some('X'),
        Item::Colour(_) | Item::Link(_) | Item::Back | Item::On(_) => None,
    }
}

/// The row a typed character picks among `items`: a row's letter in either case, or on the
/// Colour row's menu a digit, 0 for no colour and 1 to 8 for the colours. `None` when no row
/// takes it, so the card's edge flares once (`lbterm_motion::menu::bump`; the GUI maps Delete to Close tab itself)
pub fn by_key(c: char, items: &[Item]) -> Option<Item> {
    if let Some(d) = c.to_digit(10) {
        let d = d as usize;
        let colours = items.iter().any(|i| matches!(i, Item::Colour(_)));
        return (colours && d <= COLOURS.len()).then(|| Item::Colour(d.checked_sub(1)));
    }
    let c = c.to_ascii_uppercase();
    items.iter().copied().find(|&i| key(i) == Some(c))
}

/// A key as every menu sees it, while it's open: the GUI maps its key codes to these
/// (render/menu_fade.rs, `menu_key`), and nothing a menu takes reaches the pane
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A character typed with no Ctrl or Alt: a row's letter, or a digit
    Char(char),
    Enter,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Delete,
    /// Shift, Ctrl, Alt or the Windows key alone: nothing, and no light
    Modifier,
    /// Anything else: a key with Ctrl or Alt, Tab, a function key
    Other,
}

/// What a key does in the tab menu (not while a name is typed: that's [`Name`]'s)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Did {
    /// The highlight goes to this row (or swatch), or off every row
    Focus(Option<Item>),
    /// This row is picked, as a click on it would
    Pick(Item),
    /// Wait on…'s page goes back to the menu
    Back,
    /// The menu closes
    Close,
    /// No row takes it: the card's edge flares once (`lbterm_motion::menu::bump`)
    Nowhere,
    /// Nothing at all (a modifier on its own)
    Nothing,
}

/// The tab menu's answer to `key` on `page`, its rows `items`, `focus` the row lit (lbterm/NEW-TAB.md,
/// section 6 and 7): a row's letter or a swatch's digit picks it; Enter picks the lit row, or with
/// none lit lights the first (FEEDBACK.md's gap); Delete is Close tab; Esc closes the menu, or on
/// Wait on…'s page goes back, as Left does there; the arrows move the light. Any other key, a
/// letter no row has included, is nowhere to go.
pub fn answer(key: Key, page: Page, items: &[Item], focus: Option<Item>) -> Did {
    let (first, last) = (items.first().copied(), items.last().copied());
    match key {
        Key::Modifier => Did::Nothing,
        Key::Escape if page == Page::WaitOn => Did::Back,
        Key::Escape => Did::Close,
        Key::Left if page == Page::WaitOn => Did::Back,
        Key::Enter => match focus {
            Some(item) => Did::Pick(item),
            None => Did::Focus(first),
        },
        Key::Up => Did::Focus(focus.map(|f| step(f, -1, 0, items)).or(last)),
        Key::Down => Did::Focus(focus.map(|f| step(f, 1, 0, items)).or(first)),
        Key::Left => Did::Focus(focus.map(|f| step(f, 0, -1, items))),
        Key::Right => Did::Focus(focus.map(|f| step(f, 0, 1, items))),
        Key::Delete if items.contains(&Item::Close) => Did::Pick(Item::Close),
        Key::Char(c) => by_key(c, items).map_or(Did::Nowhere, Did::Pick),
        Key::Delete | Key::Other => Did::Nowhere,
    }
}

/// Sizes at 96 DPI
const WIDTH: f64 = 260.0;
const ROW: f64 = 30.0;
const RULE: f64 = 9.0;
const PAD: f64 = 6.0;
const SWATCH: f64 = 14.0;
const SWATCH_GAP: f64 = 6.0;
/// How far a row's words sit in from its sides
const INSET: f64 = 10.0;
/// The room at a row's right for its key, beside its words
pub const KEY: f64 = 22.0;
/// Wait on…'s rows: the other tab's pad, a small rounded square in its colour, left of its label
pub const DOT: f64 = 10.0;
/// Between that pad and the label
const DOT_GAP: f64 = 8.0;
/// How far from the window's edges the card keeps
const MARGIN: f64 = 6.0;

/// The menu's words as the GUI measures them, in pixels: its widest row's ([`widest`]) and a
/// line's height
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Text {
    pub widest: f64,
    pub line: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub card: Rect,
    /// Each row's rectangle, with what it is (`None` for the rule)
    pub rows: Vec<(Option<Item>, Rect)>,
    /// The swatches on the Colour row: None first, then each colour (none on Wait on…'s page)
    pub swatches: Vec<(Option<usize>, Rect)>,
    /// Where the Colour row's label ends and its swatches begin
    pub label_width: f64,
    /// Close finished tabs' second line, the tabs it closes by label ([`finished_line`])
    pub second: Option<Rect>,
    pub scale: f64,
    /// A line's height
    pub line: f64,
    /// The rows shown greyed that do nothing ([`inert`]): never under the pointer
    pub inert: Vec<Item>,
}

/// The page's widest row, its key's room included, in pixels: `measure` is a row's words' width
/// as the GUI lays them out (the frame's font is proportional, so never characters times
/// `cell_width`: lbterm/PITFALLS.md, 76). Measured as the menu opens and as its page changes,
/// not each time it's laid out
pub fn widest(facts: &Facts, page: Page, scale: f64, measure: impl Fn(&str) -> f64) -> f64 {
    shown(facts, page)
        .into_iter()
        // The Colour row's swatches are sized to fit the card as it is
        .filter(|i| !matches!(i, Item::Colour(_)))
        .map(|i| measure(&label(i, facts)) + shown_key(i, facts).map_or(0.0, |_| KEY * scale) + dot_room(i) * scale)
        .fold(0.0, f64::max)
}

/// The key a row shows at its right as the menu draws it: none on an [`inert`] row, whose
/// letter picks nothing
pub fn shown_key(item: Item, facts: &Facts) -> Option<char> {
    key(item).filter(|_| !inert(item, facts))
}

/// The room a row gives its pad's dot before its words, at 96 DPI: Wait on…'s tabs only
fn dot_room(item: Item) -> f64 {
    match item {
        Item::On(_) => DOT + DOT_GAP,
        _ => 0.0,
    }
}

/// The menu opened at `at` (the pointer, in the window) in a window `size` big, on `page`: down
/// and to the right of the pointer when it fits, flipped up or left when it doesn't. As wide as
/// its widest row needs (a link's row can name a task, New session names the project), inside
/// the window
pub fn layout(at: (f64, f64), size: (f64, f64), scale: f64, facts: &Facts, page: Page, text: Text) -> Layout {
    let all = rows(facts, page);
    let width = (WIDTH * scale)
        .max(text.widest + (2.0 * PAD + 2.0 * INSET) * scale)
        .min((size.0 - 2.0 * MARGIN * scale).max(WIDTH * scale));
    let tall = |row: &Option<Item>| match row {
        None => RULE * scale,
        // Its second line under its words
        Some(Item::CloseFinished) => ROW * scale + text.line,
        Some(_) => ROW * scale,
    };
    let height = PAD * 2.0 * scale + all.iter().map(tall).sum::<f64>();
    let margin = MARGIN * scale;
    let mut x = at.0;
    let mut y = at.1;
    if x + width + margin > size.0 {
        x = (at.0 - width).max(margin);
    }
    if y + height + margin > size.1 {
        y = (at.1 - height).max(margin);
    }
    let card = Rect {
        x,
        y,
        w: width,
        h: height,
    };
    let mut rows = vec![];
    let mut top = y + PAD * scale;
    for row in all {
        let h = tall(&row);
        rows.push((
            row,
            Rect {
                x: x + PAD * scale,
                y: top,
                w: width - 2.0 * PAD * scale,
                h,
            },
        ));
        top += h;
    }
    let second = rows
        .iter()
        .find(|(r, _)| *r == Some(Item::CloseFinished))
        .map(|(_, r)| Rect {
            x: r.x + INSET * scale,
            y: r.y + (ROW * scale - text.line) / 2.0 + text.line,
            w: r.w - 2.0 * INSET * scale,
            h: text.line,
        });
    let mut swatches = vec![];
    let mut label_width = 0.0;
    if let Some(colour_row) = rows
        .iter()
        .find(|(r, _)| matches!(r, Some(Item::Colour(_))))
        .map(|(_, rect)| *rect)
    {
        let count = COLOURS.len() + 1;
        let swatches_width = count as f64 * SWATCH * scale + (count - 1) as f64 * SWATCH_GAP * scale;
        let first = colour_row.x + colour_row.w - PAD * scale - swatches_width;
        swatches = (0..count)
            .map(|i| {
                let colour = if i == 0 { None } else { Some(i - 1) };
                (
                    colour,
                    Rect {
                        x: first + i as f64 * (SWATCH + SWATCH_GAP) * scale,
                        y: colour_row.y + (colour_row.h - SWATCH * scale) / 2.0,
                        w: SWATCH * scale,
                        h: SWATCH * scale,
                    },
                )
            })
            .collect();
        label_width = first - colour_row.x;
    }
    Layout {
        card,
        rows,
        swatches,
        label_width,
        second,
        scale,
        line: text.line,
        inert: shown(facts, page).into_iter().filter(|&i| inert(i, facts)).collect(),
    }
}

impl Layout {
    /// What's under the pointer: a swatch on the Colour row, else the row
    pub fn hit(&self, x: f64, y: f64) -> Option<Item> {
        if !self.card.contains(x, y) {
            return None;
        }
        // A little slack round each swatch: they're small
        let slack = SWATCH_GAP / 2.0 * self.scale;
        for (colour, r) in &self.swatches {
            let grown = Rect {
                x: r.x - slack,
                y: r.y - slack,
                w: r.w + 2.0 * slack,
                h: r.h + 2.0 * slack,
            };
            if grown.contains(x, y) {
                return Some(Item::Colour(*colour));
            }
        }
        self.rows
            .iter()
            .find(|(row, r)| row.is_some() && r.contains(x, y))
            .and_then(|(row, _)| match row {
                // The row itself, off the swatches, does nothing
                Some(Item::Colour(_)) => None,
                // A greyed row does nothing either: no highlight, no pick
                Some(item) if self.inert.contains(item) => None,
                other => *other,
            })
    }

    /// What flashes when `item` is picked (Lachlan's 9b of 2026-11-07): a swatch's own square for a
    /// colour, else the item's row, as the highlight lights it; None for an item this page hasn't
    pub fn flash_box(&self, item: Item) -> Option<Rect> {
        if let Item::Colour(colour) = item {
            return self.swatches.iter().find(|(c, _)| *c == colour).map(|(_, r)| *r);
        }
        self.rows.iter().find(|(row, _)| *row == Some(item)).map(|(_, r)| *r)
    }

    /// Where a row's words go: its first line, in from its sides, short of its key's room when
    /// it has a key and of the swatches on the Colour row
    pub fn words_box(&self, item: Item, row: Rect) -> Rect {
        let s = self.scale;
        let right = match item {
            Item::Colour(_) => row.w - self.label_width,
            _ if key(item).is_some() && !self.inert.contains(&item) => INSET * s + KEY * s,
            _ => INSET * s,
        };
        // Wait on…'s tabs start after their pad's dot
        let left = INSET * s + dot_room(item) * s;
        Rect {
            x: row.x + left,
            y: row.y + (ROW * s - self.line) / 2.0,
            w: (row.w - left - right).max(0.0),
            h: self.line,
        }
    }

    /// Where a Wait on… row's dot goes, the other tab's pad in its colour: at the row's left, on
    /// its first line, `None` on any other row
    pub fn dot_box(&self, item: Item, row: Rect) -> Option<Rect> {
        let s = self.scale;
        matches!(item, Item::On(_)).then(|| Rect {
            x: row.x + INSET * s,
            y: row.y + (ROW * s - DOT * s) / 2.0,
            w: DOT * s,
            h: DOT * s,
        })
    }

    /// Where a row's key goes, at its right on its first line
    pub fn key_box(&self, row: Rect) -> Rect {
        let s = self.scale;
        Rect {
            x: row.x + row.w - INSET * s - KEY * s,
            y: row.y + (ROW * s - self.line) / 2.0,
            w: KEY * s,
            h: self.line,
        }
    }
}

/// Close finished tabs' second line, whole: the tabs it closes by label, in the rail's order
pub fn finished_words(facts: &Facts) -> String {
    let labels: Vec<&str> = facts.finished.iter().map(|t| t.label.as_str()).collect();
    labels.join(", ")
}

/// Close finished tabs' second line cut to `room` pixels ([`Layout::second`]'s width), `measure`
/// a run of words' width as the GUI lays them out (never characters times `cell_width`,
/// PITFALLS 76)
pub fn finished_line(facts: &Facts, room: f64, measure: impl Fn(&str) -> f64) -> String {
    cut_to(&finished_words(facts), room, measure)
}

/// `text` as much of it as fits in `room` as `measure` lays it out, an ellipsis where it was
/// cut, or nothing when not even the ellipsis fits. A cut's width grows with what it keeps, so
/// the longest that fits is found by halving: a few measures, each kept by the GUI by its words
pub fn cut_to(text: &str, room: f64, measure: impl Fn(&str) -> f64) -> String {
    if text.is_empty() || measure(text) <= room {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let cut = |n: usize| format!("{}\u{2026}", chars[..n].iter().collect::<String>().trim_end());
    if measure(&cut(0)) > room {
        return String::new();
    }
    // cut(lo) fits; the whole text, chars.len(), doesn't
    let (mut lo, mut hi) = (0, chars.len());
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if measure(&cut(mid)) <= room {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    cut(lo)
}

/// The item the keyboard is on, moved by `rows` (up negative, down positive) and, on the
/// Colour row, `across` along the swatches, among `items` (the rows that do something, top to
/// bottom). Wraps top to bottom.
pub fn step(from: Item, rows: i32, across: i32, items: &[Item]) -> Item {
    if items.is_empty() {
        return from;
    }
    let here = items.iter().position(|i| same_row(*i, from)).unwrap_or(0);
    if rows == 0 {
        if let Item::Colour(c) = from {
            let n = COLOURS.len() as i32 + 1;
            let at = c.map_or(0, |i| i as i32 + 1);
            let next = (at + across).rem_euclid(n);
            return Item::Colour(if next == 0 { None } else { Some(next as usize - 1) });
        }
        return from;
    }
    let len = items.len() as i32;
    let next = (here as i32 + rows).rem_euclid(len) as usize;
    items[next]
}

/// Whether two items are on the same row: every swatch is on the Colour row
pub fn same_row(a: Item, b: Item) -> bool {
    match (a, b) {
        (Item::Colour(_), Item::Colour(_)) => true,
        _ => a == b,
    }
}

/// What Copy link puts on the clipboard: `[<label>](lbterm://session/<id>) in <Project>`, the
/// project left off when there's none or the label already ends with its name (as every agent
/// names a conversation to Lachlan: "InMail · Lantern", not "InMail · Lantern in Lantern")
pub fn copy_link(label: &str, session: &str, project: Option<&str>) -> String {
    let label = label.trim();
    let link = format!("[{}](lbterm://session/{})", label, session.trim());
    match project.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) if !ends_with_name(label, p) => format!("{} in {}", link, p),
        _ => link,
    }
}

/// Whether `label` ends with the word or words `name`, in any case
fn ends_with_name(label: &str, name: &str) -> bool {
    let (label, name) = (label.to_lowercase(), name.to_lowercase());
    match label.strip_suffix(&name) {
        Some(before) => before.chars().next_back().map_or(true, |c| !c.is_alphanumeric()),
        None => false,
    }
}

/// A tab's new name as it's typed. It starts as the old name, all of it selected as in
/// Windows' rename boxes: typing replaces it, Backspace clears it, and an arrow, Home or End
/// keeps it to edit from its end.
#[derive(Debug, Clone, PartialEq)]
pub struct Name {
    pub text: String,
    pub selected: bool,
}

impl Name {
    pub fn new(old: &str) -> Self {
        Self {
            text: old.to_string(),
            selected: !old.is_empty(),
        }
    }

    pub fn type_char(&mut self, c: char) {
        if self.selected {
            self.text.clear();
            self.selected = false;
        }
        self.text.push(c);
    }

    pub fn backspace(&mut self) {
        if self.selected {
            self.text.clear();
            self.selected = false;
        } else {
            self.text.pop();
        }
    }

    pub fn deselect(&mut self) {
        self.selected = false;
    }
}

#[cfg(test)]
mod tests;
