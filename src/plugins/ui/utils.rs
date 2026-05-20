use crate::plugins::ui::resources::ImagePreviewState;
use bevy_egui::egui;
use egui::text::LayoutJob;

pub const OPENSCAD_BUILTINS: &[&str] = &[
    "cube",
    "sphere",
    "cylinder",
    "polyhedron",
    "circle",
    "square",
    "polygon",
    "text",
    "translate",
    "rotate",
    "scale",
    "mirror",
    "multmatrix",
    "color",
    "offset",
    "resize",
    "union",
    "difference",
    "intersection",
    "hull",
    "minkowski",
    "linear_extrude",
    "rotate_extrude",
    "surface",
    "projection",
    "import",
    "children",
    "parent_module",
    "is_undef",
    "is_list",
    "is_num",
    "is_string",
    "is_bool",
    "len",
    "str",
    "chr",
    "ord",
    "concat",
    "lookup",
    "search",
    "abs",
    "sign",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "atan2",
    "floor",
    "ceil",
    "round",
    "ln",
    "log",
    "pow",
    "sqrt",
    "exp",
    "min",
    "max",
    "norm",
    "cross",
    "rands",
];

pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

pub fn highlight_openscad(text: &str, font_id: egui::FontId) -> LayoutJob {
    use openscad_rs::token::Token;

    let keyword_color = egui::Color32::from_rgb(198, 120, 221);
    let number_color = egui::Color32::from_rgb(209, 154, 102);
    let string_color = egui::Color32::from_rgb(152, 195, 121);
    let builtin_color = egui::Color32::from_rgb(97, 175, 239);
    let comment_color = egui::Color32::from_rgb(92, 99, 112);
    let operator_color = egui::Color32::from_rgb(171, 178, 191);
    let default_color = egui::Color32::from_rgb(220, 220, 230);
    let bool_color = egui::Color32::from_rgb(209, 154, 102);
    let special_var_color = egui::Color32::from_rgb(224, 108, 117);

    let mut job = LayoutJob::default();

    let format_for = |color: egui::Color32| -> egui::TextFormat {
        egui::TextFormat {
            font_id: font_id.clone(),
            color,
            ..Default::default()
        }
    };

    let tokens = openscad_rs::lexer::lex(text);
    let mut cursor = 0;

    for (token, span) in &tokens {
        if span.start > cursor {
            let gap = &text[cursor..span.start];
            add_gap_sections(&mut job, gap, &font_id, comment_color, default_color);
        }

        let slice = &text[span.start..span.end];
        let color = match token {
            Token::Module
            | Token::Function
            | Token::If
            | Token::Else
            | Token::For
            | Token::Let
            | Token::Assert
            | Token::Echo
            | Token::Each
            | Token::Undef
            | Token::Include
            | Token::Use => keyword_color,
            Token::True | Token::False => bool_color,
            Token::Number(_) => number_color,
            Token::String(_) => string_color,
            Token::Identifier => {
                if slice.starts_with('$') {
                    special_var_color
                } else if OPENSCAD_BUILTINS.contains(&slice) {
                    builtin_color
                } else {
                    default_color
                }
            }
            _ => operator_color,
        };

        job.append(slice, 0.0, format_for(color));
        cursor = span.end;
    }

    if cursor < text.len() {
        let gap = &text[cursor..];
        add_gap_sections(&mut job, gap, &font_id, comment_color, default_color);
    }

    job
}

fn add_gap_sections(
    job: &mut LayoutJob,
    gap: &str,
    font_id: &egui::FontId,
    comment_color: egui::Color32,
    default_color: egui::Color32,
) {
    let format_for = |color: egui::Color32| -> egui::TextFormat {
        egui::TextFormat {
            font_id: font_id.clone(),
            color,
            ..Default::default()
        }
    };

    let mut remaining = gap;
    while !remaining.is_empty() {
        if let Some(pos) = remaining.find("//") {
            if pos > 0 {
                job.append(&remaining[..pos], 0.0, format_for(default_color));
            }
            let comment_end = remaining[pos..]
                .find('\n')
                .map_or(remaining.len(), |n| pos + n);
            job.append(&remaining[pos..comment_end], 0.0, format_for(comment_color));
            remaining = &remaining[comment_end..];
        } else if let Some(pos) = remaining.find("/*") {
            if pos > 0 {
                job.append(&remaining[..pos], 0.0, format_for(default_color));
            }
            let comment_end = remaining[pos + 2..]
                .find("*/")
                .map_or(remaining.len(), |n| pos + 2 + n + 2);
            job.append(&remaining[pos..comment_end], 0.0, format_for(comment_color));
            remaining = &remaining[comment_end..];
        } else {
            job.append(remaining, 0.0, format_for(default_color));
            break;
        }
    }
}

pub fn show_texture_preview(ui: &egui::Ui, texture: &egui::TextureHandle) {
    let max_side = 400.0_f32;
    let [tw, th] = texture.size();
    #[allow(clippy::cast_precision_loss)]
    let aspect = tw as f32 / th.max(1) as f32;
    let (w, h) = if tw >= th {
        (max_side, max_side / aspect)
    } else {
        (max_side * aspect, max_side)
    };

    if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
        egui::Area::new(egui::Id::new("img_preview_popup"))
            .fixed_pos(egui::pos2(pos.x + 16.0, pos.y + 16.0))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(30, 30, 46))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(4))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 80, 100)))
                    .show(ui, |ui| {
                        ui.image(egui::load::SizedTexture::new(
                            texture.id(),
                            egui::vec2(w, h),
                        ));
                    });
            });
    }
}

