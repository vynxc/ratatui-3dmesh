# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.3] - 2026-07-29

### Added
- Opt-in, memory-bounded animation frame caching for `PreparedMesh`, including
  cache statistics, explicit invalidation, and exact sparse Ratatui-cell replay.
- Render profiling metrics and a `frame_cache_benchmark` example for measuring
  animation, projection, rasterization, allocations, memory, and cache replay.

### Changed
- Updated Ratatui to `0.30.2`.
- Accelerated prepared animated rendering with reusable sampled geometry,
  precomputed topology/material data, deferred opaque shading, and cache-friendly
  texture sampling.

### Fixed
- Added optional animation loop blending to remove visible tail-to-head jumps.
- Corrected cached animation frame selection at floating-point and loop boundaries.
- Restricted the frame-cache benchmark to its required `gltf` and `textures`
  features so every CI feature-matrix job builds successfully.

## [0.1.1] - 2026-07-07

### Added
- Bundled redistributable glTF sample models under `examples/assets/gltf/`
  (`box_textured.glb`, `box_animated.glb`, `fox.glb`) with per-model attribution in
  `examples/assets/gltf/LICENSE.md`, so the example viewer and tests work on a fresh
  clone.
- `tests/example_models.rs`: end-to-end tests that load each bundled model through the
  public `Mesh::load` API and render it into a `ratatui` test backend.
- GitHub Actions CI (`fmt`, `clippy`, `test`, a feature matrix, `doc`, and a pinned
  MSRV check), a tag-triggered crates.io release workflow, and Dependabot for the
  `cargo` and `github-actions` ecosystems.

### Changed
- Declared MSRV raised to `1.88` to match the actual dependency floor (the previous
  `1.74` no longer built).
- `Cargo.toml` now points `repository`/`homepage` at the real GitHub project and
  `exclude`s local-only directories from published packages.
- Documentation (README and wiki) now references the bundled sample assets and a
  GitHub-based install instead of local, non-redistributable model paths.

### Removed
- Tests that depended on local, non-redistributable models (Shantae) or on
  git-ignored assets that silently no-op'd in CI. They are replaced by real tests over
  the bundled corpus.

[Unreleased]: https://github.com/vynxc/ratatui-3dmesh/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/vynxc/ratatui-3dmesh/compare/v0.1.2...v0.1.3
[0.1.1]: https://github.com/vynxc/ratatui-3dmesh/releases/tag/v0.1.2
