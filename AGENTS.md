# AGENTS.md — Architecture Overview for AI Agents

## Purpose

This document describes the SynapsCAD architecture for AI agents (LLMs, copilots, and automated tools) that work with this codebase. It covers the system design, plugin structure, and the **part labeling** concept that enables users to reference specific 3D geometry parts in conversations with AI.

## System Architecture

SynapsCAD is a single-binary Rust app built on **Bevy 0.15** (ECS game engine) with **egui** for UI. It targets native desktop by default and also supports a browser/WASM build for GitHub Pages.

### Data Flow

```
SynapsCAD code (editor)
    ↓  trigger_compilation_system
Compiler thread on native / main thread on WASM (scad-rs parser → AST evaluator → csgrs CSG → boolmesh)
    ↓  mpsc channel
poll_compilation_system
    ↓  spawns Bevy entities
3D Viewport (Bevy renderer)
    ↓  egui overlay
Part Labels (@1, @2, ...)
    ↓  system prompt injection
AI Chat (genai → Anthropic/OpenAI/Gemini/...)
    ↓  code block extraction
Code Editor (auto-apply)
```

### Plugin Structure (`src/plugins/`)

| Plugin              | File             | Responsibility                                                           |
| ------------------- | ---------------- | ------------------------------------------------------------------------ |
| `ScenePlugin`       | `scene.rs`       | Camera, lights, axes, grid setup                                         |
| `CodeEditorPlugin`  | `code_editor.rs` | OpenSCAD text editor, undo/redo, view detection (`$view` variable)       |
| `CompilationPlugin` | `compilation.rs` | Triggers compilation, spawns mesh entities with `CadModel` + `PartLabel` |
| `CameraPlugin`      | `camera.rs`      | Orbit/pan/zoom controls, zoom-to-fit, keyboard toggles (G=gizmos, L=labels) |
| `UiPlugin`          | `ui.rs`          | egui side panel layout, viewport toolbar, label overlays                 |
| `AiChatPlugin`      | `ai_chat.rs`     | AI streaming chat with context injection                                 |
| `PersistencePlugin` | `persistence.rs` | Save/load settings and code                                              |

### Key Files

| File                | Purpose                                                                                                                     |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `src/compiler/`     | OpenSCAD → triangle mesh pipeline. Modular directory handling evaluation, geometry, rendering, and types.                    |
| `src/main.rs`       | App entry point, registers all plugins                                                                                      |
| `src/app_config.rs` | Developer constants (not user-facing)                                                                                       |
| `web/index.html`    | Static HTML shell for the WASM build                                                                                        |
| `.github/workflows/pages.yml` | Builds the WASM target, runs `wasm-bindgen`, and deploys the static bundle to GitHub Pages                       |

## Labels (`@N`)

### Part Numbering

Parts use a `@N` numbering scheme:

- Parts are numbered `@1`, `@2`, ... (1-based, in order of top-level geometry)

Each part gets a **unique color** (from `PART_PALETTE` or from `color()` in code). The label is rendered as a billboard overlay at the part's AABB center.

### Why This Matters for Agents

When a user says _"make @2 taller"_ or _"change the color of @1"_, the AI agent knows exactly which geometric part is being referenced. The label system provides:

1. **Visual identification** — colored labels in the viewport
2. **AI context** — part index, color, and bounding box injected into the system prompt
3. **Stable references** — part numbers correspond to top-level geometry statements in order

### How Parts Map to Code

Parts are created by the compiler in the order of top-level geometry statements:

```openscad
cube(10);           // → @1
translate([20,0,0])
    sphere(5);      // → @2
```

## For Agent Developers

### Code Block Namespace

AI-generated code uses the **`synapscad`** namespace in fenced code blocks:

