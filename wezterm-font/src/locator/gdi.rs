#![cfg(windows)]

use crate::locator::{FontDataSource, FontLocator, FontOrigin};
use crate::parser::{best_matching_font, parse_and_collect_font_info, ParsedFont};
use config::{
    FontAttributes, FontStretch as WTFontStretch, FontStyle as WTFontStyle,
    FontWeight as WTFontWeight,
};
use dwrote::{FontDescriptor, FontStretch, FontStyle, FontWeight};
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;
use winapi::shared::windef::{HDC, HFONT};
use winapi::um::dwrite::*;
use winapi::um::winbase::MulDiv;
use winapi::um::wingdi::{
    CreateCompatibleDC, CreateFontIndirectW, DeleteDC, DeleteObject, GetDeviceCaps, GetFontData,
    GetTextFaceW, SelectObject, FIXED_PITCH, GDI_ERROR, LF_FACESIZE, LOGFONTW, LOGPIXELSY,
    OUT_TT_ONLY_PRECIS,
};

/// A FontLocator implemented using the system font loading
/// functions provided by the font-loader crate.
pub struct GdiFontLocator {}

/// The name of the face GDI selected into `hdc`, if it says
fn selected_face(hdc: HDC) -> Option<String> {
    let mut name = [0u16; LF_FACESIZE];
    let len = unsafe { GetTextFaceW(hdc, name.len() as _, name.as_mut_ptr()) };
    if len <= 0 {
        return None;
    }
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    String::from_utf16(&name[..len]).ok()
}

/// `font`'s whole file, copied out of GDI. With `want` (LBTerm), only when the face GDI selected
/// for it is that family: for a family Windows doesn't have, GDI selects another (Courier New, a
/// font substitute) and upstream copied and parsed that whole file only to turn it down by name,
/// for each such family of every style a window resolves (the bundled JetBrains Mono, Noto Color
/// Emoji and Symbols Nerd Font Mono here, 11 files a config save)
fn extract_raw_font_data(
    font: HFONT,
    name: &str,
    want: Option<&str>,
) -> anyhow::Result<FontDataSource> {
    unsafe {
        let hdc = CreateCompatibleDC(std::ptr::null_mut());
        SelectObject(hdc, font as *mut _);

        if let Some(want) = want {
            match selected_face(hdc) {
                Some(face) if !face.eq_ignore_ascii_case(want) => {
                    DeleteDC(hdc);
                    crate::lbterm::SUBSTITUTES.one();
                    anyhow::bail!("gdi selected {face:?} in place of {want:?}");
                }
                _ => {}
            }
        }
        let read_at = std::time::Instant::now();

        // GetFontData can retrieve different parts of the font data.
        // We want to fetch the entire font file, but things are made
        // more complicated because the file may be a TTC file.
        // In that case, the full file data isn't full parsable
        // as a TTF so we need to ask specifically for the TTC file,
        // and then try to reverse engineer which element of the TTC
        // is the one we were looking for.

        // See if we can retrieve the ttc data as a first try
        let ttc_table = 0x66637474; // 'ttcf'

        let ttc_size = GetFontData(hdc, ttc_table, 0, std::ptr::null_mut(), 0);

        let data = if ttc_size > 0 && ttc_size != GDI_ERROR {
            let mut data = vec![0u8; ttc_size as usize];
            GetFontData(hdc, ttc_table, 0, data.as_mut_ptr() as *mut _, ttc_size);

            Ok(data)
        } else {
            // Otherwise: presumably a regular ttf

            let size = GetFontData(hdc, 0, 0, std::ptr::null_mut(), 0);
            match size {
                _ if size > 0 && size != GDI_ERROR => {
                    let mut data = vec![0u8; size as usize];
                    GetFontData(hdc, 0, 0, data.as_mut_ptr() as *mut _, size);
                    Ok(data)
                }
                _ => Err(anyhow::anyhow!("Failed to get font data")),
            }
        };
        DeleteDC(hdc);
        let data = data?;
        crate::lbterm::FILES_READ.since(read_at);

        Ok(FontDataSource::Memory {
            data: Arc::new(data.into_boxed_slice()),
            name: name.to_string(),
        })
    }
}

