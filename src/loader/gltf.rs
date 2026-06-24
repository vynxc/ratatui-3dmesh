use std::path::Path;

use crate::{
    loader::MeshLoadOptions,
    model::{Face, Material, Mesh, TextureRef, Vec2, Vec3},
    Error, Result,
};

#[cfg(feature = "textures")]
use crate::model::Texture;

/// Load a glTF/GLB mesh.
pub fn load_gltf(path: &Path, options: &MeshLoadOptions) -> Result<Mesh> {
    let (document, buffers, images) = gltf::import(path).map_err(|err| {
        Error::parse(
            path,
            None,
            format!("failed to import glTF document and buffers: {err}"),
        )
    })?;

    let mut materials = document
        .materials()
        .enumerate()
        .map(|(index, material)| {
            let mut m = Material::new(
                material
                    .name()
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("material_{index}")),
            );
            let pbr = material.pbr_metallic_roughness();
            let [r, g, b, _a] = pbr.base_color_factor();
            m.diffuse = [r, g, b];
            if let Some(info) = pbr.base_color_texture() {
                let source = info.texture().source();
                m.diffuse_texture = Some(TextureRef {
                    path: image_path(path, source.index(), image_uri(source.source())),
                    index: Some(source.index()),
                });
            }
            m
        })
        .collect::<Vec<_>>();

    let mut vertices = Vec::new();
    let mut tex_coords = Vec::new();
    let mut normals = Vec::new();
    let mut faces = Vec::new();

    for node in document.nodes() {
        let transform = node.transform().matrix();
        let Some(mesh) = node.mesh() else {
            continue;
        };
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()].0));
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let base_vertex = vertices.len();
            let base_uv = tex_coords.len();
            let base_normal = normals.len();
            let positions = positions.collect::<Vec<_>>();
            for position in &positions {
                vertices.push(transform_point(transform, *position));
            }
            let uvs = reader
                .read_tex_coords(0)
                .map(|coords| coords.into_f32().collect::<Vec<_>>())
                .unwrap_or_default();
            for uv in &uvs {
                tex_coords.push(Vec2::new(uv[0], uv[1]));
            }
            let read_normals = reader
                .read_normals()
                .map(|items| items.collect::<Vec<_>>())
                .unwrap_or_default();
            for normal in &read_normals {
                normals.push(transform_normal(transform, *normal));
            }

            let material_name = primitive
                .material()
                .index()
                .and_then(|idx| materials.get(idx).map(|material| material.name.clone()));
            let source_indices = reader
                .read_indices()
                .map(|indices| indices.into_u32().collect::<Vec<_>>())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());
            for tri in source_indices.chunks_exact(3) {
                let local = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
                if local.iter().any(|&idx| idx >= positions.len()) {
                    continue;
                }
                let mut face = Face::with_attributes(
                    local.iter().map(|idx| base_vertex + idx).collect(),
                    local
                        .iter()
                        .map(|&idx| (idx < uvs.len()).then_some(base_uv + idx))
                        .collect(),
                    local
                        .iter()
                        .map(|&idx| (idx < read_normals.len()).then_some(base_normal + idx))
                        .collect(),
                );
                face.material = material_name.clone();
                face.normal = local.iter().find_map(|&idx| {
                    (idx < read_normals.len()).then_some(normals[base_normal + idx])
                });
                faces.push(face);
            }
        }
    }

    #[cfg(feature = "textures")]
    let textures = if options.load_material_textures {
        images
            .iter()
            .enumerate()
            .map(|(index, image)| gltf_image_to_texture(path, index, image))
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };

    #[cfg(not(feature = "textures"))]
    let _ = (images, options);

    if materials.is_empty() {
        materials.push(Material::new("default"));
    }

    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("gltf mesh");
    let mesh = Mesh::with_attributes(name, vertices, tex_coords, normals, faces, materials)?;
    #[cfg(feature = "textures")]
    let mesh = {
        let mut mesh = mesh;
        mesh.textures = textures;
        mesh
    };
    Ok(mesh)
}

fn image_uri(source: gltf::image::Source<'_>) -> Option<&str> {
    match source {
        gltf::image::Source::Uri { uri, .. } => Some(uri),
        gltf::image::Source::View { .. } => None,
    }
}

fn image_path(path: &Path, index: usize, uri: Option<&str>) -> std::path::PathBuf {
    uri.map(|uri| path.parent().unwrap_or_else(|| Path::new(".")).join(uri))
        .unwrap_or_else(|| std::path::PathBuf::from(format!("gltf-image-{index}")))
}

fn transform_point(matrix: [[f32; 4]; 4], point: [f32; 3]) -> Vec3 {
    Vec3::new(
        matrix[0][0].mul_add(
            point[0],
            matrix[1][0].mul_add(point[1], matrix[2][0] * point[2]),
        ) + matrix[3][0],
        matrix[0][1].mul_add(
            point[0],
            matrix[1][1].mul_add(point[1], matrix[2][1] * point[2]),
        ) + matrix[3][1],
        matrix[0][2].mul_add(
            point[0],
            matrix[1][2].mul_add(point[1], matrix[2][2] * point[2]),
        ) + matrix[3][2],
    )
}

fn transform_normal(matrix: [[f32; 4]; 4], normal: [f32; 3]) -> Vec3 {
    Vec3::new(
        matrix[0][0].mul_add(
            normal[0],
            matrix[1][0].mul_add(normal[1], matrix[2][0] * normal[2]),
        ),
        matrix[0][1].mul_add(
            normal[0],
            matrix[1][1].mul_add(normal[1], matrix[2][1] * normal[2]),
        ),
        matrix[0][2].mul_add(
            normal[0],
            matrix[1][2].mul_add(normal[1], matrix[2][2] * normal[2]),
        ),
    )
    .normalized()
}

#[cfg(feature = "textures")]
fn gltf_image_to_texture(path: &Path, index: usize, image: &gltf::image::Data) -> Result<Texture> {
    let rgba = match image.format {
        gltf::image::Format::R8 => image.pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        gltf::image::Format::R8G8 => image
            .pixels
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        gltf::image::Format::R8G8B8 => image
            .pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        gltf::image::Format::R8G8B8A8 => image.pixels.clone(),
        other => {
            return Err(Error::texture_decode(
                image_path(path, index, None),
                format!("unsupported glTF image format {other:?}"),
            ))
        }
    };
    Ok(Texture::new(
        image_path(path, index, None),
        image.width,
        image.height,
        rgba,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_points() {
        let point = transform_point(
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [2.0, 3.0, 4.0, 1.0],
            ],
            [1.0, 2.0, 3.0],
        );
        assert_eq!(point, Vec3::new(3.0, 5.0, 7.0));
    }

    #[test]
    fn loads_axe_fixture_when_present() {
        let path = Path::new("models/axe/scene.gltf");
        if !path.exists() {
            return;
        }
        let mesh = load_gltf(
            path,
            &MeshLoadOptions::default().load_material_textures(true),
        )
        .unwrap();
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.faces.is_empty());
        assert!(!mesh.tex_coords.is_empty());
        #[cfg(feature = "textures")]
        assert!(!mesh.textures.is_empty());
    }
}
