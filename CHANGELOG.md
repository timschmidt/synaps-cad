# Changelog

All notable changes to this project will be documented in this file.


## [Unreleased]

### Added

- **Web/WASM Build** — SynapsCAD now builds for `wasm32-unknown-unknown` and includes a static `web/index.html` shell for browser hosting.
- **GitHub Pages Deployment** — added a GitHub Actions workflow that builds the WASM release, generates `wasm-bindgen` web bindings, uploads the static bundle, and deploys it to GitHub Pages.
- **Browser File Picker** — image attachments now use `rfd` file handles in the browser and read selected files directly from the Web File API.
- **Browser AI Chat** — the WASM build can now send AI chat requests from the browser using direct HTTP calls for Anthropic, OpenAI-compatible providers, Gemini, Cohere, and Ollama, while reusing existing code application, verification, and error recovery.

### Changed

- **Platform-specific Dependencies** — native-only integrations such as persistence, clipboard image access, model export, and external URL opening are now gated out of the browser build. In the WASM build, model compilation runs synchronously on the main thread and AI networking uses browser-compatible requests instead of the native `genai`/Tokio streaming path.


## [0.10.1] - 2026-03-26

### Fixed

- **CI Build Failure** — resolved `collapsible_if` clippy warnings that caused build failures on Rust 1.94+.


## [0.10.0] - 2026-03-26

### Added

- **Custom Endpoint URLs for All AI Providers** — every provider now has an "Endpoint URL" field in AI Settings, enabling use of custom/self-hosted API endpoints (e.g. LM Studio, vLLM, Azure ML Studio, LiteLLM). Previously only Ollama supported a custom host. The field shows the provider's default URL as placeholder text. Model listing for custom endpoints uses the OpenAI-compatible `/models` format with automatic fallback to Ollama format. See features request #2 for details.
- **API Key Visibility Toggle** — added an eye icon button next to the API key field to toggle between hidden (default) and visible text.
- **Automatic Error Recovery** — when AI-generated code causes a compilation error (e.g. "Parse error: unexpected token"), the error is automatically sent back to the AI with a request to fix the issue. The AI then produces corrected code, creating a self-healing feedback loop.

### Changed

- **Default Temperature** — changed the default temperature from 0.0 to 0.1 for new installations to encourage more creative responses while still being mostly deterministic. Existing users keep their current temperature setting.
- **API Key Optional for Custom URLs** — the "Required" marker is no longer shown on the API key field when a custom endpoint URL is configured, since many local inference servers don't require authentication.
- **Model Selection Optional for Custom URLs** — when using a custom endpoint URL, the model dropdown selection is no longer mandatory. Some inference servers (e.g. LM Studio with Claude compatibility) handle model routing internally.

### Fixed

- **Compile Button Requires Double Click After Edit** — fixed update scheduling so editor changes are always applied before compile triggering. A single click on `Compile` now reliably compiles the latest code without needing a second click.
- **Custom Ollama Host Without API Key** — fixed a bug where setting a custom Ollama host was silently ignored when no API key was configured, falling back to the default `localhost:11434` endpoint.
- **Adapter Kind Override** — the user-selected provider now always determines the API format used (e.g. OpenAI's `/chat/completions` vs Ollama's `/api/chat`), regardless of what the genai library infers from the model name. This fixes issues when using non-standard model names with providers like LM Studio.
- **Model List Empty on Startup** — the model selector was empty after app restart because the model fetch system's `run_if` guard prevented it from running on the first frame. Models are now fetched automatically on startup.
- **Model List Empty After Provider Switch** — switching the AI provider via the chat header selector now triggers a model list reload instead of showing an empty model selector.
- **Chat History Overwrites Draft** — pressing Arrow Up/Down in the chat input no longer wraps cyclically and discards unsent text. The current draft is preserved and restored when pressing Arrow Down past the newest history entry.
- **"Select a model" Warning with Custom URL** — the warning "Select a model in ⚙" no longer appears when using a custom endpoint URL, since model selection may be handled server-side.


## [0.9.2] - 2026-03-10

### Fixed

- **Windows GPU Selection** — removed `PowerPreference::LowPower` which forced the integrated GPU on Windows; wgpu now lets the OS/driver pick the best adapter, fixing high CPU usage on systems where the iGPU driver falls back to software rasterization.


## [0.9.1] - 2026-03-09

### Fixed

