//! The projects card (lbterm/OBJECTIVES.md, "The projects card"; Lachlan's ask, 2026-10-24): from
//! a pad at the top of the objectives strip, every project in `lbterm_projects`, the busiest first,
//! each with the Claude sessions open in it (working, waiting on him, idle), their subagents and
//! background tasks, and the InMail waiting for it, and on the row under the pointer, buttons to
//! start a session there, one told to read its InMail, or resume its last. A repo that's part of a
//! project (its `parent`: Trainer, in Musica's folder) is listed under that project's row, indented,
//! with a blue repo mark before its name (Lachlan's ask, 2026-10-28). Which project a folder is in,
//! the order, and where every word and button goes are here, where they can be tested; the counting
//! and drawing are wezterm-gui's render/projects.rs.

use crate::strip::elide;
use crate::tabs::Rect;

/// What's going on in one project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// Messages waiting in its InMail
    pub mail: usize,
    /// Claude sessions open in it: working, waiting on Lachlan, and the rest (finished, or failed)
    pub working: usize,
    pub waiting: usize,
    pub idle: usize,
    /// Their subagents and background tasks still running
    pub parts: usize,
}

impl Counts {
    pub fn sessions(&self) -> usize {
        self.working + self.waiting + self.idle
    }

    /// Nothing open and nothing waiting: its row is dimmer
    pub fn quiet(&self) -> bool {
        self.sessions() == 0 && self.mail == 0
    }
}

impl std::ops::Add for Counts {
    type Output = Counts;

    fn add(self, o: Counts) -> Counts {
        Counts {
            mail: self.mail + o.mail,
            working: self.working + o.working,
            waiting: self.waiting + o.waiting,
            idle: self.idle + o.idle,
            parts: self.parts + o.parts,
        }
    }
}

/// The repo mark's blue: lighter and greener than the rail's "your turn" (`#5B7CFF`, an idle
/// session's dot on this card), so a repo's row doesn't read as a session's status
pub const REPO: u32 = 0x4FB3FF;
/// The repo mark, before a repo's name: nf-oct-repo, from the Nerd Font symbols every build has
pub const REPO_MARK: &str = "\u{f401}";
/// How far in a repo's name starts from its project's, in columns: a column, the mark, a space
const REPO_INDENT: usize = 3;

/// Each project's parent, by its place in the list, from the names `parents` gives (any case): the
/// project it's a repo of. None for a project of its own, and for a repo whose parent isn't in the
/// list, is itself, or is a repo too: the card nests one level only.
pub fn parents(names: &[&str], parents: &[Option<&str>]) -> Vec<Option<usize>> {
    let direct: Vec<Option<usize>> = (0..names.len())
        .map(|i| {
            let want = parents.get(i).copied().flatten()?.to_lowercase();
            names.iter().position(|n| n.to_lowercase() == want).filter(|&p| p != i)
        })
        .collect();
    direct.iter().map(|p| p.filter(|&p| direct[p].is_none())).collect()
}

/// A folder as folders are compared: `\` between its parts, none at the end, lowercase (Windows
/// compares them so), and a pane's `file:///C:/...` URL as the path it names.
pub fn key(folder: &str) -> String {
    let mut s = folder.trim();
    if let Some(rest) = s.strip_prefix("file://") {
        s = rest.strip_prefix('/').filter(|r| r.get(1..2) == Some(":")).unwrap_or(rest);
    }
    // a URL's %20 and the like, as the bytes they stand for
    let bytes = s.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16);
    let mut raw = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                raw.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        raw.push(bytes[i]);
        i += 1;
    }
    let mut out = String::new();
    for ch in String::from_utf8_lossy(&raw).chars() {
        out.extend(if ch == '/' { '\\' } else { ch }.to_lowercase());
    }
    out.trim_end_matches('\\').to_string()
}

/// Which of `dirs` a folder is in: the deepest at or above it, as projects.ts's `current` finds it,
/// so a session in a repo nested in a project is the repo's.
pub fn project_of(dirs: &[String], folder: &str) -> Option<usize> {
    Folders::new(dirs).of(folder)
}