- ` ```synapscad ` — wraps the full script to replace in the editor

The parser in `extract_openscad_code()` (`ai_chat.rs`) extracts code from the block and replaces the entire editor buffer.

### Verification & Error Recovery

The AI chat system includes automatic verification and error recovery loops:

1. **Verification Loop**: After AI produces code, the system waits for compilation, then sends a verification prompt with fresh renders for the AI to confirm correctness.
2. **Error Recovery**: If AI-generated code causes a compilation error (parse error, syntax error, etc.), the error is automatically sent back to the AI with a request to fix it. The AI then produces corrected code and the cycle continues.

The `VerificationState` enum (`ai_chat.rs`) tracks this flow:
- `Idle` — no active verification
- `WaitingForCompilation` — AI produced code, waiting for compile to finish
- `ReadyToVerify` — compilation succeeded, ready to send verification prompt
- `Verifying(n)` — currently in verification round N
- `ErrorRecovery(err)` — compilation failed, send error back to AI

This ensures AI-generated code is automatically fixed when it contains syntax or parse errors.

### View System (`$view` variable)

SynapsCAD uses a **single editor buffer** with a `$view` variable to switch between views/parts of a model. Views are defined as modules and selected via `if ($view == "name")` conditionals.

**Pattern:**

```openscad
$view = "main";

module view_main() { cube(10); }
module view_assembly() { view_main(); translate([20,0,0]) sphere(5); }

if ($view == "main") view_main();
if ($view == "assembly") view_assembly();
```

**How it works:**

1. The UI parses `$view == "xxx"` occurrences to discover available views
2. A view selector appears in the Code heading row when multiple views exist
3. Selecting a view text-replaces the `$view = "..."` assignment line
4. Only the matching `if` branch executes → only that view is compiled/rendered

**Implementation:**

- `detect_views(code)` in `code_editor.rs` — returns `(active_view, all_views)`
- `set_active_view(code, name)` in `code_editor.rs` — text-replaces the assignment
- View selector dropdown in `ui.rs` Code heading row
- AI writes full scripts with `$view`, all modules, and all `if` selectors

**Important:** There is no multi-buffer/tab system. All code lives in one editor. When the AI provides code, it replaces the entire buffer.

### Making Code Changes

1. The compiler (`src/compiler/`) is the core — it evaluates OpenSCAD AST and produces meshes. Logic is split between `evaluator/`, `geometry/`, and `rendering/`.
2. UI changes go in `src/plugins/ui/` (egui-based). Logic is split between `layout.rs`, `chat.rs`, `editor.rs`, and `viewport.rs`.
3. New Bevy components/systems go in the appropriate plugin file.
4. Tests live in `src/compiler/evaluator/tests.rs` (unit tests) and `tests/openscad_examples/` (integration).
5. Run clippy and tests after changes to ensure code quality and correctness.

### Error Handling

**Never silently discard errors.** All compiler, renderer, and mesh processing errors must be surfaced to the user — not swallowed with `eprintln!` and an empty fallback.

- **Evaluator warnings** are collected in `Evaluator.warnings` (e.g. unsupported modules, recursion limits, extrude issues) and shown to the user after compilation. **Do not simply add warnings to ignore problems; prioritize removing the actual root cause of the warning whenever possible.**
- **Mesh errors** (non-manifold, empty vertices) propagate as `Result::Err` and are shown per-part. Partial models still render — only the failing part is skipped.
- **Panic recovery** via `catch_unwind` catches crashes in dependencies (boolmesh, csgrs) and surfaces them as user-visible errors.
- **Bug-report hints**: Internal errors (panics, non-manifold mesh) include a message asking the user to report the bug with their code snippet.

When adding new features, use `self.warnings.push(...)` in the `Evaluator` for recoverable issues, and `return Err(...)` for fatal per-part failures.

### Running

```sh
cargo run          # launch the app
cargo test         # run all tests
cargo clippy       # lint
```

### WASM / GitHub Pages

The browser build uses `wasm32-unknown-unknown` with target-specific Cargo config in `.cargo/config.toml` for `getrandom`'s `wasm_js` backend. Native-only dependencies are target-gated in `Cargo.toml`.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.112 --locked
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir dist/pkg --out-name synaps_cad target/wasm32-unknown-unknown/release/synaps-cad.wasm
cp web/index.html dist/index.html
touch dist/.nojekyll
```

The WASM build currently disables native integrations: AI networking (`genai`/Tokio/reqwest), persistence (`dirs`/filesystem session storage), file dialogs, clipboard image access, model export, and opening external issue URLs. Compilation runs synchronously on the main thread in WASM because standard native threads are unavailable.

### Testing Philosophy

- **Reference comparison tests**: compare output bounding box and triangle count against OpenSCAD reference data
- **No-panic tests**: for features using dependencies with known issues (spade, csgrs), verify they don't crash
- **Unit tests**: for specific compiler features (cones, polyhedra, boolean ops)