- **3MF Export: Non-Manifold Mesh** — exported meshes now weld shared vertices, eliminating "non-manifold" warnings in slicers like Bambu Studio.
- **3MF Export: Inverted Normals** — fixed triangle winding order (CW→CCW) in STL, OBJ, and 3MF exports; resolves Bambu Studio's "too small, may be in meters" false positive caused by negative signed volume.
- **3MF Export: Multiple Objects Warning** — restructured 3MF output to use a single assembly object with component references instead of separate build items, preventing "multiple objects at different heights" warnings.
- **3MF Export: Missing Material Namespace** — added required `xmlns:m` namespace declaration when color groups are present, fixing invalid XML in exported files.
- **Windows Paste** — fixed pasting full text (e.g. API tokens) on Windows; previously only the first character appeared due to per-character event filtering.
- **Windows CPU Usage** — switched to reactive rendering mode (`WinitSettings::desktop_app()`) so the app only redraws on user input or async events, dramatically reducing idle CPU usage.

### Improved

- **Chat Panel Scrolling** — chat responses now scroll horizontally (like the code editor) so wide content no longer hides the compile button and other controls.
- **Chat Input Scrolling** — chat input area now scrolls vertically with a max height, preventing the input from expanding unboundedly with large text.


## [0.9.0] - 2026-03-09

### Added

- **Performance Monitor Overlay** — toggleable overlay showing frame times, CPU, and memory usage for diagnostics.
- **Chat Auto-Scroll** — chat automatically scrolls to the bottom when sending a message, so the streaming response is immediately visible.
- **Send Button Validation** — the send button is disabled when no model is configured, with a hint to open AI Settings.

### Fixed

