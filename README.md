# ratatui-3dmesh

A reusable [Ratatui](https://ratatui.rs/) widget for viewing 3D meshes as shaded terminal ASCII with optional truecolor texture output.

`ratatui-3dmesh` is inspired by:

- [`autopawn/3d-ascii-viewer`](https://github.com/autopawn/3d-ascii-viewer) — C/ncurses OBJ/STL ASCII rendering with optional MTL diffuse colors.
- [`luisbedoia/sx3d`](https://github.com/luisbedoia/sx3d) — a simple Rust console 3D viewer UX.

This crate is built for embedding. Your app owns terminal initialization, layout, and event loops; this crate provides mesh loading, configuration, rendering, state, and optional crossterm controls.

> Status: early public crate. OBJ/STL/MTL, UV parsing, optional texture images, solid/wire/point modes, color policies, controls, docs, and an example viewer are included.

## Install

```toml
[dependencies]
ratatui-3dmesh = "0.1"
ratatui = "0.29"
```

For keyboard helpers based on crossterm:

```toml
ratatui-3dmesh = { version = "0.1", features = ["cli-example"] }
```

For PNG/JPEG texture images:

```toml
ratatui-3dmesh = { version = "0.1", features = ["textures"] }
```

## Use as a Ratatui widget

```rust,no_run
use ratatui_3dmesh::{Mesh, Mesh3dConfig, Mesh3dState, Mesh3dWidget};

# fn draw(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) -> ratatui_3dmesh::Result<()> {
let mesh = Mesh::load("model.obj")?;
let config = Mesh3dConfig::default()
    .auto_fit(true)
    .show_hints(true);
let mut state = Mesh3dState::default();

frame.render_stateful_widget(
    Mesh3dWidget::new(&mesh).config(config),
    area,
    &mut state,
);
# Ok(())
# }
```

## Textured OBJ usage

Texture rendering requires OBJ UV coordinates (`vt`) and face UV references such as `f 1/1/1 2/2/2 3/3/3`.

MTL-driven texture:

```text
model.obj  -> mtllib model.mtl
model.mtl  -> map_Kd texture.png
```

Manual texture override for models that have UVs but no usable MTL:

```rust,no_run
use ratatui_3dmesh::{Mesh, MeshLoadOptions};

# fn load() -> ratatui_3dmesh::Result<Mesh> {
let mesh = Mesh::load_with_options(
    "models/model.obj",
    MeshLoadOptions::default()
        .load_material_textures(true)
        .texture_override("models/AXEE_LP_exported_Base_color.jpg"),
)?;
# Ok(mesh)
# }
```

The texture loader sniffs image bytes instead of trusting the extension, so a PNG file named `.jpg` can still decode.

## Run the example viewer

```bash
cargo run --example viewer --features cli-example
cargo run --example viewer --features cli-example -- examples/assets/pyramid.obj
cargo run --example viewer --features cli-example -- examples/assets/tetra.stl
```

Run with texture support:

```bash
cargo run --release --example viewer --features "cli-example textures" -- \
  models/model.obj --texture models/AXEE_LP_exported_Base_color.jpg
```

Controls:

| Key | Action |
| --- | --- |
| Arrow keys / `wasd` | rotate |
| `z` / `x` | roll |
| `hjkl` | pan |
| `+` / `-` | zoom |
| `m` | cycle solid/wireframe/points |
| `c` | cycle material/lighting/texture/auto/off color |
| `space` | toggle auto-spin |
| `r` | reset view |
| `?` | help overlay |
| `q` / Esc | quit example |

## Supported formats

| Format | Support |
| --- | --- |
| OBJ | vertices, texture coordinates, normals, polygon faces, negative indices, `usemtl`, companion `mtllib` |
| MTL | `newmtl`, diffuse `Kd` colors, diffuse `map_Kd` texture paths |
| Textures | optional PNG/JPEG decode to RGBA8 via the `textures` feature; manual `--texture` override supported |
| STL | ASCII STL and binary STL |

STL files and OBJ files without UVs continue to render with material/lighting/grayscale modes.

## Configuration highlights

`Mesh3dConfig` is a typed builder with defaults that work well in a terminal:

- `glyph_ramp(...)` — dark-to-light ASCII ramp.
- `render_mode(RenderMode::{Solid, Wireframe, Points})`.
- `projection(ProjectionMode::{Perspective, Orthographic})`.
- `color_mode(ColorMode::{Material, Lighting, Texture, Auto, Off})`.
- `texture_filter(TextureFilter::{Nearest, Bilinear})`, `texture_wrap(TextureWrap::{Repeat, Clamp})`.
- `flip_texture_v(...)`, `texture_lighting(...)`.
- `scale(...)`, `fov_y_degrees(...)`, `cell_aspect_ratio(...)`.
- `backface_culling(...)`, `light_direction(...)`, `lighting(...)`.
- `auto_spin([x, y, z])`, `max_faces(...)`.
- `foreground_style(...)`, `background_style(...)`, `show_hints(...)`, `show_help_overlay(...)`.

Presets:

```rust
let fast = Mesh3dConfig::fast();
let pretty = Mesh3dConfig::quality();
```

## Feature flags

| Feature | Default | Description |
| --- | --- | --- |
| `obj` | yes | Wavefront OBJ loading |
| `stl` | yes | ASCII/binary STL loading |
| `mtl` | yes | OBJ material diffuse-color and `map_Kd` metadata loading |
| `textures` | no | PNG/JPEG texture image decoding and texture-colored rendering |
| `serde` | no | serialize/deserialize public config/model/state types where practical |
| `cli-example` | no | crossterm keyboard control helpers and example support |

## Public docs / GitHub Wiki

Wiki source lives in [`docs/wiki`](docs/wiki). GitHub Wikis are separate repositories, so these Markdown pages can be copied or synchronized when publishing the project wiki.

## Development

```bash
cargo fmt --all
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --all-features --no-deps
```

## Credits

- ASCII/luminance 3D viewer inspiration: [`autopawn/3d-ascii-viewer`](https://github.com/autopawn/3d-ascii-viewer).
- Rust terminal 3D viewer reference: [`luisbedoia/sx3d`](https://github.com/luisbedoia/sx3d).
- UI framework: [Ratatui](https://ratatui.rs/).
- Image decoding: [`image`](https://crates.io/crates/image) when the `textures` feature is enabled.
- Included example pyramid/tetrahedron assets are simple generated fixtures released under this repository's MIT license.

## License

MIT