/// Whether a folder is a repo's: the deepest of `dirs` at or above it (`project_of`) is listed under
/// another (`parents`, from [`parents`]). The InMail button's list marks such a message with the
/// repo mark, as the card marks the repo's row.
pub fn in_repo(dirs: &[String], parents: &[Option<usize>], folder: &str) -> bool {
    Folders::new(dirs).in_repo(parents, folder)
}

/// The projects' folders as folders are compared ([`key`]), made once for a list of them, so
/// asking which one a folder is in makes only that folder's key. The projects card asked for a
/// project once for every message waiting and every tab, every frame it was open, and each ask
/// made every project's key again: about 90 asks of 47 folders a frame, three or four strings
/// each (lbterm/PERF.md, "What the cards cost").
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Folders {
    keys: Vec<String>,
}

impl Folders {
    pub fn new(dirs: &[impl AsRef<str>]) -> Self {
        Self {
            keys: dirs.iter().map(|d| key(d.as_ref())).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Which a folder is in: the deepest at or above it ([`project_of`])
    pub fn of(&self, folder: &str) -> Option<usize> {
        let folder = key(folder);
        if folder.is_empty() {
            return None;
        }
        self.keys
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                !d.is_empty()
                    && folder.starts_with(d.as_str())
                    && (folder.len() == d.len() || folder.as_bytes()[d.len()] == b'\\')
            })
            .max_by_key(|(_, d)| d.len())
            .map(|(i, _)| i)
    }

    /// Whether a folder is a repo's ([`in_repo`])
    pub fn in_repo(&self, parents: &[Option<usize>], folder: &str) -> bool {
        self.of(folder).map_or(false, |i| nested(parents, self.keys.len(), i).is_some())
    }
}

/// What a button on a row does
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A new Claude session in the project
    New,
    /// A new one told to read its InMail and deal with each message
    Inmail,
    /// Its most recent conversation that isn't open, resumed
    Resume,
}

impl Action {
    /// As the `lbterm-project-action` event names it
    pub fn name(self) -> &'static str {
        match self {
            Action::New => "new",
            Action::Inmail => "inmail",
            Action::Resume => "resume",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::New => "New",
            Action::Inmail => "InMail",
            Action::Resume => "Resume",
        }
    }
}

/// The project the `i`th of `n` is listed under: its parent, while that's another of the `n` and a
/// project of its own; else it's listed as one itself
fn nested(parents: &[Option<usize>], n: usize, i: usize) -> Option<usize> {
    let parent_of = |i: usize| parents.get(i).copied().flatten();
    parent_of(i).filter(|&p| p < n && p != i && parent_of(p).is_none())
}

/// The order the card lists them in: each project followed by its repos (`parents`, from
/// [`parents`]). The projects go by what's going on in them and their repos together, and a
/// project's repos by their own: the most sessions first, then the most subagents and tasks, then
/// the most mail, then by name.
pub fn order(names: &[&str], counts: &[Counts], parents: &[Option<usize>]) -> Vec<usize> {
    let n = names.len().min(counts.len());
    let parent = |i: usize| nested(parents, n, i);
    let mut whole = counts[..n].to_vec();
    for i in 0..n {
        if let Some(p) = parent(i) {
            whole[p] = whole[p] + counts[i];
        }
    }
    let busier = |c: &[Counts], a: usize, b: usize| {
        let (ca, cb) = (c[a], c[b]);
        cb.sessions()
            .cmp(&ca.sessions())
            .then(cb.parts.cmp(&ca.parts))
            .then(cb.mail.cmp(&ca.mail))
            .then_with(|| names[a].to_lowercase().cmp(&names[b].to_lowercase()))
    };
    let mut tops: Vec<usize> = (0..n).filter(|&i| parent(i).is_none()).collect();
    tops.sort_by(|&a, &b| busier(&whole, a, b));
    let mut idx = Vec::with_capacity(n);
    for top in tops {
        idx.push(top);
        let mut repos: Vec<usize> = (0..n).filter(|&i| parent(i) == Some(top)).collect();
        repos.sort_by(|&a, &b| busier(counts, a, b));
        idx.extend(repos);
    }
    idx
}

