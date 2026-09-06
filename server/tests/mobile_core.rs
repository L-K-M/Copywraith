//! Exercise headless initialization against the actual client modules.
#![allow(dead_code, unexpected_cfgs)]

#[path = "../../src-tauri/src/mobile_core.rs"]
mod mobile_core;
#[path = "../../src-tauri/src/mobile_runtime.rs"]
mod mobile_runtime;
#[path = "../../src-tauri/src/models.rs"]
mod models;
#[path = "../../src-tauri/src/storage.rs"]
mod storage;
#[path = "../../src-tauri/src/sync.rs"]
mod sync;
