use std::time::{Duration, Instant};

/// Detailed measurements collected by [`super::render_mesh_profiled`].
///
/// Timings cover mutually exclusive top-level phases. Counters describe how much work
/// reached each part of the software rasterizer, which makes it possible to distinguish
/// expensive geometry from overdraw, depth rejection, shading, and buffer writes.
#[derive(Debug, Clone, Default)]
pub struct RenderProfile {
    pub total: Duration,
    pub animation: Duration,
    pub projection: Duration,
    pub depth_buffer: Duration,
    pub face_rendering: Duration,
    pub faces_considered: u64,
    pub blend_faces: u64,
    pub triangles_considered: u64,
    pub triangles_culled: u64,
    pub triangles_degenerate_or_offscreen: u64,
    pub raster_bbox_cells: u64,
    pub raster_visited_cells: u64,
    pub raster_inside_cells: u64,
    pub depth_rejected_cells: u64,
    pub shade_calls: u64,
    pub shade_discarded_cells: u64,
    pub coplanar_rejected_cells: u64,
    pub cells_written: u64,
}

pub(crate) trait Metrics {
    type Stamp;

    fn start(&mut self) -> Self::Stamp;
    fn finish_animation(&mut self, stamp: Self::Stamp);
    fn finish_projection(&mut self, stamp: Self::Stamp);
    fn finish_depth_buffer(&mut self, stamp: Self::Stamp);
    fn finish_face_rendering(&mut self, stamp: Self::Stamp);

    fn face(&mut self);
    fn blend_face(&mut self);
    fn triangle(&mut self);
    fn culled_triangle(&mut self);
    fn skipped_triangle(&mut self);
    fn raster_bbox(&mut self, cells: u64);
    fn raster_visited(&mut self, cells: u64);
    fn raster_inside(&mut self);
    fn depth_rejected(&mut self);
    fn shade_call(&mut self);
    fn shade_discarded(&mut self);
    fn coplanar_rejected(&mut self);
    fn cell_written(&mut self);
}

pub(crate) struct NoopMetrics;

impl Metrics for NoopMetrics {
    type Stamp = ();

    #[inline(always)]
    fn start(&mut self) {}
    #[inline(always)]
    fn finish_animation(&mut self, (): ()) {}
    #[inline(always)]
    fn finish_projection(&mut self, (): ()) {}
    #[inline(always)]
    fn finish_depth_buffer(&mut self, (): ()) {}
    #[inline(always)]
    fn finish_face_rendering(&mut self, (): ()) {}
    #[inline(always)]
    fn face(&mut self) {}
    #[inline(always)]
    fn blend_face(&mut self) {}
    #[inline(always)]
    fn triangle(&mut self) {}
    #[inline(always)]
    fn culled_triangle(&mut self) {}
    #[inline(always)]
    fn skipped_triangle(&mut self) {}
    #[inline(always)]
    fn raster_bbox(&mut self, _cells: u64) {}
    #[inline(always)]
    fn raster_visited(&mut self, _cells: u64) {}
    #[inline(always)]
    fn raster_inside(&mut self) {}
    #[inline(always)]
    fn depth_rejected(&mut self) {}
    #[inline(always)]
    fn shade_call(&mut self) {}
    #[inline(always)]
    fn shade_discarded(&mut self) {}
    #[inline(always)]
    fn coplanar_rejected(&mut self) {}
    #[inline(always)]
    fn cell_written(&mut self) {}
}

impl Metrics for RenderProfile {
    type Stamp = Instant;

    #[inline]
    fn start(&mut self) -> Instant {
        Instant::now()
    }

    #[inline]
    fn finish_animation(&mut self, stamp: Instant) {
        self.animation += stamp.elapsed();
    }

    #[inline]
    fn finish_projection(&mut self, stamp: Instant) {
        self.projection += stamp.elapsed();
    }

    #[inline]
    fn finish_depth_buffer(&mut self, stamp: Instant) {
        self.depth_buffer += stamp.elapsed();
    }

    #[inline]
    fn finish_face_rendering(&mut self, stamp: Instant) {
        self.face_rendering += stamp.elapsed();
    }

    #[inline(always)]
    fn face(&mut self) {
        self.faces_considered += 1;
    }

    #[inline(always)]
    fn blend_face(&mut self) {
        self.blend_faces += 1;
    }

    #[inline(always)]
    fn triangle(&mut self) {
        self.triangles_considered += 1;
    }

    #[inline(always)]
    fn culled_triangle(&mut self) {
        self.triangles_culled += 1;
    }

    #[inline(always)]
    fn skipped_triangle(&mut self) {
        self.triangles_degenerate_or_offscreen += 1;
    }

    #[inline(always)]
    fn raster_bbox(&mut self, cells: u64) {
        self.raster_bbox_cells += cells;
    }

    #[inline(always)]
    fn raster_visited(&mut self, cells: u64) {
        self.raster_visited_cells += cells;
    }

    #[inline(always)]
    fn raster_inside(&mut self) {
        self.raster_inside_cells += 1;
    }

    #[inline(always)]
    fn depth_rejected(&mut self) {
        self.depth_rejected_cells += 1;
    }

    #[inline(always)]
    fn shade_call(&mut self) {
        self.shade_calls += 1;
    }

    #[inline(always)]
    fn shade_discarded(&mut self) {
        self.shade_discarded_cells += 1;
    }

    #[inline(always)]
    fn coplanar_rejected(&mut self) {
        self.coplanar_rejected_cells += 1;
    }

    #[inline(always)]
    fn cell_written(&mut self) {
        self.cells_written += 1;
    }
}
