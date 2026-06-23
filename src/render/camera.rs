use crate::{config::ProjectionMode, model::Vec3};

/// Projected vertex in normalized/device-ish screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedVertex {
    /// X coordinate in terminal cells.
    pub x: f32,
    /// Y coordinate in terminal cells.
    pub y: f32,
    /// Depth where smaller is closer.
    pub depth: f32,
    /// Rotated world/view position.
    pub view: Vec3,
}

/// Project a transformed point into terminal cell coordinates.
#[must_use]
pub fn project(
    point: Vec3,
    width: u16,
    height: u16,
    projection: ProjectionMode,
    fov_y_degrees: f32,
    cell_aspect_ratio: f32,
    zoom: f32,
) -> ProjectedVertex {
    let w = f32::from(width.max(1));
    let h = f32::from(height.max(1));
    let aspect = (w / h.max(1.0)) * cell_aspect_ratio.max(0.0001);
    let camera_distance = 3.0 / zoom.max(0.0001);
    let z = point.z + camera_distance;

    let (nx, ny) = match projection {
        ProjectionMode::Perspective => {
            let f = 1.0 / (fov_y_degrees.to_radians() * 0.5).tan();
            let safe_z = z.max(0.05);
            ((point.x * f / aspect) / safe_z, (point.y * f) / safe_z)
        }
        ProjectionMode::Orthographic => (point.x * zoom / aspect, point.y * zoom),
    };

    ProjectedVertex {
        x: (nx + 1.0) * 0.5 * (w - 1.0),
        y: (1.0 - (ny + 1.0) * 0.5) * (h - 1.0),
        depth: z,
        view: point,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_origin_to_center() {
        let p = project(
            Vec3::default(),
            11,
            11,
            ProjectionMode::Perspective,
            60.0,
            1.0,
            1.0,
        );
        assert!((p.x - 5.0).abs() < 0.01);
        assert!((p.y - 5.0).abs() < 0.01);
    }
}
