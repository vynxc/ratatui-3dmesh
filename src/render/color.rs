use ratatui::style::{Color, Style};

use crate::{
    config::{ColorMode, Mesh3dConfig},
    model::{Material, Texture},
};

/// Resolve the foreground RGB of a solid cell that has no per-cell texture sample.
///
/// Mirrors [`style_for`] but returns raw RGB so the solid path can composite emissive
/// color and alpha on top before handing a final color to the rasterizer.
pub(super) fn solid_base_rgb(
    material: Option<&Material>,
    intensity: f32,
    config: &Mesh3dConfig,
) -> [u8; 3] {
    match config.color_mode {
        ColorMode::Off | ColorMode::Texture | ColorMode::Auto => material
            .filter(|_| !matches!(config.color_mode, ColorMode::Off))
            .map_or_else(
                || foreground_rgb(config),
                |material| brighten_rgb(material_rgb(material), config.color_brightness),
            ),
        ColorMode::Material => material.map_or_else(
            || foreground_rgb(config),
            |material| brighten_rgb(material_rgb(material), config.color_brightness),
        ),
        ColorMode::Lighting => {
            let value = brighten_channel(
                unit_to_channel(intensity.clamp(0.0, 1.0)),
                config.color_brightness,
            );
            [value, value, value]
        }
    }
}

/// Emissive RGB contribution for a cell, combining the material factor with an optional
/// emissive texture sample. Returns `[0, 0, 0]` when the material does not emit.
pub(super) fn emissive_rgb(
    material: Option<&Material>,
    sample: Option<[u8; 4]>,
    config: &Mesh3dConfig,
) -> [u8; 3] {
    let Some(material) = material else {
        return [0, 0, 0];
    };
    if !material.is_emissive() {
        return [0, 0, 0];
    }
    let factor = material.emissive;
    let texel = sample.map_or([1.0, 1.0, 1.0], |rgba| {
        [
            f32::from(rgba[0]) / 255.0,
            f32::from(rgba[1]) / 255.0,
            f32::from(rgba[2]) / 255.0,
        ]
    });
    brighten_rgb(
        [
            (factor[0] * texel[0] * 255.0).round().clamp(0.0, 255.0) as u8,
            (factor[1] * texel[1] * 255.0).round().clamp(0.0, 255.0) as u8,
            (factor[2] * texel[2] * 255.0).round().clamp(0.0, 255.0) as u8,
        ],
        config.color_brightness,
    )
}

/// Add emissive light on top of an already-lit base color, saturating at white.
pub(super) fn add_emissive(base: [u8; 3], emissive: [u8; 3]) -> [u8; 3] {
    [
        base[0].saturating_add(emissive[0]),
        base[1].saturating_add(emissive[1]),
        base[2].saturating_add(emissive[2]),
    ]
}

fn foreground_rgb(config: &Mesh3dConfig) -> [u8; 3] {
    match config.foreground_style.fg {
        Some(Color::Rgb(r, g, b)) => [r, g, b],
        _ => [255, 255, 255],
    }
}

fn material_rgb(material: &Material) -> [u8; 3] {
    [
        (material.diffuse[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (material.diffuse[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (material.diffuse[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

/// Select the foreground style for non-filled-texture drawing paths.
pub(super) fn style_for(
    material: Option<&Material>,
    texture: Option<&Texture>,
    intensity: f32,
    config: &Mesh3dConfig,
) -> Style {
    match config.color_mode {
        ColorMode::Off => config.foreground_style,
        ColorMode::Material => material_style(material, config),
        ColorMode::Texture => texture.map_or_else(
            || config.foreground_style,
            |texture| texture_average_style(texture, config),
        ),
        ColorMode::Auto => texture
            .map(|texture| texture_average_style(texture, config))
            .or_else(|| material.map(|material| material_style(Some(material), config)))
            .unwrap_or(config.foreground_style),
        ColorMode::Lighting => {
            let value = brighten_channel(
                unit_to_channel(intensity.clamp(0.0, 1.0)),
                config.color_brightness,
            );
            config.foreground_style.fg(Color::Rgb(value, value, value))
        }
    }
}

pub(super) fn texture_rgb(rgba: [u8; 4], intensity: f32, config: &Mesh3dConfig) -> [u8; 3] {
    let lighting = if config.texture_lighting {
        intensity
    } else {
        1.0
    };
    brighten_rgb(
        [
            lit_channel(rgba[0], lighting),
            lit_channel(rgba[1], lighting),
            lit_channel(rgba[2], lighting),
        ],
        config.color_brightness,
    )
}

pub(super) fn luminance(rgb: [u8; 3]) -> f32 {
    (0.2126 * f32::from(rgb[0]) + 0.7152 * f32::from(rgb[1]) + 0.0722 * f32::from(rgb[2])) / 255.0
}

fn material_style(material: Option<&Material>, config: &Mesh3dConfig) -> Style {
    material.map_or(config.foreground_style, |material| {
        config
            .foreground_style
            .fg(brighten_color(material.color(), config.color_brightness))
    })
}

fn texture_average_style(texture: &Texture, config: &Mesh3dConfig) -> Style {
    let [red, green, blue] = brighten_rgb(texture.average_color(), config.color_brightness);
    config.foreground_style.fg(Color::Rgb(red, green, blue))
}

fn brighten_color(color: Color, brightness: f32) -> Color {
    match color {
        Color::Rgb(red, green, blue) => Color::Rgb(
            brighten_channel(red, brightness),
            brighten_channel(green, brightness),
            brighten_channel(blue, brightness),
        ),
        color => color,
    }
}

fn brighten_rgb(rgb: [u8; 3], brightness: f32) -> [u8; 3] {
    [
        brighten_channel(rgb[0], brightness),
        brighten_channel(rgb[1], brightness),
        brighten_channel(rgb[2], brightness),
    ]
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "RGB conversion clamps into the valid u8 channel range before casting."
)]
fn brighten_channel(value: u8, brightness: f32) -> u8 {
    (f32::from(value) * brightness.clamp(0.0, 8.0))
        .round()
        .clamp(0.0, 255.0) as u8
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "RGB conversion clamps into the valid u8 channel range before casting."
)]
fn lit_channel(value: u8, lighting: f32) -> u8 {
    (f32::from(value) * lighting).round().clamp(0.0, 255.0) as u8
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "The input is clamped to 0..=1 before converting to an RGB channel."
)]
fn unit_to_channel(value: f32) -> u8 {
    (value * 255.0).round() as u8
}
