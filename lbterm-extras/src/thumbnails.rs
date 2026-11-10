//! LBTerm: thumbnails of the images a pane links to (lbterm/THUMBNAILS.md). A link to
//! `lbterm://image/<path>` is an image's caption; the rows under it hold links to the same image
//! with `?row=<r>&of=<n>`, words of blank cells only, and the image is drawn small into the cells
//! those cover. Each row says where it sits in the picture, so a picture half scrolled out of view
//! still shows the half that's in it. What a click does is the config's `open-uri` handler's.
//!
//! The reading of the links, laying the pictures out in the rows they cover, and decoding an image
//! small (with limits, off the GUI thread), tested without a window (`cargo test -p lbterm-extras
//! thumbnails`). An SVG is drawn at the size of its box instead, so a chart a dashboard writes (a
//! live tile, lbterm/THUMBNAILS.md) is as sharp at any font size. The GUI side is wezterm-gui's
//! `thumbnails.rs` and `render/thumbnails.rs`.

use anyhow::Context;
use resvg::{tiny_skia, usvg};
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// How a link to an image starts
pub const LINK: &str = "lbterm://image/";

/// The most rows a picture can take
pub const MAX_ROWS: usize = 40;

/// The hairline round a picture: the tab rail's ring at rest (`#5E5E6E`; lbterm/DESIGN.md)
pub const HAIRLINE: u32 = 0x5E5E6E;
/// ...and while the pointer's over it: white at 50%, the light's top
pub const HAIRLINE_HOVER: u32 = 0x898989;
/// A picture whose file has gone: its box filled white at 4% and outlined white at 14%, its
/// caption grey (white at 40%, the read InMail link's) and struck through
pub const GONE_FILL: u32 = 0x1C1C1C;
pub const GONE_OUTLINE: u32 = 0x333333;
pub const GONE_CAPTION: u32 = 0x717171;

/// The largest file decoded: anything bigger shows as unreadable
pub const MAX_FILE_BYTES: u64 = 256 << 20;
/// The largest side an image may say it has, and the most its decoding may allocate
pub const MAX_SIDE: u32 = 30_000;
pub const MAX_DECODE_BYTES: u64 = 512 << 20;
/// The largest SVG drawn: its text is read and parsed whole, so far less than a picture's file
pub const MAX_SVG_BYTES: u64 = 8 << 20;

/// Where a picture row sits in its picture: row `index` of `of`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Row {
    pub index: usize,
    pub of: usize,
}

/// A link to an image: its file, and for a picture row where the row sits; `None` is the caption
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageLink {
    pub path: String,
    pub row: Option<Row>,
}

/// `%XX` made the byte it stands for; a `%` without two hex digits stays as it is
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What a link to an image names, or nothing for any other link. The path, decoded, must be a
/// local drive's (`C:/...` or `C:\...`): a network path would have the GUI reach out to a server
/// to draw a link someone else wrote. A query with `row` and `of` (`row` below `of`, `of` at most
/// [`MAX_ROWS`]) makes it a picture row; one with only one of them, or out of range, isn't a link
/// to an image at all.
pub fn parse(uri: &str) -> Option<ImageLink> {
    let rest = uri.strip_prefix(LINK)?;
    let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
    let (path, query) = rest.split_once('?').map_or((rest, None), |(p, q)| (p, Some(q)));
    let path = percent_decode(path);
    let b = path.as_bytes();
    let drive = b.len() >= 4 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\');
    if !drive || path.chars().any(|c| c.is_control()) {
        return None;
    }
    let mut row = None;
    let mut of = None;
    for pair in query.unwrap_or("").split('&') {
        match pair.split_once('=') {
            Some(("row", v)) => row = Some(v.parse::<usize>().ok()?),
            Some(("of", v)) => of = Some(v.parse::<usize>().ok()?),
            _ => {}
        }
    }
    let row = match (row, of) {
        (None, None) => None,
        (Some(index), Some(of)) if index < of && of <= MAX_ROWS => Some(Row { index, of }),
        _ => return None,
    };
    Some(ImageLink { path, row })
}

/// A run of cells on screen that link to one picture row: the row of the view it's on and the
/// columns it covers
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub row: usize,
    pub cols: Range<usize>,
    pub path: String,
    pub at: Row,
}

/// A picture to draw: its file, the rows of the view its picture would take whole (`top` above
/// the view when it's scrolled part out), the columns its widest row covers, and the rows of the
/// view it's drawn in: from the first row found to the last, so what's out of view, or a row an
/// agent left out, takes nothing
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub path: String,
    pub top: isize,
    pub rows: usize,
    pub cols: Range<usize>,
    pub shown: Range<usize>,
}

