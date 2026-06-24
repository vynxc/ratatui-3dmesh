//! Broad glTF/GLB compatibility sweep over a local corpus of models.
//!
//! This test is `#[ignore]` by default so normal `cargo test` runs stay fast and offline.
//! Point it at a directory of `.gltf`/`.glb` files and run it explicitly:
//!
//! ```sh
//! # Download the Khronos glTF-Sample-Assets corpus (or any folder of models):
//! ./scripts/fetch-gltf-corpus.sh
//! GLTF_CORPUS_DIR=models/corpus cargo test --test gltf_corpus \
//!     --features "gltf textures" -- --ignored --nocapture
//! ```
//!
//! Every model must load without panicking and produce non-empty geometry. Models that load
//! but render nothing (no faces) are reported as failures so regressions in format coverage
//! are caught against real-world assets.

#![cfg(feature = "gltf")]

use std::{fs, path::Path, path::PathBuf};

use ratatui_3dmesh::Mesh;

fn collect_models(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_models(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("gltf") | Some("glb")
        ) {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "requires a local glTF corpus; set GLTF_CORPUS_DIR and run with --ignored"]
fn loads_every_model_in_corpus() {
    let Ok(dir) = std::env::var("GLTF_CORPUS_DIR") else {
        eprintln!("set GLTF_CORPUS_DIR to a folder of .gltf/.glb files");
        return;
    };
    let mut models = Vec::new();
    collect_models(Path::new(&dir), &mut models);
    models.sort();
    assert!(!models.is_empty(), "no .gltf/.glb files found under {dir}");

    let mut failures = Vec::new();
    let mut loaded = 0usize;
    for path in &models {
        match Mesh::load(path) {
            Ok(mesh) => {
                if mesh.faces.is_empty() || mesh.vertices.is_empty() {
                    failures.push(format!("{}: loaded but no geometry", path.display()));
                } else {
                    loaded += 1;
                }
            }
            Err(err) => failures.push(format!("{}: {err}", path.display())),
        }
    }

    eprintln!(
        "glTF corpus: {loaded}/{} models loaded cleanly",
        models.len()
    );
    assert!(
        failures.is_empty(),
        "{} model(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
