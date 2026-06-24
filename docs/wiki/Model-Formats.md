# Model Formats

## OBJ

Supported:

- `v x y z` vertices
- `vt u v [w]` texture coordinates
- `vn x y z` vertex normals
- `f` polygon faces in common forms: `v`, `v/vt`, `v//vn`, and `v/vt/vn`
- Positive and negative indices
- `usemtl` material references
- `mtllib` companion material files when `mtl` is enabled

Texture notes:

- Accurate texture placement requires UV coordinates and face UV indices.
- OBJ files with UVs but missing MTL files can use a manual texture override in the example with `--texture <image>` or through `MeshLoadOptions::texture_override(...)`.
- If UVs are absent, the renderer falls back to material/lighting modes instead of trying to guess an unwrap.

Ignored for now:

- Curves and advanced OBJ statements
- Normal maps, bump maps, displacement maps, and PBR material fields

## MTL

Supported:

- `newmtl name`
- `Kd r g b` diffuse color
- `map_Kd path/to/diffuse.png` diffuse texture maps
- Common `map_Kd` option flags are skipped well enough to recover normal file paths

`map_Kd` texture paths are resolved relative to the MTL file.

## Texture images

The optional `textures` feature enables image decoding through the Rust `image` crate.

Supported image workflows:

- OBJ + MTL + `map_Kd` PNG/JPEG texture
- OBJ + UVs + manual texture override
- Mislabeled extensions when the image bytes are valid PNG/JPEG, because decoding sniffs content

Example:

```bash
cargo run --release --example viewer --features "cli-example textures" -- \
  models/model.obj --texture models/AXEE_LP_exported_Base_color.jpg
```

## glTF / GLB

The optional `gltf` feature enables `.gltf` and `.glb` loading.

Supported:

- Mesh primitives and triangle indices
- Node transforms
- Vertex positions, normals, and UV set 0
- PBR base-color factors as material diffuse colors
- PBR base-color textures when `textures` is also enabled

Example:

```bash
cargo run --release --example viewer --features "cli-example gltf textures" -- \
  models/axe/scene.gltf
```


## STL

Supported:

- ASCII STL
- Binary STL
- Facet normals when present

STL does not carry UV texture coordinates in this loader, so STL renders with material/lighting/foreground color modes.

## Tips

- Prefer triangulated or moderately sized meshes for best terminal performance.
- Use `Mesh3dConfig::fast()` or `max_faces(...)` for large models.
- If a model looks stretched, tune `cell_aspect_ratio(...)` for your font and terminal.
- If a textured OBJ appears vertically inverted, toggle `flip_texture_v(false)`.
