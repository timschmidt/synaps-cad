# SynapsCAD

<p align="center">
  <img src="assets/splash@2x.png" alt="SynapsCAD" width="260">
</p>

<p align="center">
  OpenSCAD editing, exact CSG, interactive visualization, and AI-assisted modeling in one Rust application.
</p>

<p align="center">
  <a href="https://timschmidt.github.io/synaps-cad"><strong>Launch the web demo</strong></a>
  ·
  <a href="https://github.com/timschmidt/synaps-cad/releases"><strong>Download a release</strong></a>
</p>

> SynapsCAD is an early prototype. OpenSCAD compatibility is incomplete; concise bug reports with a reproducing source file are welcome.

<p align="center">
  <a href="assets/Screenshot-2026-02-28.png">
    <img src="assets/Screenshot-2026-02-28.png" alt="SynapsCAD editor and 3D viewport" width="100%">
  </a>
</p>

<p align="center">
  <a href="assets/2026-03-01_slideshow.webp"><em>View the animated demonstration</em></a>
</p>

SynapsCAD parses and evaluates OpenSCAD source without invoking the OpenSCAD executable. It presents the result in a Bevy viewport, exports STL, OBJ, or 3MF, and can ask local or hosted language models to revise the current design. The compiler is also available as a Rust library.

## Quick start

Install a stable Rust toolchain, clone this repository beside the Hyper dependencies named in `Cargo.toml`, and run:

```sh
cargo run --locked
```

The editor is on the left and the viewport is on the right. Edit the source, select **Compile**, then orbit with middle- or right-mouse drag, pan with Shift+middle-mouse drag, and zoom with the wheel. Number keys `1` through `7` select standard views; `G` toggles gizmos, `L` toggles labels, and `?` opens the shortcut guide.

