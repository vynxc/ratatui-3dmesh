use std::{fs, path::Path};

use crate::{
    loader::MeshLoadOptions,
    model::{Face, Mesh, TextureRef, Vec2, Vec3},
    Error, Result,
};

#[cfg(feature = "mtl")]
use super::mtl;
#[cfg(feature = "textures")]
use super::texture;

/// Load a Wavefront OBJ mesh.
///
/// # Errors
///
/// Returns an error when the file cannot be read or valid OBJ geometry cannot be parsed.
pub fn load_obj(path: &Path) -> Result<Mesh> {
    load_obj_with_options(path, &MeshLoadOptions::default())
}

/// Load a Wavefront OBJ mesh with loader options.
///
/// # Errors
///
/// Returns an error when the OBJ cannot be read or parsed, or when strict texture loading fails.
pub fn load_obj_with_options(path: &Path, options: &MeshLoadOptions) -> Result<Mesh> {
    let text = fs::read_to_string(path).map_err(|err| Error::io(path, err))?;
    let mut mesh = parse_obj(path, &text)?;
    attach_textures(&mut mesh, options)?;
    Ok(mesh)
}

/// Parse a Wavefront OBJ mesh from text.
///
/// # Errors
///
/// Returns an error when required vertex/face data is missing or malformed.
pub fn parse_obj(path: &Path, text: &str) -> Result<Mesh> {
    let mut vertices = Vec::new();
    let mut tex_coords = Vec::new();
    let mut normals = Vec::new();
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
                let coords = parse_f32s(
                    path,
                    line_number,
                    parts,
                    "vertex requires numeric coordinates",
                )?;
                if coords.len() < 3 {
                    return Err(Error::parse(
                        path,
                        Some(line_number),
                        "vertex requires x y z",
                    ));
                }
                vertices.push(Vec3::new(coords[0], coords[1], coords[2]));
            }
            "vt" => {
                let coords = parse_f32s(
                    path,
                    line_number,
                    parts,
                    "texture coordinate requires numeric values",
                )?;
                if coords.len() < 2 {
                    return Err(Error::parse(
                        path,
                        Some(line_number),
                        "texture coordinate requires u v",
                    ));
                }
                tex_coords.push(Vec2::new(coords[0], coords[1]));
            }
            "vn" => {
                let coords = parse_f32s(
                    path,
                    line_number,
                    parts,
                    "normal requires numeric coordinates",
                )?;
                if coords.len() < 3 {
                    return Err(Error::parse(
                        path,
                        Some(line_number),
                        "normal requires x y z",
                    ));
                }
                normals.push(Vec3::new(coords[0], coords[1], coords[2]).normalized());
            }
            "f" => {
                let mut indices = Vec::new();
                let mut tex_coord_indices = Vec::new();
                let mut normal_indices = Vec::new();
                for token in parts {
                    let parsed = parse_face_token(
                        path,
                        line_number,
                        token,
                        vertices.len(),
                        tex_coords.len(),
                        normals.len(),
                    )?;
                    indices.push(parsed.vertex);
                    tex_coord_indices.push(parsed.tex_coord);
                    normal_indices.push(parsed.normal);
                }
                if indices.len() < 3 {
                    return Err(Error::parse(
                        path,
                        Some(line_number),
                        "face requires at least three vertices",
                    ));
                }
                let mut face = Face::with_attributes(indices, tex_coord_indices, normal_indices);
                face.material.clone_from(&current_material);
                face.normal = face
                    .normal_indices
                    .iter()
                    .flatten()
                    .next()
                    .and_then(|&idx| normals.get(idx).copied());
                faces.push(face);
            }
            "usemtl" => {
                let name = parts.collect::<Vec<_>>().join(" ");
                current_material = (!name.is_empty()).then_some(name);
            }
            "mtllib" => {
                mtllibs.extend(parts.map(ToString::to_string));
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
    Mesh::with_attributes(name, vertices, tex_coords, normals, faces, materials)
}

fn parse_f32s<'a>(
    path: &Path,
    line: usize,
    parts: impl Iterator<Item = &'a str>,
    message: &str,
) -> Result<Vec<f32>> {
    parts
        .map(str::parse::<f32>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Error::parse(path, Some(line), message))
}

#[derive(Debug, Clone, Copy)]
struct FaceToken {
    vertex: usize,
    tex_coord: Option<usize>,
    normal: Option<usize>,
}

