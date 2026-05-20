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

const DEFAULT_CODE: &str = r#"// Welcome to SynapsCAD!
// Switch $view to render different models
$view = "all";

// --- Snowman ---
module view_snowman() {
    color("white") sphere(r = 12);
    color("white") translate([0, 0, 16]) sphere(r = 9);
    color("white") translate([0, 0, 27]) sphere(r = 6);
    color("orange")
        translate([0, 6, 27])
            rotate([90, 0, 0])
                cylinder(h = 8, r1 = 1.5, r2 = 0);
}

// --- Rocket ---
module view_rocket() {
    color("silver") cylinder(h = 40, r = 8);
    color("red")
        translate([0, 0, 40])
            cylinder(h = 15, r1 = 8, r2 = 0);
    color("darkgray")
        for (a = [0, 120, 240])
            rotate([0, 0, a])
                translate([6, -1, 0])
                    cube([8, 2, 12]);
}

// --- Castle ---
module view_castle() {
    color("sandybrown") difference() {
        cube([40, 40, 20], center = true);
        cube([34, 34, 21], center = true);
    }
    color("tan")
        for (x = [-18, 18])
            for (y = [-18, 18])
                translate([x, y, 0]) {
                    cylinder(h = 28, r = 5);
                    color("red") translate([0, 0, 28]) cylinder(h = 12, r1 = 6, r2 = 0);
                }
}

// --- All Together ---
module view_all() {
    view_snowman();
    translate([50, 0, 0]) view_rocket();
    translate([0, 60, 0]) view_castle();
}

// --- View selector ---
if ($view == "snowman") view_snowman();
if ($view == "rocket") view_rocket();
if ($view == "castle") view_castle();
if ($view == "all") view_all();
"#;

impl Default for ScadCode {
    fn default() -> Self {
        Self {
            text: DEFAULT_CODE.into(),
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
            last_snapshot: DEFAULT_CODE.to_string(),
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

// ---------------------------------------------------------------------------
// View detection: parses `$view = "xxx";` and `$view == "xxx"` from code
// ---------------------------------------------------------------------------

/// Detect views from the code. Returns `(active_view, all_views)`.
/// - `active_view`: the value in `$view = "xxx";` (None if not present)
/// - `all_views`: all unique view names found in `$view == "xxx"` conditionals
pub fn detect_views(code: &str) -> (Option<String>, Vec<String>) {
    let mut active: Option<String> = None;
    let mut views: Vec<String> = Vec::new();

    // Single pass: find all `$view` occurrences, classify as assignment or conditional
    let mut search_from = 0;
    while let Some(pos) = code[search_from..].find("$view") {
        let abs_pos = search_from + pos;
        let rest = &code[abs_pos + 5..];
        let trimmed = rest.trim_start();
        if trimmed.starts_with("==") {
            // Conditional: $view == "xxx"
            let after_eq = trimmed.strip_prefix("==").unwrap_or_default().trim_start();
            if let Some(val) = extract_quoted_string(after_eq)
                && !views.contains(&val)
            {
                views.push(val);
            }
        } else if trimmed.starts_with('=') {
            // Assignment: $view = "xxx";
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
