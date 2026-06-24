use ratatui::style::{Color, Style};

/// High-level rasterization mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RenderMode {
    /// Filled triangles with depth buffering and lighting.
    #[default]
    Solid,
    /// Edges only.
    Wireframe,
    /// Vertices only.
    Points,
}

/// Projection model used by the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ProjectionMode {
    /// Perspective projection with configurable field of view.
    #[default]
    Perspective,
    /// Orthographic projection.
    Orthographic,
}

/// How the widget chooses terminal colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ColorMode {
    /// Ignore model colors and render with the configured foreground style.
    Off,
    /// Use material diffuse colors when present, otherwise configured foreground.
    #[default]
    Material,
    /// Map lighting intensity to grayscale terminal colors.
    Lighting,
    /// Prefer sampled texture colors, falling back to material/foreground when unavailable.
    Texture,
    /// Prefer texture, then material, then lighting fallback.
    Auto,
}

/// Texture filtering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextureFilter {
    /// Fast nearest-neighbor sampling.
    #[default]
    Nearest,
    /// Smooth bilinear sampling.
    Bilinear,
}

/// Behavior for UVs outside the `[0, 1]` range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextureWrap {
    /// Repeat the image.
    #[default]
    Repeat,
    /// Clamp to the image edge.
    Clamp,
}

/// User-facing rendering configuration.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mesh3dConfig {
    /// Glyphs ordered from darkest to brightest.
    pub glyph_ramp: String,
    /// Rasterization mode.
    pub render_mode: RenderMode,
    /// Projection model.
    pub projection: ProjectionMode,
    /// Terminal color strategy.
    pub color_mode: ColorMode,
    /// Texture filtering mode.
    pub texture_filter: TextureFilter,
    /// Texture wrap policy.
    pub texture_wrap: TextureWrap,
    /// Flip V texture coordinates while sampling. Useful because OBJ and image origins often differ.
    pub flip_texture_v: bool,
    /// Multiplier applied to truecolor material/texture/lighting RGB output.
    /// Values above `1.0` are useful in terminals where colored glyphs look dim.
    pub color_brightness: f32,

    /// Multiply sampled texture colors by terminal lighting intensity.
    pub texture_lighting: bool,
    /// Whether to fit the model to the visible area automatically.
    pub auto_fit: bool,
    /// Additional scale multiplier.
    pub scale: f32,
    /// Perspective vertical field of view in degrees.
    pub fov_y_degrees: f32,
    /// Terminal cell width/height correction. Most terminals are taller than wide.
    pub cell_aspect_ratio: f32,
    /// Enable back-face culling.
    pub backface_culling: bool,
    /// Normalize model center and radius before rendering.
    pub normalize: bool,
    /// Direction from surface toward the light.
    pub light_direction: [f32; 3],
    /// Ambient light contribution in `[0, 1]`.
    pub ambient: f32,
    /// Diffuse light contribution in `[0, 1]`.
    pub diffuse: f32,
    /// Draw a small controls/status hint when there is room.
    pub show_hints: bool,
    /// Draw a help overlay from widget state.
    pub show_help_overlay: bool,
    /// Auto-spin velocity in radians per second for x/y/z.
    pub auto_spin: [f32; 3],
    /// Maximum number of faces to render. `None` renders every face.
    pub max_faces: Option<usize>,
    /// Style used when no material/color is selected.
    pub foreground_style: Style,
    /// Optional background style applied before rendering.
    pub background_style: Option<Style>,
}

impl Default for Mesh3dConfig {
    fn default() -> Self {
        Self {
            glyph_ramp: " .:-=+*#%@".to_string(),
            render_mode: RenderMode::Solid,
            projection: ProjectionMode::Perspective,
            color_mode: ColorMode::Material,
            texture_filter: TextureFilter::Nearest,
            texture_wrap: TextureWrap::Repeat,
            flip_texture_v: true,
            texture_lighting: true,

            color_brightness: 1.0,
            auto_fit: true,
            scale: 1.0,
            fov_y_degrees: 60.0,
            cell_aspect_ratio: 0.5,
            backface_culling: true,
            normalize: true,
            light_direction: [0.25, 0.5, 1.0],
            ambient: 0.22,
            diffuse: 0.78,
            show_hints: true,
            show_help_overlay: false,
            auto_spin: [0.0, 0.0, 0.0],
            max_faces: None,
            foreground_style: Style::default().fg(Color::White),
            background_style: None,
        }
    }
}

impl Mesh3dConfig {
    /// A fast preset suitable for large meshes.
    #[must_use]
    pub fn fast() -> Self {
        Self {
            render_mode: RenderMode::Wireframe,
            max_faces: Some(25_000),
            backface_culling: true,
            texture_filter: TextureFilter::Nearest,
            ..Self::default()
        }
    }

