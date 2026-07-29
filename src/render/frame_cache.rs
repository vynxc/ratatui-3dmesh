use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    mem::size_of,
};

use ratatui::{layout::Rect, style::Color};

use crate::{config::Mesh3dConfig, widget::Mesh3dState};

/// Settings for an in-memory cache of rendered animation frames.
///
/// The cache is opt-in on [`super::PreparedMesh`] and samples a looping animation at a
/// fixed frame rate. A cached frame is an exact sparse patch of Ratatui cells, not an
/// image, so replay does not change terminal output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCacheConfig {
    frames_per_second: u16,
    max_bytes: usize,
}

impl FrameCacheConfig {
    /// Configure a memory cache sampled at `frames_per_second`.
    ///
    /// The default memory ceiling is 256 MiB. A zero frame rate is clamped to one.
    #[must_use]
    pub const fn memory(frames_per_second: u16) -> Self {
        Self {
            frames_per_second: if frames_per_second == 0 {
                1
            } else {
                frames_per_second
            },
            max_bytes: 256 * 1024 * 1024,
        }
    }

    /// Set the maximum logical cache size.
    ///
    /// If one animation loop does not fit, caching is suspended until a render setting
    /// changes and invalidates the attempted cache.
    #[must_use]
    pub const fn max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// Return the configured animation sampling rate.
    #[must_use]
    pub const fn frames_per_second(self) -> u16 {
        self.frames_per_second
    }

    /// Return the configured logical memory ceiling.
    #[must_use]
    pub const fn maximum_bytes(self) -> usize {
        self.max_bytes
    }
}

/// Runtime counters for a prepared mesh's optional animation-frame cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameCacheStats {
    /// Frames restored from the cache.
    pub hits: u64,
    /// Frames rendered while populating the cache.
    pub misses: u64,
    /// Cache resets caused by viewport, state, or configuration changes.
    pub invalidations: u64,
    /// Number of cached animation frames.
    pub cached_frames: usize,
    /// Number of frame slots in one animation loop.
    pub total_frames: usize,
    /// Approximate logical bytes retained by frame data and its cell palette.
    pub bytes: usize,
    /// Whether the most recent cache generation exceeded its memory ceiling.
    pub memory_limit_reached: bool,
}

#[derive(Debug)]
pub(super) struct FrameCache {
    config: FrameCacheConfig,
    signature: Option<u64>,
    frames: Vec<Option<CachedFrame>>,
    palette: Vec<Paint>,
    palette_index: HashMap<u64, Vec<u32>>,
    stats: FrameCacheStats,
}

#[derive(Debug)]
struct CachedFrame {
    cells: Vec<CachedCell>,
}

#[derive(Debug, Clone, Copy)]
struct CachedCell {
    offset: u32,
    palette_index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Paint {
    ch: char,
    fg: Color,
}

impl FrameCache {
    pub(super) fn new(config: FrameCacheConfig) -> Self {
        Self {
            config,
            signature: None,
            frames: Vec::new(),
            palette: Vec::new(),
            palette_index: HashMap::new(),
            stats: FrameCacheStats::default(),
        }
    }

    pub(super) fn stats(&self) -> FrameCacheStats {
        let mut stats = self.stats;
        stats.cached_frames = self.frames.iter().flatten().count();
        stats.total_frames = self.frames.len();
        stats.bytes = self.memory_bytes();
        stats
    }

    pub(super) fn clear(&mut self) {
        self.signature = None;
        self.frames.clear();
        self.palette.clear();
        self.palette_index.clear();
        self.stats = FrameCacheStats::default();
    }

    pub(super) fn render(
        &mut self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        state: &Mesh3dState,
        config: &Mesh3dConfig,
        clip_duration: f32,
        render: impl FnOnce(&mut ratatui::buffer::Buffer, &Mesh3dState) -> Vec<u32>,
    ) {
        let frame_count =
            ((clip_duration * f32::from(self.config.frames_per_second)).round() as usize).max(1);
        let displayed = state.animation_display_time(clip_duration);
        let phase = (displayed / clip_duration).clamp(0.0, 1.0);
        // Choose the nearest sample slot so timestamps produced by `frame / fps` do not
        // slip into the preceding slot because of floating-point error. Clamping instead
        // of applying modulo keeps the final half-frame active until the actual wrap.
        let frame_index = ((phase * frame_count as f32).round() as usize).min(frame_count - 1);
        let signature = render_signature(area, state, config, frame_count);

        if self.signature != Some(signature) {
            if self.signature.is_some() {
                self.stats.invalidations += 1;
            }
            self.signature = Some(signature);
            self.frames.clear();
            self.palette.clear();
            self.palette_index.clear();
            self.stats.memory_limit_reached = false;
            let frame_table_bytes = frame_count.saturating_mul(size_of::<Option<CachedFrame>>());
            if frame_table_bytes > self.config.max_bytes {
                self.stats.memory_limit_reached = true;
            } else {
                self.frames.resize_with(frame_count, || None);
            }
        }

        if !self.stats.memory_limit_reached {
            if let Some(frame) = self.frames.get(frame_index).and_then(Option::as_ref) {
                apply_frame(frame, &self.palette, area, buf);
                self.stats.hits += 1;
                return;
            }
        }

        self.stats.misses += 1;
        if self.stats.memory_limit_reached {
            let _ = render(buf, state);
            return;
        }
        // The first observation of a slot is authoritative. This preserves the exact
        // uncached output during warm-up instead of resampling at a nearby ideal time.
        let painted = render(buf, state);

        let mut cells = Vec::with_capacity(painted.len());
        for offset in painted {
            let offset = offset as usize;
            let x = area.x + (offset % usize::from(area.width)) as u16;
            let y = area.y + (offset / usize::from(area.width)) as u16;
            let current = &buf[(x, y)];
            let paint = Paint {
                ch: current.symbol().chars().next().unwrap_or(' '),
                fg: current.fg,
            };
            cells.push(CachedCell {
                offset: offset as u32,
                palette_index: self.intern_paint(paint),
            });
        }
        self.frames[frame_index] = Some(CachedFrame { cells });

        if self.memory_bytes() > self.config.max_bytes {
            self.frames.iter_mut().for_each(|frame| *frame = None);
            self.palette.clear();
            self.palette_index.clear();
            self.stats.memory_limit_reached = true;
        } else if self.frames.iter().all(Option::is_some) {
            // This index only accelerates cache construction. Drop it after one complete
            // loop so steady-state memory contains only replay data.
            self.palette_index.clear();
        }
    }

