# Configuration

`Mesh3dConfig` uses a builder-style API and has public fields for advanced callers.

## Rendering

- `glyph_ramp(String)`: characters ordered from darkest to brightest.
- `render_mode(RenderMode)`: `Solid`, `Wireframe`, or `Points`.
- `projection(ProjectionMode)`: `Perspective` or `Orthographic`.
- `max_faces(Option<usize>)`: cap work for large meshes.

## Camera and geometry

- `auto_fit(bool)`: reserved for automatic fitting behavior; defaults true.
- `scale(f32)`: extra model scale.
- `fov_y_degrees(f32)`: perspective FOV.
- `cell_aspect_ratio(f32)`: correct terminal cell shape.
- `backface_culling(bool)`: skip triangles facing away.
- `normalize(bool)`: center and scale input bounds before rendering.

## Lighting and color

- `color_mode(ColorMode)`: `Material`, `Lighting`, `Texture`, `Auto`, or `Off`.
- `light_direction([f32; 3])`: light vector.
- `lighting(ambient, diffuse)`: light weights.

- `color_brightness(f32)`: multiply truecolor RGB output; values above `1.0` make material, texture, and lighting colors easier to see in dim terminals.
- `foreground_style(Style)`: fallback style.
- `background_style(Option<Style>)`: optional clear style.

Color modes:

- `Off`: use the configured foreground style only.
- `Material`: use MTL diffuse `Kd` colors when available.
- `Lighting`: render grayscale truecolor from lighting intensity.
- `Texture`: sample loaded diffuse textures when a triangle has UVs; otherwise fall back to material/foreground.
- `Auto`: prefer texture, then material, then fallback style.

## Textures

Available when the `textures` feature is enabled:

- `texture_filter(TextureFilter)`: `Nearest` for speed or `Bilinear` for smoother sampling.
- `texture_wrap(TextureWrap)`: `Repeat` or `Clamp` for UVs outside `[0, 1]`.
- `flip_texture_v(bool)`: defaults true to handle common OBJ/image origin differences.
- `texture_lighting(bool)`: multiply sampled RGB by mesh lighting. If colors look too dark, either disable this or raise `color_brightness(...)`.

Texture loading is configured through `MeshLoadOptions`:

```rust,no_run
use ratatui_3dmesh::{Mesh, MeshLoadOptions};

# fn load() -> ratatui_3dmesh::Result<Mesh> {
let mesh = Mesh::load_with_options(
    "model.obj",
    MeshLoadOptions::default()
        .load_material_textures(true)
        .texture_override("base_color.png"),
)?;
# Ok(mesh)
# }
```

## UX

- `show_hints(bool)`: draw a compact status hint.
- `show_help_overlay(bool)`: force the built-in controls overlay.
- `auto_spin([f32; 3])`: radians per second around x/y/z when state auto-spin is enabled.

## Presets

```rust
let fast = Mesh3dConfig::fast();
let quality = Mesh3dConfig::quality();
```

`fast()` favors wireframe and a face cap. `quality()` uses a longer glyph ramp and bilinear texture sampling.
