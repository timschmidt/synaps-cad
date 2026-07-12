use crate::plugins::code_editor::ScadCode;
use crate::plugins::ui::utils::highlight_openscad;
use bevy_egui::egui;

pub fn render_code_editor(ui: &mut egui::Ui, scad_code: &mut ScadCode) {
    let editor_size = ui.available_size();
    let mut highlighter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
        let font_id = egui::TextStyle::Monospace.resolve(ui.style());
        let mut layout_job = highlight_openscad(text, font_id);
        layout_job.wrap.max_width = wrap_width;
        ui.fonts(|f| f.layout_job(layout_job))
    };

    egui::ScrollArea::both()
        .id_salt("code_editor")
        .max_height(editor_size.y)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let response = ui.add(
                egui::TextEdit::multiline(&mut scad_code.text)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .min_size(editor_size)
                    .layouter(&mut highlighter),
            );
            scad_code.editor_focused = response.has_focus();
            if response.changed() {
                scad_code.changed_while_focused = true;
            }
            if response.lost_focus() && scad_code.changed_while_focused {
                scad_code.dirty = true;
                scad_code.changed_while_focused = false;
            }
        });
}
