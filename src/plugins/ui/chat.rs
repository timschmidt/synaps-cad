use crate::plugins::ui::utils::highlight_openscad;
use bevy_egui::egui;

/// Render chat message content with code blocks highlighted.
pub fn render_chat_content(ui: &mut egui::Ui, content: &str, is_error: bool) -> egui::Response {
    let code_bg = egui::Color32::from_rgb(30, 30, 46);
    let code_color = egui::Color32::from_rgb(220, 220, 170);
    let lang_color = egui::Color32::from_rgb(100, 100, 130);
    let use_highlighting = |lang: &str| -> bool {
        matches!(
            lang.to_lowercase().as_str(),
            "synapscad" | "openscad" | "scad"
        )
    };

    let find_bg = egui::Color32::from_rgb(46, 24, 24);
    let replace_bg = egui::Color32::from_rgb(20, 42, 24);
    let find_label_color = egui::Color32::from_rgb(200, 100, 100);
    let replace_label_color = egui::Color32::from_rgb(100, 180, 100);

    let mut last_resp: Option<egui::Response> = None;
    let mut remaining = content;

    while !remaining.is_empty() {
        let replace_pos = remaining.find("<<<REPLACE");
        let fence_pos = remaining.find("```");

        let handle_replace = replace_pos.is_some_and(|r| fence_pos.is_none_or(|f| r <= f));

        if handle_replace {
            let rp = replace_pos.unwrap();
            let before = &remaining[..rp];
            if !before.is_empty() {
                render_markdown_text(ui, before, is_error);
            }

            let after_marker = &remaining[rp + "<<<REPLACE".len()..];
            // skip optional text/whitespace on same line as <<<REPLACE
            let after_newline = if let Some(nl) = after_marker.find('\n') {
                &after_marker[nl + 1..]
            } else {
                remaining = "";
                continue;
            };
            let Some(sep) = after_newline.find("\n===\n") else {
                remaining = "";
                continue;
            };
            let search_text = &after_newline[..sep];
            let after_sep = &after_newline[sep + "\n===\n".len()..];
            let Some(end) = after_sep.find("\n>>>") else {
                remaining = "";
                continue;
            };
            let replace_text = &after_sep[..end];

            let r = egui::Frame::new()
                .fill(code_bg)
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("edit").small().color(lang_color));
                    egui::Frame::new()
                        .fill(find_bg)
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::same(4))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("find").small().color(find_label_color));
                            let font_id = egui::FontId::monospace(12.0);
                            ui.label(highlight_openscad(search_text.trim_end(), font_id));
                        });
                    egui::Frame::new()
                        .fill(replace_bg)
                        .corner_radius(egui::CornerRadius::same(3))
                        .inner_margin(egui::Margin::same(4))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("replace")
                                    .small()
                                    .color(replace_label_color),
                            );
                            if replace_text.trim().is_empty() {
                                ui.label(
                                    egui::RichText::new("(delete)")
                                        .small()
                                        .italics()
                                        .color(lang_color),
                                );
                            } else {
                                let font_id = egui::FontId::monospace(12.0);
                                ui.label(highlight_openscad(replace_text.trim_end(), font_id));
                            }
                        });
                });
            last_resp = Some(r.response);

            remaining = &after_sep[end + "\n>>>".len()..];
            if remaining.starts_with('\n') {
                remaining = &remaining[1..];
            }
            continue;
        }

        if let Some(fence_start) = fence_pos.filter(|_| !handle_replace) {
            let before = &remaining[..fence_start];
            if !before.is_empty() {
                render_markdown_text(ui, before, is_error);
            }

            let after_fence = &remaining[fence_start + 3..];
            if let Some(close_pos) = after_fence.find("```") {
                let block = &after_fence[..close_pos];
                let (lang, code) = block.find('\n').map_or(("", block), |newline| {
                    let lang_tag = block[..newline].trim();
                    (lang_tag, &block[newline + 1..])
                });

                let r = egui::Frame::new()
                    .fill(code_bg)
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        if !lang.is_empty() {
                            ui.label(egui::RichText::new(lang).small().color(lang_color));
                        }
                        let trimmed = code.trim_end();
                        if use_highlighting(lang) {
                            let font_id = egui::FontId::monospace(12.0);
                            let job = highlight_openscad(trimmed, font_id);
                            ui.label(job);
                        } else {
                            ui.label(egui::RichText::new(trimmed).monospace().color(code_color));
                        }
                    });
                last_resp = Some(r.response);
                remaining = &after_fence[close_pos + 3..];
            } else {
                let block = after_fence;
                let (lang, code) = block.find('\n').map_or(("", block), |newline| {
                    let lang_tag = block[..newline].trim();
                    (lang_tag, &block[newline + 1..])
                });
                let r = egui::Frame::new()
                    .fill(code_bg)
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        if !lang.is_empty() {
                            ui.label(egui::RichText::new(lang).small().color(lang_color));
                        }
                        let trimmed = code.trim_end();
                        if use_highlighting(lang) {
                            let font_id = egui::FontId::monospace(12.0);
                            let job = highlight_openscad(trimmed, font_id);
                            ui.label(job);
                        } else {
                            ui.label(egui::RichText::new(trimmed).monospace().color(code_color));
                        }
                    });
                last_resp = Some(r.response);
                remaining = "";
            }
        } else {
            render_markdown_text(ui, remaining, is_error);
            remaining = "";
        }
    }

    last_resp.unwrap_or_else(|| ui.label(""))
}

pub fn render_markdown_text(ui: &mut egui::Ui, text: &str, is_error: bool) {
    let error_color = egui::Color32::from_rgb(255, 120, 120);
    let text_color = if is_error {
        error_color
    } else {
        egui::Color32::from_rgb(190, 190, 210)
    };
    let strong_color = if is_error {
        error_color
    } else {
        egui::Color32::WHITE
    };

    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            ui.add_space(6.0);
            continue;
        }

        // Detect Markdown headers and checklists
        let is_header = trimmed.starts_with('#');
        let is_checklist = trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]");

        let mut job = egui::text::LayoutJob::default();
        let parts: Vec<&str> = line.split("**").collect();

        for (i, part) in parts.iter().enumerate() {
            let is_bold = i % 2 == 1 || is_header || is_checklist;
            let color = if is_bold { strong_color } else { text_color };

            let font_id = if is_header {
                egui::FontId::proportional(16.0)
            } else {
                egui::FontId::proportional(14.0)
            };

            job.append(
                part,
                0.0,
                egui::text::TextFormat {
                    font_id,
                    color,
                    ..Default::default()
                },
            );
        }

        job.wrap.max_width = ui.available_width();
        ui.label(job);
    }
}

pub fn render_thinking_content(ui: &mut egui::Ui, text: &str) {
    let text_color = egui::Color32::from_rgb(150, 150, 150);
    let strong_color = egui::Color32::from_rgb(200, 200, 200);

    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            ui.add_space(6.0);
            continue;
        }

        let mut job = egui::text::LayoutJob::default();
        let parts: Vec<&str> = line.split("**").collect();

        for (i, part) in parts.iter().enumerate() {
            let is_bold = i % 2 == 1;
            let color = if is_bold { strong_color } else { text_color };

            job.append(
                part,
                0.0,
                egui::text::TextFormat {
                    font_id: egui::FontId::proportional(14.0),
                    color,
                    italics: true,
                    ..Default::default()
                },
            );
        }

        job.wrap.max_width = ui.available_width();
        ui.label(job);
    }
}
