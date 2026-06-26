# Repository Guidelines

## Project Structure & Module Organization
- This is a Rust 2021 library crate named `ratatui-3dmesh` with MSRV `1.88` in `Cargo.toml`.
- Public API is re-exported from `src/lib.rs`; core modules live under `src/animation.rs`, `src/config.rs`, `src/model.rs`, `src/widget.rs`, and `src/error.rs`.
- Format loading is organized under `src/loader/` (`obj.rs`, `mtl.rs`, `gltf.rs`, `texture.rs`). Rendering internals live under `src/render/` (`camera.rs`, `color.rs`, `pipeline.rs`, `raster.rs`).
- The interactive demo is `examples/viewer.rs`; bundled redistributable assets are under `examples/assets/`.
- Integration tests live in `tests/`. Wiki source lives in `docs/wiki/`; `scripts/publish-wiki.sh` syncs those pages to a GitHub wiki checkout.
- Ignore generated or local-only directories such as `target/`, `.mimir/`, `.codebase-memory/`, `.wiki/`, and `models/` unless a task explicitly targets them.

## Build, Test, and Development Commands
- Format: `cargo fmt --all`; CI checks with `cargo fmt --all --check`.
- Test all features: `cargo test --all-features`.
- Lint strictly: `cargo clippy --all-targets --all-features -- -D warnings`.
- Build docs: `cargo doc --all-features --no-deps`.
- Check MSRV compatibility when relevant: `cargo +1.88.0 check --all-features`.
- Run the example viewer: `cargo run --example viewer --features cli-example` or pass an asset path such as `examples/assets/pyramid.obj`.

## Coding Style & Naming Conventions
- Follow standard Rust formatting and idioms. Keep modules focused and preserve existing public re-export patterns in `src/lib.rs`.
- Feature-gate optional APIs consistently: defaults include `obj`, `mtl`, `gltf`, and `textures`; `cli-example` gates crossterm controls and the viewer support; `serde` gates serialization derives where practical.
- Keep terminal lifecycle and event-loop code out of the library; `CONTRIBUTING.md` states standalone terminal code belongs in examples or downstream apps.

## Testing Guidelines
- Prefer public-API integration coverage for loader and renderer behavior, matching `tests/example_models.rs` (`Mesh::load` plus rendering into a Ratatui `TestBackend`).
- Normal `cargo test` runs must stay offline. The broad glTF sweep in `tests/gltf_corpus.rs` is ignored and requires `GLTF_CORPUS_DIR`.
- To run the optional corpus sweep: `./scripts/fetch-gltf-corpus.sh` then `GLTF_CORPUS_DIR=models/corpus cargo test --test gltf_corpus --features "gltf textures" -- --ignored --nocapture`.

## Security & Configuration Notes
- Only add model assets that are public domain, MIT/Apache compatible, or explicitly redistributable; include attribution and license notes.
- Avoid adding heavy decoders or dependencies unless the crate can actually support the associated format or extension.

## Agent Workflow Notes
- Preserve user changes and avoid editing generated package copies under `target/package/`.
- When changing feature-gated code, run the narrow affected tests plus the relevant feature combination from the CI matrix.
