use bevy::prelude::*;

pub struct CodeEditorPlugin;

#[derive(Resource)]
pub struct ScadCode {
    pub text: String,
    pub dirty: bool,
    pub editor_focused: bool,
    /// Tracks whether text changed while the editor had focus.
    pub changed_while_focused: bool,
    /// Global `$fn` override — number of segments for curved surfaces.
    pub fn_value: u32,
}

use crate::compiler::DEFAULT_SCAD_CODE;

impl Default for ScadCode {
    fn default() -> Self {
        Self {
            text: DEFAULT_SCAD_CODE.into(),
            dirty: false,
            editor_focused: false,
            changed_while_focused: false,
            fn_value: 16,
        }
    }
}

/// Tracks code snapshots for undo/redo.
#[derive(Resource)]
pub struct UndoHistory {
    pub undo_stack: Vec<String>,
    pub redo_stack: Vec<String>,
    /// Last known code text, used to detect changes.
    pub last_snapshot: String,
}

impl Default for UndoHistory {
    fn default() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_snapshot: DEFAULT_SCAD_CODE.to_string(),
        }
    }
}

impl UndoHistory {
    pub fn push(&mut self, old_text: String) {
        self.undo_stack.push(old_text);
        self.redo_stack.clear();
    }

    pub const fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub const fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

impl Plugin for CodeEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScadCode>()
            .init_resource::<UndoHistory>()
            .add_systems(Update, track_code_changes_system);
    }
}

/// Detects when code text changes and pushes to the undo stack.
fn track_code_changes_system(scad_code: Res<ScadCode>, mut history: ResMut<UndoHistory>) {
    if scad_code.text != history.last_snapshot {
        let old = std::mem::replace(&mut history.last_snapshot, scad_code.text.clone());
        history.push(old);
    }
}

/// Detect views from the code. Returns `(active_view, all_views)`.
/// - `active_view`: the value in `$view = "xxx";` (None if not present)
/// - `all_views`: all unique view names found in `$view == "xxx"` conditionals
pub fn detect_views(code: &str) -> (Option<String>, Vec<String>) {
    let mut active: Option<String> = None;
    let mut views: Vec<String> = Vec::new();

    // Classify each `$view` occurrence as an assignment or comparison.
    let mut search_from = 0;
    while let Some(pos) = code[search_from..].find("$view") {
        let abs_pos = search_from + pos;
        let rest = &code[abs_pos + 5..];
        let trimmed = rest.trim_start();
        if trimmed.starts_with("==") {
            let after_eq = trimmed.strip_prefix("==").unwrap_or_default().trim_start();
            if let Some(val) = extract_quoted_string(after_eq)
                && !views.contains(&val)
            {
                views.push(val);
            }
        } else if trimmed.starts_with('=') {
            let after_eq = trimmed.strip_prefix('=').unwrap_or_default().trim_start();
            if let Some(val) = extract_quoted_string(after_eq)
                && active.is_none()
            {
                active = Some(val);
            }
        }
        search_from = abs_pos + 5;
    }

    (active, views)
}

/// Set the active view by replacing `$view = "old";` with `$view = "new";` in the code.
/// Scans all `$view` occurrences to find the assignment (skipping comments etc.).
/// Returns true if a replacement was made.
pub fn set_active_view(code: &mut String, view_name: &str) -> bool {
    let mut search_from = 0;
    while let Some(pos) = code[search_from..].find("$view") {
        let abs_pos = search_from + pos;
        let rest = &code[abs_pos + 5..];
        let trimmed = rest.trim_start();
        if trimmed.starts_with('=') && !trimmed.starts_with("==") {
            let eq_offset = abs_pos + 5 + (rest.len() - trimmed.len());
            let after_eq = &code[eq_offset + 1..];
            let after_eq_trimmed = after_eq.trim_start();
            let quote_offset = eq_offset + 1 + (after_eq.len() - after_eq_trimmed.len());

            if after_eq_trimmed.starts_with('"')
                && let Some(end_quote) = after_eq_trimmed[1..].find('"')
            {
                let old_start = quote_offset;
                let old_end = quote_offset + 1 + end_quote + 1;
                let new_val = format!("\"{view_name}\"");
                code.replace_range(old_start..old_end, &new_val);
                return true;
            }
        }
        search_from = abs_pos + 5;
    }
    false
}

/// Extract a double-quoted string value from the start of `s`.
fn extract_quoted_string(s: &str) -> Option<String> {
    if !s.starts_with('"') {
        return None;
    }
    let end = s[1..].find('"')?;
    let val = s[1..=end].to_string();
    if val.is_empty() { None } else { Some(val) }
}
