use ratatui::{buffer::Buffer, layout::Rect, style::Style};

use crate::{
    config::{ColorMode, Mesh3dConfig, RenderMode},
    model::{Mesh, Vec3},
    widget::Mesh3dState,
};

use super::{
    camera::{project, ProjectedVertex},
    raster::{draw_line, fill_triangle, plot},
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
        for tri in triangulate(&face.indices) {
            let [a_i, b_i, c_i] = tri;
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
            let ch = config.glyph_for_intensity(intensity);
            let style = style_for(
                mesh.material(face.material.as_deref().unwrap_or_default()),
                intensity,
                config,
            );
            match config.render_mode {
                RenderMode::Solid => fill_triangle(area, buf, &mut zbuf, [a, b, c], ch, style),
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

fn triangulate(indices: &[usize]) -> impl Iterator<Item = [usize; 3]> + '_ {
    (1..indices.len().saturating_sub(1)).map(|i| [indices[0], indices[i], indices[i + 1]])
}

fn style_for(
    material: Option<&crate::model::Material>,
    intensity: f32,
    config: &Mesh3dConfig,
) -> Style {
    match config.color_mode {
        ColorMode::Off => config.foreground_style,
        ColorMode::Material => material.map_or(config.foreground_style, |m| {
            config.foreground_style.fg(m.color())
        }),
        ColorMode::Lighting => {
            let v = (intensity.clamp(0.0, 1.0) * 255.0).round() as u8;
            config
                .foreground_style
                .fg(ratatui::style::Color::Rgb(v, v, v))
        }
    }
}

#[allow(dead_code)]
fn _assert_projected_copy(_: ProjectedVertex) {}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::{
        model::{Face, Mesh, Vec3},
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
        assert!(content.chars().any(|c| c != " ".chars().next().unwrap()));
    }
}