/// How a bit of a row's words is drawn
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Working,
    Waiting,
    Idle,
    Parts,
    Mail,
    /// The repo mark, in [`REPO`]
    Repo,
}

/// Words at `x`, in a tone
#[derive(Debug, Clone, PartialEq)]
pub struct Bit {
    pub x: f64,
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    pub action: Action,
    pub rect: Rect,
    /// Where its label starts
    pub text_x: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Its index in `lbterm_projects.projects`
    pub project: usize,
    /// All of it across the card: what the pointer can be on
    pub rect: Rect,
    /// Its words' top, and where the name starts: a repo's further in than its project's
    pub text_y: f64,
    pub name_x: f64,
    pub name: String,
    pub quiet: bool,
    /// A repo's mark, before its name; None on a project's row
    pub repo: Option<Bit>,
    /// Its sessions, subagents and mail
    pub bits: Vec<Bit>,
    /// Its buttons, drawn and pressed only while the pointer's on the row; InMail only with mail
    pub chips: Vec<Chip>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub rect: Rect,
    /// "Projects", on the top line, and what the dots mean, right of it when there's room
    pub title: (f64, f64),
    pub legend: Vec<Bit>,
    pub rows: Vec<Row>,
    /// "N more" under the last row, when they don't all fit: the quietest are left out
    pub more: Option<(f64, f64, String)>,
}

const PAD: f64 = 12.0;
const GAP: f64 = 4.0;
/// A row's height over its line's
const ROW_EXTRA: f64 = 8.0;
/// Room for the sessions' bits ("●2 ●1 ●3 +4") and the mail ("12 mail"), in columns
const BITS_COLUMNS: usize = 14;
const MAIL_COLUMNS: usize = 7;
pub const TITLE: &str = "Projects";
const LEGEND: [(&str, Tone); 4] = [
    ("\u{25cf} working", Tone::Working),
    ("\u{25cf} waiting", Tone::Waiting),
    ("\u{25cf} idle", Tone::Idle),
    ("+ subagents", Tone::Parts),
];

fn columns(text: &str) -> f64 {
    text.chars().count() as f64
}

