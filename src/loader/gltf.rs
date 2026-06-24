use std::path::{Path, PathBuf};

use crate::{
    loader::MeshLoadOptions,
    model::{Face, Material, Mesh, TextureRef, Vec2, Vec3},
    Error, Result,
};

#[cfg(feature = "textures")]
use crate::model::Texture;

/// Load a glTF/GLB mesh.
///
/// # Errors
///
/// Returns an error when the glTF document cannot be imported, decoded texture data uses an
/// unsupported format, or the resulting mesh geometry is invalid.
pub fn load_gltf(path: &Path, options: &MeshLoadOptions) -> Result<Mesh> {
    let (document, buffers, images) = gltf::import(path).map_err(|err| {
        Error::parse(
            path,
            None,
            format!("failed to import glTF document and buffers: {err}"),
        )
    })?;

    let mut materials = collect_materials(path, &document);
    let geometry = collect_geometry(&document, &buffers, &materials);

    #[cfg(feature = "textures")]
    let textures = load_textures(path, options, &images)?;

    #[cfg(not(feature = "textures"))]
    let _ = (images, options);

    if materials.is_empty() {
        materials.push(Material::new("default"));
    }

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("gltf mesh");
    let mesh = Mesh::with_attributes(
        name,
        geometry.vertices,
        geometry.tex_coords,
        geometry.normals,
        geometry.faces,
        materials,
    )?;

    #[cfg(feature = "textures")]
    let mesh = {
        let mut mesh = mesh;
        mesh.textures = textures;
        mesh
    };

    Ok(mesh)
}

#[derive(Default)]
struct GeometryParts {
    vertices: Vec<Vec3>,
    tex_coords: Vec<Vec2>,
    normals: Vec<Vec3>,
    faces: Vec<Face>,
}

fn collect_materials(path: &Path, document: &gltf::Document) -> Vec<Material> {
    document
        .materials()
        .enumerate()
        .map(|(index, material)| {
            let name = material
                .name()
                .map_or_else(|| format!("material_{index}"), ToOwned::to_owned);
            let mut output = Material::new(name);
            let pbr = material.pbr_metallic_roughness();
            let [red, green, blue, _alpha] = pbr.base_color_factor();
            output.diffuse = [red, green, blue];
            if let Some(info) = pbr.base_color_texture() {
                let source = info.texture().source();
                output.diffuse_texture = Some(TextureRef {
                    path: image_path(path, source.index(), image_uri(source.source())),
                    index: Some(source.index()),
                });
            }
            output
        })
        .collect()
}

fn collect_geometry(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    materials: &[Material],
) -> GeometryParts {
    let mut geometry = GeometryParts::default();
    for node in document.nodes() {
        let transform = node.transform().matrix();
        let Some(mesh) = node.mesh() else {
            continue;
        };
        for primitive in mesh.primitives() {
            append_primitive(&mut geometry, transform, &primitive, buffers, materials);
        }
    }
    geometry
}

fn append_primitive(
    geometry: &mut GeometryParts,
    transform: [[f32; 4]; 4],
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    materials: &[Material],
) {
    let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()].0));
    let Some(positions) = reader.read_positions() else {
        return;
    };

    let base_vertex = geometry.vertices.len();
    let base_uv = geometry.tex_coords.len();
    let base_normal = geometry.normals.len();
    let positions = positions.collect::<Vec<_>>();

    geometry.vertices.extend(
        positions
            .iter()
            .map(|&position| transform_point(transform, position)),
    );

    let uvs = reader
        .read_tex_coords(0)
        .map_or_else(Vec::new, |coords| coords.into_f32().collect::<Vec<_>>());
    geometry
        .tex_coords
        .extend(uvs.iter().map(|uv| Vec2::new(uv[0], uv[1])));

    let read_normals = reader
        .read_normals()
        .map_or_else(Vec::new, Iterator::collect);
    geometry.normals.extend(
        read_normals
            .iter()
            .map(|&normal| transform_normal(transform, normal)),
    );

    let material_name = primitive
        .material()
        .index()
        .and_then(|idx| materials.get(idx).map(|material| material.name.clone()));
    let source_indices = reader.read_indices().map_or_else(
        || indices_for_positions(positions.len()),
        |indices| indices.into_u32().collect::<Vec<_>>(),
    );

    for tri in source_indices.chunks_exact(3) {
        let local = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        if local.iter().any(|&idx| idx >= positions.len()) {
            continue;
        }
        let attributes = PrimitiveFaceAttributes {
            base_vertex,
            base_uv,
            base_normal,
            uvs: &uvs,
            read_normals: &read_normals,
            normals: &geometry.normals,
        };
        geometry
            .faces
            .push(build_face(local, &attributes, material_name.clone()));
    }
}

struct PrimitiveFaceAttributes<'a> {
    base_vertex: usize,
    base_uv: usize,
    base_normal: usize,
    uvs: &'a [[f32; 2]],
    read_normals: &'a [[f32; 3]],
    normals: &'a [Vec3],
}

fn build_face(
    local: [usize; 3],
    attributes: &PrimitiveFaceAttributes<'_>,
    material_name: Option<String>,
) -> Face {
    let mut face = Face::with_attributes(
        local
            .iter()
            .map(|idx| attributes.base_vertex + idx)
            .collect(),
        local
            .iter()
            .map(|&idx| (idx < attributes.uvs.len()).then_some(attributes.base_uv + idx))
            .collect(),
        local
            .iter()
            .map(|&idx| {
                (idx < attributes.read_normals.len()).then_some(attributes.base_normal + idx)
            })
            .collect(),
    );
    face.material = material_name;
    face.normal = local.iter().find_map(|&idx| {
        (idx < attributes.read_normals.len())
            .then_some(attributes.normals[attributes.base_normal + idx])
    });
    face
}

fn indices_for_positions(len: usize) -> Vec<u32> {
    (0..len)
        .filter_map(|index| u32::try_from(index).ok())
        .collect()
}

fn image_uri(source: gltf::image::Source<'_>) -> Option<&str> {
    match source {
        gltf::image::Source::Uri { uri, .. } => Some(uri),
        gltf::image::Source::View { .. } => None,
    }
}

fn image_path(path: &Path, index: usize, uri: Option<&str>) -> PathBuf {
    uri.map_or_else(
        || PathBuf::from(format!("gltf-image-{index}")),
        |uri| path.parent().unwrap_or_else(|| Path::new(".")).join(uri),
    )
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
fn load_textures(
    path: &Path,
    options: &MeshLoadOptions,
    images: &[gltf::image::Data],
) -> Result<Vec<Texture>> {
    if options.load_material_textures {
        images
            .iter()
            .enumerate()
            .map(|(index, image)| gltf_image_to_texture(path, index, image))
            .collect()
    } else {
        Ok(Vec::new())
    }
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
