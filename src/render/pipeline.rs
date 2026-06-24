use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use crate::{
    animation::sample_mesh_animation,
    config::{ColorMode, Mesh3dConfig, RenderMode, TextureFilter},
    model::{AlphaMode, Face, Material, Mesh, Texture, Vec2, Vec3},
    widget::Mesh3dState,
};

use super::{
    camera::{project, ProjectedVertex},
    color::{add_emissive, emissive_rgb, luminance, solid_base_rgb, style_for, texture_rgb},
    raster::{draw_line, fill_triangle_shaded, plot, Fragment},
};

/// Depth bias (in post-normalize world units) applied to translucent BLEND fragments so
/// decals coincident with the opaque surface behind them win the depth test.
const DECAL_DEPTH_BIAS: f32 = 0.01;

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

    let mesh = state
        .selected_animation
        .and_then(|clip| {
            sample_mesh_animation(
                mesh,
                clip,
                state.animation_time_seconds,
                state.animation_looping,
            )
        })
        .unwrap_or_else(|| mesh.clone());
    let mesh = if config.normalize {
        mesh.normalized()
    } else {
        mesh
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
    let backdrop = backdrop_rgb(config);

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

    // Two passes so authored transparency (glTF BLEND) layers correctly over opaque
    // geometry: opaque/mask faces write depth first, then blend faces composite on top,
    // sorted back-to-front. Wireframe/points ignore the split and draw in source order.
    let ctx = DrawContext {
        mesh: &mesh,
        projected: &projected,
        rotation,
        light,
        backdrop,
    };
    if matches!(config.render_mode, RenderMode::Solid) {
        let (opaque, blend) = partition_faces(&mesh, &projected, config);
        for &face_index in &opaque {
            draw_face(&ctx, face_index, area, buf, &mut zbuf, config);
        }
        for &face_index in &blend {
            draw_face(&ctx, face_index, area, buf, &mut zbuf, config);
        }
    } else {
        let limit = config.max_faces.unwrap_or(usize::MAX);
        for face_index in 0..mesh.faces.len().min(limit) {
            draw_face(&ctx, face_index, area, buf, &mut zbuf, config);
        }
    }
}

struct DrawContext<'a> {
    mesh: &'a Mesh,
    projected: &'a [ProjectedVertex],
    rotation: Vec3,
    light: Vec3,
    backdrop: [u8; 3],
}

