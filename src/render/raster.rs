use ratatui::{buffer::Buffer, layout::Rect, style::Style};

use super::camera::ProjectedVertex;

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

/// Fill a projected triangle with barycentric rasterization.
pub fn fill_triangle(
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    tri: [ProjectedVertex; 3],
    ch: char,
    style: Style,
) {
    fill_triangle_with(area, buf, zbuf, tri, |_, _| (ch, style));
}

/// Fill a projected triangle and choose the glyph/style per covered cell.
pub fn fill_triangle_with(
    area: Rect,
    buf: &mut Buffer,
    zbuf: &mut [f32],
    tri: [ProjectedVertex; 3],
    mut paint: impl FnMut([f32; 3], f32) -> (char, Style),
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
        let (ch, style) = paint([1.0, 0.0, 0.0], a.depth);
        draw_line(area, buf, zbuf, a, b, ch, style);
        draw_line(area, buf, zbuf, b, c, ch, style);
        draw_line(area, buf, zbuf, c, a, ch, style);
        return;
    }

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge(b.x, b.y, c.x, c.y, px, py) / denom;
            let w1 = edge(c.x, c.y, a.x, a.y, px, py) / denom;
            let w2 = edge(a.x, a.y, b.x, b.y, px, py) / denom;
            if w0 >= -0.0001 && w1 >= -0.0001 && w2 >= -0.0001 {
                let depth = w0.mul_add(a.depth, w1.mul_add(b.depth, w2 * c.depth));
                let (ch, style) = paint([w0, w1, w2], depth);
                plot_i32(area, buf, zbuf, CellPoint { x, y, depth }, ch, style);
            }
        }
    }
}

fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}
