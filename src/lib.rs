#![forbid(unsafe_code)]

//! `OpenSCAD` parsing, evaluation, mesh conversion, and preview rendering used
//! by the `SynapsCAD` application.
//!
//! [`compiler::compile_scad_code`] is the primary library entry point. It
//! returns [`compiler::CompilationResult`] so callers can distinguish complete,
//! canceled, and failed compilations without starting Bevy.

pub mod compiler;
