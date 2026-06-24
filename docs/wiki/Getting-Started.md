# Getting Started

## Add the crate

```toml
[dependencies]
ratatui-3dmesh = "0.1"
```

Enable keyboard helpers for crossterm apps:

```toml
ratatui-3dmesh = { version = "0.1", features = ["cli-example"] }
```

Enable texture images:

```toml
ratatui-3dmesh = { version = "0.1", features = ["textures"] }
```

## Run the bundled viewer

```bash
cargo run --example viewer --features cli-example
cargo run --example viewer --features cli-example -- examples/assets/pyramid.obj
```

Run a textured OBJ in release mode:

```bash
cargo run --release --example viewer --features "cli-example textures" -- \
  models/model.obj --texture models/AXEE_LP_exported_Base_color.jpg
```

The `--texture` flag is useful when an OBJ has UVs but no usable MTL file. OBJ + MTL + `map_Kd` textures can load without `--texture` when `load_material_textures` is enabled by the example.

## Basic controls

- Arrow keys / `wasd`: rotate
- `hjkl`: pan
- `+` / `-`: zoom
- `m`: cycle render modes
- `c`: cycle color modes, including texture modes when texture data is loaded
- `space`: auto-spin
- `?`: help
- `q`: quit the example
