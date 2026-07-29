use ratatui::{buffer::Buffer, layout::Rect, style::Style};

use super::camera::ProjectedVertex;
use super::metrics::{Metrics, NoopMetrics};

/// Treat opaque fragments this close in depth as the same surface. This is only a
/// floating-point tolerance; authored close-surface decals still need to overwrite.
const OPAQUE_COPLANAR_EPSILON: f32 = 0.001;

/// A shaded fragment produced by the solid rasterizer's paint closure.
#[derive(Debug, Clone, Copy)]
pub struct Fragment {
    /// Glyph to draw.
    pub ch: char,
    /// Final RGB color before any blending.
    pub rgb: [u8; 3],
    /// Coverage/opacity in `[0, 1]`. `1.0` is fully opaque.
    pub alpha: f32,
}

/// Plot a projected point with z-buffering.
pub fn plot(
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    p: ProjectedVertex,
    ch: char,
    style: Style,
) {
    let x = p.x.round() as i32;
    let y = p.y.round() as i32;
    plot_i32(
        area,
        buf,
        zbuf,
        CellPoint {
            x,
            y,
            depth: p.depth,
        },
        ch,
        style,
    );
}

#[derive(Debug, Clone, Copy)]
struct CellPoint {
    x: i32,
    y: i32,
    depth: f32,
}

fn plot_i32(
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    point: CellPoint,
    ch: char,
    style: Style,
) {
    if point.x < 0
        || point.y < 0
        || point.x >= i32::from(area.width)
        || point.y >= i32::from(area.height)
    {
        return;
    }
    let ux = point.x as u16;
    let uy = point.y as u16;
    let idx = usize::from(uy) * usize::from(area.width) + usize::from(ux);
    if point.depth < zbuf[idx] {
        zbuf[idx] = point.depth;
        let cell = &mut buf[(area.x + ux, area.y + uy)];
        cell.set_char(ch);
        cell.set_style(style);
    }
}

/// Draw a z-buffered line.
pub fn draw_line(
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    a: ProjectedVertex,
    b: ProjectedVertex,
    ch: char,
    style: Style,
) {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as i32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = (a.x + dx * t).round() as i32;
        let y = (a.y + dy * t).round() as i32;
        let depth = a.depth + (b.depth - a.depth) * t;
        plot_i32(area, buf, zbuf, CellPoint { x, y, depth }, ch, style);
    }
}