/// The card, `width` wide, its right edge at `right`, from `top` down to `bottom` at most: a title
/// line, then a row per project in `order`'s order, each repo under its project (`parents`, from
/// [`parents`]). `cell` is a column's width and a line's height, in pixels; `scale` the DPI over 96.
#[allow(clippy::too_many_arguments)]
pub fn card(
    names: &[&str],
    counts: &[Counts],
    parents: &[Option<usize>],
    right: f64,
    (top, bottom): (f64, f64),
    width: f64,
    cell: (f64, f64),
    scale: f64,
) -> Option<Card> {
    if names.is_empty() || cell.0 <= 0.0 || width <= 0.0 {
        return None;
    }
    let pad = PAD * scale;
    let gap = GAP * scale;
    let row_h = (cell.1 + ROW_EXTRA * scale).round();
    let chip_h = (cell.1 + 4.0 * scale).round();
    let x = right - width;
    let name_x = x + pad;

    // The title, and the legend right of it when it fits
    let title_y = (top + pad).round();
    let legend_w: f64 = LEGEND.iter().map(|(t, _)| columns(t)).sum::<f64>() + 2.0 * (LEGEND.len() - 1) as f64;
    let mut legend = vec![];
    if columns(TITLE) + 3.0 + legend_w <= (width - 2.0 * pad) / cell.0 {
        let mut lx = right - pad - legend_w * cell.0;
        for (text, tone) in LEGEND {
            legend.push(Bit { x: lx, text: text.to_string(), tone });
            lx += (columns(text) + 2.0) * cell.0;
        }
    }

    // The buttons from the right, each its label and a column either side; InMail's place kept on
    // every row, so New and Resume line up
    let chip_w = |a: Action| (columns(a.label()) + 2.0) * cell.0;
    let actions = [Action::New, Action::Inmail, Action::Resume];
    let chips_w: f64 = actions.iter().map(|a| chip_w(*a)).sum::<f64>() + gap * (actions.len() - 1) as f64;
    let chips_x = right - pad - chips_w;
    let avail = ((chips_x - gap - name_x) / cell.0).floor() as usize;
    let name_columns = avail.saturating_sub(BITS_COLUMNS + MAIL_COLUMNS + 2).max(6);
    let bits_x = name_x + (name_columns + 1) as f64 * cell.0;
    let mail_x = bits_x + (BITS_COLUMNS + 1) as f64 * cell.0;

    let first = (title_y + cell.1 + 6.0 * scale).round();
    let fit = (((bottom - pad / 2.0 - first) / row_h).floor().max(0.0)) as usize;
    let all = order(names, counts, parents);
    let (shown, more) = if all.len() > fit {
        let shown = fit.saturating_sub(1);
        (shown, Some(all.len() - shown))
    } else {
        (all.len(), None)
    };
    let mut rows = vec![];
    for (i, &p) in all.iter().take(shown).enumerate() {
        let c = counts[p];
        let y = first + i as f64 * row_h;
        let text_y = (y + (row_h - cell.1) / 2.0).round();
        let mut bits = vec![];
        let mut bx = bits_x;
        for (n, tone, mark) in [
            (c.working, Tone::Working, "\u{25cf}"),
            (c.waiting, Tone::Waiting, "\u{25cf}"),
            (c.idle, Tone::Idle, "\u{25cf}"),
            (c.parts, Tone::Parts, "+"),
        ] {
            if n > 0 {
                let text = format!("{mark}{n}");
                let w = columns(&text);
                bits.push(Bit { x: bx, text, tone });
                bx += (w + 1.0) * cell.0;
            }
        }
        if c.mail > 0 {
            bits.push(Bit { x: mail_x, text: format!("{} mail", c.mail), tone: Tone::Mail });
        }
        let mut chips = vec![];
        let mut cx = chips_x;
        let chip_y = (y + (row_h - chip_h) / 2.0).round();
        for a in actions {
            let w = chip_w(a);
            if a != Action::Inmail || c.mail > 0 {
                chips.push(Chip { action: a, rect: Rect { x: cx, y: chip_y, w, h: chip_h }, text_x: cx + cell.0 });
            }
            cx += w + gap;
        }
        // A repo's name further in, its mark between, so it reads as part of the project above
        let is_repo = nested(parents, names.len().min(counts.len()), p).is_some();
        let (repo, row_name_x, row_name_columns) = if is_repo {
            let mark = Bit { x: name_x + cell.0, text: REPO_MARK.to_string(), tone: Tone::Repo };
            (Some(mark), name_x + REPO_INDENT as f64 * cell.0, name_columns.saturating_sub(REPO_INDENT).max(3))
        } else {
            (None, name_x, name_columns)
        };
        rows.push(Row {
            project: p,
            rect: Rect { x, y, w: width, h: row_h },
            text_y,
            name_x: row_name_x,
            name: elide(names[p], row_name_columns),
            quiet: c.quiet(),
            repo,
            bits,
            chips,
        });
    }
    let mut end = first + shown as f64 * row_h;
    let more = more.map(|n| {
        let line = (end + (row_h - cell.1) / 2.0).round();
        end += row_h;
        (name_x, line, format!("{n} more, quieter"))
    });
    Some(Card {
        rect: Rect { x, y: top, w: width.ceil(), h: (end + pad / 2.0 - top).ceil() },
        title: (name_x, title_y),
        legend,
        rows,
        more,
    })
}