pub fn show_image_preview(
    ui: &egui::Ui,
    img: &crate::plugins::ai_chat::ChatImage,
    preview_state: &mut ImagePreviewState,
) {
    use base64::Engine;

    let key = format!("{}_{}", img.filename, img.base64_data.len());
    let texture = if preview_state
        .active
        .as_ref()
        .is_some_and(|(k, _)| k == &key)
    {
        preview_state.active.as_ref().unwrap().1.clone()
    } else {
        let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(&img.base64_data) else {
            return;
        };
        let Ok(dyn_img) = image::load_from_memory(&raw) else {
            return;
        };

        let max_side = crate::app_config::MAX_TEXTURE_SIDE;
        let dyn_img = if dyn_img.width() > max_side || dyn_img.height() > max_side {
            dyn_img.resize(max_side, max_side, image::imageops::FilterType::Lanczos3)
        } else {
            dyn_img
        };

        let rgba = dyn_img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let pixels = rgba.into_raw();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
        let tex = ui
            .ctx()
            .load_texture(&key, color_image, egui::TextureOptions::LINEAR);
        preview_state.active = Some((key, tex.clone()));
        tex
    };

    show_texture_preview(ui, &texture);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_image_as_chat_image(
    path: &std::path::Path,
) -> Option<crate::plugins::ai_chat::ChatImage> {
    use base64::Engine;

    let data = std::fs::read(path).ok()?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mime_type = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "image/png",
    };
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image")
        .to_string();

    let max_bytes = crate::app_config::MAX_IMAGE_BYTES;
    #[allow(clippy::cast_precision_loss)]
    let data = if data.len() > max_bytes {
        let dyn_img = image::load_from_memory(&data).ok()?;
        let mut scale = (max_bytes as f64 / data.len() as f64).sqrt();
        loop {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let new_w = (f64::from(dyn_img.width()) * scale) as u32;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let new_h = (f64::from(dyn_img.height()) * scale) as u32;
            let resized = dyn_img.resize(
                new_w.max(1),
                new_h.max(1),
                image::imageops::FilterType::Lanczos3,
            );
            let mut buf = std::io::Cursor::new(Vec::new());
            resized.write_to(&mut buf, image::ImageFormat::Jpeg).ok()?;
            let encoded = buf.into_inner();
            if encoded.len() <= max_bytes || scale < 0.1 {
                return Some(crate::plugins::ai_chat::ChatImage {
                    filename,
                    mime_type: "image/jpeg".to_string(),
                    base64_data: base64::engine::general_purpose::STANDARD.encode(&encoded),
                });
            }
            scale *= 0.8;
        }
    } else {
        data
    };

    let base64_data = base64::engine::general_purpose::STANDARD.encode(&data);
    Some(crate::plugins::ai_chat::ChatImage {
        filename,
        mime_type: mime_type.to_string(),
        base64_data,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn clipboard_image_as_chat_image() -> Option<crate::plugins::ai_chat::ChatImage> {
    use base64::Engine;
    if let Some(path) = clipboard_file_path() {
        let is_image = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()));
        if is_image {
            return load_image_as_chat_image(&path);
        }
    }

    let mut clipboard = arboard::Clipboard::new().ok()?;
    let img_data = clipboard.get_image().ok()?;

    #[allow(clippy::cast_possible_truncation)]
    let rgba = image::RgbaImage::from_raw(
        img_data.width as u32,
        img_data.height as u32,
        img_data.bytes.into_owned(),
    )?;
    let dyn_img = image::DynamicImage::from(rgba);

    let max_bytes = crate::app_config::MAX_IMAGE_BYTES;
    let mut buf = std::io::Cursor::new(Vec::new());
    dyn_img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    let mut encoded = buf.into_inner();

    #[allow(clippy::cast_precision_loss)]
    if encoded.len() > max_bytes {
        let mut scale = (max_bytes as f64 / encoded.len() as f64).sqrt();
        loop {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let new_w = (f64::from(dyn_img.width()) * scale) as u32;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let new_h = (f64::from(dyn_img.height()) * scale) as u32;
            let resized = dyn_img.resize(
                new_w.max(1),
                new_h.max(1),
                image::imageops::FilterType::Lanczos3,
            );
            let mut cursor = std::io::Cursor::new(Vec::new());
            resized
                .write_to(&mut cursor, image::ImageFormat::Png)
                .ok()?;
            encoded = cursor.into_inner();
            if encoded.len() <= max_bytes || scale < 0.1 {
                break;
            }
            scale *= 0.8;
        }
    }

    let now = chrono::Local::now();
    let filename = now.format("Pasted %Y-%m-%d %H-%M-%S.png").to_string();
    let base64_data = base64::engine::general_purpose::STANDARD.encode(&encoded);
    Some(crate::plugins::ai_chat::ChatImage {
        filename,
        mime_type: "image/png".to_string(),
        base64_data,
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::missing_const_for_fn)]
fn clipboard_file_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("osascript")
            .args(["-e", "try\nPOSIX path of (the clipboard as \u{00AB}class furl\u{00BB})\non error\n\"\"\nend try"])
            .output().ok()?;
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path_str.is_empty() {
                let path = std::path::PathBuf::from(&path_str);
                if path.exists() && path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
pub fn copy_chat_image_to_clipboard(img: &crate::plugins::ai_chat::ChatImage) {
    use base64::Engine;
    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(&img.base64_data) else {
        return;
    };
    let Ok(dyn_img) = image::load_from_memory(&raw) else {
        return;
    };
    let rgba = dyn_img.to_rgba8();
    let img_data = arboard::ImageData {
        width: rgba.width() as usize,
        height: rgba.height() as usize,
        bytes: rgba.into_raw().into(),
    };
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let _ = clipboard.set_image(img_data);
    }
}