Prebuilt Linux, macOS, and Windows artifacts are published on the [Releases](https://github.com/timschmidt/synaps-cad/releases) page. Unsigned macOS builds may need their quarantine attribute removed after the user has verified the download:

```sh
xattr -rd com.apple.quarantine /path/to/SynapsCAD.app
```

## Using the compiler crate

`compile_scad_code` is the high-level entry point. It accepts source text, an optional global `$fn` override, and an optional atomic cancellation flag:

```rust
use synaps_cad::compiler::{CompilationResult, compile_scad_code};

match compile_scad_code("color(\"gold\") sphere(r = exact(\"5/3\"));", 32, None) {
    CompilationResult::Success { parts, views, warnings } => {
        println!("{} meshes, {} previews", parts.len(), views.len());
        for warning in warnings {
            eprintln!("warning: {warning}");
        }
    }
    CompilationResult::Error(error) => eprintln!("compile error: {error}"),
    CompilationResult::Canceled => eprintln!("compilation canceled"),
}
```

The principal API types and functions are:

- `CompilationResult`, which distinguishes success, failure, and cancellation.
- `MeshData`, an indexed triangle mesh in Bevy's Y-up coordinate system, and `ViewImage`, a labeled base64-encoded PNG preview.
- `compile_views_only`, a convenience wrapper for callers that need previews but not meshes or recoverable warnings.
- `Evaluator` and `Value`, the lower-level OpenSCAD evaluator and its ordinary or exact runtime values.
- `render_orthographic_views`, which renders previews from existing `MeshData` values.
- `DEFAULT_SCAD_CODE`, the scene loaded by a new application workspace.

The crate is not yet published, so downstream users should currently use a Git or path dependency. The binary and library share the same compilation pipeline.

Build the local API documentation with `cargo doc --no-deps --open`.

## Exact numbers

SynapsCAD extends OpenSCAD expressions with `exact()`. Pass a string to construct a rational or symbolic Hyperreal without first rounding through `f64`:

```openscad
third = exact("1/3");

translate([cos(exact("60")), sin(exact("30")), exact("pi")])
    cube([third, exact("2/5"), exact("3/7")]);
```

Strings may contain integers, decimals, fractions, or the symbolic constants `"pi"`, `"tau"`, and `"e"`. Arithmetic, degree-based trigonometry, vectors, primitive dimensions, polygons, polyhedra, affine transformations, offsets, and extrusion parameters preserve exact values through the `csgrs` pipeline. Passing an ordinary numeric expression to `exact()` preserves its already-parsed binary value; use a string when the source's decimal or rational meaning must remain exact.

## Architecture

| Layer | Main API | Responsibility |
| --- | --- | --- |
| Parsing | `openscad_rs::parse` | OpenSCAD source to a typed syntax tree |
| Evaluation | `Evaluator`, `Value`, `Shape` | Expressions, modules, primitives, transformations, and CSG |
| Geometry | `csgrs` and the Hyper stack | Exact profiles, meshes, booleans, offsets, and tessellation |
| Compilation | `compile_scad_code` | Mesh conversion, diagnostics, and orthographic previews |
| Application | Bevy 0.15 and `bevy_egui` | Editor, viewport, picking, camera, persistence, and export |
| AI | `genai` or browser HTTP | Model discovery, streaming edits, and verification rounds |

Bevy owns the main thread. Native compilation and AI work run in background tasks and communicate with nonblocking channels; WebAssembly uses browser-local tasks. The web target omits native persistence, clipboard-image capture, and model export.

## AI providers

Ollama works locally without a key. Desktop cloud providers read the following environment variables and can also be configured in **AI Settings**. The browser sends requests directly, so a provider must permit browser CORS or be accessed through an appropriate proxy.

| Provider | Environment variable |
| --- | --- |
| Anthropic | `ANTHROPIC_API_KEY` |
| OpenAI | `OPENAI_API_KEY` |
| Gemini | `GEMINI_API_KEY` |
| Groq | `GROQ_API_KEY` |
| DeepSeek | `DEEPSEEK_API_KEY` |
| Cohere | `COHERE_API_KEY` |
| Fireworks | `FIREWORKS_API_KEY` |
| Together | `TOGETHER_API_KEY` |
| xAI | `XAI_API_KEY` |
| ZAI | `ZAI_API_KEY` |
| Ollama | none |

## Web and development builds

The CI-equivalent native checks are:

```sh
cargo fmt --all -- --check
cargo clippy --locked -- -D warnings
.github/scripts/test-ci.sh
cargo build --locked --release
```

Build the static web application with the exact `wasm-bindgen-cli` version recorded in `Cargo.lock`:

```sh
rustup target add wasm32-unknown-unknown
.github/scripts/build-web.sh
```

The script creates and validates `dist/`. Run the end-to-end compiler benchmark with `cargo bench --bench compile_default`; `SYNAPS_BENCH_SCENE`, `SYNAPS_BENCH_FILE`, `SYNAPS_BENCH_FN_OVERRIDE`, and `SYNAPS_BENCH_ITERATIONS` select its workload.

The compatibility corpus under `tests/openscad_examples` originates from OpenSCAD. `tests/generate_references.sh` uses an installed OpenSCAD CLI to regenerate the bounding-box and mesh-count references used by the corpus tests.

## References

- [OpenSCAD documentation and language reference](https://openscad.org/documentation.html)
- [OpenSCAD source and compatibility corpus](https://github.com/openscad/openscad)
- [`openscad-rs` parser](https://github.com/timschmidt/openscad-rs)
- [Bevy 0.15 documentation](https://docs.rs/bevy/0.15)
- [`bevy_egui` documentation](https://docs.rs/bevy_egui/0.33)
- [egui documentation](https://docs.rs/egui/0.31)
- [`genai` documentation](https://docs.rs/genai/0.6.0-beta.3)
- [3MF Core Specification](https://3mf.io/spec/core-v1-3-0/)
- [STL file format](https://en.wikipedia.org/wiki/STL_(file_format)) and [Wavefront OBJ format](https://www.loc.gov/preservation/digital/formats/fdd/fdd000507.shtml)
- [WebAssembly Rust target](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html) and [`wasm-bindgen` guide](https://rustwasm.github.io/docs/wasm-bindgen/)

## Hyper ecosystem

SynapsCAD builds on [`csgrs`](https://github.com/timschmidt/csgrs), [`hyperreal`](https://github.com/timschmidt/hyperreal), [`hypercurve`](https://github.com/timschmidt/hypercurve), [`hypermesh`](https://github.com/timschmidt/hypermesh), [`hypertriangulate`](https://github.com/timschmidt/hypertriangulate), and [`hyperlattice`](https://github.com/timschmidt/hyperlattice). Their READMEs describe the exact-number, curve, topology, triangulation, and spatial-indexing layers in more detail.

## License and contact

SynapsCAD is licensed under [GPL-3.0-or-later](LICENSE). Contact [@boerni@chaos.social](https://chaos.social/@boerni).