    /// A high quality preset for smaller meshes.
    #[must_use]
    pub fn quality() -> Self {
        Self {
            glyph_ramp: " .'`^\",:;Il!i><~+_-?][}{1)(|\\/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$"
                .to_string(),
            render_mode: RenderMode::Solid,
            texture_filter: TextureFilter::Bilinear,
            max_faces: None,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn glyph_ramp(mut self, ramp: impl Into<String>) -> Self {
        self.glyph_ramp = ramp.into();
        self
    }

    #[must_use]
    pub fn render_mode(mut self, mode: RenderMode) -> Self {
        self.render_mode = mode;
        self
    }

    #[must_use]
    pub fn projection(mut self, projection: ProjectionMode) -> Self {
        self.projection = projection;
        self
    }

    #[must_use]
    pub fn color_mode(mut self, mode: ColorMode) -> Self {
        self.color_mode = mode;
        self
    }

    #[must_use]
    pub fn texture_filter(mut self, filter: TextureFilter) -> Self {
        self.texture_filter = filter;
        self
    }

    #[must_use]
    pub fn texture_wrap(mut self, wrap: TextureWrap) -> Self {
        self.texture_wrap = wrap;
        self
    }

    #[must_use]
    pub fn flip_texture_v(mut self, enabled: bool) -> Self {
        self.flip_texture_v = enabled;
        self
    }

    #[must_use]
    pub fn texture_lighting(mut self, enabled: bool) -> Self {
        self.texture_lighting = enabled;
        self
    }
    #[must_use]
    pub fn color_brightness(mut self, brightness: f32) -> Self {
        self.color_brightness = brightness.clamp(0.0, 8.0);
        self
    }

    #[must_use]
    pub fn auto_fit(mut self, enabled: bool) -> Self {
        self.auto_fit = enabled;
        self
    }

    #[must_use]
    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale.max(0.0001);
        self
    }

    #[must_use]
    pub fn fov_y_degrees(mut self, fov: f32) -> Self {
        self.fov_y_degrees = fov.clamp(5.0, 150.0);
        self
    }

    #[must_use]
    pub fn cell_aspect_ratio(mut self, ratio: f32) -> Self {
        self.cell_aspect_ratio = ratio.max(0.0001);
        self
    }

    #[must_use]
    pub fn backface_culling(mut self, enabled: bool) -> Self {
        self.backface_culling = enabled;
        self
    }

    #[must_use]
    pub fn normalize(mut self, enabled: bool) -> Self {
        self.normalize = enabled;
        self
    }

    #[must_use]
    pub fn light_direction(mut self, direction: [f32; 3]) -> Self {
        self.light_direction = direction;
        self
    }

    #[must_use]
    pub fn lighting(mut self, ambient: f32, diffuse: f32) -> Self {
        self.ambient = ambient.clamp(0.0, 1.0);
        self.diffuse = diffuse.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub fn show_hints(mut self, enabled: bool) -> Self {
        self.show_hints = enabled;
        self
    }

    #[must_use]
    pub fn show_help_overlay(mut self, enabled: bool) -> Self {
        self.show_help_overlay = enabled;
        self
    }

    #[must_use]
    pub fn auto_spin(mut self, xyz_radians_per_second: [f32; 3]) -> Self {
        self.auto_spin = xyz_radians_per_second;
        self
    }

    #[must_use]
    pub fn max_faces(mut self, max: Option<usize>) -> Self {
        self.max_faces = max;
        self
    }

    #[must_use]
    pub fn foreground_style(mut self, style: Style) -> Self {
        self.foreground_style = style;
        self
    }

    #[must_use]
    pub fn background_style(mut self, style: Option<Style>) -> Self {
        self.background_style = style;
        self
    }

    pub(crate) fn glyph_for_intensity(&self, intensity: f32) -> char {
        let glyphs: Vec<char> = self.glyph_ramp.chars().collect();
        if glyphs.is_empty() {
            return '#';
        }
        let idx =
            (intensity.clamp(0.0, 1.0) * (glyphs.len().saturating_sub(1)) as f32).round() as usize;
        glyphs[idx.min(glyphs.len() - 1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_clamp_values() {
        let cfg = Mesh3dConfig::default()
            .scale(-1.0)
            .fov_y_degrees(500.0)
            .cell_aspect_ratio(0.0)
            .lighting(-1.0, 2.0);
        assert!(cfg.scale > 0.0);
        assert_eq!(cfg.fov_y_degrees, 150.0);
        assert!(cfg.cell_aspect_ratio > 0.0);
        assert_eq!(cfg.ambient, 0.0);
        assert_eq!(cfg.diffuse, 1.0);
    }
}
