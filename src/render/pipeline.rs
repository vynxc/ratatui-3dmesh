use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use std::ops::Range;

use crate::{
    animation::{sample_mesh_geometry_reusing, SampledGeometry},
    config::{ColorMode, Mesh3dConfig, RenderMode, TextureFilter, TextureWrap},
    model::{AlphaMode, Face, Material, Mesh, Texture, Vec2, Vec3},
    widget::Mesh3dState,
};

use super::{
    camera::{project, ProjectedVertex},
    color::{
        add_emissive, emissive_rgb_from_factor, luminance, solid_base_rgb, style_for, texture_rgb,
    },
    metrics::{Metrics, NoopMetrics, RenderProfile},
    raster::{
        draw_line, fill_triangle_deferred_profiled, fill_triangle_shaded_with_setup, plot,
        setup_triangle, Fragment,
    },
    FrameCacheConfig, FrameCacheStats,
};

/// Depth bias (in post-normalize world units) applied to translucent BLEND fragments so
/// decals coincident with the opaque surface behind them win the depth test.
const DECAL_DEPTH_BIAS: f32 = 0.01;

/// Immutable mesh topology prepared for cache-friendly repeated rendering.
#[derive(Debug)]
pub struct PreparedMesh<'a> {
    mesh: &'a Mesh,
    faces: Vec<PreparedFace<'a>>,
    triangles: Vec<PreparedTriangle>,
    emissive_maps: Vec<Option<Box<[[u8; 3]]>>>,
    animation_scratch: std::sync::Mutex<Option<SampledGeometry>>,
    deferred_scratch: std::sync::Mutex<DeferredScratch>,
    frame_cache: std::sync::Mutex<Option<super::frame_cache::FrameCache>>,
    has_blend: bool,
    deferred_opaque: bool,
}

#[derive(Debug)]
struct PreparedFace<'a> {
    triangles: Range<usize>,
    material: Option<&'a Material>,
    diffuse_texture: Option<&'a Texture>,
    emissive_texture: Option<&'a Texture>,
    emissive_map: Option<usize>,
    emissive_factor: Option<[f32; 3]>,
    alpha_mode: AlphaMode,
    base_alpha: f32,
    alpha_cutoff: f32,
    double_sided: bool,
    unlit: bool,
}

#[derive(Debug, Clone, Copy)]
struct PreparedTriangle {
    indices: [usize; 3],
    normal_index: Option<usize>,
    uvs: Option<[Vec2; 3]>,
}

impl<'a> PreparedMesh<'a> {
    /// Flatten immutable face/material metadata once for repeated rendering.
    #[must_use]
    pub fn new(mesh: &'a Mesh) -> Self {
        let emissive_maps = mesh
            .materials
            .iter()
            .map(|material| {
                let factor = material.is_emissive().then_some(material.emissive)?;
                let texture = emissive_texture(mesh, Some(material))?;
                Some(
                    texture
                        .rgba
                        .chunks_exact(4)
                        .map(|rgba| {
                            emissive_rgb_from_factor(
                                factor,
                                Some([rgba[0], rgba[1], rgba[2], rgba[3]]),
                                1.0,
                            )
                        })
                        .collect(),
                )
            })
            .collect();
        let texture_is_opaque = mesh
            .textures
            .iter()
            .map(|texture| texture.rgba.chunks_exact(4).all(|rgba| rgba[3] >= 16))
            .collect::<Vec<_>>();
        let mut faces = Vec::with_capacity(mesh.faces.len());
        let mut triangles = Vec::with_capacity(mesh.faces.len());
        let mut has_blend = false;
        let mut deferred_opaque = true;
        for face in &mesh.faces {
            let material = mesh.material(face.material.as_deref().unwrap_or_default());
            let material_index = material.and_then(|material| {
                mesh.materials
                    .iter()
                    .position(|candidate| std::ptr::eq(candidate, material))
            });
            let alpha_mode = material.map_or(AlphaMode::Opaque, |material| material.alpha_mode);
            has_blend |= matches!(alpha_mode, AlphaMode::Blend);
            let diffuse_is_opaque = material
                .and_then(|material| material.diffuse_texture.as_ref())
                .and_then(|texture| texture.index)
                .and_then(|index| texture_is_opaque.get(index))
                .copied()
                .unwrap_or(true);
            deferred_opaque &= matches!(alpha_mode, AlphaMode::Opaque) && diffuse_is_opaque;
            let start = triangles.len();
            for corners in triangulate_corners(face.indices.len()) {
                let [a, b, c] = corners.map(|corner| face.indices[corner]);
                let normal_index = face.normal_indices.iter().flatten().next().copied();
                triangles.push(PreparedTriangle {
                    indices: [a, b, c],
                    normal_index,
                    uvs: triangle_uvs(mesh, face, corners),
                });
            }
            faces.push(PreparedFace {
                triangles: start..triangles.len(),
                material,
                diffuse_texture: diffuse_texture(mesh, material),
                emissive_texture: emissive_texture(mesh, material),
                emissive_map: material_index,
                emissive_factor: material
                    .filter(|material| material.is_emissive())
                    .map(|material| material.emissive),
                alpha_mode,
                base_alpha: material.map_or(1.0, |material| material.base_color_alpha),
                alpha_cutoff: material.map_or(0.5, |material| material.alpha_cutoff),
                double_sided: material.is_some_and(|material| material.double_sided),
                unlit: material.is_some_and(|material| material.unlit),
            });
        }
        deferred_opaque &= !has_blend && triangles.len() <= u32::MAX as usize;
        Self {
            mesh,
            faces,
            triangles,
            emissive_maps,
            animation_scratch: std::sync::Mutex::new(None),
            deferred_scratch: std::sync::Mutex::new(DeferredScratch::default()),
            frame_cache: std::sync::Mutex::new(None),
            has_blend,
            deferred_opaque,
        }
    }

