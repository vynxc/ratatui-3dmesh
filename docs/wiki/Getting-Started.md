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

## Run the bundled viewer

```bash
cargo run --example viewer --features cli-example
cargo run --example viewer --features cli-example -- examples/assets/pyramid.obj
```

## Basic controls

- Arrow keys / `wasd`: rotate
- `hjkl`: pan
- `+` / `-`: zoom
- `m`: cycle render modes
- `c`: cycle color modes
- `space`: auto-spin
- `?`: help
- `q`: quit the example
