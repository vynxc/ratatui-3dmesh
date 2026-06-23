# Model Formats

## OBJ

Supported:

- `v x y z` vertices
- `f` polygon faces
- Positive and negative vertex indices
- `usemtl` material references
- `mtllib` companion material files when `mtl` is enabled

Ignored for now:

- Texture coordinates
- Vertex normals from OBJ face tuples
- Texture images
- Curves and advanced OBJ statements

## MTL

Supported:

- `newmtl name`
- `Kd r g b` diffuse color

## STL

Supported:

- ASCII STL
- Binary STL
- Facet normals when present

## Tips

- Prefer triangulated or moderately sized meshes for best terminal performance.
- Use `Mesh3dConfig::fast()` or `max_faces(...)` for large models.
- If a model looks stretched, tune `cell_aspect_ratio(...)` for your font and terminal.
