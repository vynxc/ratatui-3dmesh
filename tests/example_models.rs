//! End-to-end checks against the small, redistributable glTF models bundled under
//! `examples/assets/gltf/`. Unlike `tests/gltf_corpus.rs` (an opt-in sweep over a
//! locally fetched Khronos corpus), these run on every `cargo test` because the assets
//! ship in the repository.
//!
//! They drive the public API only: `Mesh::load` plus the stateful widget rendered into
//! a `ratatui` test backend, so a regression in loading *or* rasterization is caught.

#![cfg(all(feature = "gltf", feature = "textures"))]

use std::path::Path;

use ratatui::{backend::TestBackend, Terminal};
use ratatui_3dmesh::{Mesh, Mesh3dConfig, Mesh3dState, Mesh3dWidget};

fn asset(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/assets/gltf")
        .join(name)
}

/// Render a mesh into an off-screen buffer and report whether any glyph was painted.
fn renders_something(mesh: &Mesh) -> bool {
    let backend = TestBackend::new(48, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = Mesh3dState::default();
    terminal
        .draw(|frame| {
            frame.render_stateful_widget(
                Mesh3dWidget::new(mesh).config(Mesh3dConfig::default().backface_culling(false)),
                frame.area(),
                &mut state,
            );
        })
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .any(|cell| cell.symbol() != " ")
}

#[test]
fn box_textured_loads_and_renders() {
    let mesh = Mesh::load(asset("box_textured.glb")).expect("load box_textured.glb");
    assert!(!mesh.vertices.is_empty(), "geometry must load");
    assert!(!mesh.faces.is_empty(), "faces must load");
    assert!(!mesh.tex_coords.is_empty(), "textured box has UVs");
    assert!(!mesh.textures.is_empty(), "embedded texture must decode");
    assert!(renders_something(&mesh), "textured box must paint glyphs");
}

#[test]
fn box_animated_loads_and_renders() {
    let mesh = Mesh::load(asset("box_animated.glb")).expect("load box_animated.glb");
    assert!(!mesh.faces.is_empty(), "faces must load");
    assert!(!mesh.animations.is_empty(), "node animation must import");
    assert!(renders_something(&mesh), "animated box must paint glyphs");
}

#[test]
fn fox_loads_skinned_and_renders() {
    let mesh = Mesh::load(asset("fox.glb")).expect("load fox.glb");
    assert!(!mesh.faces.is_empty(), "faces must load");
    assert!(!mesh.skins.is_empty(), "fox is skinned");
    assert!(
        mesh.animations.len() >= 2,
        "fox ships multiple clips, got {}",
        mesh.animations.len()
    );
    assert!(renders_something(&mesh), "fox must paint glyphs");
}