/// The pictures `runs` make: runs of the same file whose rows put the picture's top on the same
/// row and start in the same column are one picture's
pub fn blocks(runs: &[Run]) -> Vec<Block> {
    let mut out: Vec<Block> = vec![];
    for run in runs {
        let top = run.row as isize - run.at.index as isize;
        match out
            .iter_mut()
            .find(|b| b.path == run.path && b.top == top && b.rows == run.at.of && b.cols.start == run.cols.start)
        {
            Some(b) => {
                b.cols.end = b.cols.end.max(run.cols.end);
                b.shown = b.shown.start.min(run.row)..b.shown.end.max(run.row + 1);
            }
            None => out.push(Block {
                path: run.path.clone(),
                top,
                rows: run.at.of,
                cols: run.cols.clone(),
                shown: run.row..run.row + 1,
            }),
        }
    }
    out
}

/// The cells of `cols` linking to a picture row, as `(column, uri)` for each cell with a link, put
/// together as runs: consecutive cells with the same link. Cells of any other link, or none, end
/// a run.
pub fn runs_in_row<'a>(row: usize, cells: impl IntoIterator<Item = (usize, usize, &'a str)>) -> Vec<Run> {
    let mut out: Vec<Run> = vec![];
    let mut last: Option<(&str, usize)> = None;
    for (col, width, uri) in cells {
        if let (Some((was, end)), Some(run)) = (last, out.last_mut()) {
            if was == uri && end == col {
                run.cols.end = col + width;
                last = Some((uri, col + width));
                continue;
            }
        }
        last = None;
        let Some(ImageLink { path, row: Some(at) }) = parse(uri) else {
            continue;
        };
        out.push(Run { row, cols: col..col + width, path, at });
        last = Some((uri, col + width));
    }
    out
}

/// The size a picture of `w` × `h` is drawn at in a box of `box_w` × `box_h` pixels: as big as
/// fits with its shape kept, never bigger than it is, at least a pixel each way
pub fn fit(w: u32, h: u32, box_w: u32, box_h: u32) -> (u32, u32) {
    if w == 0 || h == 0 || box_w == 0 || box_h == 0 {
        return (0, 0);
    }
    let scale = (box_w as f64 / w as f64).min(box_h as f64 / h as f64).min(1.0);
    (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1))
}

/// The size a picture with no pixel size of its own, an SVG `w` × `h` across, is drawn at in a box
/// of `box_w` × `box_h` pixels: as big as fits with its shape kept, bigger than it says it is too
pub fn fit_vector(w: f32, h: f32, box_w: u32, box_h: u32) -> (u32, u32) {
    if !(w > 0.0 && h > 0.0) || box_w == 0 || box_h == 0 {
        return (0, 0);
    }
    let scale = (box_w as f64 / w as f64).min(box_h as f64 / h as f64);
    let side = |v: f32, most: u32| ((v as f64 * scale).round() as u32).clamp(1, most);
    (side(w, box_w), side(h, box_h))
}

