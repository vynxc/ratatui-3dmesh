# ratatui-3dmesh

A reusable [Ratatui](https://ratatui.rs/) widget for viewing 3D meshes as shaded terminal ASCII with optional truecolor texture output.

`ratatui-3dmesh` is inspired by:

- [`autopawn/3d-ascii-viewer`](https://github.com/autopawn/3d-ascii-viewer) — C/ncurses OBJ/STL ASCII rendering with optional MTL diffuse colors.
- [`luisbedoia/sx3d`](https://github.com/luisbedoia/sx3d) — a simple Rust console 3D viewer UX.

This crate is built for embedding. Your app owns terminal initialization, layout, and event loops; this crate provides mesh loading, configuration, rendering, state, and optional crossterm controls.

> Status: early public crate. OBJ/STL/glTF/MTL, glTF node animation playback, UV parsing, optional texture images, solid/wire/point modes, color policies, controls, docs, and an example viewer are included.

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

For glTF/GLB loading:

```toml
ratatui-3dmesh = { version = "0.1", features = ["gltf"] }
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


## glTF usage

glTF support is available through the optional `gltf` feature. The loader reads `.gltf`/`.glb` mesh primitives, indices, normals, UVs, base-color factors, and base-color textures when `textures` is also enabled.

Embedded glTF/GLB animations are imported as `mesh.animations`. This first pass supports node translation, rotation, and scale channels with linear or step interpolation, including CPU skinning for glTF meshes with `JOINTS_0`/`WEIGHTS_0`. Morph-target weights and cubic-spline interpolation are left as follow-up scope.

Run the axe asset:

```bash
cargo run --release --example viewer --features "cli-example gltf textures" -- \
  models/axe/scene.gltf
```

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


cargo run --release --example viewer --features "cli-example gltf textures" -- \
  models/axe/scene.gltf
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
| `o` | toggle perspective/orthographic projection |
| `[` / `]` | decrease/increase color brightness |
| `space` | toggle auto-spin |
| `p` | play/pause animation |
| `n` / `b` | next/previous animation clip |
| `0` | restart animation |
| `,` / `.` | slow down/speed up animation |
| `v` | toggle animation looping |
| `r` | reset view |
| `?` | help overlay |
| `q` / Esc | quit example |

## Supported formats

| Format | Support |
| --- | --- |
| OBJ | vertices, texture coordinates, normals, polygon faces, negative indices, `usemtl`, companion `mtllib` |
| MTL | `newmtl`, diffuse `Kd` colors, diffuse `map_Kd` texture paths |
| Textures | optional PNG/JPEG decode to RGBA8 via the `textures` feature; manual `--texture` override supported |
| glTF/GLB | mesh primitives, indices, normals, UVs, base-color factors, node TRS animations, base-color textures with `gltf` + `textures` |
| STL | ASCII STL and binary STL |

STL and OBJ are static formats in this crate and expose `mesh.animations.is_empty()`. STL files and OBJ/glTF primitives without UVs continue to render with material/lighting/grayscale modes.

## Configuration highlights

`Mesh3dConfig` is a typed builder with defaults that work well in a terminal:

- `glyph_ramp(...)` — dark-to-light ASCII ramp.
- `render_mode(RenderMode::{Solid, Wireframe, Points})`.
- `projection(ProjectionMode::{Perspective, Orthographic})`.
- `color_mode(ColorMode::{Material, Lighting, Texture, Auto, Off})`.
- `texture_filter(TextureFilter::{Nearest, Bilinear})`, `texture_wrap(TextureWrap::{Repeat, Clamp})`.
- `flip_texture_v(...)`, `texture_lighting(...)`, `color_brightness(...)`.
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
| `gltf` | no | glTF/GLB mesh, material, UV, and base-color texture loading |
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
