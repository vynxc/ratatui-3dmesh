use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::StatefulWidget,
};

use crate::{
    config::Mesh3dConfig,
    model::{Mesh, Vec3},
    render::render_mesh,
};

/// Persistent viewer state for [`Mesh3dWidget`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mesh3dState {
    /// Euler rotation in radians.
    pub rotation: Vec3,
    /// Pan in normalized model space.
    pub pan: Vec3,
    /// Zoom multiplier.
    pub zoom: f32,
    /// Whether auto-spin is currently enabled.
    pub auto_spin_enabled: bool,
    /// Whether the built-in help overlay should be drawn.
    pub help_visible: bool,
}

impl Default for Mesh3dState {
    fn default() -> Self {
        Self {
            rotation: Vec3::new(0.35, -0.45, 0.0),
            pan: Vec3::default(),
            zoom: 1.0,
            auto_spin_enabled: false,
            help_visible: false,
        }
    }
}

impl Mesh3dState {
    /// Rotate by Euler deltas in radians.
    pub fn rotate(&mut self, delta: Vec3) {
        self.rotation += delta;
    }

    /// Pan in normalized screen/model space.
    pub fn pan(&mut self, delta: Vec3) {
        self.pan += delta;
    }

    /// Multiply zoom by `factor`.
    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(0.05, 100.0);
    }

    /// Reset view controls to defaults.
    pub fn reset_view(&mut self) {
        let help_visible = self.help_visible;
        let auto_spin_enabled = self.auto_spin_enabled;
        *self = Self::default();
        self.help_visible = help_visible;
        self.auto_spin_enabled = auto_spin_enabled;
    }

    /// Toggle auto-spin.
    pub fn toggle_auto_spin(&mut self) {
        self.auto_spin_enabled = !self.auto_spin_enabled;
    }

    /// Toggle the built-in help overlay.
    pub fn toggle_help(&mut self) {
        self.help_visible = !self.help_visible;
    }

    /// Advance time-based state such as auto-spin.
    pub fn tick(&mut self, delta_seconds: f32, config: &Mesh3dConfig) {
        if self.auto_spin_enabled {
            self.rotation.x += config.auto_spin[0] * delta_seconds;
            self.rotation.y += config.auto_spin[1] * delta_seconds;
            self.rotation.z += config.auto_spin[2] * delta_seconds;
        }
    }
}

/// A reusable Ratatui widget that renders a [`Mesh`].
#[derive(Debug, Clone)]
pub struct Mesh3dWidget<'a> {
    mesh: &'a Mesh,
    config: Mesh3dConfig,
}

impl<'a> Mesh3dWidget<'a> {
    /// Create a widget for `mesh` with default configuration.
    #[must_use]
    pub fn new(mesh: &'a Mesh) -> Self {
        Self {
            mesh,
            config: Mesh3dConfig::default(),
        }
    }

    /// Set widget configuration.
    #[must_use]
    pub fn config(mut self, config: Mesh3dConfig) -> Self {
        self.config = config;
        self
    }

    /// Borrow the active configuration.
    #[must_use]
    pub fn config_ref(&self) -> &Mesh3dConfig {
        &self.config
    }
}

impl StatefulWidget for Mesh3dWidget<'_> {
    type State = Mesh3dState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        render_mesh(self.mesh, area, buf, state, &self.config);
        if self.config.show_hints {
            draw_hints(area, buf, self.mesh, state);
        }
        if self.config.show_help_overlay || state.help_visible {
            draw_help(area, buf);
        }
    }
}

fn draw_hints(area: Rect, buf: &mut Buffer, mesh: &Mesh, state: &Mesh3dState) {
    if area.width < 20 || area.height == 0 {
        return;
    }
    let text = format!(
        " {} | faces:{} | zoom:{:.2} | ? help ",
        mesh.name,
        mesh.faces.len(),
        state.zoom
    );
    draw_text(
        area.x,
        area.y,
        area.width,
        buf,
        &text,
        Style::default().fg(Color::Gray),
    );
}

fn draw_help(area: Rect, buf: &mut Buffer) {
    if area.width < 30 || area.height < 9 {
        return;
    }
    let lines = [
        " ratatui-3dmesh controls ",
        " arrows / wasd : rotate ",
        " hjkl          : pan ",
        " + / -         : zoom ",
        " m             : render mode ",
        " c             : color mode ",
        " [ / ]         : brightness ",
        " space         : auto-spin ",
        " r reset | ? help | q quit ",
    ];
    let width = lines.iter().map(|line| line.len()).max().unwrap_or(0) as u16 + 2;
    let height = lines.len() as u16 + 2;
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let style = Style::default()
        .fg(Color::White)
        .bg(Color::Black)
        .add_modifier(Modifier::BOLD);

    for row in 0..height.min(area.height) {
        for col in 0..width.min(area.width) {
            let bx = x + col;
            let by = y + row;
            if bx < area.x + area.width && by < area.y + area.height {
                buf[(bx, by)].set_char(' ').set_style(style);
            }
        }
    }
    for (i, line) in lines.iter().enumerate() {
        draw_text(
            x + 1,
            y + 1 + i as u16,
            width.saturating_sub(2),
            buf,
            line,
            style,
        );
    }
}

fn draw_text(x: u16, y: u16, width: u16, buf: &mut Buffer, text: &str, style: Style) {
    for (offset, ch) in text.chars().take(width as usize).enumerate() {
        let cell = &mut buf[(x + offset as u16, y)];
        cell.set_char(ch);
        cell.set_style(style);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::model::{Face, Vec3};

    #[test]
    fn widget_renders_without_panicking() {
        let mesh = Mesh::new(
            "tri",
            vec![
                Vec3::new(-1.0, -1.0, 0.0),
                Vec3::new(1.0, -1.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![Face::new(vec![0, 1, 2])],
            vec![],
        )
        .unwrap();
        let backend = TestBackend::new(30, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = Mesh3dState::default();
        terminal
            .draw(|frame| {
                frame.render_stateful_widget(Mesh3dWidget::new(&mesh), frame.area(), &mut state)
            })
            .unwrap();
    }
}
