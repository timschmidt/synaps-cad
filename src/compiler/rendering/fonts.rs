use csgrs::csg::CSG;
use csgrs::sketch::Sketch;

/// Bundled Liberation Sans Regular font data.
const LIBERATION_SANS_REGULAR: &[u8] =
    include_bytes!("../../../assets/fonts/LiberationSans-Regular.ttf");
const LIBERATION_SANS_BOLD: &[u8] = include_bytes!("../../../assets/fonts/LiberationSans-Bold.ttf");
const LIBERATION_SANS_ITALIC: &[u8] =
    include_bytes!("../../../assets/fonts/LiberationSans-Italic.ttf");
const LIBERATION_SANS_BOLD_ITALIC: &[u8] =
    include_bytes!("../../../assets/fonts/LiberationSans-BoldItalic.ttf");

/// Resolve font data from a font name parameter.
/// Tries system fonts first, falls back to bundled Liberation Sans.
#[must_use]
pub fn resolve_font_data(font_param: Option<&str>) -> Vec<u8> {
    let Some(font_str) = font_param else {
        return LIBERATION_SANS_REGULAR.to_vec();
    };

    // Parse "FontName:style=Bold" format
    let (family, style) = font_str.find(":style=").map_or_else(
        || (font_str, String::new()),
        |idx| (&font_str[..idx], font_str[idx + 7..].to_lowercase()),
    );

    // Check for bundled Liberation Sans variants
    let family_lower = family.to_lowercase();
    if family_lower == "liberation sans" || family_lower.is_empty() {
        return match style.as_str() {
            "bold" => LIBERATION_SANS_BOLD.to_vec(),
            "italic" => LIBERATION_SANS_ITALIC.to_vec(),
            "bold italic" | "bolditalic" | "bold_italic" => LIBERATION_SANS_BOLD_ITALIC.to_vec(),
            _ => LIBERATION_SANS_REGULAR.to_vec(),
        };
    }

    // Try to load from system font directories
    if let Some(data) = find_system_font(family, &style) {
        return data;
    }

    // Fallback to bundled Liberation Sans
    match style.as_str() {
        "bold" => LIBERATION_SANS_BOLD.to_vec(),
        "italic" => LIBERATION_SANS_ITALIC.to_vec(),
        "bold italic" | "bolditalic" | "bold_italic" => LIBERATION_SANS_BOLD_ITALIC.to_vec(),
        _ => LIBERATION_SANS_REGULAR.to_vec(),
    }
}

/// Search system font directories for a matching font file.
#[cfg(not(target_arch = "wasm32"))]
fn find_system_font(family: &str, style: &str) -> Option<Vec<u8>> {
    let font_dirs: &[&str] = if cfg!(target_os = "macos") {
        &["/System/Library/Fonts", "/Library/Fonts"]
    } else if cfg!(target_os = "windows") {
        &["C:\\Windows\\Fonts"]
    } else {
        // Linux / FreeBSD
        &["/usr/share/fonts", "/usr/local/share/fonts"]
    };

    let family_lower = family.to_lowercase().replace(' ', "");

    // Build expected filename patterns
    let style_suffix = match style {
        "bold" => "-Bold",
        "italic" => "-Italic",
        "bold italic" | "bolditalic" | "bold_italic" => "-BoldItalic",
        _ => "-Regular",
    };

    for dir in font_dirs {
        let dir_path = std::path::Path::new(dir);
        if !dir_path.exists() {
            continue;
        }
        if let Some(data) = search_font_dir(dir_path, &family_lower, style_suffix) {
            return Some(data);
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn find_system_font(_family: &str, _style: &str) -> Option<Vec<u8>> {
    None
}

/// Recursively search a directory for a matching font file.
#[cfg(not(target_arch = "wasm32"))]
fn search_font_dir(
    dir: &std::path::Path,
    family_lower: &str,
    style_suffix: &str,
) -> Option<Vec<u8>> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(data) = search_font_dir(&path, family_lower, style_suffix) {
                return Some(data);
            }
        } else if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
            let name_lower = name.to_lowercase().replace(' ', "");
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if (ext == "ttf" || ext == "otf")
                && name_lower.contains(family_lower)
                && (style_suffix == "-Regular"
                    || name_lower.contains(&style_suffix.to_lowercase().replace('-', "")))
                && let Ok(data) = std::fs::read(&path)
            {
                return Some(data);
            }
        }
    }
    None
}

/// Apply halign/valign offsets to a text sketch by computing its bounding box.
pub fn apply_text_alignment(sketch: Sketch<()>, halign: &str, valign: &str) -> Sketch<()> {
    // Compute bounding box by triangulating and scanning vertices
    let tris = sketch.triangulate();
    if tris.is_empty() {
        return sketch;
    }

    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for tri in &tris {
        for pt in tri {
            min_x = min_x.min(pt.x);
            max_x = max_x.max(pt.x);
            min_y = min_y.min(pt.y);
            max_y = max_y.max(pt.y);
        }
    }

    let width = max_x - min_x;
    let height = max_y - min_y;

    let dx = match halign {
        "center" => -(min_x + width / 2.0),
        "right" => -max_x,
        _ => 0.0, // "left" — default
    };

    let dy = match valign {
        "center" => -(min_y + height / 2.0),
        "top" => -max_y,
        "bottom" => -min_y,
        _ => 0.0, // "baseline" — default
    };

    if dx.abs() < 1e-12 && dy.abs() < 1e-12 {
        sketch
    } else {
        sketch.translate(dx, dy, 0.0)
    }
}

