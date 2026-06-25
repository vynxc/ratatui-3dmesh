# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/vynxc/ratatui-3dmesh/commits/main
