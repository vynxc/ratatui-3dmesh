use std::{fs, path::Path};

use crate::{
    model::{Face, Mesh, Vec3},
    Error, Result,
};

#[cfg(feature = "mtl")]
use super::mtl;

/// Load a Wavefront OBJ mesh.
pub fn load_obj(path: &Path) -> Result<Mesh> {
    let text = fs::read_to_string(path).map_err(|err| Error::io(path, err))?;
    parse_obj(path, &text)
}

/// Parse a Wavefront OBJ mesh from text.
pub fn parse_obj(path: &Path, text: &str) -> Result<Mesh> {
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    let mut current_material: Option<String> = None;
    let mut mtllibs = Vec::new();

    for (line_index, raw_line) in text.lines().enumerate() {
        let line_number = line_index + 1;
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next().unwrap_or_default() {
            "v" => {
                let coords = parts
                    .map(str::parse::<f32>)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|_| {
                        Error::parse(
                            path,
                            Some(line_number),
                            "vertex requires numeric coordinates",
                        )
                    })?;
                if coords.len() < 3 {
                    return Err(Error::parse(
                        path,
                        Some(line_number),
                        "vertex requires x y z",
                    ));
                }
                vertices.push(Vec3::new(coords[0], coords[1], coords[2]));
            }
            "f" => {
                let mut indices = Vec::new();
                for token in parts {
                    let vertex_token = token.split('/').next().unwrap_or_default();
                    if vertex_token.is_empty() {
                        return Err(Error::parse(
                            path,
                            Some(line_number),
                            "face contains an empty vertex index",
                        ));
                    }
                    let raw_index = vertex_token.parse::<isize>().map_err(|_| {
                        Error::parse(
                            path,
                            Some(line_number),
                            "face contains an invalid vertex index",
                        )
                    })?;
                    let index =
                        obj_index_to_zero_based(raw_index, vertices.len()).ok_or_else(|| {
                            Error::parse(
                                path,
                                Some(line_number),
                                "face vertex index is out of range",
                            )
                        })?;
                    indices.push(index);
                }
                if indices.len() < 3 {
                    return Err(Error::parse(
                        path,
                        Some(line_number),
                        "face requires at least three vertices",
                    ));
                }
                let mut face = Face::new(indices);
                face.material = current_material.clone();
                faces.push(face);
            }
            "usemtl" => {
                let name = parts.collect::<Vec<_>>().join(" ");
                current_material = (!name.is_empty()).then_some(name);
            }
            "mtllib" => {
                let name = parts.collect::<Vec<_>>().join(" ");
                if !name.is_empty() {
                    mtllibs.push(name);
                }
            }
            _ => {}
        }
    }

    let mut materials = Vec::new();
    #[cfg(feature = "mtl")]
    {
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        for lib in mtllibs {
            let material_path = base.join(lib);
            match mtl::load_mtl(&material_path) {
                Ok(mut loaded) => materials.append(&mut loaded),
                Err(Error::Io { .. }) => {}
                Err(err) => return Err(err),
            }
        }
    }

    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("obj mesh");
    Mesh::new(name, vertices, faces, materials)
}

fn obj_index_to_zero_based(index: isize, vertex_count: usize) -> Option<usize> {
    if index > 0 {
        let idx = usize::try_from(index - 1).ok()?;
        (idx < vertex_count).then_some(idx)
    } else if index < 0 {
        let idx = isize::try_from(vertex_count).ok()?.checked_add(index)?;
        (idx >= 0).then_some(idx as usize)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_obj_faces_and_materials() {
        let mesh = parse_obj(
            Path::new("inline.obj"),
            "v 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl red\nf 1 2 3\n",
        )
        .unwrap();
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.faces.len(), 1);
        assert_eq!(mesh.faces[0].material.as_deref(), Some("red"));
    }

    #[test]
    fn supports_negative_indices() {
        let mesh = parse_obj(
            Path::new("inline.obj"),
            "v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\n",
        )
        .unwrap();
        assert_eq!(mesh.faces[0].indices, vec![0, 1, 2]);
    }
}