/// Render text with proper character spacing and direction support.
///
/// Works around csgrs's broken space-character advance by rendering
/// character-by-character with advance widths from the font's horizontal metrics.
pub fn render_text_with_direction(
    text: &str,
    font_data: &[u8],
    size: f64,
    spacing: f64,
    direction: &str,
) -> Sketch<()> {
    // Normalize direction: OpenSCAD accepts "ltr", "rtl", "ttb", "btt"
    // OpenSCAD uses HarfBuzz which matches direction by first character:
    // 'r' → RTL, 't' → TTB, 'b' → BTT, anything else → LTR
    let dir = match direction.chars().next() {
        Some('r' | 'R') => "rtl",
        Some('t' | 'T') => "ttb",
        Some('b' | 'B') => "btt",
        _ => "ltr",
    };

    let Ok(face) = ttf_parser::Face::parse(font_data, 0) else {
        return Sketch::new();
    };

    let upem = f64::from(face.units_per_em());
    // OpenSCAD uses FreeType with FT_Set_Char_Size(face, 0, size*64, 100, 100).
    // This gives a scale of (size * 100/72) / upem from font units to output units.
    // csgrs internally scales glyphs by (input_size * 0.3527777 / 2048).
    // We solve: corrected_size * 0.3527777 / 2048 = size * (100/72) / upem
    // → corrected_size = size * 100 / (72 * 0.3527777)  [when upem=2048]
    let corrected_size = size * 100.0 / (72.0 * 0.352_777_7);
    let font_scale = size * 100.0 / (72.0 * upem);

    // For vertical layout, use OS/2 Typo metrics if available (matches OpenSCAD/Qt tight spacing).
    // Fallback to hhea ascender/descender (usually larger).
    let (ascender, descender) = face.tables().os2.map_or_else(
        || (face.ascender(), face.descender()),
        |os2| (os2.typographic_ascender(), os2.typographic_descender()),
    );
    let ascender = f64::from(ascender) * font_scale;
    let descender = f64::from(descender) * font_scale;
    // Ignore line_gap and do NOT apply spacing to horizontal advance.
    let line_height = (ascender - descender) * spacing;

    let is_vertical = dir == "ttb" || dir == "btt";

    // OpenSCAD direction semantics (verified against OpenSCAD STL output):
    //
    // LTR: chars left-to-right starting at x=0 (default).
    // RTL: string is REVERSED, then rendered left-to-right from x=0.
    //      "Right to left" → displays as "tfel ot thgiR" starting at x=0.
    // TTB: chars top-to-bottom, text extends DOWNWARD from y≈0.
    //      First char at top, ascent line at y=0.
    // BTT: same as TTB but with REVERSED char order.
    //      Last char at top, first char at bottom. Read bottom-to-top.
    //
    // For RTL and BTT, we reverse the character array so layout is always
    // LTR (horizontal) or TTB (vertical).
    let chars: Vec<char> = if dir == "rtl" || dir == "btt" {
        text.chars().rev().collect()
    } else {
        text.chars().collect()
    };

    let mut combined: Option<Sketch<()>> = None;
    let mut cursor = 0.0_f64;

    for ch in &chars {
        if ch.is_control() {
            continue;
        }

        // Get advance width for this character from the font
        let advance = face.glyph_index(*ch).map_or(size * 0.25, |gid| {
            face.glyph_hor_advance(gid)
                .map_or(size * 0.25, |a| f64::from(a) * font_scale)
        });

        // Only render non-space characters (those with an outline)
        let has_outline = face
            .glyph_index(*ch)
            .and_then(|gid| {
                struct Checker(bool);
                impl ttf_parser::OutlineBuilder for Checker {
                    fn move_to(&mut self, _: f32, _: f32) {
                        self.0 = true;
                    }
                    fn line_to(&mut self, _: f32, _: f32) {}
                    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
                    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
                    fn close(&mut self) {}
                }
                let mut checker = Checker(false);
                face.outline_glyph(gid, &mut checker);
                if checker.0 { Some(()) } else { None }
            })
            .is_some();

        if has_outline {
            let glyph = Sketch::text(&ch.to_string(), font_data, corrected_size, None);
            let positioned = if is_vertical {
                // TTB/BTT: top-to-bottom layout. Text extends downward from y≈0.
                // Shift baseline down by ascender so the ascent line is at y=0.
                // Center character horizontally.
                glyph.translate(-advance / 2.0, -(cursor + ascender), 0.0)
            } else {
                // LTR (and RTL after reversal): left-to-right from x=0.
                glyph.translate(cursor, 0.0, 0.0)
            };

            combined = Some(match combined {
                Some(acc) => acc.union(&positioned),
                None => positioned,
            });
        }

        // Advance cursor
        if is_vertical {
            cursor += line_height;
        } else {
            cursor += advance;
        }
    }

    combined.unwrap_or_else(Sketch::new)
}