fn extract_font_data(
    font: HFONT,
    attr: &FontAttributes,
    pixel_size: u16,
) -> anyhow::Result<ParsedFont> {
    let source = extract_raw_font_data(font, &attr.family, Some(&attr.family))?;

    let mut font_info = vec![];
    parse_and_collect_font_info(&source, &mut font_info, FontOrigin::Gdi)?;
    let matches = ParsedFont::best_match(attr, pixel_size, font_info);

    match matches {
        Some(m) => Ok(m),
        None => anyhow::bail!("No font matching {:?} in {:?}", attr, source),
    }
}

/// Convert a rust string to a windows wide string
fn wide_string(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn load_font(font_attr: &FontAttributes, pixel_size: u16) -> anyhow::Result<ParsedFont> {
    let mut log_font = LOGFONTW {
        lfHeight: 0,
        lfWidth: 0,
        lfEscapement: 0,
        lfOrientation: 0,
        lfWeight: font_attr.weight.to_opentype_weight() as _,
        lfItalic: if font_attr.style != WTFontStyle::Normal {
            1
        } else {
            0
        },
        lfUnderline: 0,
        lfStrikeOut: 0,
        lfCharSet: 0,
        lfOutPrecision: OUT_TT_ONLY_PRECIS as u8,
        lfClipPrecision: 0,
        lfQuality: 0,
        lfPitchAndFamily: FIXED_PITCH as u8,
        lfFaceName: [0u16; 32],
    };

    let name = wide_string(&font_attr.family);
    if name.len() > LF_FACESIZE {
        anyhow::bail!(
            "family name {:?} is too large for LOGFONTW",
            font_attr.family
        );
    }
    for (i, &c) in name.iter().enumerate() {
        log_font.lfFaceName[i] = c;
    }

    unsafe {
        let font = CreateFontIndirectW(&log_font);
        let result = extract_font_data(font, font_attr, pixel_size);
        DeleteObject(font as *mut _);
        result
    }
}

/// LBTerm: the caption font a window asks for as it's made, on every config reload and on every
/// theme change (window/src/os/windows/window.rs's update_title_font), kept while it's the same
/// font at the same dpi: upstream copied its whole file out of GDI and parsed it each time, one
/// file a save
pub fn parse_log_font(log_font: &LOGFONTW, hdc: HDC) -> anyhow::Result<(ParsedFont, f64)> {
    static LAST: std::sync::Mutex<Option<(Vec<u8>, i32, ParsedFont, f64)>> =
        std::sync::Mutex::new(None);
    let key = unsafe {
        std::slice::from_raw_parts(
            log_font as *const LOGFONTW as *const u8,
            std::mem::size_of::<LOGFONTW>(),
        )
    }
    .to_vec();
    let dpi = unsafe { GetDeviceCaps(hdc, LOGPIXELSY) };
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, d, parsed, size)) = last.as_ref() {
        if *k == key && *d == dpi {
            return Ok((parsed.clone(), *size));
        }
    }
    let (parsed, size) = parse_log_font_now(log_font, hdc)?;
    *last = Some((key, dpi, parsed.clone(), size));
    Ok((parsed, size))
}

