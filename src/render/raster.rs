use ratatui::{buffer::Buffer, layout::Rect, style::Style};

use super::camera::ProjectedVertex;

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
    mut paint: impl FnMut([f32; 3], f32) -> Option<Fragment>,
) {
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
    if denom.abs() <= f32::EPSILON {
        return;
    }

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge(b.x, b.y, c.x, c.y, px, py) / denom;
            let w1 = edge(c.x, c.y, a.x, a.y, px, py) / denom;
            let w2 = edge(a.x, a.y, b.x, b.y, px, py) / denom;
            if w0 < -0.0001 || w1 < -0.0001 || w2 < -0.0001 {
                continue;
            }
            let depth = w0.mul_add(a.depth, w1.mul_add(b.depth, w2 * c.depth));
            // Subtracting the bias lets a decal at the same depth as the surface behind it
            // still pass `depth < existing` and paint on top.
            if depth - decal_bias >= depth_at(zbuf, area, x, y) {
                continue;
            }
            let Some(fragment) = paint([w0, w1, w2], depth) else {
                continue;
            };
            composite(
                area,
                buf,
                zbuf,
                CellPoint { x, y, depth },
                fragment,
                backdrop,
            );
        }
    }
}

fn depth_at(zbuf: &[f32], area: Rect, x: i32, y: i32) -> f32 {
    if x < 0 || y < 0 || x >= i32::from(area.width) || y >= i32::from(area.height) {
        return f32::NEG_INFINITY;
    }
    let idx = (y as usize) * usize::from(area.width) + x as usize;
    zbuf.get(idx).copied().unwrap_or(f32::NEG_INFINITY)
}

fn composite(
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    point: CellPoint,
    fragment: Fragment,
    backdrop: [u8; 3],
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
    let alpha = fragment.alpha.clamp(0.0, 1.0);
    let cell = &mut buf[(area.x + ux, area.y + uy)];
    if alpha >= 0.996 {
        zbuf[idx] = point.depth;
        cell.set_char(fragment.ch);
        cell.set_fg(ratatui::style::Color::Rgb(
            fragment.rgb[0],
            fragment.rgb[1],
            fragment.rgb[2],
        ));
        return;
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
    zbuf[idx] = point.depth;
    cell.set_char(fragment.ch);
    cell.set_fg(ratatui::style::Color::Rgb(
        blended[0], blended[1], blended[2],
    ));
}

fn blend_channel(src: u8, dst: u8, alpha: f32) -> u8 {
    (f32::from(src) * alpha + f32::from(dst) * (1.0 - alpha))
        .round()
        .clamp(0.0, 255.0) as u8
}

fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}
