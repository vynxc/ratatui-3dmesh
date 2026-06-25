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
  your-model.obj --texture your-basecolor.png
```

## glTF / GLB

The `gltf` feature (on by default) enables `.gltf` and `.glb` loading.

Supported:

- Mesh primitives and triangle indices
- Node transforms
- Node translation/rotation/scale animation clips with step or linear interpolation
- CPU skinning for meshes with `JOINTS_0` and `WEIGHTS_0`
- Vertex positions, normals, and UV set 0
- PBR base-color factor and base-color texture
- `alphaMode` (OPAQUE/MASK/BLEND), `alphaCutoff`, and `doubleSided`
- Emissive factor and emissive texture
- Embedded images, decoded automatically when the `textures` feature is enabled

The renderer honors these material semantics: double-sided surfaces are never back-face culled, masked surfaces cut out below the cutoff, blended surfaces composite back-to-front, and emissive contributions are added on top of the lit color.

Imported clips are exposed as `mesh.animations`. Advanced glTF animation features such as morph-target weights and cubic-spline interpolation are not evaluated in this pass.

Example:

```bash
cargo run --release --example viewer --features cli-example -- \
  examples/assets/gltf/fox.glb
```

To sweep a broad corpus of real-world models, see `scripts/fetch-gltf-corpus.sh` and the
`tests/gltf_corpus.rs` ignored test.

## Tips

- Prefer triangulated or moderately sized meshes for best terminal performance.
- Use `Mesh3dConfig::fast()` or `max_faces(...)` for large models.
- If a model looks stretched, tune `cell_aspect_ratio(...)` for your font and terminal.
- If a textured OBJ appears vertically inverted, toggle `flip_texture_v(false)`.