fn parse_log_font_now(log_font: &LOGFONTW, hdc: HDC) -> anyhow::Result<(ParsedFont, f64)> {
    let name = String::from_utf16(&log_font.lfFaceName)?;
    unsafe {
        let font = CreateFontIndirectW(log_font);
        let source = extract_raw_font_data(font, &name, None);
        DeleteObject(font as *mut _);
        let source = source?;

        let point_size = MulDiv(-log_font.lfHeight, 72, GetDeviceCaps(hdc, LOGPIXELSY)) as f64;
        let pixel_size = log_font.lfHeight.abs() as u16;

        let mut attr = FontAttributes::new(&name);
        attr.weight = config::FontWeight::from_opentype_weight(log_font.lfWeight as u16);
        if log_font.lfItalic == 1 {
            attr.style = WTFontStyle::Italic;
        }

        let mut font_info = vec![];
        parse_and_collect_font_info(&source, &mut font_info, FontOrigin::Gdi)?;
        let matches = ParsedFont::best_match(&attr, pixel_size, font_info);

        match matches {
            Some(m) => Ok((m, point_size)),
            None => anyhow::bail!("No font matching {:?} in {:?}", attr, source),
        }
    }
}

fn attributes_to_descriptor(font_attr: &FontAttributes) -> FontDescriptor {
    FontDescriptor {
        family_name: font_attr.family.to_string(),
        weight: FontWeight::from_u32(font_attr.weight.to_opentype_weight() as u32),
        stretch: FontStretch::Normal,
        style: match font_attr.style {
            WTFontStyle::Italic => FontStyle::Italic,
            WTFontStyle::Oblique => FontStyle::Oblique,
            WTFontStyle::Normal => FontStyle::Normal,
        },
    }
}

fn handle_from_descriptor(
    attr: &FontAttributes,
    collection: &dwrote::FontCollection,
    descriptor: &FontDescriptor,
    pixel_size: u16,
) -> Option<ParsedFont> {
    let font = collection.get_font_from_descriptor(&descriptor)?;
    let face = font.create_font_face();
    for file in face.get_files() {
        if let Some(path) = file.get_font_file_path() {
            let family_name = font.family_name();

            log::debug!("{} -> {}", family_name, path.display());
            let source = FontDataSource::OnDisk(path);
            match best_matching_font(&source, attr, FontOrigin::DirectWrite, pixel_size) {
                Ok(Some(parsed)) => {
                    return Some(parsed);
                }
                Ok(None) => {}
                Err(err) => log::warn!("While parsing: {:?}: {:#}", source, err),
            }
        }
    }
    None
}

impl FontLocator for GdiFontLocator {
    fn load_fonts(
        &self,
        fonts_selection: &[FontAttributes],
        loaded: &mut HashSet<FontAttributes>,
        pixel_size: u16,
    ) -> anyhow::Result<Vec<ParsedFont>> {
        let mut fonts = Vec::new();
        let collection = dwrote::FontCollection::system();

        for font_attr in fonts_selection {
            let descriptor = attributes_to_descriptor(font_attr);

            fn try_handle(
                font_attr: &FontAttributes,
                parsed: ParsedFont,
                fonts: &mut Vec<ParsedFont>,
                loaded: &mut HashSet<FontAttributes>,
            ) -> bool {
                if parsed.matches_name(font_attr) {
                    fonts.push(parsed);
                    loaded.insert(font_attr.clone());
                    true
                } else {
                    log::debug!("parsed {:?} doesn't match {:?}", parsed, font_attr);
                    false
                }
            }

            match handle_from_descriptor(font_attr, &collection, &descriptor, pixel_size) {
                Some(handle) => {
                    log::debug!("Got {:?} from dwrote", handle);
                    if try_handle(font_attr, handle, &mut fonts, loaded) {
                        continue;
                    }
                }
                None => {
                    log::debug!("dwrote couldn't resolve {:?}", font_attr);
                }
            }

            match load_font(font_attr, pixel_size) {
                Ok(handle) => {
                    log::debug!("Got {:?} from gdi", handle);
                    try_handle(font_attr, handle, &mut fonts, loaded);
                }
                Err(err) => {
                    log::debug!("gdi couldn't resolve {:?} to a path: {:#}", font_attr, err);
                }
            }
        }

        Ok(fonts)
    }