    fn intern_paint(&mut self, paint: Paint) -> u32 {
        let hash = hash_value(&paint);
        if let Some(index) = self.palette_index.get(&hash).and_then(|indices| {
            indices
                .iter()
                .copied()
                .find(|&index| self.palette[index as usize] == paint)
        }) {
            return index;
        }
        let index = self.palette.len() as u32;
        self.palette.push(paint);
        self.palette_index.entry(hash).or_default().push(index);
        index
    }

    fn memory_bytes(&self) -> usize {
        self.frames.capacity() * size_of::<Option<CachedFrame>>()
            + self
                .frames
                .iter()
                .flatten()
                .map(|frame| frame.cells.capacity() * size_of::<CachedCell>())
                .sum::<usize>()
            + self.palette.capacity() * size_of::<Paint>()
            + self
                .palette_index
                .values()
                .map(|indices| indices.capacity() * size_of::<u32>())
                .sum::<usize>()
    }
}

fn apply_frame(
    frame: &CachedFrame,
    palette: &[Paint],
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    let width = usize::from(area.width);
    for cached in &frame.cells {
        let offset = cached.offset as usize;
        let x = area.x + (offset % width) as u16;
        let y = area.y + (offset / width) as u16;
        let paint = palette[cached.palette_index as usize];
        buf[(x, y)].set_char(paint.ch).set_fg(paint.fg);
    }
}

fn render_signature(
    area: Rect,
    state: &Mesh3dState,
    config: &Mesh3dConfig,
    frame_count: usize,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    area.hash(&mut hasher);
    frame_count.hash(&mut hasher);
    state.rotation.x.to_bits().hash(&mut hasher);
    state.rotation.y.to_bits().hash(&mut hasher);
    state.rotation.z.to_bits().hash(&mut hasher);
    state.pan.x.to_bits().hash(&mut hasher);
    state.pan.y.to_bits().hash(&mut hasher);
    state.pan.z.to_bits().hash(&mut hasher);
    state.zoom.to_bits().hash(&mut hasher);
    state.auto_spin_enabled.hash(&mut hasher);
    state.help_visible.hash(&mut hasher);
    state.selected_animation.hash(&mut hasher);
    state.animation_speed.to_bits().hash(&mut hasher);
    state.animation_playing.hash(&mut hasher);
    state.animation_looping.hash(&mut hasher);
    state
        .animation_loop_blend_seconds
        .to_bits()
        .hash(&mut hasher);
    config.glyph_ramp.hash(&mut hasher);
    config.render_mode.hash(&mut hasher);
    config.projection.hash(&mut hasher);
    config.color_mode.hash(&mut hasher);
    config.texture_filter.hash(&mut hasher);
    config.texture_wrap.hash(&mut hasher);
    config.flip_texture_v.hash(&mut hasher);
    config.color_brightness.to_bits().hash(&mut hasher);
    config.texture_lighting.hash(&mut hasher);
    config.auto_fit.hash(&mut hasher);
    config.scale.to_bits().hash(&mut hasher);
    config.fov_y_degrees.to_bits().hash(&mut hasher);
    config.cell_aspect_ratio.to_bits().hash(&mut hasher);
    config.backface_culling.hash(&mut hasher);
    config.normalize.hash(&mut hasher);
    for component in config.light_direction {
        component.to_bits().hash(&mut hasher);
    }
    config.ambient.to_bits().hash(&mut hasher);
    config.diffuse.to_bits().hash(&mut hasher);
    config.show_hints.hash(&mut hasher);
    config.show_help_overlay.hash(&mut hasher);
    for component in config.auto_spin {
        component.to_bits().hash(&mut hasher);
    }
    config.max_faces.hash(&mut hasher);
    config.foreground_style.hash(&mut hasher);
    config.background_style.hash(&mut hasher);
    hasher.finish()
}

fn hash_value(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