**Visual verification is mandatory.** When implementing or fixing rendering features (text, extrusion, boolean ops, transforms, etc.), you **must** visually compare SynapsCAD's output with OpenSCAD's output for the same code. Do not rely solely on bounding-box or triangle-count tests — they can pass while the rendering is visibly wrong. Render the code in both OpenSCAD and SynapsCAD, compare the results, and only consider the feature correct when they match visually.

### Debug Logging

Use `cfg!(debug_assertions)` to guard debug output. This ensures logs appear only in `cargo run` (debug mode) and are stripped entirely from release builds (`cargo build --release`).

```rust
if cfg!(debug_assertions) {
    eprintln!("[DEBUG] Some diagnostic info: {value}");
}
```

**Convention:** Prefix debug lines with `[DEBUG]`, info/status lines with `[SynapsCAD]`. Never use `println!` — all diagnostic output goes to stderr via `eprintln!`.

The AI chat pipeline (`ai_chat.rs`) logs the full request (system prompt, messages, model) and response in debug mode.

## UI Conventions

- **Hand cursor on hover**: All clickable/interactive widgets display a pointing-hand cursor. This is set globally via `style.interaction.interact_cursor` in `setup_egui_theme`. For custom widgets using `sense(Sense::click())`, explicitly add `.on_hover_cursor(egui::CursorIcon::PointingHand)`.
- **Dialog behavior**: All floating windows (Settings, Cheatsheet, etc.) must close when the **Escape** key is pressed.
- **AI Settings**: Opened via a ⚙ gear button in the "AI Assistant" header row; rendered as a floating `egui::Window`, not inline.
- **Custom Endpoint URLs**: Every AI provider has an "Endpoint URL" field in AI Settings. When empty, the provider's default endpoint is used. When set, the custom URL overrides the endpoint for both model listing and chat API calls. Custom URLs are stored per-provider in `AiConfig.custom_urls: HashMap<String, String>` and persisted in `session.json`. The `OLLAMA_HOST` environment variable still works as the default for Ollama. Default placeholder URLs match the genai library's built-in endpoints (e.g. `https://api.openai.com/v1/` for OpenAI, `http://localhost:11434/` for Ollama). For non-Ollama providers with custom URLs, model listing tries the OpenAI-compatible `GET {url}models` endpoint, with fallback to Ollama format.
  - Some custom Claude/OpenAI-compatible endpoints (e.g. LM Studio wrappers) may not return a model list and may encode model routing in the API token or server config. In this case, do not force dropdown model selection or show "Select model in ⚙" warnings just because no models were listed; manual model entry can remain optional.
  - API tokens are **not mandatory** when a custom endpoint URL is set. Do not show required markers or block usage solely because the API key field is empty in custom-URL mode.
- **Compile button**: Right-aligned in the "Code" heading row.

## Part Colors

When generating models, always use `color()` to give each part a realistic, semantically meaningful color:

- **Green** for plants, leaves, grass
- **Brown** for wood, soil, tree trunks
- **Red** for flowers, berries, fire
- **Gray** for metal, concrete, stone
- **Blue** for water, sky, ice
- **White** for snow, clouds
- **Orange** for flames, autumn leaves

Example: `color("green") cylinder(h = 20, r = 3);` for a plant stem.

## Git Workflow

**Do not commit automatically.** Never run `git commit` unless the user explicitly asks to commit. Stage and verify changes, but wait for the user to decide when to commit.

**Do not update `CHANGELOG.md` automatically.** Only update the changelog when explicitly requested by the user.

## Maintaining This Document

**Keep `AGENTS.md` up to date.** When making architecture-relevant changes — new plugins, new resources/components, changed data flow, new UI patterns, or new conventions — update this file so that AI agents always have accurate context about the codebase.

**Keep `README.md` keyboard shortcuts table up to date.** When adding or changing keyboard shortcuts (in `camera.rs` or elsewhere), update the "Keyboard Shortcuts" and "3D Viewport Navigation" sections in `README.md` to match.

**Keep the in-app keyboard cheatsheet up to date.** The `cheatsheet_system` in `ui.rs` contains a `shortcuts` array listing all keyboard shortcuts shown to the user. When adding or changing shortcuts, update that array alongside the `README.md` tables.


## Tasks

### Prepare feature commit

- update Changelog.md with a summary of the new feature and any relevant details for users
- print a commit message but don't commit yet, so that I can review and edit the message before committing
