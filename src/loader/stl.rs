use std::{fs, path::Path};

use crate::{
    model::{Face, Mesh, Vec3},
    Error, Result,
};

/// Load an ASCII or binary STL mesh.
///
/// # Errors
///
/// Returns an error when the file cannot be read or the STL data is malformed.
pub fn load_stl(path: &Path) -> Result<Mesh> {
    let bytes = fs::read(path).map_err(|err| Error::io(path, err))?;
    parse_stl(path, &bytes)
}

/// Parse STL bytes, auto-detecting binary vs ASCII.
///
/// # Errors
///
/// Returns an error when the bytes are neither valid binary STL nor valid ASCII STL.
pub fn parse_stl(path: &Path, bytes: &[u8]) -> Result<Mesh> {
    if looks_like_binary_stl(bytes) {
        parse_binary_stl(path, bytes)
    } else {
        let text = std::str::from_utf8(bytes).map_err(|_| {
            Error::parse(
                path,
                None,
                "STL is neither valid UTF-8 ASCII nor valid binary",
            )
        })?;
        parse_ascii_stl(path, text)
    }
}

fn looks_like_binary_stl(bytes: &[u8]) -> bool {
    if bytes.len() < 84 {
        return false;
    }
    let tri_count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    84usize.saturating_add(tri_count.saturating_mul(50)) == bytes.len()
}

/// Parse binary STL.
///
/// # Errors
///
/// Returns an error when the binary header or triangle data is truncated or invalid.
pub fn parse_binary_stl(path: &Path, bytes: &[u8]) -> Result<Mesh> {
    if bytes.len() < 84 {
        return Err(Error::InvalidBinaryStl {
            path: path.to_path_buf(),
            message: "file is shorter than STL header".into(),
        });
    }
    let tri_count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    let expected = 84usize.saturating_add(tri_count.saturating_mul(50));
    if bytes.len() < expected {
        return Err(Error::InvalidBinaryStl {
            path: path.to_path_buf(),
            message: "triangle data is truncated".into(),
        });
    }
    let mut vertices = Vec::with_capacity(tri_count * 3);
    let mut faces = Vec::with_capacity(tri_count);
    let mut offset = 84;
    for _ in 0..tri_count {
        let normal = read_vec3(bytes, offset);
        offset += 12;
        let start = vertices.len();
        for _ in 0..3 {
            vertices.push(read_vec3(bytes, offset));
            offset += 12;
        }
        offset += 2;
        let mut face = Face::new(vec![start, start + 1, start + 2]);
        face.normal = Some(normal.normalized());
        faces.push(face);
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("stl mesh");
    Mesh::new(name, vertices, faces, vec![])
}

fn read_vec3(bytes: &[u8], offset: usize) -> Vec3 {
    let f = |i| {
        f32::from_le_bytes([
            bytes[offset + i],
            bytes[offset + i + 1],
            bytes[offset + i + 2],
            bytes[offset + i + 3],
        ])
    };
    Vec3::new(f(0), f(4), f(8))
}

/// Parse ASCII STL.
///
/// # Errors
///
/// Returns an error when facet normals or vertices are missing or malformed.
pub fn parse_ascii_stl(path: &Path, text: &str) -> Result<Mesh> {
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    let mut current_normal = None;
    let mut current = Vec::new();

    for (line_index, raw_line) in text.lines().enumerate() {
        let line_number = line_index + 1;
        let line = raw_line.trim();
        let parts = line.split_whitespace().collect::<Vec<_>>();
        match parts.as_slice() {
            ["facet", "normal", x, y, z] => {
                current_normal = Some(
                    Vec3::new(
                        parse_f32(path, line_number, x)?,
                        parse_f32(path, line_number, y)?,
                        parse_f32(path, line_number, z)?,
                    )
                    .normalized(),
                );
            }
            ["vertex", x, y, z] => {
                current.push(Vec3::new(
                    parse_f32(path, line_number, x)?,
                    parse_f32(path, line_number, y)?,
                    parse_f32(path, line_number, z)?,
                ));
            }
            ["endfacet"] => {
                if current.len() != 3 {
                    return Err(Error::parse(
                        path,
                        Some(line_number),
                        "facet must contain exactly three vertices",
                    ));
                }
                let start = vertices.len();
                vertices.append(&mut current);
                let mut face = Face::new(vec![start, start + 1, start + 2]);
                face.normal = current_normal.take();
                faces.push(face);
            }
            _ => {}
        }
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("stl mesh");
    Mesh::new(name, vertices, faces, vec![])
}

fn parse_f32(path: &Path, line: usize, text: &str) -> Result<f32> {
    text.parse::<f32>()
        .map_err(|_| Error::parse(path, Some(line), "expected a float"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_ascii_stl() {
        let mesh = parse_ascii_stl(Path::new("inline.stl"), "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n").unwrap();
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.faces.len(), 1);
        assert_eq!(mesh.faces[0].normal, Some(Vec3::new(0.0, 0.0, 1.0)));
        assert!(mesh.animations.is_empty());
        assert!(mesh.animation_nodes.is_empty());
    }
}