fn parse_face_token(
    path: &Path,
    line: usize,
    token: &str,
    vertex_count: usize,
    tex_coord_count: usize,
    normal_count: usize,
) -> Result<FaceToken> {
    let mut parts = token.split('/');
    let vertex = parse_obj_index(
        path,
        line,
        parts.next().unwrap_or_default(),
        vertex_count,
        "face vertex index",
    )?
    .ok_or_else(|| Error::parse(path, Some(line), "face contains an empty vertex index"))?;
    let tex_coord = parts
        .next()
        .map(|s| parse_obj_index(path, line, s, tex_coord_count, "face texture index"))
        .transpose()?
        .flatten();
    let normal = parts
        .next()
        .map(|s| parse_obj_index(path, line, s, normal_count, "face normal index"))
        .transpose()?
        .flatten();
    Ok(FaceToken {
        vertex,
        tex_coord,
        normal,
    })
}

fn parse_obj_index(
    path: &Path,
    line: usize,
    text: &str,
    count: usize,
    label: &str,
) -> Result<Option<usize>> {
    if text.is_empty() {
        return Ok(None);
    }
    let raw = text
        .parse::<isize>()
        .map_err(|_| Error::parse(path, Some(line), format!("{label} is invalid")))?;
    obj_index_to_zero_based(raw, count)
        .map(Some)
        .ok_or_else(|| Error::parse(path, Some(line), format!("{label} is out of range")))
}

fn obj_index_to_zero_based(index: isize, count: usize) -> Option<usize> {
    match index.cmp(&0) {
        std::cmp::Ordering::Greater => {
            let idx = usize::try_from(index - 1).ok()?;
            (idx < count).then_some(idx)
        }
        std::cmp::Ordering::Less => {
            let idx = isize::try_from(count).ok()?.checked_add(index)?;
            usize::try_from(idx).ok()
        }
        std::cmp::Ordering::Equal => None,
    }
}

#[cfg(feature = "textures")]
fn attach_textures(mesh: &mut Mesh, options: &MeshLoadOptions) -> Result<()> {
    if options.load_material_textures {
        for material in &mut mesh.materials {
            let Some(texture_ref) = material.diffuse_texture.as_mut() else {
                continue;
            };
            match texture::load_texture(&texture_ref.path) {
                Ok(texture) => {
                    texture_ref.index = Some(mesh.textures.len());
                    mesh.textures.push(texture);
                }
                Err(err) if options.strict_textures => return Err(err),
                Err(_) => {}
            }
        }
    }

    if let Some(path) = options.texture_override.as_ref() {
        match texture::load_texture(path) {
            Ok(texture) => {
                mesh.default_texture = Some(TextureRef {
                    path: path.clone(),
                    index: Some(mesh.textures.len()),
                });
                mesh.textures.push(texture);
            }
            Err(err) if options.strict_textures => return Err(err),
            Err(_) => {}
        }
    }
    Ok(())
}

#[cfg(not(feature = "textures"))]
fn attach_textures(mesh: &mut Mesh, options: &MeshLoadOptions) -> Result<()> {
    if let Some(path) = options.texture_override.as_ref() {
        if options.strict_textures {
            return Err(Error::texture_decode(
                path,
                "the `textures` feature is required to load image textures",
            ));
        }
        mesh.default_texture = Some(TextureRef::new(path));
    }
    Ok(())
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
        assert!(mesh.animations.is_empty());
        assert!(mesh.animation_nodes.is_empty());
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

    #[test]
    fn parses_uv_and_normal_face_indices() {
        let mesh = parse_obj(
            Path::new("inline.obj"),
            "v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nvn 0 0 1\nf 1/1/1 2/2/1 3/3/1\n",
        )
        .unwrap();
        assert_eq!(mesh.tex_coords.len(), 3);
        assert_eq!(mesh.normals.len(), 1);
        assert_eq!(
            mesh.faces[0].tex_coord_indices,
            vec![Some(0), Some(1), Some(2)]
        );
        assert_eq!(
            mesh.faces[0].normal_indices,
            vec![Some(0), Some(0), Some(0)]
        );
    }

    #[test]
    fn parses_face_forms() {
        let mesh = parse_obj(
            Path::new("inline.obj"),
            "v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nvn 0 0 1\nf 1/1 2//1 3/3/1\n",
        )
        .unwrap();
        assert_eq!(
            mesh.faces[0].tex_coord_indices,
            vec![Some(0), None, Some(2)]
        );
        assert_eq!(mesh.faces[0].normal_indices, vec![None, Some(0), Some(0)]);
    }
}