/// Whether `path` names an SVG, by its extension: an SVG is drawn at its box's size, not decoded
pub fn is_svg(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

/// An image decoded small: its size and its pixels, RGBA, row by row
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The file at `path` decoded and made the size it's drawn at in a box of `box_w` × `box_h`
/// ([`fit`]), the first frame of an animation. Refused: a file over [`MAX_FILE_BYTES`], an image
/// saying a side is over [`MAX_SIDE`], or one whose decoding would allocate over
/// [`MAX_DECODE_BYTES`], so nothing a link names can take the memory a 41 GB viewer once did
/// (Terminal's image viewer, 2026-10-21).
///
/// An SVG ([`is_svg`]) is drawn instead ([`draw_svg`]).
pub fn decode(path: &Path, box_w: u32, box_h: u32) -> anyhow::Result<Decoded> {
    if is_svg(path) {
        return draw_svg(path, box_w, box_h);
    }
    let len = std::fs::metadata(path)?.len();
    anyhow::ensure!(len <= MAX_FILE_BYTES, "{len} bytes, over the {MAX_FILE_BYTES} a thumbnail reads");
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let image = reader.decode()?;
    let (w, h) = fit(image.width(), image.height(), box_w, box_h);
    anyhow::ensure!(w > 0 && h > 0, "an empty image, or no room for it");
    let image = if (w, h) == (image.width(), image.height()) {
        image
    } else {
        image.resize_exact(w, h, image::imageops::FilterType::CatmullRom)
    };
    let rgba = image.to_rgba8();
    Ok(Decoded { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

/// The fonts an SVG's text is drawn in: the system's, found the first time an SVG is drawn (on the
/// thumbnails' thread) and kept, since finding them reads every font file's tables
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    Arc::clone(FONTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    }))
}

/// The SVG at `path` drawn as big as fits a box of `box_w` × `box_h` ([`fit_vector`]), its text in
/// the system's fonts. It draws only itself: no file its `<image>` names is read, an image inside
/// it as a data URL isn't drawn either (no raster decoders are built in), and a compressed one
/// (gzip, `.svgz`) isn't opened, so a small file can't unpack into a big one. Refused: a file over
/// [`MAX_SVG_BYTES`], or one that isn't an SVG.
pub fn draw_svg(path: &Path, box_w: u32, box_h: u32) -> anyhow::Result<Decoded> {
    let len = std::fs::metadata(path)?.len();
    anyhow::ensure!(len <= MAX_SVG_BYTES, "{len} bytes, over the {MAX_SVG_BYTES} an SVG thumbnail reads");
    let data = std::fs::read(path)?;
    anyhow::ensure!(!data.starts_with(&[0x1f, 0x8b]), "a compressed SVG, which a thumbnail doesn't open");
    let mut opt = usvg::Options::default();
    opt.fontdb = fonts();
    opt.image_href_resolver.resolve_string = Box::new(|_: &str, _: &usvg::Options| None);
    let tree = usvg::Tree::from_data(&data, &opt)?;
    let size = tree.size();
    let (w, h) = fit_vector(size.width(), size.height(), box_w, box_h);
    anyhow::ensure!(w > 0 && h > 0, "an empty SVG, or no room for it");
    let mut pixmap = tiny_skia::Pixmap::new(w, h).context("a pixmap for the SVG")?;
    let transform = tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    // tiny-skia's pixels have their alpha multiplied in; a thumbnail's are straight, as decoded
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Ok(Decoded { width: w, height: h, rgba })
}

/// What a file's thumbnail is now: what's decoded is only good while the file's time and size
/// stay as they were when it was read
pub fn stamp(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    meta.is_file().then(|| (meta.modified().unwrap_or(std::time::UNIX_EPOCH), meta.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caption_and_a_row_name_the_same_file() {
        assert_eq!(
            parse("lbterm://image/C:/Users/x/shot%201.png"),
            Some(ImageLink { path: "C:/Users/x/shot 1.png".into(), row: None })
        );
        assert_eq!(
            parse("lbterm://image/C:/Users/x/shot%201.png?row=2&of=5"),
            Some(ImageLink { path: "C:/Users/x/shot 1.png".into(), row: Some(Row { index: 2, of: 5 }) })
        );
        // Backslashes, a fragment, a parameter it doesn't know
        assert_eq!(
            parse("lbterm://image/D:%5Cpics%5Ca.jpg?of=3&x=1&row=0#top"),
            Some(ImageLink { path: "D:\\pics\\a.jpg".into(), row: Some(Row { index: 0, of: 3 }) })
        );
    }

    #[test]
    fn only_a_local_drive_s_files() {
        for uri in [
            "lbterm://image///server/share/a.png",
            "lbterm://image/%5C%5Cserver%5Cshare%5Ca.png",
            "lbterm://image/relative/a.png",
            "lbterm://image/C:",
            "lbterm://image/C:a.png",
            "lbterm://image/C:/a%0A.png",
            "lbterm://inmail/One",
            "https://example.com/a.png",
        ] {
            assert_eq!(parse(uri), None, "{uri}");
        }
    }

    #[test]
    fn a_row_needs_both_numbers_in_range() {
        for uri in [
            "lbterm://image/C:/a.png?row=1",
            "lbterm://image/C:/a.png?of=5",
            "lbterm://image/C:/a.png?row=5&of=5",
            "lbterm://image/C:/a.png?row=-1&of=5",
            "lbterm://image/C:/a.png?row=0&of=41",
            "lbterm://image/C:/a.png?row=x&of=5",
        ] {
            assert_eq!(parse(uri), None, "{uri}");
        }
        assert!(parse("lbterm://image/C:/a.png?row=39&of=40").is_some());
    }

    fn cells(spec: &[(usize, &'static str)]) -> Vec<(usize, usize, &'static str)> {
        spec.iter().map(|&(col, uri)| (col, 1, uri)).collect()
    }

    #[test]
    fn runs_are_cells_of_one_link_side_by_side() {
        let a = "lbterm://image/C:/a.png?row=1&of=3";
        let b = "lbterm://image/C:/b.png?row=1&of=3";
        let other = "https://example.com";
        let runs = runs_in_row(7, cells(&[(2, a), (3, a), (4, a), (6, other), (8, b), (9, b), (11, b)]));
        assert_eq!(
            runs,
            vec![
                Run { row: 7, cols: 2..5, path: "C:/a.png".into(), at: Row { index: 1, of: 3 } },
                Run { row: 7, cols: 8..10, path: "C:/b.png".into(), at: Row { index: 1, of: 3 } },
                // A gap ends a run, even of the same link
                Run { row: 7, cols: 11..12, path: "C:/b.png".into(), at: Row { index: 1, of: 3 } },
            ]
        );
        // A caption isn't a picture row
        assert!(runs_in_row(0, cells(&[(2, "lbterm://image/C:/a.png")])).is_empty());
    }

    fn run(row: usize, cols: Range<usize>, path: &str, index: usize, of: usize) -> Run {
        Run { row, cols, path: path.into(), at: Row { index, of } }
    }

    #[test]
    fn rows_of_one_picture_make_one_block() {
        let runs = [
            run(4, 2..20, "C:/a.png", 0, 3),
            run(4, 24..30, "C:/b.png", 0, 3),
            run(5, 2..22, "C:/a.png", 1, 3),
            run(5, 24..30, "C:/b.png", 1, 3),
            run(6, 2..20, "C:/a.png", 2, 3),
            run(6, 24..30, "C:/b.png", 2, 3),
        ];
        assert_eq!(
            blocks(&runs),
            vec![
                Block { path: "C:/a.png".into(), top: 4, rows: 3, cols: 2..22, shown: 4..7 },
                Block { path: "C:/b.png".into(), top: 4, rows: 3, cols: 24..30, shown: 4..7 },
            ]
        );
    }

    #[test]
    fn a_picture_scrolled_half_out_shows_what_s_in_view() {
        // Rows 3 and 4 of 5 at the top of the view: the picture's top is three rows above it
        let runs = [run(0, 2..20, "C:/a.png", 3, 5), run(1, 2..20, "C:/a.png", 4, 5)];
        assert_eq!(blocks(&runs), vec![Block { path: "C:/a.png".into(), top: -3, rows: 5, cols: 2..20, shown: 0..2 }]);
    }

    #[test]
    fn the_same_image_twice_is_two_pictures() {
        let runs = [run(2, 2..20, "C:/a.png", 0, 2), run(3, 2..20, "C:/a.png", 1, 2), run(9, 2..20, "C:/a.png", 0, 2)];
        let found = blocks(&runs);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].top, found[0].shown.clone()), (2, 2..4));
        assert_eq!((found[1].top, found[1].shown.clone()), (9, 9..10));
    }

    #[test]
    fn fit_keeps_the_shape_and_never_grows() {
        assert_eq!(fit(2400, 1600, 180, 100), (150, 100));
        assert_eq!(fit(2400, 160, 400, 100), (400, 27));
        assert_eq!(fit(64, 48, 400, 100), (64, 48));
        assert_eq!(fit(10_000, 1, 400, 100), (400, 1));
        assert_eq!(fit(0, 10, 400, 100), (0, 0));
    }

    fn png(dir: &Path, name: &str, w: u32, h: u32) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut img = image::RgbaImage::new(w, h);
        for p in img.pixels_mut() {
            *p = image::Rgba([255, 0, 255, 255]);
        }
        img.save(&path).unwrap();
        path
    }

    #[test]
    fn decode_makes_it_the_size_it_s_drawn_at() {
        let dir = tempfile::tempdir().unwrap();
        let big = decode(&png(dir.path(), "big.png", 800, 400), 180, 100).unwrap();
        assert_eq!((big.width, big.height, big.rgba.len()), (180, 90, 180 * 90 * 4));
        assert_eq!(&big.rgba[..4], &[255, 0, 255, 255]);
        let small = decode(&png(dir.path(), "small.png", 40, 30), 180, 100).unwrap();
        assert_eq!((small.width, small.height), (40, 30));
    }

    #[test]
    fn decode_refuses_what_isn_t_an_image_or_is_too_big() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("not.png");
        std::fs::write(&text, "not an image").unwrap();
        assert!(decode(&text, 180, 100).is_err());
        assert!(decode(&dir.path().join("missing.png"), 180, 100).is_err());
        // A PNG whose header says it's 40,000 pixels wide: refused before anything's allocated
        let mut header = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
        header.extend(40_000u32.to_be_bytes());
        header.extend(10u32.to_be_bytes());
        header.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let huge = dir.path().join("huge.png");
        std::fs::write(&huge, header).unwrap();
        assert!(decode(&huge, 180, 100).is_err());
    }

    #[test]
    fn fit_vector_fills_the_box_with_the_shape_kept() {
        assert_eq!(fit_vector(900.0, 230.0, 1800, 230), (900, 230));
        // An SVG grows to its box, as nothing it says is its size in pixels
        assert_eq!(fit_vector(90.0, 23.0, 1800, 230), (900, 230));
        assert_eq!(fit_vector(900.0, 230.0, 450, 230), (450, 115));
        assert_eq!(fit_vector(0.0, 10.0, 400, 100), (0, 0));
        assert_eq!(fit_vector(f32::NAN, 10.0, 400, 100), (0, 0));
        assert_eq!(fit_vector(10.0, 10.0, 0, 100), (0, 0));
    }

    fn svg(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    fn pixel(d: &Decoded, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * d.width + x) * 4) as usize;
        d.rgba[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn an_svg_is_drawn_at_its_box_s_size() {
        let dir = tempfile::tempdir().unwrap();
        // Magenta on its left half, nothing on its right
        let path = svg(
            dir.path(),
            "chart.SVG",
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 90 23"><rect width="45" height="23" fill="#ff00ff"/></svg>"##,
        );
        assert!(is_svg(&path));
        let d = decode(&path, 1800, 230).unwrap();
        assert_eq!((d.width, d.height, d.rgba.len()), (900, 230, 900 * 230 * 4));
        assert_eq!(pixel(&d, 10, 100), [255, 0, 255, 255]);
        assert_eq!(pixel(&d, 800, 100)[3], 0);
    }

    #[test]
    fn an_svg_s_half_covered_pixels_keep_their_colour() {
        let dir = tempfile::tempdir().unwrap();
        // Half opaque: tiny-skia multiplies the alpha in, and it's taken out again
        let path = svg(
            dir.path(),
            "half.svg",
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#ff8000" fill-opacity="0.5"/></svg>"##,
        );
        let [r, g, b, a] = pixel(&decode(&path, 10, 10).unwrap(), 5, 5);
        assert!((127..=129).contains(&a), "{a}");
        assert_eq!(r, 255);
        assert!((126..=130).contains(&g), "{g}");
        assert_eq!(b, 0);
    }

    #[test]
    fn an_svg_s_text_is_drawn_in_the_system_s_fonts() {
        let dir = tempfile::tempdir().unwrap();
        let path = svg(
            dir.path(),
            "text.svg",
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 40"><text x="4" y="30" font-family="Arial, sans-serif" font-size="28" fill="#ffffff">CPU 74°</text></svg>"##,
        );
        let d = decode(&path, 200, 40).unwrap();
        let inked = d.rgba.chunks(4).filter(|p| p[3] > 0).count();
        assert!(inked > 200, "only {inked} pixels drawn: no font found for the text");
    }

    #[test]
    fn an_svg_draws_only_itself() {
        let dir = tempfile::tempdir().unwrap();
        let red = png(dir.path(), "red.png", 10, 10);
        let href = red.to_string_lossy().replace('\\', "/");
        let path = svg(
            dir.path(),
            "links.svg",
            &format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10"><image width="10" height="10" xlink:href="{href}"/><image width="10" height="10" href="red.png"/></svg>"##
            ),
        );
        let d = decode(&path, 10, 10).unwrap();
        assert!(d.rgba.chunks(4).all(|p| p[3] == 0), "drew a file the SVG names");
    }

    #[test]
    fn an_svg_that_isn_t_one_or_is_too_big_or_packed_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(decode(&svg(dir.path(), "not.svg", "not an svg"), 100, 100).is_err());
        let empty = svg(dir.path(), "empty.svg", r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"/>"#);
        assert!(decode(&empty, 100, 100).is_err());
        assert!(decode(&dir.path().join("missing.svg"), 100, 100).is_err());
        let packed = dir.path().join("packed.svg");
        std::fs::write(&packed, [0x1f, 0x8b, 8, 0, 0, 0, 0, 0]).unwrap();
        assert!(decode(&packed, 100, 100).is_err());
        let big = dir.path().join("big.svg");
        std::fs::File::create(&big).unwrap().set_len(MAX_SVG_BYTES + 1).unwrap();
        assert!(decode(&big, 100, 100).is_err());
    }

    #[test]
    fn a_stamp_while_the_file_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let path = png(dir.path(), "a.png", 4, 4);
        assert_eq!(stamp(&path).map(|(_, len)| len), Some(std::fs::metadata(&path).unwrap().len()));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(stamp(&path), None);
        assert_eq!(stamp(dir.path()), None);
    }
}