    /// Borrow the source mesh.
    #[must_use]
    pub const fn mesh(&self) -> &'a Mesh {
        self.mesh
    }

    /// Enable an opt-in in-memory cache for looping animation frames.
    ///
    /// The cache belongs to this prepared mesh and is reused across widget instances.
    /// Animation sampling is quantized to the configured frame rate so a replayed frame
    /// is byte-for-byte identical to the corresponding warm-up frame.
    #[must_use]
    pub fn with_frame_cache(mut self, config: FrameCacheConfig) -> Self {
        self.frame_cache = std::sync::Mutex::new(Some(super::frame_cache::FrameCache::new(config)));
        self
    }

    /// Return counters for the optional animation-frame cache.
    #[must_use]
    pub fn frame_cache_stats(&self) -> Option<FrameCacheStats> {
        self.frame_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(super::frame_cache::FrameCache::stats)
    }

    /// Drop all retained animation frames and reset cache counters.
    pub fn clear_frame_cache(&self) {
        if let Some(cache) = self
            .frame_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            cache.clear();
        }
    }

    fn can_defer_opaque(&self, config: &Mesh3dConfig) -> bool {
        self.deferred_opaque
            && matches!(config.render_mode, RenderMode::Solid)
            && matches!(config.color_mode, ColorMode::Texture | ColorMode::Auto)
            && matches!(config.texture_wrap, TextureWrap::Repeat)
            && config.texture_lighting
            && config.color_brightness == 1.0
            && config.glyph_ramp.is_ascii()
            && !config.glyph_ramp.is_empty()
    }
}

/// Render a mesh into a Ratatui buffer.
pub fn render_mesh(
    mesh: &Mesh,
    area: Rect,
    buf: &mut Buffer,
    state: &Mesh3dState,
    config: &Mesh3dConfig,
) {
    render_mesh_impl(mesh, None, area, buf, state, config, &mut NoopMetrics);
}

/// Render cache-friendly prepared mesh topology.
pub fn render_prepared_mesh(
    prepared: &PreparedMesh<'_>,
    area: Rect,
    buf: &mut Buffer,
    state: &Mesh3dState,
    config: &Mesh3dConfig,
) {
    let clip_duration = state
        .selected_animation
        .filter(|_| state.animation_looping)
        .and_then(|index| prepared.mesh.animations.get(index))
        .map(|clip| clip.duration_seconds)
        .filter(|duration| duration.is_finite() && *duration > f32::EPSILON);
    if let Some(clip_duration) = clip_duration.filter(|_| {
        !state.auto_spin_enabled
            && prepared.can_defer_opaque(config)
            && config.background_style.is_none()
    }) {
        let mut cache = prepared
            .frame_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cache) = cache.as_mut() {
            cache.render(
                area,
                buf,
                state,
                config,
                clip_duration,
                |buf, sampled_state| {
                    render_mesh_impl(
                        prepared.mesh,
                        Some(prepared),
                        area,
                        buf,
                        sampled_state,
                        config,
                        &mut NoopMetrics,
                    );
                    prepared
                        .deferred_scratch
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .painted
                        .clone()
                },
            );
            return;
        }
    }
    render_mesh_impl(
        prepared.mesh,
        Some(prepared),
        area,
        buf,
        state,
        config,
        &mut NoopMetrics,
    );
}

/// Render a mesh while collecting detailed phase timings and raster-work counters.
///
/// Use this for diagnostics and benchmarks. [`render_mesh`] uses compile-time-elided
/// no-op counters, so normal rendering does not pay the per-cell profiling overhead.
#[must_use]
pub fn render_mesh_profiled(
    mesh: &Mesh,
    area: Rect,
    buf: &mut Buffer,
    state: &Mesh3dState,
    config: &Mesh3dConfig,
) -> RenderProfile {
    let total_started = std::time::Instant::now();
    let mut profile = RenderProfile::default();
    render_mesh_impl(mesh, None, area, buf, state, config, &mut profile);
    profile.total = total_started.elapsed();
    profile
}

/// Render prepared topology while collecting detailed diagnostics.
#[must_use]
pub fn render_prepared_mesh_profiled(
    prepared: &PreparedMesh<'_>,
    area: Rect,
    buf: &mut Buffer,
    state: &Mesh3dState,
    config: &Mesh3dConfig,
) -> RenderProfile {
    let total_started = std::time::Instant::now();
    let mut profile = RenderProfile::default();
    render_mesh_impl(
        prepared.mesh,
        Some(prepared),
        area,
        buf,
        state,
        config,
        &mut profile,
    );
    profile.total = total_started.elapsed();
    profile
}

