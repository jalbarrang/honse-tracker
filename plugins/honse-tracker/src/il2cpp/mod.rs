//! Thin IL2CPP helpers for skill-shop purchase (fork `crate::il2cpp` surface).
//!
//! Backed by `edge_sdk::Api` / `Sdk`. Preserves the names `purchase.rs` imports
//! without pulling host internals. Layout constants match IL2CPP 64-bit
//! (`kIl2CppSizeOfArray = 32` → `Array::data_ptr` = `this.add(1)`).

pub mod api;
pub mod symbols;
pub mod types;
