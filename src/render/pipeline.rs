use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use crate::{
    config::{ColorMode, Mesh3dConfig, RenderMode, TextureFilter},
    model::{Material, Mesh, Texture, Vec2, Vec3},
    widget::Mesh3dState,
};

use super::{
    camera::{project, ProjectedVertex},
    color::{luminance, style_for, texture_rgb},
    raster::{draw_line, fill_triangle, fill_triangle_with, plot},
};

/// Render a mesh into a Ratatui buffer.
pub fn render_mesh(
    mesh: &Mesh,
    area: Rect,
    buf: &mut Buffer,
    state: &Mesh3dState,
    config: &Mesh3dConfig,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if let Some(style) = config.background_style {
        for y in area.y..area.y.saturating_add(area.height) {
            for x in area.x..area.x.saturating_add(area.width) {
                buf[(x, y)].set_style(style);
            }
        }
    }

    let mesh = if config.normalize {
        mesh.normalized()
    } else {
        mesh.clone()
    };
    let rotation = state.rotation;
    let pan = state.pan;
    let zoom = state.zoom * config.scale;
    let light = Vec3::new(
        config.light_direction[0],
        config.light_direction[1],
        config.light_direction[2],
    )
    .normalized();

    let projected = mesh
        .vertices
        .iter()
        .map(|&v| {
            let transformed = v.rotate_euler(rotation) + Vec3::new(pan.x, pan.y, 0.0);
            project(
                transformed,
                area.width,
                area.height,
                config.projection,
                config.fov_y_degrees,
                config.cell_aspect_ratio,
                zoom,
            )
        })
        .collect::<Vec<_>>();

    let mut zbuf = vec![f32::INFINITY; usize::from(area.width) * usize::from(area.height)];
    for face in mesh
        .faces
        .iter()
        .take(config.max_faces.unwrap_or(usize::MAX))
    {
        if face.indices.len() < 3 {
            continue;
        }
        for corners in triangulate_corners(face.indices.len()) {
            let [ca, cb, cc] = corners;
            let [a_i, b_i, c_i] = [face.indices[ca], face.indices[cb], face.indices[cc]];
            let Some(a) = projected.get(a_i).copied() else {
                continue;
            };
            let Some(b) = projected.get(b_i).copied() else {
                continue;
            };
            let Some(c) = projected.get(c_i).copied() else {
                continue;
            };
            let normal = face
                .normal
                .map(|n| n.rotate_euler(rotation).normalized())
                .unwrap_or_else(|| (b.view - a.view).cross(c.view - a.view).normalized());
            if config.backface_culling && normal.z <= 0.0 {
                continue;
            }
            let intensity =
                (config.ambient + config.diffuse * normal.dot(light).abs()).clamp(0.0, 1.0);
            let material = mesh.material(face.material.as_deref().unwrap_or_default());
            let textured = if texture_enabled(config.color_mode) {
                textured_triangle(&mesh, material, face, corners)
            } else {
                None
            };
            let ch = config.glyph_for_intensity(intensity);
            let style = style_for(
                material,
                textured.as_ref().map(|(texture, _)| *texture),
                intensity,
                config,
            );
            match config.render_mode {
                RenderMode::Solid => {
                    if let Some((texture, uvs)) = textured {
                        let textured = TexturedTriangle {
                            tri: [a, b, c],
                            uvs,
                            texture,
                            intensity,
                        };
                        fill_textured_triangle(area, buf, &mut zbuf, textured, config);
                    } else {
                        fill_triangle(area, buf, &mut zbuf, [a, b, c], ch, style);
                    }
                }
                RenderMode::Wireframe => {
                    draw_line(area, buf, &mut zbuf, a, b, ch, style);
                    draw_line(area, buf, &mut zbuf, b, c, ch, style);
                    draw_line(area, buf, &mut zbuf, c, a, ch, style);
                }
                RenderMode::Points => {
                    plot(area, buf, &mut zbuf, a, ch, style);
                    plot(area, buf, &mut zbuf, b, ch, style);
                    plot(area, buf, &mut zbuf, c, ch, style);
                }
            }
        }
    }
}

fn triangulate_corners(len: usize) -> impl Iterator<Item = [usize; 3]> {
    (1..len.saturating_sub(1)).map(|i| [0, i, i + 1])
}

fn texture_enabled(color_mode: ColorMode) -> bool {
    matches!(color_mode, ColorMode::Texture | ColorMode::Auto)
}

fn textured_triangle<'a>(
    mesh: &'a Mesh,
    material: Option<&'a Material>,
    face: &crate::model::Face,
    corners: [usize; 3],
) -> Option<(&'a Texture, [Vec2; 3])> {
    let texture_index = material
        .and_then(|m| m.diffuse_texture.as_ref())
        .and_then(|r| r.index)
        .or_else(|| mesh.default_texture.as_ref().and_then(|r| r.index))?;
    let texture = mesh.textures.get(texture_index)?;
    let uv = corners.map(|corner| {
        face.tex_coord_indices
            .get(corner)
            .and_then(|idx| idx.and_then(|idx| mesh.tex_coords.get(idx).copied()))
    });
    Some((texture, [uv[0]?, uv[1]?, uv[2]?]))
}

struct TexturedTriangle<'a> {
    tri: [ProjectedVertex; 3],
    uvs: [Vec2; 3],
    texture: &'a Texture,
    intensity: f32,
}