    fn locate_fallback_for_codepoints(
        &self,
        codepoints: &[char],
    ) -> anyhow::Result<Vec<ParsedFont>> {
        let text: Vec<u16> = codepoints
            .iter()
            .map(|&c| c as u16)
            .chain(std::iter::once(0))
            .collect();

        let collection = dwrote::FontCollection::system();
        struct Source {
            locale: String,
            len: u32,
        }
        impl dwrote::TextAnalysisSourceMethods for Source {
            fn get_locale_name<'a>(&'a self, _: u32) -> (Cow<'a, str>, u32) {
                (Cow::Borrowed(&self.locale), self.len)
            }
            fn get_paragraph_reading_direction(&self) -> u32 {
                DWRITE_READING_DIRECTION_LEFT_TO_RIGHT
            }
        }

        let source = dwrote::TextAnalysisSource::from_text(
            Box::new(Source {
                locale: "".to_string(),
                len: codepoints.len() as u32,
            }),
            Cow::Borrowed(&text),
        );

        let mut handles = vec![];
        let mut resolved = HashSet::new();

        if let Some(fallback) = dwrote::FontFallback::get_system_fallback() {
            let mut start = 0usize;
            let mut len = codepoints.len();
            loop {
                let result = fallback.map_characters(
                    &source,
                    start as u32,
                    len as u32,
                    &collection,
                    None,
                    FontWeight::Regular,
                    FontStyle::Normal,
                    FontStretch::Normal,
                );

                if let Some(font) = result.mapped_font {
                    log::trace!(
                        "DirectWrite Suggested fallback: {} {}",
                        font.family_name(),
                        font.face_name()
                    );

                    let attr = FontAttributes {
                        weight: WTFontWeight::from_opentype_weight(font.weight().to_u32() as _),
                        stretch: WTFontStretch::from_opentype_stretch(font.stretch().to_u32() as _),
                        style: WTFontStyle::Normal,
                        family: font.family_name(),
                        is_fallback: true,
                        is_synthetic: true,
                        harfbuzz_features: None,
                        freetype_load_target: None,
                        freetype_render_target: None,
                        freetype_load_flags: None,
                        scale: None,
                        assume_emoji_presentation: None,
                    };

                    if !resolved.contains(&attr) {
                        resolved.insert(attr.clone());

                        let descriptor = attributes_to_descriptor(&attr);
                        if let Some(handle) = handle_from_descriptor(
                            &attr,
                            &collection,
                            &descriptor,
                            16, /* pixel_size: irrelevant really as we kinda want a scalable font for fallback */
                        ) {
                            handles.push(handle);
                        }
                    }
                }
                if result.mapped_length > 0 {
                    start += result.mapped_length
                } else {
                    break;
                }
                if start == codepoints.len() {
                    break;
                }
                len = codepoints.len() - start;
            }
        } else {
            log::error!("Unable to get system fallback from dwrote");
        }

        Ok(handles)
    }

    fn enumerate_all_fonts(&self) -> anyhow::Result<Vec<ParsedFont>> {
        let collection = dwrote::FontCollection::system();
        let mut fonts = vec![];
        let mut files = HashSet::new();
        for family in collection.families_iter() {
            let count = family.get_font_count();
            for idx in 0..count {
                let font = family.get_font(idx);
                let face = font.create_font_face();
                for file in face.get_files() {
                    if let Some(path) = file.get_font_file_path() {
                        if files.contains(&path) {
                            continue;
                        }
                        files.insert(path.clone());

                        let source = FontDataSource::OnDisk(path);
                        if let Err(err) = parse_and_collect_font_info(
                            &source,
                            &mut fonts,
                            FontOrigin::DirectWrite,
                        ) {
                            log::warn!("While parsing: {:?}: {:#}", source, err);
                        }
                    }
                }
            }
        }
        fonts.sort();

        Ok(fonts)
    }
}