- **Gemini Authentication** — fixed 401 errors when streaming chat with Gemini by scoping the Ollama Bearer auth workaround to Ollama models only (was incorrectly applied to all providers).
- **Clipboard Paste** — fixed clipboard paste events for cross-platform compatibility (#1).
- **Model Reload on Config Change** — changing API key or Ollama host now reliably triggers a model list reload on focus loss.
- **API Key Trimming** — API keys are now trimmed of whitespace when entered and when loaded from storage.
- **Model Warning Accuracy** — "Previously configured model is no longer available" warning now only appears when models were successfully fetched but the configured model is missing, not on auth/network errors.

### Changed

- **Compilation Restarts on Code Change** — undo, redo, and other code-changing actions now cancel any running compilation and immediately start a fresh one, instead of waiting for the current compile to finish.
- **AI Temperature Default** — default temperature set to 0.0 for more deterministic responses.
- **Settings Dialog** — can now be closed even without a configured model; settings only auto-open on first launch.

### Improved

- **Performance Optimizations** — VSync, reduced compilation polling frequency, and optimized UI redraws for lower CPU usage.
- **Code Header Layout** — fixed render button order for better narrow panel support.


## [0.8.0] - 2026-03-08

### Added

- **Compilation Cancellation** — long-running compilations can now be cancelled with a "⏹ Cancel" button that appears during compilation, providing better control over the compile process.
- **Compilation Timeout Support** — added timeout support for compilation process in test infrastructure to prevent hanging tests.
- **Custom Ollama Host Configuration** — added support for configuring custom Ollama host URLs in AI settings, allowing connection to Ollama instances running on different hosts or ports.

### Changed

- **AI Assistant Header Layout** — redesigned header with improved responsive layout, better button placement, and comprehensive hover tooltips. Settings icon moved beside provider selector for better grouping.
- **Code Block Support** — enhanced AI chat to support both `synapscad` and `openscad` code block formats for improved flexibility.

### Improved

- **Button Tooltips** — all header buttons now have descriptive hover tooltips for better usability.
- **Responsive Design** — AI Assistant header adapts better to narrow panel widths by hiding non-essential text labels.


## [0.7.2] - 2026-03-07

### Added

- **Find/Replace Edit Blocks** — the AI can now respond with `<<<REPLACE` diff blocks that render as a visual find/replace UI in the chat, making targeted code edits easier to review.

### Fixed

- **Settings Dialog — Provider Dropdown** — all providers are now always selectable in the settings dialog regardless of configuration state. Graying out unconfigured providers is kept only in the main toolbar dropdown.
- **Settings Dialog — Error Display** — API/model errors in the settings dialog are now shown in a fixed-height scrollable area instead of expanding the dialog height indefinitely.
- **Settings Dialog — System Prompt** — the system prompt field is now scrollable with a fixed height instead of expanding to show all content.

### Internal

- Added CI workflow for automated testing and Clippy linting.
- Enhanced AI assistant guidelines for targeted edits and large rewrites.


## [0.7.1] - 2026-03-04

### Changed

- **Splash Screen Timer** — adjusted default splash screen display duration.
- **Markdown Rendering** — improved header detection and checklist rendering in chat messages.

### Fixed

- **Side Panel Auto-Expansion** — fixed a bug where the left panel would auto-expand when adding attachments, submitting chat messages, or resizing content. The panel width now stays fixed unless manually resized by the user via the drag handle.
- **Attachment Filename Overflow** — long attachment filenames (e.g., screenshot names) no longer push the panel wider. Filenames are truncated to 20 characters with full name shown on hover tooltip. Multiple attachments wrap to new lines instead of extending horizontally.
- **Color Parsing** — fixed color parsing to support hex color strings in `parse_color_args`.
- **Zoom Limits** — extended from 0.5–1000.0 to 0.1–5000.0 for both mouse and keyboard, so you can zoom much further in/out before hitting limits


## [0.7.0] - 2026-03-03

### Added

- **Enhanced Chat UI** — improved message styling with distinct background colors for user, AI, and error messages.
- **Thinking Process Display** — collapsible "thinking" section in chat responses to show the model's reasoning process.
- **Streaming Indicator** — visual feedback while the AI is generating a response.
- **$fn dropdown** — quick selection of common $fn values in the code editor toolbar.
- **Chat History Draft** — if you start cycling through previous messages and then return to the draft, your unsent text is preserved.
- **Dynamic Grid** — the XYZ grid now grows automatically based on the model's bounding box (with margin), minimum 50 units.
- **Grid Toggle (`G` key)** — toggling grid visibility now correctly applies to dynamically resized grids.
- **Agent Timer** — elapsed time is shown next to the spinner while the AI is working.

### Changed

- **Part Label Contrast** — improved visibility of part labels against the background.
- **Auto-scroll Behavior** — chat now smarter about scrolling to new messages vs. preserving scroll position.
- **Refactored Codebase** — split large `ui.rs` and `compilation.rs` files into modular components for better maintainability.
- **AI Context Improvements** — added physical realism checks in AI instructions.

### Fixed

- **UI Overlap** — Part labels are now hidden when they would overlap with the top viewport toolbar or the left side panel, preventing visual clutter.
- **Markdown Rendering** — Fixed bold text rendering (`**text**`) in chat messages and thinking blocks, ensuring inline bolding works correctly and markers are hidden.
- **BMesh Transformations** — Refactor BMesh transformations to include fallback to CsgMesh on panic.


## [0.6.0] - 2026-03-02

### Added

- **Better AI context for views** — the AI now knows exactly which `$view` you are currently seeing in the viewport. Standard orthographic views (Front, Right, Top, Bottom, Iso) include descriptive orientation labels (e.g., "Looking from +Y towards origin") for better spatial grounding.
- **Physical Realism guidelines** — the AI's internal instructions now emphasize checking the physical "fit" and structural integrity of individual parts in multi-part assemblies, including proper tolerances and alignment.

### Changed

- **Improved chat auto-scrolling** — the chat now respects your manual scrolling. It will only "stick to the bottom" if you are already at the end of the conversation. If you scroll up to read previous messages, new incoming text won't force-scroll you back down.

### Fixed

- **UI Overflow** — the "View" selector and "Attached" image strip now use wrapped layouts. If you have many views or attachments, they will wrap to new lines instead of overflowing the right edge of the sidebar.
- **Compilation error in UI system** — fixed a Rust compile error where `CompilationState` was incorrectly accessed as an immutable resource during a zoom-to-fit request.


## [0.5.2] - 2026-03-01

### Fixed

- **Non-manifold mesh fallback** — parts that fail manifold creation (e.g. thin `linear_extrude`) now render via direct polygon conversion instead of being silently dropped
- **Removed unsafe code** — bumped `genai` to 0.6.0-beta.3 which threads auth resolver through `all_model_names()`, eliminating the `set_var` workaround; `unsafe_code` lint reverted to `forbid`
- **Verification state reset** — verification state now properly resets to Idle after AI streaming ends


## [0.5.1] - 2026-03-01

### Fixed

- **Per-provider API keys** — each AI provider (Anthropic, OpenAI, Gemini, etc.) now stores its own API key; switching providers no longer loses your key
- **Per-provider model memory** — switching between providers remembers your last-used model for each
- **Multi-view context for AI** — when using `$view` branches, all views are rendered and sent to the AI as context (non-active views at 128px for efficiency)
- **View image cycling with spinner** — while waiting for AI response, cycles through rendered model views with a spinner overlay
- **Stale view images after code clear** — clearing code now properly clears cached view textures instead of showing old images
- **View cycling images not displaying** — textures are now cached across frames for proper GPU upload instead of re-created each frame


## [0.5.0] - 2026-02-28

### Added

- **Search-and-replace diffs** — AI can now send targeted `<<<REPLACE` / `===` / `>>>` blocks instead of full code replacement, saving tokens on large scripts. Falls back to full replacement automatically.
- **Syntax-highlighted code blocks** in AI chat responses — OpenSCAD/synapscad code uses the same color scheme as the editor (keywords, builtins, strings, numbers, comments)
- **Bottom and Isometric views** — AI now receives 5 rendered views (Front, Right, Top, Bottom, Iso) for better spatial understanding
- **Chat input history preserves images** — pressing ↑/↓ in chat input restores both text and attached images
- **Session-aware chat** — after app restart, previous chat messages are displayed but not re-sent to the AI, preventing context pollution from old sessions
- **Code clear resets AI chat** — clearing the code editor also resets the AI chat for a fresh session

### Fixed

- **Error messages always expanded** — error responses in chat are forced open regardless of persisted collapse state
- **macOS .app bundle launch** — added `NSPrincipalClass` to Info.plist and ad-hoc code signing in release workflow to prevent Gatekeeper blocking
- **Verification prompt rendering** — backtick-fenced text in verification prompts no longer incorrectly rendered as code blocks

### Changed

- **Splash screen** duration reduced from 3s to 1.5s (fade from 0.5s to 0.3s)


## [0.4.0] - 2026-02-28

### Added

- AI response streaming — see model output as it's generated, including live thinking/reasoning display
- Multiline chat input (3 rows) with word wrap; Enter sends, Shift+Enter inserts newline
- Compilation errors and warnings highlighted in red (⚠) in chat
- Compact icon-only Send (⬆), Stop (⏹), and Attach (📎) buttons
- Chat auto-scrolls to latest streaming content
- Debug mode: orthographic view images saved to `var/tmp/` for inspection
- App icon for macOS (.app bundle with .icns) and Windows (embedded .ico)

### Fixed

- `intersection_for` now correctly intersects all iteration results (was incorrectly treated as `for`/union)
- Boolean operation panics no longer cascade — failed parts are skipped with a warning, other parts still render
- BSP-tree boolean fallback: when boolmesh panics, operations automatically retry using csgrs BSP booleans
- AI model selection restored correctly after app restart (was being cleared during model list fetch)
- User input and image attachments preserved on AI stream errors (no longer lost on retry)

### Changed

- Chat messages use `is_error` flag for reliable error styling (no string matching)
- Boolean operations refactored into `bool_op_with_fallback` for unified boolmesh → BSP fallback logic
- `Shape::Failed` variant prevents corrupted geometry from propagating through subsequent operations


## [0.3.0] - 2026-02-28

### Added

- Toggle part labels (@1, @2, ...) visibility with toolbar button or `L` key
- Keyboard shortcuts cheatsheet dialog — open via toolbar `⌨` button or `?` key, close with `Esc`
- UI settings persistence — label visibility is now remembered across sessions
- Save-on-exit and immediate save on UI setting changes (no longer relying solely on 30-second auto-save)

### Changed

- Persistence config now has a `ui` section for UI-related settings (backward compatible)
- AGENTS.md updated with reminders to maintain keyboard shortcuts in both README and in-app cheatsheet

## [0.2.3] - 2026-02-28

### Added

- API keys entered in ⚙ AI Settings are now persisted across sessions
- "Set API key first" hint shown in settings when no API key is configured
- Local model support via Ollama highlighted in README

### Changed

- Model list is now fetched live from the provider API — no more hardcoded fallback models
- Model list and selection are cleared immediately when the API key or provider changes, preventing stale models from being shown

### Fixed

- API keys entered via the UI were not used for fetching the model list (genai workaround)


## [0.2.2] - 2026-06-27

- Fix: Changed build target macos-13 to macos-latest


## [0.2.1] - 2026-06-27

- Fix: Updated README with correct release version and download instructions


## [0.2.0] - 2026-06-27

- First binary release with pre-built executables for Linux, macOS (Apple Silicon & Intel), and Windows


## [0.1.0] - 2026-02-27

- Initial release
