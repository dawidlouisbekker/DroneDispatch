//! Compiles every `.proto` under the repository's `proto/` directory with the
//! pure-Rust `protox` compiler (no `protoc` needed), then generates prost
//! messages and tonic clients and servers.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use prost::Message;

fn main() -> Result<(), Box<dyn Error>> {
    let proto_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../proto");
    println!("cargo:rerun-if-changed={}", proto_root.display());

    let mut files = Vec::new();
    collect_protos(&proto_root, &proto_root, &mut files)?;
    files.sort();

    let descriptors = protox::compile(&files, [&proto_root])?;
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    fs::write(out_dir.join("descriptor.bin"), descriptors.encode_to_vec())?;

    tonic_prost_build::configure()
        .include_file("dronedrop.rs")
        .compile_fds(descriptors)?;
    Ok(())
}

/// Collects `.proto` paths relative to `root`, as protox expects them.
fn collect_protos(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_protos(root, &path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "proto") {
            files.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}
