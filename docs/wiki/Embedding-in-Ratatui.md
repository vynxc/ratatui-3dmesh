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
