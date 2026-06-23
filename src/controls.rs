use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    config::{ColorMode, Mesh3dConfig, RenderMode},
    model::Vec3,
    widget::Mesh3dState,
};

/// Semantic action produced by a control mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    /// View rotation changed.
    Rotate,
    /// View pan changed.
    Pan,
    /// View zoom changed.
    Zoom,
    /// View was reset.
    Reset,
    /// Render mode was changed.
    ToggleRenderMode,
    /// Color mode was changed.
    ToggleColorMode,
    /// Auto-spin was toggled.
    ToggleAutoSpin,
    /// Help overlay was toggled.
    ToggleHelp,
    /// Caller should quit the viewer.
    Quit,
}

/// Configurable keyboard control helper for Ratatui/crossterm apps.
#[derive(Debug, Clone)]
pub struct ControlMap {
    /// Rotation step in radians.
    pub rotate_step: f32,
    /// Pan step in normalized model space.
    pub pan_step: f32,
    /// Zoom-in multiplier.
    pub zoom_in_factor: f32,
    /// Zoom-out multiplier.
    pub zoom_out_factor: f32,
}

impl Default for ControlMap {
    fn default() -> Self {
        Self {
            rotate_step: 0.12,
            pan_step: 0.08,
            zoom_in_factor: 1.12,
            zoom_out_factor: 1.0 / 1.12,
        }
    }
}

impl ControlMap {
    /// Apply a crossterm key event to widget state/config.
    pub fn handle_key(
        &self,
        key: KeyEvent,
        state: &mut Mesh3dState,
        config: &mut Mesh3dConfig,
    ) -> Option<ControlAction> {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let fast = if shift { 2.0 } else { 1.0 };
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(ControlAction::Quit),
            KeyCode::Left | KeyCode::Char('a') => {
                state.rotate(Vec3::new(0.0, -self.rotate_step * fast, 0.0));
                Some(ControlAction::Rotate)
            }
            KeyCode::Right | KeyCode::Char('d') => {
                state.rotate(Vec3::new(0.0, self.rotate_step * fast, 0.0));
                Some(ControlAction::Rotate)
            }
            KeyCode::Up | KeyCode::Char('w') => {
                state.rotate(Vec3::new(-self.rotate_step * fast, 0.0, 0.0));
                Some(ControlAction::Rotate)
            }
            KeyCode::Down | KeyCode::Char('s') => {
                state.rotate(Vec3::new(self.rotate_step * fast, 0.0, 0.0));
                Some(ControlAction::Rotate)
            }
            KeyCode::Char('z') => {
                state.rotate(Vec3::new(0.0, 0.0, -self.rotate_step * fast));
                Some(ControlAction::Rotate)
            }
            KeyCode::Char('x') => {
                state.rotate(Vec3::new(0.0, 0.0, self.rotate_step * fast));
                Some(ControlAction::Rotate)
            }
            KeyCode::Char('h') => {
                state.pan(Vec3::new(-self.pan_step * fast, 0.0, 0.0));
                Some(ControlAction::Pan)
            }
            KeyCode::Char('l') => {
                state.pan(Vec3::new(self.pan_step * fast, 0.0, 0.0));
                Some(ControlAction::Pan)
            }
            KeyCode::Char('j') => {
                state.pan(Vec3::new(0.0, -self.pan_step * fast, 0.0));
                Some(ControlAction::Pan)
            }
            KeyCode::Char('k') => {
                state.pan(Vec3::new(0.0, self.pan_step * fast, 0.0));
                Some(ControlAction::Pan)
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                state.zoom_by(self.zoom_in_factor);
                Some(ControlAction::Zoom)
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                state.zoom_by(self.zoom_out_factor);
                Some(ControlAction::Zoom)
            }
            KeyCode::Char('r') => {
                state.reset_view();
                Some(ControlAction::Reset)
            }
            KeyCode::Char(' ') => {
                state.toggle_auto_spin();
                Some(ControlAction::ToggleAutoSpin)
            }
            KeyCode::Char('?') => {
                state.toggle_help();
                Some(ControlAction::ToggleHelp)
            }
            KeyCode::Char('m') => {
                config.render_mode = match config.render_mode {
                    RenderMode::Solid => RenderMode::Wireframe,
                    RenderMode::Wireframe => RenderMode::Points,
                    RenderMode::Points => RenderMode::Solid,
                };
                Some(ControlAction::ToggleRenderMode)
            }
            KeyCode::Char('c') => {
                config.color_mode = match config.color_mode {
                    ColorMode::Off => ColorMode::Material,
                    ColorMode::Material => ColorMode::Lighting,
                    ColorMode::Lighting => ColorMode::Off,
                };
                Some(ControlAction::ToggleColorMode)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_mutate_state() {
        let map = ControlMap::default();
        let mut state = Mesh3dState::default();
        let mut config = Mesh3dConfig::default();
        let before = state.zoom;
        let action = map.handle_key(KeyEvent::from(KeyCode::Char('+')), &mut state, &mut config);
        assert_eq!(action, Some(ControlAction::Zoom));
        assert!(state.zoom > before);
    }
}