fn render_mesh_impl<M: Metrics>(
    mesh: &Mesh,
    prepared: Option<&PreparedMesh<'_>>,
    area: Rect,
    buf: &mut Buffer,
    state: &Mesh3dState,
    config: &Mesh3dConfig,
    metrics: &mut M,
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

    let phase_started = metrics.start();
    let sampled_geometry = state.selected_animation.and_then(|clip| {
        let reuse = prepared.and_then(|prepared| {
            prepared
                .animation_scratch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
        });
        sample_mesh_geometry_reusing(
            mesh,
            clip,
            state.animation_time_seconds,
            state.animation_looping,
            state.animation_loop_blend_seconds,
            reuse,
        )
    });
    metrics.finish_animation(phase_started);
    let vertices = sampled_geometry
        .as_ref()
        .map_or(mesh.vertices.as_slice(), |geometry| {
            geometry.vertices.as_slice()
        });
    let normals = sampled_geometry
        .as_ref()
        .map_or(mesh.normals.as_slice(), |geometry| {
            geometry.normals.as_slice()
        });
    let bounds = sampled_geometry
        .as_ref()
        .map_or(mesh.bounds, |geometry| geometry.bounds);
    let (normalization_center, normalization_scale) = if config.normalize {
        let radius = bounds.radius();
        (
            bounds.center(),
            if radius > f32::EPSILON {
                radius.recip()
            } else {
                1.0
            },
        )
    } else {
        (Vec3::default(), 1.0)
    };
    let rotation = EulerRotation::new(state.rotation);
    let pan = state.pan;
    let zoom = state.zoom * config.scale;
    let light = Vec3::new(
        config.light_direction[0],
        config.light_direction[1],
        config.light_direction[2],
    )
    .normalized();
    let backdrop = backdrop_rgb(config);
    let glyph_ramp_is_ascii = config.glyph_ramp.is_ascii();

    let phase_started = metrics.start();
    let projected = vertices
        .iter()
        .map(|&v| {
            let normalized = (v - normalization_center) * normalization_scale;
            let transformed = rotation.apply(normalized) + Vec3::new(pan.x, pan.y, 0.0);
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
    metrics.finish_projection(phase_started);

    let phase_started = metrics.start();
    let mut zbuf = vec![f32::INFINITY; usize::from(area.width) * usize::from(area.height)];
    metrics.finish_depth_buffer(phase_started);

    // Two passes so authored transparency (glTF BLEND) layers correctly over opaque
    // geometry: opaque/mask faces write depth first, then blend faces composite on top,
    // sorted back-to-front. Wireframe/points ignore the split and draw in source order.
    let ctx = DrawContext {
        mesh,
        projected: &projected,
        normals,
        light,
        model_light: rotation.inverse_apply(light),
        model_camera: rotation.inverse_apply(Vec3::new(0.0, 0.0, 1.0)),
        backdrop,
        glyph_ramp_is_ascii,
    };
    let phase_started = metrics.start();
    if let Some(prepared) = prepared {
        if prepared.can_defer_opaque(config) {
            draw_prepared_deferred(prepared, &ctx, area, buf, &mut zbuf, config, metrics);
        } else {
            draw_prepared_faces(prepared, &ctx, area, buf, &mut zbuf, config, metrics);
        }
    } else if matches!(config.render_mode, RenderMode::Solid) {
        let blend = blend_faces(mesh, &projected, config);
        for _ in &blend {
            metrics.blend_face();
        }
        let limit = mesh.faces.len().min(config.max_faces.unwrap_or(usize::MAX));
        let mut cached_material_name = None;
        let mut cached_material = None;
        for face_index in 0..limit {
            metrics.face();
            let face = &mesh.faces[face_index];
            let material_name = face.material.as_deref().unwrap_or_default();
            let material = if cached_material_name == Some(material_name) {
                cached_material
            } else {
                cached_material_name = Some(material_name);
                cached_material = mesh.material(material_name);
                cached_material
            };
            if matches!(material.map(|m| m.alpha_mode), Some(AlphaMode::Blend)) {
                continue;
            }
            draw_face(
                &ctx, face_index, material, area, buf, &mut zbuf, config, metrics,
            );
        }
        for &face_index in &blend {
            let face = &mesh.faces[face_index];
            let material = mesh.material(face.material.as_deref().unwrap_or_default());
            draw_face(
                &ctx, face_index, material, area, buf, &mut zbuf, config, metrics,
            );
        }
    } else {
        let limit = config.max_faces.unwrap_or(usize::MAX);
        for face_index in 0..mesh.faces.len().min(limit) {
            metrics.face();
            let face = &mesh.faces[face_index];
            let material = mesh.material(face.material.as_deref().unwrap_or_default());
            draw_face(
                &ctx, face_index, material, area, buf, &mut zbuf, config, metrics,
            );
        }
    }
    metrics.finish_face_rendering(phase_started);
    if let (Some(prepared), Some(geometry)) = (prepared, sampled_geometry) {
        *prepared
            .animation_scratch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(geometry);
    }
}

#[derive(Debug, Clone, Copy)]
struct DeferredCell {
    shader: u32,
    weights: [f32; 3],
}

impl Default for DeferredCell {
    fn default() -> Self {
        Self {
            shader: u32::MAX,
            weights: [0.0; 3],
        }
    }
}

#[derive(Debug, Default)]
struct DeferredScratch {
    occupied: Vec<bool>,
    cells: Vec<DeferredCell>,
    painted: Vec<u32>,
}

#[derive(Clone, Copy)]
struct DeferredShader<'a> {
    shading: FaceShading<'a>,
    fast: bool,
}

fn draw_prepared_deferred<M: Metrics>(
    prepared: &PreparedMesh<'_>,
    ctx: &DrawContext<'_>,
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    config: &Mesh3dConfig,
    metrics: &mut M,
) {
    let cell_count = usize::from(area.width) * usize::from(area.height);
    let mut scratch = prepared
        .deferred_scratch
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    scratch.occupied.clear();
    scratch.occupied.resize(cell_count, false);
    scratch.cells.clear();
    scratch.cells.resize(cell_count, DeferredCell::default());
    scratch.painted.clear();
    let DeferredScratch {
        occupied,
        cells,
        painted,
    } = &mut *scratch;
    let mut shaders = Vec::with_capacity(prepared.triangles.len().min(32_768));
    let limit = prepared
        .faces
        .len()
        .min(config.max_faces.unwrap_or(usize::MAX));

    for face_index in 0..limit {
        metrics.face();
        let face = &prepared.faces[face_index];
        for triangle_index in face.triangles.clone() {
            metrics.triangle();
            let triangle = prepared.triangles[triangle_index];
            let [a_index, b_index, c_index] = triangle.indices;
            let (Some(a), Some(b), Some(c)) = (
                ctx.projected.get(a_index).copied(),
                ctx.projected.get(b_index).copied(),
                ctx.projected.get(c_index).copied(),
            ) else {
                continue;
            };
            let (facing, light_dot) = if let Some(normal) = triangle
                .normal_index
                .and_then(|index| ctx.normals.get(index).copied())
            {
                (normal.dot(ctx.model_camera), normal.dot(ctx.model_light))
            } else {
                let normal = (b.view - a.view).cross(c.view - a.view).normalized();
                (normal.z, normal.dot(ctx.light))
            };
            if config.backface_culling && !face.double_sided && facing >= 0.0 {
                metrics.culled_triangle();
                continue;
            }
            let Some(setup) = setup_triangle(area, [a, b, c]) else {
                metrics.skipped_triangle();
                continue;
            };
            let intensity = (config.ambient + config.diffuse * light_dot.abs()).clamp(0.0, 1.0);
            let shading = FaceShading {
                material: face.material,
                uvs: triangle.uvs,
                vertex_colors: triangle_vertex_colors_for_indices(prepared.mesh, triangle.indices),
                diffuse_texture: texture_enabled(config.color_mode)
                    .then_some(face.diffuse_texture)
                    .flatten(),
                emissive_texture: texture_enabled(config.color_mode)
                    .then_some(face.emissive_texture)
                    .flatten(),
                emissive_map: face
                    .emissive_map
                    .and_then(|index| prepared.emissive_maps[index].as_deref()),
                emissive_factor: face.emissive_factor,
                glyph_ramp_is_ascii: ctx.glyph_ramp_is_ascii,
                flip_v: prepared.mesh.flip_texture_v && config.flip_texture_v,
                alpha_mode: face.alpha_mode,
                base_alpha: face.base_alpha,
                alpha_cutoff: face.alpha_cutoff,
                unlit: face.unlit && !matches!(config.color_mode, ColorMode::Lighting),
                intensity,
                fallback_glyph: '#',
            };
            let shader = DeferredShader {
                fast: shading.fast_opaque_texture(config),
                shading,
            };
            let shader_index = shaders.len() as u32;
            shaders.push(shader);
            fill_triangle_deferred_profiled(
                area,
                zbuf,
                occupied,
                [a, b, c],
                setup,
                0.0,
                metrics,
                |index, weights| {
                    cells[index] = DeferredCell {
                        shader: shader_index,
                        weights,
                    };
                },
            );
        }
    }

    let width = usize::from(area.width);
    for (index, deferred) in cells.iter().enumerate() {
        if deferred.shader == u32::MAX {
            continue;
        }
        metrics.shade_call();
        let shader = shaders[deferred.shader as usize];
        let fragment = if shader.fast {
            shade_opaque_texture_cell(&shader.shading, deferred.weights, config)
        } else {
            shade_cell(&shader.shading, deferred.weights, config)
        };
        let Some(fragment) = fragment else {
            metrics.shade_discarded();
            continue;
        };
        let x = (index % width) as u16;
        let y = (index / width) as u16;
        let cell = &mut buf[(area.x + x, area.y + y)];
        cell.set_char(fragment.ch);
        cell.set_fg(Color::Rgb(
            fragment.rgb[0],
            fragment.rgb[1],
            fragment.rgb[2],
        ));
        painted.push(index as u32);
        metrics.cell_written();
    }
}

fn draw_prepared_faces<M: Metrics>(
    prepared: &PreparedMesh<'_>,
    ctx: &DrawContext<'_>,
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    config: &Mesh3dConfig,
    metrics: &mut M,
) {
    if !matches!(config.render_mode, RenderMode::Solid) {
        let limit = prepared
            .faces
            .len()
            .min(config.max_faces.unwrap_or(usize::MAX));
        for face_index in 0..limit {
            metrics.face();
            draw_prepared_face(prepared, ctx, face_index, area, buf, zbuf, config, metrics);
        }
        return;
    }

    let limit = prepared
        .faces
        .len()
        .min(config.max_faces.unwrap_or(usize::MAX));
    for face_index in 0..limit {
        metrics.face();
        if matches!(prepared.faces[face_index].alpha_mode, AlphaMode::Blend) {
            continue;
        }
        draw_prepared_face(prepared, ctx, face_index, area, buf, zbuf, config, metrics);
    }
    if prepared.has_blend {
        let mut blend = (0..limit)
            .filter(|&index| matches!(prepared.faces[index].alpha_mode, AlphaMode::Blend))
            .collect::<Vec<_>>();
        for _ in &blend {
            metrics.blend_face();
        }
        blend.sort_by(|&a, &b| {
            face_depth(prepared.mesh, ctx.projected, b)
                .partial_cmp(&face_depth(prepared.mesh, ctx.projected, a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for face_index in blend {
            draw_prepared_face(prepared, ctx, face_index, area, buf, zbuf, config, metrics);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_prepared_face<M: Metrics>(
    prepared: &PreparedMesh<'_>,
    ctx: &DrawContext<'_>,
    face_index: usize,
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    config: &Mesh3dConfig,
    metrics: &mut M,
) {
    let face = &prepared.faces[face_index];
    let decal_bias = if matches!(face.alpha_mode, AlphaMode::Blend) {
        DECAL_DEPTH_BIAS
    } else {
        0.0
    };
    for triangle_index in face.triangles.clone() {
        metrics.triangle();
        let triangle = prepared.triangles[triangle_index];
        let [a_index, b_index, c_index] = triangle.indices;
        let (Some(a), Some(b), Some(c)) = (
            ctx.projected.get(a_index).copied(),
            ctx.projected.get(b_index).copied(),
            ctx.projected.get(c_index).copied(),
        ) else {
            continue;
        };
        let (facing, light_dot) = if let Some(normal) = triangle
            .normal_index
            .and_then(|index| ctx.normals.get(index).copied())
        {
            (normal.dot(ctx.model_camera), normal.dot(ctx.model_light))
        } else {
            let normal = (b.view - a.view).cross(c.view - a.view).normalized();
            (normal.z, normal.dot(ctx.light))
        };
        if config.backface_culling && !face.double_sided && facing >= 0.0 {
            metrics.culled_triangle();
            continue;
        }
        let intensity = (config.ambient + config.diffuse * light_dot.abs()).clamp(0.0, 1.0);
        if matches!(config.render_mode, RenderMode::Solid) {
            let Some(setup) = setup_triangle(area, [a, b, c]) else {
                metrics.skipped_triangle();
                continue;
            };
            let fallback_glyph = if matches!(config.color_mode, ColorMode::Off) {
                config.glyph_for_intensity_with_ascii(intensity, ctx.glyph_ramp_is_ascii)
            } else {
                '#'
            };
            let shading = FaceShading {
                material: face.material,
                uvs: triangle.uvs,
                vertex_colors: triangle_vertex_colors_for_indices(prepared.mesh, triangle.indices),
                diffuse_texture: texture_enabled(config.color_mode)
                    .then_some(face.diffuse_texture)
                    .flatten(),
                emissive_texture: texture_enabled(config.color_mode)
                    .then_some(face.emissive_texture)
                    .flatten(),
                emissive_map: face
                    .emissive_map
                    .and_then(|index| prepared.emissive_maps[index].as_deref()),
                emissive_factor: face.emissive_factor,
                glyph_ramp_is_ascii: ctx.glyph_ramp_is_ascii,
                flip_v: prepared.mesh.flip_texture_v && config.flip_texture_v,
                alpha_mode: face.alpha_mode,
                base_alpha: face.base_alpha,
                alpha_cutoff: face.alpha_cutoff,
                unlit: face.unlit && !matches!(config.color_mode, ColorMode::Lighting),
                intensity,
                fallback_glyph,
            };
            if shading.fast_opaque_texture(config) {
                fill_triangle_shaded_with_setup(
                    area,
                    buf,
                    zbuf,
                    [a, b, c],
                    setup,
                    ctx.backdrop,
                    decal_bias,
                    metrics,
                    |weights, _| shade_opaque_texture_cell(&shading, weights, config),
                );
            } else {
                fill_triangle_shaded_with_setup(
                    area,
                    buf,
                    zbuf,
                    [a, b, c],
                    setup,
                    ctx.backdrop,
                    decal_bias,
                    metrics,
                    |weights, _| shade_cell(&shading, weights, config),
                );
            }
        } else {
            let glyph = config.glyph_for_intensity_with_ascii(intensity, ctx.glyph_ramp_is_ascii);
            let style = style_for(face.material, None, intensity, config);
            match config.render_mode {
                RenderMode::Wireframe => {
                    draw_line(area, buf, zbuf, a, b, glyph, style);
                    draw_line(area, buf, zbuf, b, c, glyph, style);
                    draw_line(area, buf, zbuf, c, a, glyph, style);
                }
                RenderMode::Points => {
                    plot(area, buf, zbuf, a, glyph, style);
                    plot(area, buf, zbuf, b, glyph, style);
                    plot(area, buf, zbuf, c, glyph, style);
                }
                RenderMode::Solid => unreachable!(),
            }
        }
    }
}

struct DrawContext<'a> {
    mesh: &'a Mesh,
    projected: &'a [ProjectedVertex],
    normals: &'a [Vec3],
    light: Vec3,
    model_light: Vec3,
    model_camera: Vec3,
    backdrop: [u8; 3],
    glyph_ramp_is_ascii: bool,
}

/// Collect solid-mode blend faces for the sorted transparency pass.
///
/// Opaque faces are streamed directly from the mesh, avoiding a large per-frame allocation
/// for the overwhelmingly common opaque-only case. Respects `max_faces`.
fn blend_faces(mesh: &Mesh, projected: &[ProjectedVertex], config: &Mesh3dConfig) -> Vec<usize> {
    if !mesh
        .materials
        .iter()
        .any(|material| matches!(material.alpha_mode, AlphaMode::Blend))
    {
        return Vec::new();
    }
    let limit = config.max_faces.unwrap_or(usize::MAX);
    let mut blend = Vec::new();
    for (index, face) in mesh.faces.iter().take(limit).enumerate() {
        let material = mesh.material(face.material.as_deref().unwrap_or_default());
        if matches!(material.map(|m| m.alpha_mode), Some(AlphaMode::Blend)) {
            blend.push(index);
        }
    }
    // Back-to-front: larger view depth is farther from the camera, so draw it first.
    blend.sort_by(|&a, &b| {
        face_depth(mesh, projected, b)
            .partial_cmp(&face_depth(mesh, projected, a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    blend
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

#[allow(clippy::too_many_arguments)]
fn draw_face<M: Metrics>(
    ctx: &DrawContext<'_>,
    face_index: usize,
    material: Option<&Material>,
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    config: &Mesh3dConfig,
    metrics: &mut M,
) {
    let mesh = ctx.mesh;
    let face = &mesh.faces[face_index];
    if face.indices.len() < 3 {
        return;
    }
    let double_sided = material.is_some_and(|m| m.double_sided);
    // Translucent decals (glTF BLEND, e.g. eye irises) sit exactly on the opaque surface
    // behind them. A small depth bias lets them win the depth test instead of z-fighting.
    let decal_bias = if matches!(material.map(|m| m.alpha_mode), Some(AlphaMode::Blend)) {
        DECAL_DEPTH_BIAS
    } else {
        0.0
    };
    for corners in triangulate_corners(face.indices.len()) {
        metrics.triangle();
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
        let (facing, light_dot) = if let Some(normal) = face
            .normal_indices
            .iter()
            .flatten()
            .next()
            .and_then(|&index| ctx.normals.get(index).copied())
        {
            (normal.dot(ctx.model_camera), normal.dot(ctx.model_light))
        } else {
            let normal = (b.view - a.view).cross(c.view - a.view).normalized();
            (normal.z, normal.dot(ctx.light))
        };
        // The camera looks down +Z, so front faces have normals pointing back toward -Z.
        // Double-sided materials (hair cards, eye/brow decals) must never be culled.
        if config.backface_culling && !double_sided && facing >= 0.0 {
            metrics.culled_triangle();
            continue;
        }
        let intensity = (config.ambient + config.diffuse * light_dot.abs()).clamp(0.0, 1.0);

        match config.render_mode {
            RenderMode::Solid => {
                let Some(setup) = setup_triangle(area, [a, b, c]) else {
                    metrics.skipped_triangle();
                    continue;
                };
                let fallback_glyph = if matches!(config.color_mode, ColorMode::Off) {
                    config.glyph_for_intensity_with_ascii(intensity, ctx.glyph_ramp_is_ascii)
                } else {
                    '#'
                };
                let shading = FaceShading {
                    material,
                    uvs: triangle_uvs(mesh, face, corners),
                    vertex_colors: triangle_vertex_colors(mesh, face, corners),
                    diffuse_texture: if texture_enabled(config.color_mode) {
                        diffuse_texture(mesh, material)
                    } else {
                        None
                    },
                    emissive_texture: if texture_enabled(config.color_mode) {
                        emissive_texture(mesh, material)
                    } else {
                        None
                    },
                    emissive_map: None,
                    emissive_factor: material
                        .filter(|material| material.is_emissive())
                        .map(|material| material.emissive),
                    glyph_ramp_is_ascii: ctx.glyph_ramp_is_ascii,
                    flip_v: mesh.flip_texture_v && config.flip_texture_v,
                    alpha_mode: material.map_or(AlphaMode::Opaque, |m| m.alpha_mode),
                    base_alpha: material.map_or(1.0, |m| m.base_color_alpha),
                    alpha_cutoff: material.map_or(0.5, |m| m.alpha_cutoff),
                    unlit: material.is_some_and(|m| m.unlit)
                        && !matches!(config.color_mode, ColorMode::Lighting),
                    intensity,
                    fallback_glyph,
                };
                fill_triangle_shaded_with_setup(
                    area,
                    buf,
                    zbuf,
                    [a, b, c],
                    setup,
                    ctx.backdrop,
                    decal_bias,
                    metrics,
                    |weights, _| shade_cell(&shading, weights, config),
                );
            }
            RenderMode::Wireframe => {
                let ch = config.glyph_for_intensity_with_ascii(intensity, ctx.glyph_ramp_is_ascii);
                let style = style_for(material, None, intensity, config);
                draw_line(area, buf, zbuf, a, b, ch, style);
                draw_line(area, buf, zbuf, b, c, ch, style);
                draw_line(area, buf, zbuf, c, a, ch, style);
            }
            RenderMode::Points => {
                let ch = config.glyph_for_intensity_with_ascii(intensity, ctx.glyph_ramp_is_ascii);
                let style = style_for(material, None, intensity, config);
                plot(area, buf, zbuf, a, ch, style);
                plot(area, buf, zbuf, b, ch, style);
                plot(area, buf, zbuf, c, ch, style);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct EulerRotation {
    sx: f32,
    cx: f32,
    sy: f32,
    cy: f32,
    sz: f32,
    cz: f32,
}

impl EulerRotation {
    fn new(rotation: Vec3) -> Self {
        let (sx, cx) = rotation.x.sin_cos();
        let (sy, cy) = rotation.y.sin_cos();
        let (sz, cz) = rotation.z.sin_cos();
        Self {
            sx,
            cx,
            sy,
            cy,
            sz,
            cz,
        }
    }

    #[inline(always)]
    fn apply(self, value: Vec3) -> Vec3 {
        let mut rotated = Vec3::new(
            value.x,
            value.y * self.cx - value.z * self.sx,
            value.y * self.sx + value.z * self.cx,
        );
        rotated = Vec3::new(
            rotated.x * self.cy + rotated.z * self.sy,
            rotated.y,
            -rotated.x * self.sy + rotated.z * self.cy,
        );
        Vec3::new(
            rotated.x * self.cz - rotated.y * self.sz,
            rotated.x * self.sz + rotated.y * self.cz,
            rotated.z,
        )
    }

    fn inverse_apply(self, value: Vec3) -> Vec3 {
        let mut rotated = Vec3::new(
            value.x * self.cz + value.y * self.sz,
            -value.x * self.sz + value.y * self.cz,
            value.z,
        );
        rotated = Vec3::new(
            rotated.x * self.cy - rotated.z * self.sy,
            rotated.y,
            rotated.x * self.sy + rotated.z * self.cy,
        );
        Vec3::new(
            rotated.x,
            rotated.y * self.cx + rotated.z * self.sx,
            -rotated.y * self.sx + rotated.z * self.cx,
        )
    }
}

#[derive(Clone, Copy)]
struct FaceShading<'a> {
    material: Option<&'a Material>,
    uvs: Option<[Vec2; 3]>,
    vertex_colors: Option<[[f32; 4]; 3]>,
    diffuse_texture: Option<&'a Texture>,
    emissive_texture: Option<&'a Texture>,
    emissive_map: Option<&'a [[u8; 3]]>,
    emissive_factor: Option<[f32; 3]>,
    glyph_ramp_is_ascii: bool,
    flip_v: bool,
    alpha_mode: AlphaMode,
    base_alpha: f32,
    alpha_cutoff: f32,
    unlit: bool,
    intensity: f32,
    fallback_glyph: char,
}

impl FaceShading<'_> {
    fn fast_opaque_texture(&self, config: &Mesh3dConfig) -> bool {
        matches!(config.color_mode, ColorMode::Texture | ColorMode::Auto)
            && self.glyph_ramp_is_ascii
            && !config.glyph_ramp.is_empty()
            && matches!(config.texture_filter, TextureFilter::Nearest)
            && matches!(config.texture_wrap, TextureWrap::Repeat)
            && config.texture_lighting
            && config.color_brightness == 1.0
            && matches!(self.alpha_mode, AlphaMode::Opaque)
            && !self.unlit
            && self.uvs.is_some()
            && self.vertex_colors.is_none()
            && self.diffuse_texture.is_some_and(|diffuse| {
                self.emissive_texture.is_some_and(|emissive| {
                    diffuse.width == emissive.width
                        && diffuse.height == emissive.height
                        && (diffuse.width as usize)
                            .checked_mul(diffuse.height as usize)
                            .and_then(|texels| texels.checked_mul(4))
                            .is_some_and(|bytes| diffuse.rgba.len() >= bytes)
                })
            })
            && self.emissive_map.is_some_and(|map| {
                self.diffuse_texture.is_some_and(|texture| {
                    (texture.width as usize)
                        .checked_mul(texture.height as usize)
                        .is_some_and(|texels| map.len() >= texels)
                })
            })
            && self
                .material
                .is_none_or(|material| material.diffuse == [1.0; 3])
    }
}

#[inline(always)]
fn shade_opaque_texture_cell(
    shading: &FaceShading<'_>,
    weights: [f32; 3],
    config: &Mesh3dConfig,
) -> Option<Fragment> {
    let texture = shading.diffuse_texture?;
    let uv = interpolate_uv(shading.uvs, weights)?;
    let index = texture.nearest_texel_index_repeat(uv, shading.flip_v)?;
    // `fast_opaque_texture` validates the full declared texture extent.
    let rgba = unsafe { texture.rgba_at_unchecked(index) };
    if rgba[3] < 16 {
        return None;
    }
    let emissive = shading
        .emissive_map
        .and_then(|map| map.get(index / 4))
        .copied()
        .unwrap_or([0, 0, 0]);
    let rgb = add_emissive(texture_rgb(rgba, shading.intensity, config), emissive);
    Some(Fragment {
        ch: fast_ascii_glyph(shading, rgb, config),
        rgb,
        alpha: 1.0,
    })
}

#[inline(always)]
fn fast_ascii_glyph(shading: &FaceShading<'_>, rgb: [u8; 3], config: &Mesh3dConfig) -> char {
    let lum = luminance(rgb);
    let value = if config.texture_lighting {
        lum.max(shading.intensity * 0.35)
    } else {
        lum.max(shading.intensity)
    };
    let glyphs = config.glyph_ramp.as_bytes();
    let index = (value.clamp(0.0, 1.0) * (glyphs.len() - 1) as f32).round() as usize;
    char::from(glyphs[index.min(glyphs.len() - 1)])
}

/// Shade a single covered cell: sample the diffuse texture (if any), apply the material
/// alpha mode, light the color, and add emissive contribution. Returns `None` to discard.
#[inline(always)]
fn shade_cell(
    shading: &FaceShading<'_>,
    weights: [f32; 3],
    config: &Mesh3dConfig,
) -> Option<Fragment> {
    let material = shading.material;
    let uv = interpolate_uv(shading.uvs, weights);
    let (diffuse_sample, emissive_sample, shared_texel_index) = uv
        .map_or((None, None, None), |uv| {
            sample_face_textures(shading, uv, config)
        });

    // A fully transparent texel carries no usable color in a terminal cell, so skip it
    // regardless of the material's alpha mode. This keeps texture cut-outs (sprite-style
    // holes) from painting stray glyphs.
    if let Some(rgba) = diffuse_sample {
        if rgba[3] < 16 {
            return None;
        }
    }

    let vertex_color = interpolate_vertex_color(shading.vertex_colors, weights);
    let vertex_alpha = vertex_color.map_or(1.0, |color| color[3].clamp(0.0, 1.0));

    // Coverage from material alpha factor and (for non-opaque modes) texture alpha.
    let texel_alpha = diffuse_sample.map_or(1.0, |rgba| f32::from(rgba[3]) / 255.0);
    let alpha = match shading.alpha_mode {
        AlphaMode::Opaque => 1.0,
        AlphaMode::Mask => {
            if shading.base_alpha * texel_alpha * vertex_alpha < shading.alpha_cutoff {
                return None;
            }
            1.0
        }
        AlphaMode::Blend => shading.base_alpha * texel_alpha * vertex_alpha,
    };
    if alpha <= 0.003 {
        return None;
    }

    // Lit base color. Unlit materials (KHR_materials_unlit) ignore scene
    // lighting and show their flat base color, so drive shading at full
    // intensity. Lighting-only mode discards material color by design, so it is
    // left untouched.
    let shade_intensity = if shading.unlit {
        1.0
    } else {
        shading.intensity
    };
    let lit = diffuse_sample.map_or_else(
        || lit_solid_rgb(material, vertex_color, shade_intensity, config),
        |rgba| {
            apply_color_factors(
                texture_rgb(rgba, shade_intensity, config),
                material,
                vertex_color,
            )
        },
    );

    // Emissive contribution keeps authored glowing detail (eye irises) visible even when
    // lighting is dim.
    let emissive = shared_texel_index
        .and_then(|index| shading.emissive_map.and_then(|map| map.get(index / 4)))
        .copied()
        .filter(|_| config.color_brightness == 1.0)
        .unwrap_or_else(|| {
            shading.emissive_factor.map_or([0, 0, 0], |factor| {
                emissive_rgb_from_factor(factor, emissive_sample, config.color_brightness)
            })
        });
    let rgb = add_emissive(lit, emissive);

    let glyph = glyph_for_cell(shading, rgb, config);
    Some(Fragment {
        ch: glyph,
        rgb,
        alpha,
    })
}

#[inline(always)]
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
            config.glyph_for_intensity_with_ascii(value, shading.glyph_ramp_is_ascii)
        }
    }
}

fn lit_solid_rgb(
    material: Option<&Material>,
    vertex_color: Option<[f32; 4]>,
    intensity: f32,
    config: &Mesh3dConfig,
) -> [u8; 3] {
    let mut base = solid_base_rgb(material, intensity, config);
    if matches!(config.color_mode, ColorMode::Lighting) {
        return base;
    }
    if !matches!(config.color_mode, ColorMode::Off) {
        base = apply_vertex_color(base, vertex_color);
    }
    [
        (f32::from(base[0]) * intensity).round() as u8,
        (f32::from(base[1]) * intensity).round() as u8,
        (f32::from(base[2]) * intensity).round() as u8,
    ]
}

fn triangle_uvs(mesh: &Mesh, face: &Face, corners: [usize; 3]) -> Option<[Vec2; 3]> {
    let [a, b, c] = corners.map(|corner| {
        face.tex_coord_indices
            .get(corner)
            .and_then(|idx| idx.and_then(|idx| mesh.tex_coords.get(idx).copied()))
    });
    Some([a?, b?, c?])
}

#[inline(always)]
fn interpolate_uv(uvs: Option<[Vec2; 3]>, weights: [f32; 3]) -> Option<Vec2> {
    let [u0, u1, u2] = uvs?;
    Some(Vec2::new(
        weights[0].mul_add(u0.u, weights[1].mul_add(u1.u, weights[2] * u2.u)),
        weights[0].mul_add(u0.v, weights[1].mul_add(u1.v, weights[2] * u2.v)),
    ))
}

fn triangle_vertex_colors(mesh: &Mesh, face: &Face, corners: [usize; 3]) -> Option<[[f32; 4]; 3]> {
    if mesh.vertex_colors.is_empty() {
        return None;
    }
    let [a, b, c] = corners.map(|corner| {
        face.indices
            .get(corner)
            .and_then(|&idx| mesh.vertex_colors.get(idx).copied())
    });
    Some([a?, b?, c?])
}

#[inline]
fn triangle_vertex_colors_for_indices(mesh: &Mesh, indices: [usize; 3]) -> Option<[[f32; 4]; 3]> {
    if mesh.vertex_colors.is_empty() {
        return None;
    }
    Some([
        *mesh.vertex_colors.get(indices[0])?,
        *mesh.vertex_colors.get(indices[1])?,
        *mesh.vertex_colors.get(indices[2])?,
    ])
}

#[inline(always)]
fn interpolate_vertex_color(colors: Option<[[f32; 4]; 3]>, weights: [f32; 3]) -> Option<[f32; 4]> {
    let [c0, c1, c2] = colors?;
    Some([
        interpolate_channel(c0[0], c1[0], c2[0], weights),
        interpolate_channel(c0[1], c1[1], c2[1], weights),
        interpolate_channel(c0[2], c1[2], c2[2], weights),
        interpolate_channel(c0[3], c1[3], c2[3], weights),
    ])
}

fn interpolate_channel(a: f32, b: f32, c: f32, weights: [f32; 3]) -> f32 {
    weights[0]
        .mul_add(a, weights[1].mul_add(b, weights[2] * c))
        .clamp(0.0, 1.0)
}

fn apply_color_factors(
    rgb: [u8; 3],
    material: Option<&Material>,
    vertex_color: Option<[f32; 4]>,
) -> [u8; 3] {
    apply_vertex_color(apply_material_factor(rgb, material), vertex_color)
}

fn apply_material_factor(rgb: [u8; 3], material: Option<&Material>) -> [u8; 3] {
    material.map_or(rgb, |material| {
        if material.diffuse == [1.0; 3] {
            return rgb;
        }
        [
            scale_channel(rgb[0], material.diffuse[0]),
            scale_channel(rgb[1], material.diffuse[1]),
            scale_channel(rgb[2], material.diffuse[2]),
        ]
    })
}

fn apply_vertex_color(rgb: [u8; 3], vertex_color: Option<[f32; 4]>) -> [u8; 3] {
    vertex_color.map_or(rgb, |color| {
        [
            scale_channel(rgb[0], color[0]),
            scale_channel(rgb[1], color[1]),
            scale_channel(rgb[2], color[2]),
        ]
    })
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "RGB factor scaling clamps into the valid u8 channel range before casting."
)]
fn scale_channel(value: u8, factor: f32) -> u8 {
    (f32::from(value) * factor.clamp(0.0, 1.0))
        .round()
        .clamp(0.0, 255.0) as u8
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

#[inline(always)]
fn sample_texture(texture: &Texture, uv: Vec2, flip_v: bool, config: &Mesh3dConfig) -> [u8; 4] {
    match config.texture_filter {
        TextureFilter::Nearest => texture.sample_nearest(uv, config.texture_wrap, flip_v),
        TextureFilter::Bilinear => sample_bilinear(texture, uv, flip_v, config),
    }
}

#[inline(always)]
fn sample_face_textures(
    shading: &FaceShading<'_>,
    uv: Vec2,
    config: &Mesh3dConfig,
) -> (Option<[u8; 4]>, Option<[u8; 4]>, Option<usize>) {
    if matches!(config.texture_filter, TextureFilter::Nearest) {
        if let (Some(diffuse), Some(emissive)) = (shading.diffuse_texture, shading.emissive_texture)
        {
            if diffuse.width == emissive.width && diffuse.height == emissive.height {
                let index = diffuse.nearest_texel_index(uv, config.texture_wrap, shading.flip_v);
                return (
                    index.map(|index| diffuse.rgba_at(index)),
                    if shading.emissive_map.is_some() && config.color_brightness == 1.0 {
                        None
                    } else {
                        index.map(|index| emissive.rgba_at(index))
                    },
                    index,
                );
            }
        }
    }
    (
        shading
            .diffuse_texture
            .map(|texture| sample_texture(texture, uv, shading.flip_v, config)),
        shading
            .emissive_texture
            .map(|texture| sample_texture(texture, uv, shading.flip_v, config)),
        None,
    )
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
    let x0 = x.floor() as u32;
    let x1 = x.ceil() as u32;
    let y0 = y.floor() as u32;
    let y1 = y.ceil() as u32;
    let tx = x.fract();
    let ty = y.fract();
    let p00 = texture_pixel(texture, x0, y0);
    let p10 = texture_pixel(texture, x1, y0);
    let p01 = texture_pixel(texture, x0, y1);
    let p11 = texture_pixel(texture, x1, y1);
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

fn texture_pixel(texture: &Texture, x: u32, y: u32) -> [u8; 4] {
    let index = (y as usize * texture.width as usize + x as usize) * 4;
    texture
        .rgba
        .get(index..index + 4)
        .map_or([255; 4], |pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
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
        // A quad wound clockwise in screen space has a normal pointing toward the camera
        // (normal.z < 0) because this renderer's camera looks down +Z.
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

    fn render_prepared(mesh: &Mesh, config: &Mesh3dConfig) -> Terminal<TestBackend> {
        let prepared = PreparedMesh::new(mesh);
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_prepared_mesh(
                    &prepared,
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
    fn prepared_opaque_render_matches_direct_render() {
        let mut mesh = quad_mesh();
        let mut material = Material::new("opaque");
        material.diffuse_texture = Some(TextureRef {
            path: "opaque.png".into(),
            index: Some(0),
        });
        mesh.materials.push(material);
        mesh.faces[0].material = Some("opaque".into());
        mesh.textures.push(Texture::new(
            "opaque.png",
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        ));
        let config = Mesh3dConfig::default()
            .backface_culling(false)
            .color_mode(ColorMode::Texture)
            .texture_filter(TextureFilter::Nearest);

        let direct = render(&mesh, &config);
        let prepared = render_prepared(&mesh, &config);
        assert_eq!(
            direct.backend().buffer().content(),
            prepared.backend().buffer().content()
        );
    }

    #[test]
    fn vertex_colors_modulate_material_color() {
        let mut face = Face::new(vec![0, 1, 2]);
        face.material = Some("pink".into());
        let mut material = Material::new("pink");
        material.diffuse = [1.0, 0.4, 0.7];
        let mut mesh = Mesh::new(
            "tri",
            vec![
                Vec3::new(-0.8, -0.8, 0.0),
                Vec3::new(0.8, -0.8, 0.0),
                Vec3::new(0.0, 0.8, 0.0),
            ],
            vec![face],
            vec![material],
        )
        .unwrap();
        mesh.vertex_colors = vec![[0.0, 0.0, 0.0, 1.0]; 3];

        let terminal = render(
            &mesh,
            &Mesh3dConfig::default()
                .backface_culling(false)
                .color_mode(ColorMode::Material)
                .lighting(1.0, 0.0),
        );
        assert!(terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.fg == Color::Rgb(0, 0, 0)));
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
    fn close_opaque_decal_overwrites_base_color() {
        let mut body = Face::new(vec![0, 1, 2, 3]);
        body.material = Some("body".into());
        let mut face = Face::new(vec![4, 5, 6, 7]);
        face.material = Some("face".into());
        let mut body_material = Material::new("body");
        body_material.diffuse = [1.0, 0.75, 0.69];
        let mut face_material = Material::new("face");
        face_material.diffuse = [0.0, 0.15, 0.19];
        let mesh = Mesh::new(
            "layered",
            vec![
                Vec3::new(-0.8, -0.8, 0.0),
                Vec3::new(-0.8, 0.8, 0.0),
                Vec3::new(0.8, 0.8, 0.0),
                Vec3::new(0.8, -0.8, 0.0),
                Vec3::new(-0.3, -0.3, -0.01),
                Vec3::new(-0.3, 0.3, -0.01),
                Vec3::new(0.3, 0.3, -0.01),
                Vec3::new(0.3, -0.3, -0.01),
            ],
            vec![body, face],
            vec![body_material, face_material],
        )
        .unwrap();

        let terminal = render(
            &mesh,
            &Mesh3dConfig::default()
                .backface_culling(false)
                .color_mode(ColorMode::Material)
                .lighting(1.0, 0.0)
                .normalize(false),
        );

        assert!(terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.fg == Color::Rgb(0, 38, 48)));
    }

    #[test]
    fn backface_culling_keeps_front_faces_and_discards_back_faces() {
        let mut mesh = quad_mesh();
        mesh.materials.push(Material::new("front"));
        mesh.faces[0].material = Some("front".into());

        let config = Mesh3dConfig::default()
            .backface_culling(true)
            .color_mode(ColorMode::Material);
        assert!(
            painted(&render(&mesh, &config)),
            "one-sided front face should render"
        );

        mesh.faces[0].indices.reverse();
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
