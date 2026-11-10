//! A row's words, ported from TabPanel.exe's Text.cs so both panels say the
//! same thing about a tab.

/// A title without the glyphs Claude Code puts in front of it (✳, a
/// braille spinner frame, ◑ ...): everything up to the first letter, digit,
/// `_` or `/` goes.
pub fn strip_glyphs(title: &str) -> &str {
    title
        .trim_start_matches(|c: char| !(c.is_alphanumeric() || c == '_' || c == '/'))
        .trim()
}

/// A pane title that is just a program path (`C:\...\pwsh.exe`) reads
/// better as `pwsh`.
pub fn program_name(title: &str) -> &str {
    let lower = title.to_ascii_lowercase();
    if !lower.ends_with(".exe") {
        return title;
    }
    let file = title.rsplit(['\\', '/']).next().unwrap_or(title);
    &file[..file.len() - 4]
}

/// A Windows path from a plain path or a `file://` URL as WezTerm keeps a
/// pane's working directory.
pub fn cwd_path(cwd: &str) -> String {
    let mut p = cwd.to_string();
    if p.len() >= 7 && p[..7].eq_ignore_ascii_case("file://") {
        let rest = &p[7..];
        let rest = match rest.strip_prefix('/') {
            Some(path) => path,
            // file://host/C:/... drops the host
            None => rest.split_once('/').map_or("", |(_, path)| path),
        };
        p = percent_decode(rest);
    }
    let mut p = p.replace('/', "\\");
    while p.len() > 3 && p.ends_with('\\') {
        p.pop();
    }
    p
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied(), bytes.get(i + 2).copied()) {
            (b'%', Some(h), Some(l)) if hex(h).is_some() && hex(l).is_some() => {
                out.push((hex(h).unwrap() * 16 + hex(l).unwrap()) as u8);
                i += 3;
            }
            (b, ..) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The project a working directory belongs to: the megaproject's one name for its folder,
/// from Blueprint's names (`names.rs`), as the statusline and every other surface give it.
pub fn project(cwd: &str) -> String {
    crate::names::project(&cwd_path(cwd))
}

/// How long ago, as the panel shows it: `now` for the first minute, then
/// `12m`, `3h`, `2d`. Minutes and no seconds, so an idle row repaints once
/// a minute at most.
pub fn ago(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    let m = s / 60;
    if m == 0 {
        "now".to_string()
    } else if m < 60 {
        format!("{m}m")
    } else if m < 1440 {
        format!("{}h", m / 60)
    } else {
        format!("{}d", m / 1440)
    }
}

/// How long a guess says is left, short: "~4 min", "~1 min" for anything under a minute and a
/// half, "~2 h" from an hour and a half.
pub fn left(seconds: u32) -> String {
    if seconds < 90 {
        "~1 min".to_string()
    } else if seconds < 90 * 60 {
        format!("~{} min", (seconds as f64 / 60.0).round() as u64)
    } else {
        format!("~{} h", (seconds as f64 / 3600.0).round() as u64)
    }
}

/// What a conversation has cost, short enough for a pill on the strip or a row's corner:
/// cents under $100 ("$0.42", "$12.34"), whole dollars to $10,000 ("$148", "$2304"), then
/// thousands ("$12.3k", "$140k"). Never rounded up past what it is to the cent: $99.996 is
/// "$99.99", so the dollars shown never run ahead of what was spent.
pub fn dollars(usd: f64) -> String {
    let usd = if usd.is_finite() { usd.max(0.0) } else { 0.0 };
    let cents = (usd * 100.0 + 1e-6).floor() / 100.0;
    if cents < 100.0 {
        format!("${cents:.2}")
    } else if usd < 10_000.0 {
        format!("${}", usd.floor() as u64)
    } else {
        let k = (usd / 100.0).floor() / 10.0;
        if k < 100.0 {
            format!("${k:.1}k")
        } else {
            format!("${}k", k.floor() as u64)
        }
    }
}

/// Seconds after `seconds` until [`ago`] reads differently.
pub fn ago_changes_in(seconds: f64) -> f64 {
    let s = seconds.max(0.0);
    let unit = if s < 3600.0 {
        60.0
    } else if s < 86_400.0 {
        3600.0
    } else {
        86_400.0
    };
    ((s / unit).floor() + 1.0) * unit - s
}

/// `text` cut to `columns` with an ellipsis when it doesn't fit; `width`
/// says how many columns a character takes.
pub fn fit(text: &str, columns: usize, width: impl Fn(char) -> usize) -> String {
    let total: usize = text.chars().map(&width).sum();
    if total <= columns {
        return text.to_string();
    }
    if columns == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = width(c);
        if used + w > columns - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.truncate(out.trim_end().len());
    out.push('…');
    out
}

/// `text` in at most `lines` lines of at most `columns` columns, broken
/// between words: a word too long for a line is cut short on a line of its
/// own, and the last line takes what's left, cut short if it must be.
pub fn wrap(text: &str, columns: usize, lines: usize, width: impl Fn(char) -> usize + Copy) -> Vec<String> {
    let span = |s: &str| s.chars().map(width).sum::<usize>();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() && out.len() < lines {
        if out.len() + 1 == lines {
            out.push(fit(&words[i..].join(" "), columns, width));
            break;
        }
        let mut line = words[i].to_string();
        i += 1;
        if span(&line) > columns {
            out.push(fit(&line, columns, width));
            continue;
        }
        while i < words.len() && span(&line) + 1 + span(words[i]) <= columns {
            line.push(' ');
            line.push_str(words[i]);
            i += 1;
        }
        out.push(line);
    }
    out
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn dollars_read_short_and_never_run_ahead_of_the_cent() {
        assert_eq!(dollars(0.0), "$0.00");
        assert_eq!(dollars(0.84), "$0.84");
        assert_eq!(dollars(12.34), "$12.34");
        assert_eq!(dollars(12.349), "$12.34", "cut, not rounded up");
        assert_eq!(dollars(99.996), "$99.99");
        assert_eq!(dollars(100.0), "$100");
        assert_eq!(dollars(148.73), "$148");
        assert_eq!(dollars(2304.5), "$2304");
        assert_eq!(dollars(12_345.0), "$12.3k");
        assert_eq!(dollars(140_200.0), "$140k");
        assert_eq!(dollars(-3.0), "$0.00");
        assert_eq!(dollars(f64::NAN), "$0.00");
    }

    #[test]
    fn time_left_reads_short() {
        assert_eq!(left(0), "~1 min");
        assert_eq!(left(89), "~1 min");
        assert_eq!(left(90), "~2 min");
        assert_eq!(left(240), "~4 min");
        assert_eq!(left(89 * 60), "~89 min");
        assert_eq!(left(90 * 60), "~2 h");
        assert_eq!(left(5 * 3600), "~5 h");
    }

    #[test]
    fn labels_wrap_between_words_and_cut_what_wont_fit() {
        let one = |_: char| 1;
        assert_eq!(wrap("Tab bar bugs", 7, 2, one), ["Tab bar", "bugs"]);
        assert_eq!(wrap("Lodestone", 7, 2, one), ["Lodest…"]);
        assert_eq!(wrap("Dogfooding LBTerm", 7, 2, one), ["Dogfoo…", "LBTerm"]);
        assert_eq!(wrap("Work status", 7, 1, one), ["Work s…"]);
        assert_eq!(wrap("a b c d e f g h", 3, 2, one), ["a b", "c…"]);
        assert_eq!(wrap("  ", 7, 2, one), Vec::<String>::new());
        assert_eq!(wrap("Summary bar", 7, 2, one), ["Summary", "bar"]);
    }

    #[test]
    fn claude_glyphs_come_off_the_title() {
        assert_eq!(strip_glyphs("✳ Tab panel for LBTerm"), "Tab panel for LBTerm");
        assert_eq!(strip_glyphs("⠂ Building"), "Building");
        assert_eq!(strip_glyphs("◐ ◑ two"), "two");
        assert_eq!(strip_glyphs("/usr/bin"), "/usr/bin");
        assert_eq!(strip_glyphs("✳ "), "");
    }

    #[test]
    fn a_program_path_reads_as_its_name() {
        assert_eq!(program_name(r"C:\Program Files\PowerShell\7\pwsh.exe"), "pwsh");
        assert_eq!(program_name("C:/Windows/System32/cmd.EXE"), "cmd");
        assert_eq!(program_name("vim notes.md"), "vim notes.md");
    }

    #[test]
    fn cwd_urls_become_windows_paths() {
        assert_eq!(
            cwd_path("file:///C:/Users/me/My%20Notes/"),
            r"C:\Users\me\My Notes"
        );
        assert_eq!(cwd_path("file://DESKTOP-1/C:/Code/"), r"C:\Code");
        assert_eq!(cwd_path(r"C:\"), r"C:\");
        assert_eq!(cwd_path(""), "");
    }

    #[test]
    fn ages_count_minutes_hours_days() {
        assert_eq!(ago(0.0), "now");
        assert_eq!(ago(59.9), "now");
        assert_eq!(ago(60.0), "1m");
        assert_eq!(ago(3599.0), "59m");
        assert_eq!(ago(3600.0), "1h");
        assert_eq!(ago(86_400.0 * 3.0), "3d");
    }

    #[test]
    fn the_age_changes_when_its_text_does() {
        assert_eq!(ago_changes_in(0.0), 60.0);
        assert_eq!(ago_changes_in(90.0), 30.0);
        assert_eq!(ago_changes_in(3600.0), 3600.0);
        let t = 7000.0;
        assert_ne!(ago(t), ago(t + ago_changes_in(t)));
        assert_eq!(ago(t), ago(t + ago_changes_in(t) - 0.5));
    }

    #[test]
    fn long_text_is_cut_with_an_ellipsis() {
        let one = |_c: char| 1;
        assert_eq!(fit("short", 10, one), "short");
        assert_eq!(fit("exactly ten", 11, one), "exactly ten");
        assert_eq!(fit("a much longer title", 10, one), "a much lo…");
        // No space left dangling before the ellipsis
        assert_eq!(fit("tab panel motion", 5, one), "tab…");
        let wide = |c: char| if c == '漢' { 2 } else { 1 };
        assert_eq!(fit("漢漢漢漢", 5, wide), "漢漢…");
        assert_eq!(fit("anything", 0, one), "");
    }
}