/// Fill a projected triangle, shading and compositing each covered cell.
///
/// The `paint` closure returns a [`Fragment`] with a final color and a coverage value.
/// Fully opaque fragments overwrite the cell and write the depth buffer. Partially
/// transparent fragments are alpha-composited over the existing cell color.
///
/// `decal_bias` nudges the depth test so coincident translucent surfaces (decals such as
/// glTF eye irises sitting exactly on an opaque eyeball) pass the test and draw on top
/// instead of z-fighting away. Pass `0.0` for the opaque pass and a small positive value
/// (a few thousandths of a unit) for the blend pass.
pub fn fill_triangle_shaded(
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    tri: [ProjectedVertex; 3],
    backdrop: [u8; 3],
    decal_bias: f32,
    paint: impl FnMut([f32; 3], f32) -> Option<Fragment>,
) {
    fill_triangle_shaded_profiled(
        area,
        buf,
        zbuf,
        tri,
        backdrop,
        decal_bias,
        &mut NoopMetrics,
        paint,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn fill_triangle_shaded_profiled<M: Metrics>(
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    tri: [ProjectedVertex; 3],
    backdrop: [u8; 3],
    decal_bias: f32,
    metrics: &mut M,
    paint: impl FnMut([f32; 3], f32) -> Option<Fragment>,
) {
    let Some(setup) = setup_triangle(area, tri) else {
        metrics.skipped_triangle();
        return;
    };
    fill_triangle_shaded_with_setup(
        area, buf, zbuf, tri, setup, backdrop, decal_bias, metrics, paint,
    );
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TriangleSetup {
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
    step_x: [f32; 3],
    step_y: [f32; 3],
    row_weights: [f32; 3],
}

#[inline]
pub(crate) fn setup_triangle(area: Rect, tri: [ProjectedVertex; 3]) -> Option<TriangleSetup> {
    let [a, b, c] = tri;
    let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as i32;
    let max_x =
        a.x.max(b.x)
            .max(c.x)
            .ceil()
            .min(f32::from(area.width.saturating_sub(1))) as i32;
    let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as i32;
    let max_y =
        a.y.max(b.y)
            .max(c.y)
            .ceil()
            .min(f32::from(area.height.saturating_sub(1))) as i32;
    let denom = edge(a.x, a.y, b.x, b.y, c.x, c.y);
    if denom.abs() <= f32::EPSILON || min_x > max_x || min_y > max_y {
        return None;
    }
    let inverse_denom = denom.recip();
    let step_x = [
        (c.y - b.y) * inverse_denom,
        (a.y - c.y) * inverse_denom,
        (b.y - a.y) * inverse_denom,
    ];
    let step_y = [
        (b.x - c.x) * inverse_denom,
        (c.x - a.x) * inverse_denom,
        (a.x - b.x) * inverse_denom,
    ];
    let first_x = min_x as f32 + 0.5;
    let first_y = min_y as f32 + 0.5;
    let row_weights = [
        edge(b.x, b.y, c.x, c.y, first_x, first_y) * inverse_denom,
        edge(c.x, c.y, a.x, a.y, first_x, first_y) * inverse_denom,
        edge(a.x, a.y, b.x, b.y, first_x, first_y) * inverse_denom,
    ];
    Some(TriangleSetup {
        min_x,
        max_x,
        min_y,
        max_y,
        step_x,
        step_y,
        row_weights,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn fill_triangle_shaded_with_setup<M: Metrics>(
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    tri: [ProjectedVertex; 3],
    setup: TriangleSetup,
    backdrop: [u8; 3],
    decal_bias: f32,
    metrics: &mut M,
    mut paint: impl FnMut([f32; 3], f32) -> Option<Fragment>,
) {
    let [a, b, c] = tri;
    let TriangleSetup {
        min_x,
        max_x,
        min_y,
        max_y,
        step_x,
        step_y,
        mut row_weights,
    } = setup;
    let bbox_cells = (max_x - min_x + 1) as u64 * (max_y - min_y + 1) as u64;
    metrics.raster_bbox(bbox_cells);
    metrics.raster_visited(bbox_cells);
    let row_width = usize::from(area.width);

    for y in min_y..=max_y {
        let mut weights = row_weights;
        let mut idx = y as usize * row_width + min_x as usize;
        for x in min_x..=max_x {
            let [w0, w1, w2] = weights;
            if w0 < -0.0001 || w1 < -0.0001 || w2 < -0.0001 {
                weights[0] += step_x[0];
                weights[1] += step_x[1];
                weights[2] += step_x[2];
                idx += 1;
                continue;
            }
            metrics.raster_inside();
            let depth = w0.mul_add(a.depth, w1.mul_add(b.depth, w2 * c.depth));
            if depth - decal_bias >= zbuf[idx] {
                metrics.depth_rejected();
                weights[0] += step_x[0];
                weights[1] += step_x[1];
                weights[2] += step_x[2];
                idx += 1;
                continue;
            }
            metrics.shade_call();
            let Some(fragment) = paint([w0, w1, w2], depth) else {
                metrics.shade_discarded();
                weights[0] += step_x[0];
                weights[1] += step_x[1];
                weights[2] += step_x[2];
                idx += 1;
                continue;
            };
            if composite_indexed(
                area, buf, zbuf, x as u16, y as u16, idx, depth, fragment, backdrop,
            ) {
                metrics.cell_written();
            } else {
                metrics.coplanar_rejected();
            }
            weights[0] += step_x[0];
            weights[1] += step_x[1];
            weights[2] += step_x[2];
            idx += 1;
        }
        row_weights[0] += step_y[0];
        row_weights[1] += step_y[1];
        row_weights[2] += step_y[2];
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::explicit_counter_loop)]
pub(crate) fn fill_triangle_deferred_profiled<M: Metrics>(
    area: Rect,
    zbuf: &mut [f32],
    occupied: &mut [bool],
    tri: [ProjectedVertex; 3],
    setup: TriangleSetup,
    decal_bias: f32,
    metrics: &mut M,
    mut store: impl FnMut(usize, [f32; 3]),
) {
    let [a, b, c] = tri;
    let TriangleSetup {
        min_x,
        max_x,
        min_y,
        max_y,
        step_x,
        step_y,
        mut row_weights,
    } = setup;
    let bbox_cells = (max_x - min_x + 1) as u64 * (max_y - min_y + 1) as u64;
    metrics.raster_bbox(bbox_cells);
    metrics.raster_visited(bbox_cells);
    let row_width = usize::from(area.width);

    for y in min_y..=max_y {
        let mut weights = row_weights;
        let mut idx = y as usize * row_width + min_x as usize;
        let mut entered = false;
        for _ in min_x..=max_x {
            let [w0, w1, w2] = weights;
            if w0 >= -0.0001 && w1 >= -0.0001 && w2 >= -0.0001 {
                entered = true;
                metrics.raster_inside();
                let depth = w0.mul_add(a.depth, w1.mul_add(b.depth, w2 * c.depth));
                if depth - decal_bias >= zbuf[idx] {
                    metrics.depth_rejected();
                } else if occupied[idx] && depth >= zbuf[idx] - OPAQUE_COPLANAR_EPSILON {
                    metrics.coplanar_rejected();
                } else {
                    zbuf[idx] = depth;
                    occupied[idx] = true;
                    store(idx, weights);
                }
            } else if entered {
                break;
            }
            weights[0] += step_x[0];
            weights[1] += step_x[1];
            weights[2] += step_x[2];
            idx += 1;
        }
        row_weights[0] += step_y[0];
        row_weights[1] += step_y[1];
        row_weights[2] += step_y[2];
    }
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn composite_indexed(
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    x: u16,
    y: u16,
    idx: usize,
    depth: f32,
    fragment: Fragment,
    backdrop: [u8; 3],
) -> bool {
    let alpha = fragment.alpha.clamp(0.0, 1.0);
    let cell = &mut buf[(area.x + x, area.y + y)];
    if alpha >= 0.996 {
        if depth >= zbuf[idx] - OPAQUE_COPLANAR_EPSILON && cell.symbol() != " " {
            return false;
        }
        zbuf[idx] = depth;
        cell.set_char(fragment.ch);
        cell.set_fg(ratatui::style::Color::Rgb(
            fragment.rgb[0],
            fragment.rgb[1],
            fragment.rgb[2],
        ));
        return true;
    }
    // Transparent: blend over whatever currently occupies the cell, then advance the depth
    // buffer to this fragment. Because blend faces are drawn back-to-front, writing depth
    // lets a nearer translucent layer correctly sit on top of farther ones (and stops the
    // back side of a double-sided surface from painting over its own front).
    let dst = match cell.fg {
        ratatui::style::Color::Rgb(r, g, b) => [r, g, b],
        _ => backdrop,
    };
    let blended = [
        blend_channel(fragment.rgb[0], dst[0], alpha),
        blend_channel(fragment.rgb[1], dst[1], alpha),
        blend_channel(fragment.rgb[2], dst[2], alpha),
    ];
    zbuf[idx] = depth;
    cell.set_char(fragment.ch);
    cell.set_fg(ratatui::style::Color::Rgb(
        blended[0], blended[1], blended[2],
    ));
    true
}

fn blend_channel(src: u8, dst: u8, alpha: f32) -> u8 {
    (f32::from(src) * alpha + f32::from(dst) * (1.0 - alpha))
        .round()
        .clamp(0.0, 255.0) as u8
}

fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect, style::Color};

    use super::*;
    use crate::model::Vec3;

    fn vertex(x: f32, y: f32, depth: f32) -> ProjectedVertex {
        ProjectedVertex {
            x,
            y,
            depth,
            view: Vec3::new(x, y, depth),
        }
    }

    #[test]
    fn effectively_coplanar_opaque_fragments_keep_existing_detail() {
        let area = Rect::new(0, 0, 5, 5);
        let mut buf = Buffer::empty(area);
        let mut zbuf = vec![f32::INFINITY; usize::from(area.width) * usize::from(area.height)];
        let tri = [
            vertex(1.0, 1.0, 1.0),
            vertex(4.0, 1.0, 1.0),
            vertex(1.0, 4.0, 1.0),
        ];

        fill_triangle_shaded(area, &mut buf, &mut zbuf, tri, [0; 3], 0.0, |_, _| {
            Some(Fragment {
                ch: '*',
                rgb: [255, 255, 255],
                alpha: 1.0,
            })
        });
        let nearer = [
            vertex(1.0, 1.0, 0.9995),
            vertex(4.0, 1.0, 0.9995),
            vertex(1.0, 4.0, 0.9995),
        ];
        fill_triangle_shaded(area, &mut buf, &mut zbuf, nearer, [0; 3], 0.0, |_, _| {
            Some(Fragment {
                ch: '-',
                rgb: [255, 0, 0],
                alpha: 1.0,
            })
        });

        assert_eq!(buf[(2, 2)].symbol(), "*");
        assert_eq!(buf[(2, 2)].fg, Color::Rgb(255, 255, 255));
    }

    #[test]
    fn clearly_nearer_opaque_fragments_overwrite_existing_detail() {
        let area = Rect::new(0, 0, 5, 5);
        let mut buf = Buffer::empty(area);
        let mut zbuf = vec![f32::INFINITY; usize::from(area.width) * usize::from(area.height)];
        let tri = [
            vertex(1.0, 1.0, 1.0),
            vertex(4.0, 1.0, 1.0),
            vertex(1.0, 4.0, 1.0),
        ];

        fill_triangle_shaded(area, &mut buf, &mut zbuf, tri, [0; 3], 0.0, |_, _| {
            Some(Fragment {
                ch: '*',
                rgb: [255, 255, 255],
                alpha: 1.0,
            })
        });
        let nearer = [
            vertex(1.0, 1.0, 0.8),
            vertex(4.0, 1.0, 0.8),
            vertex(1.0, 4.0, 0.8),
        ];
        fill_triangle_shaded(area, &mut buf, &mut zbuf, nearer, [0; 3], 0.0, |_, _| {
            Some(Fragment {
                ch: '-',
                rgb: [255, 0, 0],
                alpha: 1.0,
            })
        });

        assert_eq!(buf[(2, 2)].symbol(), "-");
        assert_eq!(buf[(2, 2)].fg, Color::Rgb(255, 0, 0));
    }
}