/// Split solid-mode faces into an opaque set (drawn first, writing depth) and a blend set
/// (drawn after, sorted back-to-front). Respects `max_faces`.
fn partition_faces(
    mesh: &Mesh,
    projected: &[ProjectedVertex],
    config: &Mesh3dConfig,
) -> (Vec<usize>, Vec<usize>) {
    let limit = config.max_faces.unwrap_or(usize::MAX);
    let mut opaque = Vec::new();
    let mut blend = Vec::new();
    for (index, face) in mesh.faces.iter().take(limit).enumerate() {
        let material = mesh.material(face.material.as_deref().unwrap_or_default());
        if matches!(material.map(|m| m.alpha_mode), Some(AlphaMode::Blend)) {
            blend.push(index);
        } else {
            opaque.push(index);
        }
    }
    // Back-to-front: larger view depth is farther from the camera, so draw it first.
    blend.sort_by(|&a, &b| {
        face_depth(mesh, projected, b)
            .partial_cmp(&face_depth(mesh, projected, a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    (opaque, blend)
}

fn face_depth(mesh: &Mesh, projected: &[ProjectedVertex], face_index: usize) -> f32 {
    let face = &mesh.faces[face_index];
    let mut sum = 0.0;
    let mut count = 0.0;
    for &idx in &face.indices {
        if let Some(v) = projected.get(idx) {
            sum += v.depth;
            count += 1.0;
        }
    }
    if count == 0.0 {
        f32::NEG_INFINITY
    } else {
        sum / count
    }
}

fn draw_face(
    ctx: &DrawContext<'_>,
    face_index: usize,
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    config: &Mesh3dConfig,
) {
    let mesh = ctx.mesh;
    let face = &mesh.faces[face_index];
    if face.indices.len() < 3 {
        return;
    }
    let material = mesh.material(face.material.as_deref().unwrap_or_default());
    let double_sided = material.is_some_and(|m| m.double_sided);
    // Translucent decals (glTF BLEND, e.g. eye irises) sit exactly on the opaque surface
    // behind them. A small depth bias lets them win the depth test instead of z-fighting.
    let decal_bias = if matches!(material.map(|m| m.alpha_mode), Some(AlphaMode::Blend)) {
        DECAL_DEPTH_BIAS
    } else {
        0.0
    };
    for corners in triangulate_corners(face.indices.len()) {
        let [ca, cb, cc] = corners;
        let [a_i, b_i, c_i] = [face.indices[ca], face.indices[cb], face.indices[cc]];
        let Some(a) = ctx.projected.get(a_i).copied() else {
            continue;
        };
        let Some(b) = ctx.projected.get(b_i).copied() else {
            continue;
        };
        let Some(c) = ctx.projected.get(c_i).copied() else {
            continue;
        };
        let normal = face
            .normal
            .map(|n| n.rotate_euler(ctx.rotation).normalized())
            .unwrap_or_else(|| (b.view - a.view).cross(c.view - a.view).normalized());
        // Double-sided materials (hair cards, eye/brow decals) must never be culled.
        if config.backface_culling && !double_sided && normal.z <= 0.0 {
            continue;
        }
        let intensity =
            (config.ambient + config.diffuse * normal.dot(ctx.light).abs()).clamp(0.0, 1.0);
        let ch = config.glyph_for_intensity(intensity);

        match config.render_mode {
            RenderMode::Solid => {
                let shading = FaceShading {
                    mesh,
                    material,
                    face,
                    corners,
                    intensity,
                    fallback_glyph: ch,
                };
                fill_triangle_shaded(
                    area,
                    buf,
                    zbuf,
                    [a, b, c],
                    ctx.backdrop,
                    decal_bias,
                    |weights, _| shade_cell(&shading, weights, config),
                );
            }
            RenderMode::Wireframe => {
                let style = style_for(material, None, intensity, config);
                draw_line(area, buf, zbuf, a, b, ch, style);
                draw_line(area, buf, zbuf, b, c, ch, style);
                draw_line(area, buf, zbuf, c, a, ch, style);
            }
            RenderMode::Points => {
                let style = style_for(material, None, intensity, config);
                plot(area, buf, zbuf, a, ch, style);
                plot(area, buf, zbuf, b, ch, style);
                plot(area, buf, zbuf, c, ch, style);
            }
        }
    }
}

struct FaceShading<'a> {
    mesh: &'a Mesh,
    material: Option<&'a Material>,
    face: &'a Face,
    corners: [usize; 3],
    intensity: f32,
    fallback_glyph: char,
}

/// Shade a single covered cell: sample the diffuse texture (if any), apply the material
/// alpha mode, light the color, and add emissive contribution. Returns `None` to discard.
fn shade_cell(
    shading: &FaceShading<'_>,
    weights: [f32; 3],
    config: &Mesh3dConfig,
) -> Option<Fragment> {
    let mesh = shading.mesh;
    let material = shading.material;
    let uv = interpolate_uv(shading, weights);
    let flip_v = mesh.flip_texture_v && config.flip_texture_v;

    let diffuse_sample = if texture_enabled(config.color_mode) {
        uv.and_then(|uv| {
            diffuse_texture(mesh, material)
                .map(|texture| sample_texture(texture, uv, flip_v, config))
        })
    } else {
        None
    };

    // A fully transparent texel carries no usable color in a terminal cell, so skip it
    // regardless of the material's alpha mode. This keeps texture cut-outs (sprite-style
    // holes) from painting stray glyphs.
    if let Some(rgba) = diffuse_sample {
        if rgba[3] < 16 {
            return None;
        }
    }

    // Coverage from material alpha factor and (for non-opaque modes) texture alpha.
    let alpha_mode = material.map_or(AlphaMode::Opaque, |m| m.alpha_mode);
    let base_alpha = material.map_or(1.0, |m| m.base_color_alpha);
    let texel_alpha = diffuse_sample.map_or(1.0, |rgba| f32::from(rgba[3]) / 255.0);
    let alpha = match alpha_mode {
        AlphaMode::Opaque => 1.0,
        AlphaMode::Mask => {
            let cutoff = material.map_or(0.5, |m| m.alpha_cutoff);
            if base_alpha * texel_alpha < cutoff {
                return None;
            }
            1.0
        }
        AlphaMode::Blend => base_alpha * texel_alpha,
    };
    if alpha <= 0.003 {
        return None;
    }

    // Lit base color. Unlit materials (KHR_materials_unlit) ignore scene
    // lighting and show their flat base color, so drive shading at full
    // intensity. Lighting-only mode discards material color by design, so it is
    // left untouched.
    let unlit = material.is_some_and(|m| m.unlit) && !matches!(config.color_mode, ColorMode::Lighting);
    let shade_intensity = if unlit { 1.0 } else { shading.intensity };
    let lit = diffuse_sample.map_or_else(
        || lit_solid_rgb(material, shade_intensity, config),
        |rgba| texture_rgb(rgba, shade_intensity, config),
    );

    // Emissive contribution keeps authored glowing detail (eye irises) visible even when
    // lighting is dim.
    let emissive_sample = if texture_enabled(config.color_mode) {
        uv.and_then(|uv| {
            emissive_texture(mesh, material)
                .map(|texture| sample_texture(texture, uv, flip_v, config))
        })
    } else {
        None
    };
    let emissive = emissive_rgb(material, emissive_sample, config);
    let rgb = add_emissive(lit, emissive);

    let glyph = glyph_for_cell(shading, rgb, config);
    Some(Fragment {
        ch: glyph,
        rgb,
        alpha,
    })
}

fn glyph_for_cell(shading: &FaceShading<'_>, rgb: [u8; 3], config: &Mesh3dConfig) -> char {
    match config.color_mode {
        ColorMode::Off => shading.fallback_glyph,
        _ => {
            let lum = luminance(rgb);
            let value = if config.texture_lighting {
                lum.max(shading.intensity * 0.35)
            } else {
                lum.max(shading.intensity)
            };
            config.glyph_for_intensity(value)
        }
    }
}

fn lit_solid_rgb(material: Option<&Material>, intensity: f32, config: &Mesh3dConfig) -> [u8; 3] {
    let base = solid_base_rgb(material, intensity, config);
    if matches!(config.color_mode, ColorMode::Lighting) {
        return base;
    }
    [
        (f32::from(base[0]) * intensity).round() as u8,
        (f32::from(base[1]) * intensity).round() as u8,
        (f32::from(base[2]) * intensity).round() as u8,
    ]
}

fn interpolate_uv(shading: &FaceShading<'_>, weights: [f32; 3]) -> Option<Vec2> {
    let uvs = shading.corners.map(|corner| {
        shading
            .face
            .tex_coord_indices
            .get(corner)
            .and_then(|idx| idx.and_then(|idx| shading.mesh.tex_coords.get(idx).copied()))
    });
    let [u0, u1, u2] = [uvs[0]?, uvs[1]?, uvs[2]?];
    Some(Vec2::new(
        weights[0].mul_add(u0.u, weights[1].mul_add(u1.u, weights[2] * u2.u)),
        weights[0].mul_add(u0.v, weights[1].mul_add(u1.v, weights[2] * u2.v)),
    ))
}

fn diffuse_texture<'a>(mesh: &'a Mesh, material: Option<&'a Material>) -> Option<&'a Texture> {
    let index = material
        .and_then(|m| m.diffuse_texture.as_ref())
        .and_then(|r| r.index)
        .or_else(|| mesh.default_texture.as_ref().and_then(|r| r.index))?;
    mesh.textures.get(index)
}

fn emissive_texture<'a>(mesh: &'a Mesh, material: Option<&'a Material>) -> Option<&'a Texture> {
    let index = material
        .and_then(|m| m.emissive_texture.as_ref())
        .and_then(|r| r.index)?;
    mesh.textures.get(index)
}

fn sample_texture(texture: &Texture, uv: Vec2, flip_v: bool, config: &Mesh3dConfig) -> [u8; 4] {
    match config.texture_filter {
        TextureFilter::Nearest => texture.sample_nearest(uv, config.texture_wrap, flip_v),
        TextureFilter::Bilinear => sample_bilinear(texture, uv, flip_v, config),
    }
}

fn backdrop_rgb(config: &Mesh3dConfig) -> [u8; 3] {
    config
        .background_style
        .and_then(|style| match style.bg {
            Some(Color::Rgb(r, g, b)) => Some([r, g, b]),
            _ => None,
        })
        .unwrap_or([0, 0, 0])
}

fn triangulate_corners(len: usize) -> impl Iterator<Item = [usize; 3]> {
    (1..len.saturating_sub(1)).map(|i| [0, i, i + 1])
}

fn texture_enabled(color_mode: ColorMode) -> bool {
    matches!(color_mode, ColorMode::Texture | ColorMode::Auto)
}

fn sample_bilinear(texture: &Texture, uv: Vec2, flip_v: bool, config: &Mesh3dConfig) -> [u8; 4] {
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
    if flip_v {
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
    let alpha = bilinear_channel(p00[3], p10[3], p01[3], p11[3], tx, ty);
    out[3] = alpha;
    let alpha_f = f32::from(alpha).max(1.0);
    for i in 0..3 {
        let c00 = f32::from(p00[i]) * f32::from(p00[3]) / 255.0;
        let c10 = f32::from(p10[i]) * f32::from(p10[3]) / 255.0;
        let c01 = f32::from(p01[i]) * f32::from(p01[3]) / 255.0;
        let c11 = f32::from(p11[i]) * f32::from(p11[3]) / 255.0;
        let premultiplied = bilinear_f32(c00, c10, c01, c11, tx, ty);
        out[i] = (premultiplied * 255.0 / alpha_f).round().clamp(0.0, 255.0) as u8;
    }
    out
}

fn bilinear_channel(c00: u8, c10: u8, c01: u8, c11: u8, tx: f32, ty: f32) -> u8 {
    bilinear_f32(
        f32::from(c00),
        f32::from(c10),
        f32::from(c01),
        f32::from(c11),
        tx,
        ty,
    )
    .round()
    .clamp(0.0, 255.0) as u8
}

fn bilinear_f32(c00: f32, c10: f32, c01: f32, c11: f32, tx: f32, ty: f32) -> f32 {
    let a = c00 * (1.0 - tx) + c10 * tx;
    let b = c01 * (1.0 - tx) + c11 * tx;
    a * (1.0 - ty) + b * ty
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::{
        model::{AlphaMode, Face, Material, Mesh, Texture, TextureRef, Vec2, Vec3},
        widget::Mesh3dState,
    };

    fn quad_mesh() -> Mesh {
        // A quad wound clockwise in screen space so its computed normal points away from the
        // camera (normal.z < 0). One-sided backface culling discards it; a double-sided
        // material must keep it.
        Mesh::with_attributes(
            "quad",
            vec![
                Vec3::new(-0.8, -0.8, 0.0),
                Vec3::new(-0.8, 0.8, 0.0),
                Vec3::new(0.8, 0.8, 0.0),
                Vec3::new(0.8, -0.8, 0.0),
            ],
            vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(1.0, 0.0),
            ],
            vec![],
            vec![Face::with_attributes(
                vec![0, 1, 2, 3],
                vec![Some(0), Some(1), Some(2), Some(3)],
                vec![None, None, None, None],
            )],
            vec![],
        )
        .unwrap()
    }

    fn render(mesh: &Mesh, config: &Mesh3dConfig) -> Terminal<TestBackend> {
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_mesh(
                    mesh,
                    frame.area(),
                    frame.buffer_mut(),
                    &Mesh3dState::default(),
                    config,
                );
            })
            .unwrap();
        terminal
    }

    fn painted(terminal: &Terminal<TestBackend>) -> bool {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() != " ")
    }

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

    #[test]
    fn transparent_texture_samples_do_not_paint() {
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
            path: "transparent.png".into(),
            index: Some(0),
        });
        mesh.textures
            .push(Texture::new("transparent.png", 1, 1, vec![0, 0, 255, 0]));
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
            .all(|cell| cell.symbol() == " "));
    }

    #[test]
    fn double_sided_material_survives_backface_culling() {
        // The quad faces away from the camera. A one-sided material is culled; a
        // double-sided one (glTF `doubleSided`) must still render.
        let mut mesh = quad_mesh();
        mesh.materials.push(Material::new("front"));
        mesh.faces[0].material = Some("front".into());

        let config = Mesh3dConfig::default()
            .backface_culling(true)
            .color_mode(ColorMode::Material);
        assert!(
            !painted(&render(&mesh, &config)),
            "one-sided back face should cull"
        );

        mesh.materials[0].double_sided = true;
        assert!(
            painted(&render(&mesh, &config)),
            "double-sided face must render even when facing away"
        );
    }

    #[test]
    fn masked_material_discards_below_cutoff() {
        let mut mesh = quad_mesh();
        let mut material = Material::new("mask");
        material.alpha_mode = AlphaMode::Mask;
        material.alpha_cutoff = 0.5;
        material.diffuse_texture = Some(TextureRef {
            path: "mask.png".into(),
            index: Some(0),
        });
        mesh.materials.push(material);
        mesh.faces[0].material = Some("mask".into());
        // Texel alpha 0.1 < cutoff 0.5 -> nothing painted.
        mesh.textures
            .push(Texture::new("mask.png", 1, 1, vec![255, 0, 0, 26]));

        let config = Mesh3dConfig::default()
            .backface_culling(false)
            .color_mode(ColorMode::Texture)
            .texture_lighting(false);
        assert!(
            !painted(&render(&mesh, &config)),
            "below cutoff should discard"
        );

        // Raise texel alpha above the cutoff -> renders fully opaque.
        mesh.textures[0] = Texture::new("mask.png", 1, 1, vec![255, 0, 0, 255]);
        let terminal = render(&mesh, &config);
        assert!(terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.fg == Color::Rgb(255, 0, 0)));
    }

    #[test]
    fn blend_material_composites_over_background() {
        // A blue blend quad at alpha 0.5 over a red opaque quad behind it should land on a
        // purple-ish blend rather than pure blue or pure red.
        let mut mesh = Mesh::with_attributes(
            "two-quads",
            vec![
                // back quad (red, opaque) slightly farther
                Vec3::new(-0.8, -0.8, 0.2),
                Vec3::new(0.8, -0.8, 0.2),
                Vec3::new(0.8, 0.8, 0.2),
                Vec3::new(-0.8, 0.8, 0.2),
                // front quad (blue, blend) nearer
                Vec3::new(-0.8, -0.8, -0.2),
                Vec3::new(0.8, -0.8, -0.2),
                Vec3::new(0.8, 0.8, -0.2),
                Vec3::new(-0.8, 0.8, -0.2),
            ],
            vec![Vec2::new(0.0, 0.0); 8],
            vec![],
            vec![
                {
                    let mut f = Face::new(vec![0, 1, 2, 3]);
                    f.material = Some("red".into());
                    f
                },
                {
                    let mut f = Face::new(vec![4, 5, 6, 7]);
                    f.material = Some("blue".into());
                    f
                },
            ],
            vec![],
        )
        .unwrap();
        let mut red = Material::new("red");
        red.diffuse = [1.0, 0.0, 0.0];
        let mut blue = Material::new("blue");
        blue.diffuse = [0.0, 0.0, 1.0];
        blue.alpha_mode = AlphaMode::Blend;
        blue.base_color_alpha = 0.5;
        mesh.materials = vec![red, blue];

        let config = Mesh3dConfig::default()
            .backface_culling(false)
            .color_mode(ColorMode::Material)
            .normalize(false)
            .background_style(Some(ratatui::style::Style::default().bg(Color::Black)));
        let terminal = render(&mesh, &config);
        let blended = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| matches!(cell.fg, Color::Rgb(r, _, b) if r > 20 && b > 20));
        assert!(
            blended,
            "blend material should mix with the opaque quad behind it"
        );
    }

    #[test]
    fn emissive_lifts_color_above_lit_base() {
        // With near-zero lighting, an emissive material should still produce visible color.
        let mut mesh = quad_mesh();
        let mut material = Material::new("glow");
        material.diffuse = [0.0, 0.0, 0.0];
        material.emissive = [0.0, 1.0, 0.0];
        material.diffuse_texture = Some(TextureRef {
            path: "glow.png".into(),
            index: Some(0),
        });
        mesh.materials.push(material);
        mesh.faces[0].material = Some("glow".into());
        // Black diffuse texel; all visible green must come from emissive.
        mesh.textures
            .push(Texture::new("glow.png", 1, 1, vec![0, 0, 0, 255]));

        let config = Mesh3dConfig::default()
            .backface_culling(false)
            .color_mode(ColorMode::Texture)
            .lighting(0.0, 0.0);
        let terminal = render(&mesh, &config);
        let has_green = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| matches!(cell.fg, Color::Rgb(_, g, _) if g > 100));
        assert!(
            has_green,
            "emissive color should remain visible without lighting"
        );
    }
}
