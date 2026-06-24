# Embedding in Ratatui

The library exposes a stateful widget. Your application owns terminal setup, event polling, and layout.

```rust,no_run
use ratatui_3dmesh::{Mesh, Mesh3dConfig, Mesh3dState, Mesh3dWidget};

# fn draw(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) -> ratatui_3dmesh::Result<()> {
let mesh = Mesh::load("model.obj")?;
let mut state = Mesh3dState::default();
let config = Mesh3dConfig::default();

frame.render_stateful_widget(
    Mesh3dWidget::new(&mesh).config(config),
    area,
    &mut state,
);
# Ok(())
# }
```

## Event handling

With the `cli-example` feature, `ControlMap` can mutate `Mesh3dState` and `Mesh3dConfig` from crossterm key events. Host applications may ignore it and map input however they like.

## Animation playback

glTF/GLB files may populate `mesh.animations`; OBJ and STL leave it empty. Embedders can inspect clip metadata, choose a clip on `Mesh3dState`, advance state in their event loop, and render the same `Mesh3dWidget`.

```rust,no_run
use ratatui_3dmesh::{Mesh, Mesh3dConfig, Mesh3dState, Mesh3dWidget};

# fn draw(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) -> ratatui_3dmesh::Result<()> {
let mesh = Mesh::load("animated.glb")?;
let config = Mesh3dConfig::default();
let mut state = Mesh3dState::default();

if !mesh.animations.is_empty() {
    state.select_animation(0, mesh.animations.len());
}

state.tick(1.0 / 60.0, &config);
frame.render_stateful_widget(
    Mesh3dWidget::new(&mesh).config(config),
    area,
    &mut state,
);
# Ok(())
# }
```
