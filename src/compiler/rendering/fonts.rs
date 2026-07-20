use csgrs::Profile;
use csgrs::Real;
use csgrs::csg::CSG;

/// Bundled Liberation Sans Regular font data.
const LIBERATION_SANS_REGULAR: &[u8] =
    include_bytes!("../../../assets/fonts/LiberationSans-Regular.ttf");
const LIBERATION_SANS_BOLD: &[u8] = include_bytes!("../../../assets/fonts/LiberationSans-Bold.ttf");
const LIBERATION_SANS_ITALIC: &[u8] =
    include_bytes!("../../../assets/fonts/LiberationSans-Italic.ttf");
const LIBERATION_SANS_BOLD_ITALIC: &[u8] =
    include_bytes!("../../../assets/fonts/LiberationSans-BoldItalic.ttf");

/// Resolves a requested system font, falling back to bundled Liberation Sans.
#[must_use]
pub fn resolve_font_data(font_param: Option<&str>) -> Vec<u8> {
    let Some(font_str) = font_param else {
        return LIBERATION_SANS_REGULAR.to_vec();
    };

    // OpenSCAD encodes style in `Family:style=Variant` form.
    let (family, style) = font_str.find(":style=").map_or_else(
        || (font_str, String::new()),
        |idx| (&font_str[..idx], font_str[idx + 7..].to_lowercase()),
    );

    let family_lower = family.to_lowercase();
    if family_lower == "liberation sans" || family_lower.is_empty() {
        return match style.as_str() {
            "bold" => LIBERATION_SANS_BOLD.to_vec(),
            "italic" => LIBERATION_SANS_ITALIC.to_vec(),
            "bold italic" | "bolditalic" | "bold_italic" => LIBERATION_SANS_BOLD_ITALIC.to_vec(),
            _ => LIBERATION_SANS_REGULAR.to_vec(),
        };
    }

    if let Some(data) = find_system_font(family, &style) {
        return data;
    }

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
        // Common Linux and FreeBSD font locations.
        &["/usr/share/fonts", "/usr/local/share/fonts"]
    };

    let family_lower = family.to_lowercase().replace(' ', "");

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
const fn find_system_font(_family: &str, _style: &str) -> Option<Vec<u8>> {
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

pub fn apply_text_alignment(sketch: Profile, halign: &str, valign: &str) -> Profile {
    if sketch.is_empty() {
        return sketch;
    }

    let bounds = sketch.bounding_box();
    let min_x = bounds.mins.x;
    let max_x = bounds.maxs.x;
    let min_y = bounds.mins.y;
    let max_y = bounds.maxs.y;
    let width = &max_x - &min_x;
    let height = &max_y - &min_y;
    let two = Real::from(2_u8);

    let dx = match halign {
        "center" => -(min_x + (width / &two).unwrap_or_else(|_| Real::zero())),
        "right" => -max_x,
        _ => Real::zero(),
    };

    let dy = match valign {
        "center" => -(min_y + (height / two).unwrap_or_else(|_| Real::zero())),
        "top" => -max_y,
        "bottom" => -min_y,
        _ => Real::zero(),
    };

    if dx == Real::zero() && dy == Real::zero() {
        sketch
    } else {
        sketch.translate(dx, dy, Real::zero())
    }
}

/// Renders text per glyph so spacing and direction use the font's metrics.
pub fn render_text_with_direction(
    text: &str,
    font_data: &[u8],
    size: &Real,
    spacing: &Real,
    direction: &str,
) -> Profile {
    // OpenSCAD accepts ltr, rtl, ttb, and btt and dispatches on the first
    // character, matching HarfBuzz direction parsing.
    let dir = match direction.chars().next() {
        Some('r' | 'R') => "rtl",
        Some('t' | 'T') => "ttb",
        Some('b' | 'B') => "btt",
        _ => "ltr",
    };

    let Ok(face) = ttf_parser::Face::parse(font_data, 0) else {
        return Profile::new();
    };

    let upem = Real::from(face.units_per_em());
    // Match OpenSCAD's 100-DPI FreeType sizing against csgrs's point-to-mm
    // scale. The correction is exact for the common 2048-unit em square.
    let points_per_millimeter =
        (Real::from(3_527_777_u64) / Real::from(10_000_000_u64)).unwrap_or_else(|_| Real::zero());
    let corrected_size = (size * Real::from(100_u8) / (Real::from(72_u8) * points_per_millimeter))
        .unwrap_or_else(|_| Real::zero());
    let font_scale =
        (size * Real::from(100_u8) / (Real::from(72_u8) * upem)).unwrap_or_else(|_| Real::zero());

    // OS/2 typographic metrics most closely match OpenSCAD/Qt vertical layout;
    // hhea metrics provide the portable fallback.
    let (ascender, descender) = face.tables().os2.map_or_else(
        || (face.ascender(), face.descender()),
        |os2| (os2.typographic_ascender(), os2.typographic_descender()),
    );
    let ascender = Real::from(ascender) * &font_scale;
    let descender = Real::from(descender) * &font_scale;
    // OpenSCAD applies `spacing` to vertical line height, not glyph advance.
    let line_height = (&ascender - &descender) * spacing;

    let is_vertical = dir == "ttb" || dir == "btt";

    // OpenSCAD reverses rtl and btt character order before applying its ltr or
    // top-to-bottom placement rule.
    let chars: Vec<char> = if dir == "rtl" || dir == "btt" {
        text.chars().rev().collect()
    } else {
        text.chars().collect()
    };

    let mut combined: Option<Profile> = None;
    let mut cursor = Real::zero();

    for ch in &chars {
        if ch.is_control() {
            continue;
        }

        let fallback_advance = (size / Real::from(4_u8)).unwrap_or_else(|_| Real::zero());
        let advance = face
            .glyph_index(*ch)
            .and_then(|gid| face.glyph_hor_advance(gid))
            .map_or(fallback_advance, |value| Real::from(value) * &font_scale);

        // Spaces advance the cursor without producing an outline.
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
            let glyph = Profile::text(&ch.to_string(), font_data, corrected_size.clone());
            let positioned = if is_vertical {
                // Vertical text extends down from the ascent line and centers
                // each glyph on the text axis.
                glyph.translate(
                    -(advance.clone() / Real::from(2_u8)).unwrap_or_else(|_| Real::zero()),
                    -(&cursor + &ascender),
                    Real::zero(),
                )
            } else {
                glyph.translate(cursor.clone(), Real::zero(), Real::zero())
            };

            combined = Some(match combined {
                Some(acc) => acc.union(&positioned),
                None => positioned,
            });
        }

        if is_vertical {
            cursor += &line_height;
        } else {
            cursor += advance;
        }
    }

    combined.unwrap_or_else(Profile::new)
}