/// Where a card `height` tall goes for a pad whose zone runs from `zone.0` to `zone.1`, between
/// `ceiling` and `floor`: down from the pad's top when it fits (the pad at the strip's top, level
/// with the Conductor's), else up from the pad's foot (the pad at the strip's foot), never above
/// the ceiling.
pub fn place(zone: (f64, f64), height: f64, (ceiling, floor): (f64, f64)) -> f64 {
    if zone.0 + height <= floor {
        zone.0
    } else {
        (zone.1 - height).max(ceiling)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn the_card_opens_down_from_a_pad_at_the_top_and_up_from_one_at_the_foot() {
        assert_eq!(place((60.0, 105.0), 300.0, (40.0, 730.0)), 60.0);
        assert_eq!(place((659.0, 734.0), 300.0, (40.0, 730.0)), 434.0);
        assert_eq!(place((659.0, 734.0), 900.0, (40.0, 730.0)), 40.0, "never above the ceiling");
    }

    #[test]
    fn a_folder_is_compared_without_case_slashes_or_url() {
        assert_eq!(key(r"C:\Users\x\AI\Terminal\"), r"c:\users\x\ai\terminal");
        assert_eq!(key("file:///C:/Users/x/AI/AI%20Dashboard/"), r"c:\users\x\ai\ai dashboard");
        assert_eq!(key("C:/Users/x/AI"), r"c:\users\x\ai");
        assert_eq!(key(""), "");
    }

    #[test]
    fn a_folder_is_the_deepest_projects() {
        let dirs = vec![r"C:\AI".to_string(), r"C:\AI\Terminal".to_string(), r"C:\AI\Terminal\code\LBTerm".to_string(), r"C:\AI\Orchard".to_string()];
        assert_eq!(project_of(&dirs, r"C:\AI\Terminal"), Some(1));
        assert_eq!(project_of(&dirs, r"c:\ai\terminal\shared\claude"), Some(1));
        assert_eq!(project_of(&dirs, r"C:\AI\Terminal\code\LBTerm\.claude\worktrees\x"), Some(2), "a repo nested in a project is its own");
        assert_eq!(project_of(&dirs, r"C:\AI\Terminals"), Some(0), "a folder that only starts the same isn't inside it");
        assert_eq!(project_of(&dirs, "file:///C:/AI/Orchard"), Some(3));
        assert_eq!(project_of(&dirs, r"D:\Elsewhere"), None);
        assert_eq!(project_of(&dirs, ""), None);
    }

    #[test]
    fn the_busiest_come_first() {
        let names = ["Zeta", "alpha", "Beta", "Gamma"];
        let c = |working, waiting, idle, parts, mail| Counts { working, waiting, idle, parts, mail };
        let counts = [c(0, 0, 0, 0, 2), c(0, 0, 0, 0, 0), c(1, 1, 0, 0, 0), c(0, 0, 2, 3, 0)];
        assert_eq!(order(&names, &counts, &[None; 4]), vec![3, 2, 0, 1], "two sessions with subagents, two sessions, mail, then by name");
        assert!(counts[1].quiet() && !counts[0].quiet());
    }

    #[test]
    fn a_repo_names_its_project() {
        let names = ["Home", "Musica", "Trainer", "Experience", "Terminal", "Odd", "Deep"];
        let named = [None, None, Some("musica"), Some("Musica"), None, Some("Nowhere"), Some("Trainer")];
        assert_eq!(
            parents(&names, &named),
            vec![None, None, Some(1), Some(1), None, None, None],
            "by name in any case; a name not in the list, and a repo's repo, are projects of their own"
        );
        assert_eq!(parents(&["Self"], &[Some("self")]), vec![None], "never its own");
    }

    #[test]
    fn folders_made_once_answer_as_each_ask_did() {
        let dirs = vec![r"C:\AI\".to_string(), "file:///C:/AI/Terminal".to_string(), r"C:\AI\Terminal\code\LBTerm".to_string(), r"C:\".to_string(), String::new()];
        let folders = Folders::new(&dirs);
        assert_eq!(folders.len(), 5);
        for folder in [r"C:\AI", r"c:\ai\terminal\x", r"C:\AI\Terminal\code\LBTerm\src", r"C:\AIs", r"C:\Windows", r"D:\x", "", "file:///C:/AI/Terminal/"] {
            let asked = folders.of(folder);
            // As project_of made every key again for each ask
            let each = dirs
                .iter()
                .enumerate()
                .map(|(i, d)| (i, key(d)))
                .filter(|(_, d)| !d.is_empty() && (key(folder) == *d || key(folder).starts_with(&format!("{d}\\"))))
                .max_by_key(|(_, d)| d.len())
                .map(|(i, _)| i);
            assert_eq!(asked, if key(folder).is_empty() { None } else { each }, "{folder}");
        }
        assert_eq!(folders.of(r"C:\AIs"), Some(3), "only the drive: C:\\AI isn't above C:\\AIs");
        assert!(Folders::new(&dirs).in_repo(&[None, None, Some(1), None, None], r"C:\AI\Terminal\code\LBTerm\x"));
    }

    #[test]
    fn a_folder_in_a_repo_is_the_repos() {
        let dirs = vec![r"C:\AI".to_string(), r"C:\AI\Musica".to_string(), r"C:\AI\Musica\code\Trainer".to_string()];
        let parents = vec![None, None, Some(1)];
        assert!(in_repo(&dirs, &parents, r"C:\AI\Musica\code\Trainer"));
        assert!(in_repo(&dirs, &parents, r"c:\ai\musica\code\trainer\src"), "in any case, and below");
        assert!(!in_repo(&dirs, &parents, r"C:\AI\Musica"), "the project itself");
        assert!(!in_repo(&dirs, &parents, r"C:\AI\Musica\code"), "the project's, above the repo");
        assert!(!in_repo(&dirs, &parents, r"D:\Elsewhere"));
        assert!(!in_repo(&dirs, &[None; 3], r"C:\AI\Musica\code\Trainer"), "no parent named, no repo");
    }

    #[test]
    fn repos_come_under_their_project_which_counts_what_they_have() {
        let names = ["Home", "Musica", "Trainer", "Experience", "Terminal", "LBTerm"];
        let c = |working, mail| Counts { working, mail, ..Default::default() };
        let counts = [c(0, 0), c(0, 0), c(0, 3), c(1, 0), c(2, 0), c(0, 0)];
        let parents = [None, None, Some(1), Some(1), None, Some(4)];
        // Terminal has two sessions; Musica none of its own, but a session and mail in its repos
        assert_eq!(order(&names, &counts, &parents), vec![4, 5, 1, 3, 2, 0]);
        // A repo whose parent is a repo itself, or isn't there, is listed as a project
        assert_eq!(order(&["A", "B", "C"], &[c(0, 0); 3], &[Some(1), Some(0), Some(9)]), vec![0, 1, 2]);
    }

    #[test]
    fn a_repos_row_is_indented_with_its_mark() {
        let names = ["Musica", "Trainer", "Orchard"];
        let counts = [Counts::default(), Counts { mail: 2, ..Default::default() }, Counts::default()];
        let card = card(&names, &counts, &[None, Some(0), None], 1000.0, (100.0, 900.0), 480.0, (7.0, 16.0), 1.0).unwrap();
        let order: Vec<usize> = card.rows.iter().map(|r| r.project).collect();
        assert_eq!(order, vec![0, 1, 2], "the repo under its project, which its mail brings up");
        let (musica, trainer) = (&card.rows[0], &card.rows[1]);
        assert_eq!(musica.repo, None);
        assert_eq!(musica.name_x, 532.0);
        let mark = trainer.repo.as_ref().expect("the repo mark");
        assert_eq!((mark.x, mark.text.as_str(), mark.tone), (539.0, REPO_MARK, Tone::Repo));
        assert_eq!(trainer.name_x, 532.0 + 3.0 * 7.0, "its name past the mark");
        assert_eq!(trainer.name, "Trainer");
        assert!(musica.quiet && !trainer.quiet, "the project dimmer for having nothing of its own");
        let bits: Vec<_> = trainer.bits.iter().map(|b| (b.text.as_str(), b.tone)).collect();
        assert_eq!(bits, vec![("2 mail", Tone::Mail)], "its own mail, on its own row");
        assert_eq!(trainer.chips.len(), 3, "a repo's own buttons, InMail with its mail");
    }

    #[test]
    fn a_row_says_its_sessions_and_mail_and_has_its_buttons() {
        let names = ["Terminal", "Orchard", "A project with a very long name indeed"];
        let counts = [
            Counts { working: 2, waiting: 1, idle: 0, parts: 3, mail: 4 },
            Counts::default(),
            Counts { idle: 1, ..Default::default() },
        ];
        let card = card(&names, &counts, &[None; 3], 1000.0, (100.0, 900.0), 480.0, (7.0, 16.0), 1.0).unwrap();
        assert_eq!((card.rect.x, card.rect.y), (520.0, 100.0));
        assert_eq!(card.title, (532.0, 112.0));
        assert_eq!(card.legend.len(), 4, "the legend fits beside the title");
        assert_eq!(card.legend.last().unwrap().x + 11.0 * 7.0, 988.0, "and ends a pad in from the right");
        let order: Vec<usize> = card.rows.iter().map(|r| r.project).collect();
        assert_eq!(order, vec![0, 2, 1]);
        let t = &card.rows[0];
        assert_eq!((t.rect.y, t.rect.h, t.text_y), (134.0, 24.0, 138.0));
        let bits: Vec<_> = t.bits.iter().map(|b| (b.text.as_str(), b.tone)).collect();
        assert_eq!(bits, vec![("\u{25cf}2", Tone::Working), ("\u{25cf}1", Tone::Waiting), ("+3", Tone::Parts), ("4 mail", Tone::Mail)]);
        let chips: Vec<_> = t.chips.iter().map(|c| c.action).collect();
        assert_eq!(chips, vec![Action::New, Action::Inmail, Action::Resume]);
        assert_eq!(t.chips[2].rect.x + t.chips[2].rect.w, 988.0, "the buttons end a pad in from the right");
        assert!(t.bits.iter().all(|b| b.x + b.text.chars().count() as f64 * 7.0 <= t.chips[0].rect.x), "nothing runs under the buttons");
        // without mail, no InMail button, and New and Resume where they are on every row
        let quiet = &card.rows[2];
        assert!(quiet.quiet && quiet.bits.is_empty());
        assert_eq!(quiet.chips.iter().map(|c| c.action).collect::<Vec<_>>(), vec![Action::New, Action::Resume]);
        assert_eq!((quiet.chips[0].rect.x, quiet.chips[1].rect.x), (t.chips[0].rect.x, t.chips[2].rect.x));
        assert!(card.rows[1].name.ends_with('\u{2026}'), "a long name is cut");
        assert_eq!(card.more, None);
        assert_eq!(card.rect.h, 134.0 + 3.0 * 24.0 + 6.0 - 100.0);
    }

    #[test]
    fn rows_that_dont_fit_are_the_quietest_and_are_counted() {
        let names: Vec<String> = (0..20).map(|i| format!("P{i:02}")).collect();
        let names: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let mut counts = vec![Counts::default(); 20];
        counts[19].working = 1;
        let card = card(&names, &counts, &[None; 20], 1000.0, (100.0, 300.0), 480.0, (7.0, 16.0), 1.0).unwrap();
        // 134 to 300 less half a pad holds 6 rows: 5 and the line saying the rest
        assert_eq!(card.rows.len(), 5);
        assert_eq!(card.rows[0].project, 19, "the busy one first");
        assert_eq!(card.more.as_ref().map(|m| m.2.as_str()), Some("15 more, quieter"));
        assert!(card.rect.y + card.rect.h <= 300.0);
        // too narrow for the legend
        let narrow = card_narrow(&names, &counts);
        assert!(narrow.legend.is_empty());
    }

    fn card_narrow(names: &[&str], counts: &[Counts]) -> Card {
        card(names, counts, &[None; 20], 1000.0, (100.0, 900.0), 260.0, (7.0, 16.0), 1.0).unwrap()
    }
}
