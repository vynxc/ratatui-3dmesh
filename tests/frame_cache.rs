#![cfg(feature = "gltf")]

use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use ratatui_3dmesh::{
    render::{render_prepared_mesh, PreparedMesh},
    ColorMode, FrameCacheConfig, Mesh, Mesh3dConfig, Mesh3dState, Vec3,
};

fn animated_box() -> Mesh {
    Mesh::load("examples/assets/gltf/box_animated.glb").expect("load animated fixture")
}

fn render_at(
    prepared: &PreparedMesh<'_>,
    area: Rect,
    state: &Mesh3dState,
    config: &Mesh3dConfig,
) -> Buffer {
    let mut buf = Buffer::empty(area);
    render_prepared_mesh(prepared, area, &mut buf, state, config);
    buf
}

#[test]
fn replay_is_cell_exact_and_records_a_hit() {
    let mesh = animated_box();
    let prepared =
        PreparedMesh::new(&mesh).with_frame_cache(FrameCacheConfig::memory(15).max_bytes(8 << 20));
    let area = Rect::new(0, 0, 80, 30);
    let config = Mesh3dConfig::default()
        .color_mode(ColorMode::Auto)
        .show_hints(false);
    let mut state = Mesh3dState {
        animation_time_seconds: 0.4,
        ..Mesh3dState::default()
    };

    let warm = render_at(&prepared, area, &state, &config);
    let duration = mesh.animations[0].duration_seconds;
    state.animation_time_seconds += duration;
    let replay = render_at(&prepared, area, &state, &config);

    assert_eq!(warm, replay);
    let stats = prepared.frame_cache_stats().expect("cache enabled");
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.cached_frames, 1);
}

#[test]
fn viewport_state_config_and_clip_changes_invalidate() {
    let mut mesh = animated_box();
    mesh.animations.push(mesh.animations[0].clone());
    let prepared = PreparedMesh::new(&mesh).with_frame_cache(FrameCacheConfig::memory(15));
    let config = Mesh3dConfig::default()
        .color_mode(ColorMode::Auto)
        .show_hints(false);
    let mut state = Mesh3dState::default();

    let _ = render_at(&prepared, Rect::new(0, 0, 40, 20), &state, &config);
    let _ = render_at(&prepared, Rect::new(0, 0, 41, 20), &state, &config);
    state.rotation += Vec3::new(0.1, 0.0, 0.0);
    let _ = render_at(&prepared, Rect::new(0, 0, 41, 20), &state, &config);
    let relit = config.clone().lighting(0.3, 0.7);
    let _ = render_at(&prepared, Rect::new(0, 0, 41, 20), &state, &relit);
    state.selected_animation = Some(1);
    let _ = render_at(&prepared, Rect::new(0, 0, 41, 20), &state, &relit);

    let stats = prepared.frame_cache_stats().expect("cache enabled");
    assert_eq!(stats.invalidations, 4);
    assert_eq!(stats.hits, 0);
}

#[test]
fn replay_preserves_unpainted_baseline_cells() {
    let mesh = animated_box();
    let prepared = PreparedMesh::new(&mesh).with_frame_cache(FrameCacheConfig::memory(15));
    let area = Rect::new(0, 0, 80, 30);
    let config = Mesh3dConfig::default()
        .color_mode(ColorMode::Auto)
        .show_hints(false);
    let state = Mesh3dState::default();

    let _ = render_at(&prepared, area, &state, &config);
    let mut baseline = Buffer::empty(area);
    baseline[(0, 0)].set_char('X').set_fg(Color::Red);
    render_prepared_mesh(&prepared, area, &mut baseline, &state, &config);

    assert_eq!(baseline[(0, 0)].symbol(), "X");
    assert_eq!(baseline[(0, 0)].fg, Color::Red);
    assert_eq!(prepared.frame_cache_stats().expect("cache enabled").hits, 1);
}

#[test]
fn final_half_frame_does_not_replay_the_first_frame_early() {
    let mesh = animated_box();
    let fps = 15;
    let prepared = PreparedMesh::new(&mesh).with_frame_cache(FrameCacheConfig::memory(fps));
    let area = Rect::new(0, 0, 80, 30);
    let config = Mesh3dConfig::default()
        .color_mode(ColorMode::Auto)
        .show_hints(false);
    let mut state = Mesh3dState::default();

    let _ = render_at(&prepared, area, &state, &config);
    let duration = mesh.animations[0].duration_seconds;
    let frame_count = (duration * f32::from(fps)).round();
    state.animation_time_seconds = duration * ((frame_count - 0.25) / frame_count);
    let _ = render_at(&prepared, area, &state, &config);

    let stats = prepared.frame_cache_stats().expect("cache enabled");
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.cached_frames, 2);
}

#[test]
fn auto_spin_bypasses_frame_cache() {
    let mesh = animated_box();
    let prepared = PreparedMesh::new(&mesh).with_frame_cache(FrameCacheConfig::memory(15));
    let area = Rect::new(0, 0, 80, 30);
    let config = Mesh3dConfig::default()
        .color_mode(ColorMode::Auto)
        .show_hints(false);
    let mut state = Mesh3dState {
        auto_spin_enabled: true,
        ..Mesh3dState::default()
    };

    let _ = render_at(&prepared, area, &state, &config);
    state.rotation += Vec3::new(0.1, 0.1, 0.0);
    let _ = render_at(&prepared, area, &state, &config);
    let bypassed = prepared.frame_cache_stats().expect("cache enabled");
    assert_eq!(bypassed.hits, 0);
    assert_eq!(bypassed.misses, 0);
    assert_eq!(bypassed.cached_frames, 0);

    state.auto_spin_enabled = false;
    let _ = render_at(&prepared, area, &state, &config);
    assert_eq!(
        prepared.frame_cache_stats().expect("cache enabled").misses,
        1
    );
}

#[test]
fn memory_ceiling_suspends_cache_without_changing_output() {
    let mesh = animated_box();
    let cached =
        PreparedMesh::new(&mesh).with_frame_cache(FrameCacheConfig::memory(15).max_bytes(1));
    let direct = PreparedMesh::new(&mesh);
    let area = Rect::new(0, 0, 80, 30);
    let config = Mesh3dConfig::default()
        .color_mode(ColorMode::Auto)
        .show_hints(false);
    let state = Mesh3dState::default();

    let cached_output = render_at(&cached, area, &state, &config);
    let direct_output = render_at(&direct, area, &state, &config);

    assert_eq!(cached_output, direct_output);
    assert!(
        cached
            .frame_cache_stats()
            .expect("cache enabled")
            .memory_limit_reached
    );
}
