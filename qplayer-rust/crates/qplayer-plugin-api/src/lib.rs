//! QPlayer Plugin API — WASM plugin host interface.
//!
//! Plugins compile to `.wasm` modules and are executed in a `wasmtime` sandbox.

pub mod host;
pub mod wit;