fn fill_textured_triangle(
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    textured: TexturedTriangle<'_>,
    config: &Mesh3dConfig,
) {
    fill_triangle_with(area, buf, zbuf, textured.tri, |weights, _depth| {
        let uv = Vec2::new(
            weights[0].mul_add(
                textured.uvs[0].u,
                weights[1].mul_add(textured.uvs[1].u, weights[2] * textured.uvs[2].u),
            ),
            weights[0].mul_add(
                textured.uvs[0].v,
                weights[1].mul_add(textured.uvs[1].v, weights[2] * textured.uvs[2].v),
            ),
        );
        let rgba = match config.texture_filter {
            TextureFilter::Nearest => {
                textured
                    .texture
                    .sample_nearest(uv, config.texture_wrap, config.flip_texture_v)
            }
            TextureFilter::Bilinear => sample_bilinear(textured.texture, uv, config),
        };
        let rgb = texture_rgb(rgba, textured.intensity, config);
        let luminance = luminance(rgb);
        let ch = config.glyph_for_intensity(if config.texture_lighting {
            luminance.max(textured.intensity * 0.35)
        } else {
            luminance
        });
        (
            ch,
            config
                .foreground_style
                .fg(Color::Rgb(rgb[0], rgb[1], rgb[2])),
        )
    });
}

fn sample_bilinear(texture: &Texture, uv: Vec2, config: &Mesh3dConfig) -> [u8; 4] {
    // Terminal cells are coarse; a compact 4-tap sampler is enough and keeps Texture simple.
    let w = texture.width.max(1) as f32;
    let h = texture.height.max(1) as f32;
    let u = match config.texture_wrap {
        crate::config::TextureWrap::Repeat => uv.u.rem_euclid(1.0),
        crate::config::TextureWrap::Clamp => uv.u.clamp(0.0, 1.0),
    };
    let mut v = match config.texture_wrap {
        crate::config::TextureWrap::Repeat => uv.v.rem_euclid(1.0),
        crate::config::TextureWrap::Clamp => uv.v.clamp(0.0, 1.0),
    };
    if config.flip_texture_v {
        v = 1.0 - v;
    }
    let x = u * (w - 1.0);
    let y = v * (h - 1.0);
    let x0 = x.floor() / (w - 1.0).max(1.0);
    let x1 = x.ceil() / (w - 1.0).max(1.0);
    let y0 = y.floor() / (h - 1.0).max(1.0);
    let y1 = y.ceil() / (h - 1.0).max(1.0);
    let tx = x.fract();
    let ty = y.fract();
    let p00 = texture.sample_nearest(Vec2::new(x0, y0), config.texture_wrap, false);
    let p10 = texture.sample_nearest(Vec2::new(x1, y0), config.texture_wrap, false);
    let p01 = texture.sample_nearest(Vec2::new(x0, y1), config.texture_wrap, false);
    let p11 = texture.sample_nearest(Vec2::new(x1, y1), config.texture_wrap, false);
    let mut out = [0; 4];
    for i in 0..4 {
        let a = f32::from(p00[i]) * (1.0 - tx) + f32::from(p10[i]) * tx;
        let b = f32::from(p01[i]) * (1.0 - tx) + f32::from(p11[i]) * tx;
        out[i] = (a * (1.0 - ty) + b * ty).round().clamp(0.0, 255.0) as u8;
    }
    out
}

#[allow(dead_code)]
fn _assert_projected_copy(_: ProjectedVertex) {}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::{
        model::{Face, Mesh, Texture, TextureRef, Vec2, Vec3},
        widget::Mesh3dState,
    };

    #[test]
    fn renders_triangle_into_buffer() {
        let mesh = Mesh::new(
            "tri",
            vec![
                Vec3::new(-0.8, -0.8, 0.0),
                Vec3::new(0.8, -0.8, 0.0),
                Vec3::new(0.0, 0.8, 0.0),
            ],
            vec![Face::new(vec![0, 1, 2])],
            vec![],
        )
        .unwrap();
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_mesh(
                    &mesh,
                    area,
                    frame.buffer_mut(),
                    &Mesh3dState::default(),
                    &Mesh3dConfig::default().backface_culling(false),
                );
            })
            .unwrap();
        let content = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(content.chars().any(|c| c != ' '));
    }

    #[test]
    fn renders_textured_triangle_color() {
        let mut mesh = Mesh::with_attributes(
            "tri",
            vec![
                Vec3::new(-0.8, -0.8, 0.0),
                Vec3::new(0.8, -0.8, 0.0),
                Vec3::new(0.0, 0.8, 0.0),
            ],
            vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0),
            ],
            vec![],
            vec![Face::with_attributes(
                vec![0, 1, 2],
                vec![Some(0), Some(1), Some(2)],
                vec![None, None, None],
            )],
            vec![],
        )
        .unwrap();
        mesh.default_texture = Some(TextureRef {
            path: "inline.png".into(),
            index: Some(0),
        });
        mesh.textures
            .push(Texture::new("inline.png", 1, 1, vec![255, 0, 0, 255]));
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_mesh(
                    &mesh,
                    frame.area(),
                    frame.buffer_mut(),
                    &Mesh3dState::default(),
                    &Mesh3dConfig::default()
                        .backface_culling(false)
                        .color_mode(ColorMode::Texture)
                        .texture_lighting(false),
                );
            })
            .unwrap();
        assert!(terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.fg == Color::Rgb(255, 0, 0)));
    }
}
