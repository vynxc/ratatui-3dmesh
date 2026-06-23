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

- `color_mode(ColorMode)`: `Material`, `Lighting`, or `Off`.
- `light_direction([f32; 3])`: light vector.
- `lighting(ambient, diffuse)`: light weights.
- `foreground_style(Style)`: fallback style.
- `background_style(Option<Style>)`: optional clear style.

## UX

- `show_hints(bool)`: draw a compact status hint.
- `show_help_overlay(bool)`: force the built-in controls overlay.
- `auto_spin([f32; 3])`: radians per second around x/y/z when state auto-spin is enabled.

## Presets

```rust
let fast = Mesh3dConfig::fast();
let quality = Mesh3dConfig::quality();
```
