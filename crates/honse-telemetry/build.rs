//! Compile the telemetry `.proto` schema into Rust with prost.
//!
//! Uses `protox` (a pure-Rust protobuf compiler) so the build does NOT depend on
//! a `protoc` binary being installed — important for CI portability.

use std::path::PathBuf;

fn main() {
    let proto = "proto/hachimi/telemetry/v1/telemetry.proto";
    println!("cargo:rerun-if-changed={proto}");
    println!("cargo:rerun-if-changed=proto");

    let file_descriptors = protox::compile([proto], ["proto"]).expect("protox failed to compile telemetry.proto");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));
    prost_build::Config::new()
        .file_descriptor_set_path(out_dir.join("telemetry_fds.bin"))
        .compile_fds(file_descriptors)
        .expect("prost-build failed to generate code");
}
