use std::path::{Path, PathBuf};

use crate::{
    animation::{
        identity_matrix, multiply_matrix, transform_normal as animation_transform_normal,
        transform_point as animation_transform_point, AnimatedProperty, AnimationChannel,
        AnimationClip, AnimationNode, AnimationSampler, AnimationValue, Interpolation, MeshRange,
        NodeTransform, Quaternion, SkinBinding, SkinnedVertex,
    },
    loader::MeshLoadOptions,
    model::{AlphaMode, Face, Material, Mesh, TextureRef, Vec2, Vec3},
    Error, Result,
};

#[cfg(feature = "textures")]
use crate::{loader::texture::decode_texture, model::Texture};

/// Load a glTF/GLB mesh.
///
/// # Errors
///
/// Returns an error when the glTF document cannot be imported, decoded texture data uses an
/// unsupported format, or the resulting mesh geometry is invalid.
pub fn load_gltf(path: &Path, options: &MeshLoadOptions) -> Result<Mesh> {
    let gltf = gltf::Gltf::open(path).map_err(|err| {
        Error::parse(path, None, format!("failed to import glTF document: {err}"))
    })?;
    let base = path.parent().unwrap_or_else(|| Path::new("./"));
    let buffers = gltf::import_buffers(&gltf.document, Some(base), gltf.blob)
        .map_err(|err| Error::parse(path, None, format!("failed to import glTF buffers: {err}")))?;
    let document = gltf.document;

    let mut materials = collect_materials(path, &document);
    let geometry = collect_geometry(&document, &buffers, &materials);
    let animations = collect_animations(&document, &buffers);

    #[cfg(feature = "textures")]
    let textures = load_textures(path, options, &document, &buffers)?;

    #[cfg(not(feature = "textures"))]
    let _ = options;

    if materials.is_empty() {
        materials.push(Material::new("default"));
    }

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("gltf mesh");
    let mut mesh = Mesh::with_attributes(
        name,
        geometry.vertices,
        geometry.tex_coords,
        geometry.normals,
        geometry.faces,
        materials,
    )?;
    mesh.bind_vertices = geometry.bind_vertices;
    mesh.bind_normals = geometry.bind_normals;
    mesh.animation_nodes = geometry.animation_nodes;
    mesh.skins = geometry.skins;
    mesh.animations = animations;
    mesh.flip_texture_v = false;

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
    bind_vertices: Vec<Vec3>,
    tex_coords: Vec<Vec2>,
    normals: Vec<Vec3>,
    bind_normals: Vec<Vec3>,
    faces: Vec<Face>,
    animation_nodes: Vec<AnimationNode>,
    skins: Vec<SkinBinding>,
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
            let [red, green, blue, alpha] = pbr.base_color_factor();
            output.diffuse = [red, green, blue];
            output.base_color_alpha = alpha;
            output.double_sided = material.double_sided();
            output.alpha_mode = match material.alpha_mode() {
                gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
                gltf::material::AlphaMode::Mask => AlphaMode::Mask,
                gltf::material::AlphaMode::Blend => AlphaMode::Blend,
            };
            output.alpha_cutoff = material.alpha_cutoff().unwrap_or(0.5);
            output.emissive = material.emissive_factor();
            // KHR_materials_emissive_strength scales the emissive factor, often
            // far above 1.0 for HDR bloom. The terminal has no HDR range, so fold
            // the multiplier in and let downstream clamping cap it at full bright.
            if let Some(strength) = material.emissive_strength() {
                output.emissive = output.emissive.map(|channel| channel * strength);
            }
            // KHR_materials_unlit: render the base color flat, ignoring lighting.
            output.unlit = material.unlit();
            if let Some(info) = pbr.base_color_texture() {
                if let Some(source) = texture_image(document, &info.texture()) {
                    output.diffuse_texture = Some(texture_ref(path, source));
                }
            }
            // Some assets use the deprecated KHR_materials_pbrSpecularGlossiness
            // extension, which carries the diffuse color/texture instead of the
            // standard metallic-roughness block. Pull those values in as the
            // base color when present.
            if let Some(sg) = material.pbr_specular_glossiness() {
                let [red, green, blue, alpha] = sg.diffuse_factor();
                output.diffuse = [red, green, blue];
                output.base_color_alpha = alpha;
                if let Some(info) = sg.diffuse_texture() {
                    if let Some(source) = texture_image(document, &info.texture()) {
                        output.diffuse_texture = Some(texture_ref(path, source));
                    }
                }
            }
            if let Some(info) = material.emissive_texture() {
                if let Some(source) = texture_image(document, &info.texture()) {
                    output.emissive_texture = Some(texture_ref(path, source));
                }
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
    let parents = collect_node_parents(document);
    let mut geometry = GeometryParts {
        animation_nodes: document
            .nodes()
            .map(|node| {
                let (translation, rotation, scale) = node.transform().decomposed();
                AnimationNode {
                    index: node.index(),
                    name: node.name().map(ToOwned::to_owned),
                    parent: parents.get(node.index()).copied().flatten(),
                    base_transform: NodeTransform::new(
                        Vec3::new(translation[0], translation[1], translation[2]),
                        Quaternion::new(rotation[0], rotation[1], rotation[2], rotation[3]),
                        Vec3::new(scale[0], scale[1], scale[2]),
                    ),
                    vertex_ranges: Vec::new(),
                    normal_ranges: Vec::new(),
                }
            })
            .collect(),
        ..GeometryParts::default()
    };
    let base_transforms = geometry
        .animation_nodes
        .iter()
        .map(|node| (node.index, node.base_transform))
        .collect::<Vec<_>>();
    let world_transforms = geometry
        .animation_nodes
        .iter()
        .map(|node| {
            (
                node.index,
                base_global_matrix(node.index, &geometry.animation_nodes, &base_transforms),
            )
        })
        .collect::<Vec<_>>();

    for node in document.nodes() {
        let transform = world_transforms
            .iter()
            .find(|(index, _)| *index == node.index())
            .map(|(_, matrix)| *matrix)
            .unwrap_or_else(|| node.transform().matrix());
        let Some(mesh) = node.mesh() else {
            continue;
        };
        for primitive in mesh.primitives() {
            let ranges = append_primitive(
                &mut geometry,
                transform,
                node.skin().as_ref(),
                &primitive,
                buffers,
                materials,
            );
            if let Some(animation_node) = geometry
                .animation_nodes
                .iter_mut()
                .find(|animation_node| animation_node.index == node.index())
            {
                if let Some(vertex_range) = ranges.vertex_range {
                    animation_node.vertex_ranges.push(vertex_range);
                }
                if let Some(normal_range) = ranges.normal_range {
                    animation_node.normal_ranges.push(normal_range);
                }
            }
            if let Some(skin) = ranges.skin {
                geometry.skins.push(skin);
            }
        }
    }
    geometry
}

#[derive(Default)]
struct PrimitiveRanges {
    vertex_range: Option<MeshRange>,
    normal_range: Option<MeshRange>,
    skin: Option<SkinBinding>,
}

fn append_primitive(
    geometry: &mut GeometryParts,
    transform: [[f32; 4]; 4],
    skin: Option<&gltf::Skin<'_>>,
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    materials: &[Material],
) -> PrimitiveRanges {
    let reader =
        primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| data.0.as_slice()));
    let Some(positions) = reader.read_positions() else {
        return PrimitiveRanges::default();
    };

    let base_vertex = geometry.vertices.len();
    let base_uv = geometry.tex_coords.len();
    let base_normal = geometry.normals.len();
    let positions = positions.collect::<Vec<_>>();

    geometry.bind_vertices.extend(
        positions
            .iter()
            .map(|position| Vec3::new(position[0], position[1], position[2])),
    );
    geometry.vertices.extend(
        positions
            .iter()
            .map(|&position| transform_point(transform, position)),
    );

    let uvs = reader
        .read_tex_coords(0)
        .map_or_else(Vec::new, |coords| coords.into_f32().collect::<Vec<_>>());
    let uv_transform = diffuse_uv_transform(&primitive.material());
    geometry.tex_coords.extend(uvs.iter().map(|uv| {
        let [u, v] = uv_transform.map_or(*uv, |t| t.apply(*uv));
        Vec2::new(u, v)
    }));

    let read_normals = reader
        .read_normals()
        .map_or_else(Vec::new, Iterator::collect);
    geometry.bind_normals.extend(
        read_normals
            .iter()
            .map(|normal| Vec3::new(normal[0], normal[1], normal[2])),
    );
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

    let vertex_range = MeshRange::new(base_vertex, positions.len());
    let normal_range =
        (!read_normals.is_empty()).then_some(MeshRange::new(base_normal, read_normals.len()));
    let skin = skin.and_then(|skin| {
        let joints = reader
            .read_joints(0)?
            .into_u16()
            .map(|joints| joints.map(usize::from))
            .collect::<Vec<_>>();
        let weights = reader.read_weights(0)?.into_f32().collect::<Vec<_>>();
        build_skin_binding(skin, buffers, vertex_range, normal_range, joints, weights)
    });

    PrimitiveRanges {
        vertex_range: Some(vertex_range),
        normal_range,
        skin,
    }
}

fn collect_node_parents(document: &gltf::Document) -> Vec<Option<usize>> {
    let mut parents = vec![None; document.nodes().count()];
    for node in document.nodes() {
        for child in node.children() {
            if let Some(parent) = parents.get_mut(child.index()) {
                *parent = Some(node.index());
            }
        }
    }
    parents
}

fn base_global_matrix(
    node_index: usize,
    nodes: &[AnimationNode],
    local_transforms: &[(usize, NodeTransform)],
) -> [[f32; 4]; 4] {
    base_global_matrix_inner(node_index, nodes, local_transforms, 0)
}

fn base_global_matrix_inner(
    node_index: usize,
    nodes: &[AnimationNode],
    local_transforms: &[(usize, NodeTransform)],
    depth: usize,
) -> [[f32; 4]; 4] {
    if depth > nodes.len() {
        return identity_matrix();
    }
    let Some(node) = nodes.iter().find(|node| node.index == node_index) else {
        return identity_matrix();
    };
    let local = local_transforms
        .iter()
        .find(|(index, _)| *index == node_index)
        .map(|(_, transform)| transform.matrix())
        .unwrap_or_else(|| node.base_transform.matrix());
    if let Some(parent) = node.parent {
        multiply_matrix(
            base_global_matrix_inner(parent, nodes, local_transforms, depth + 1),
            local,
        )
    } else {
        local
    }
}

fn build_skin_binding(
    skin: &gltf::Skin<'_>,
    buffers: &[gltf::buffer::Data],
    vertex_range: MeshRange,
    normal_range: Option<MeshRange>,
    joints: Vec<[usize; 4]>,
    weights: Vec<[f32; 4]>,
) -> Option<SkinBinding> {
    if joints.is_empty() || weights.is_empty() {
        return None;
    }
    let joint_nodes = skin.joints().map(|node| node.index()).collect::<Vec<_>>();
    if joint_nodes.is_empty() {
        return None;
    }
    let mut inverse_bind_matrices = skin
        .reader(|buffer| buffers.get(buffer.index()).map(|data| data.0.as_slice()))
        .read_inverse_bind_matrices()
        .map_or_else(Vec::new, Iterator::collect);
    if inverse_bind_matrices.len() < joint_nodes.len() {
        inverse_bind_matrices.resize(joint_nodes.len(), identity_matrix());
    }
    let vertices = (0..vertex_range.len)
        .map(|index| SkinnedVertex {
            joints: joints.get(index).copied().unwrap_or([0; 4]),
            weights: weights.get(index).copied().unwrap_or([0.0; 4]),
        })
        .collect::<Vec<_>>();

    Some(SkinBinding {
        vertex_range,
        normal_range,
        joint_nodes,
        inverse_bind_matrices,
        vertices,
    })
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
            .then(|| attributes.normals[attributes.base_normal + idx])
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
    uri.and_then(|uri| (!uri.starts_with("data:")).then_some(uri))
        .map_or_else(
            || PathBuf::from(format!("gltf-image-{index}")),
            |uri| {
                path.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(decode_uri_path(uri))
            },
        )
}

fn texture_ref(path: &Path, image: gltf::image::Image<'_>) -> TextureRef {
    TextureRef {
        path: image_path(path, image.index(), image_uri(image.source())),
        index: Some(image.index()),
    }
}

fn texture_image<'a>(
    document: &'a gltf::Document,
    texture: &gltf::Texture<'a>,
) -> Option<gltf::image::Image<'a>> {
    texture.source().or_else(|| {
        extension_texture_source_index(texture).and_then(|index| document.images().nth(index))
    })
}

fn extension_texture_source_index(texture: &gltf::Texture<'_>) -> Option<usize> {
    texture
        .extension_value("EXT_texture_webp")
        .or_else(|| texture.extension_value("KHR_texture_basisu"))
        .or_else(|| texture.extension_value("MSFT_texture_dds"))
        .and_then(|extension| extension.get("source"))
        .and_then(|source| source.as_u64())
        .and_then(|index| usize::try_from(index).ok())
}

fn decode_uri_path(uri: &str) -> String {
    urlencoding::decode(uri)
        .map(|decoded| decoded.into_owned())
        .unwrap_or_else(|_| uri.to_owned())
}

/// A `KHR_texture_transform` UV transform (offset, rotation, scale) applied at load
/// time so the renderer never needs to know about the extension.
#[derive(Clone, Copy)]
struct UvTransform {
    offset: [f32; 2],
    rotation: f32,
    scale: [f32; 2],
}

impl UvTransform {
    /// Transform a UV coordinate using the glTF convention:
    /// `uv' = translation * rotation * scale * uv` (rotation is counter-clockwise).
    fn apply(self, uv: [f32; 2]) -> [f32; 2] {
        let [su, sv] = [uv[0] * self.scale[0], uv[1] * self.scale[1]];
        let (sin, cos) = self.rotation.sin_cos();
        [
            cos * su + sin * sv + self.offset[0],
            -sin * su + cos * sv + self.offset[1],
        ]
    }
}

/// Read the `KHR_texture_transform` applied to a material's base-color (diffuse)
/// texture, including the diffuse texture of the spec-gloss extension. Returns
/// `None` when the extension is absent or an identity transform.
fn diffuse_uv_transform(material: &gltf::Material<'_>) -> Option<UvTransform> {
    let info = material.pbr_metallic_roughness().base_color_texture();
    let info = info.or_else(|| {
        material
            .pbr_specular_glossiness()
            .and_then(|sg| sg.diffuse_texture())
    })?;
    let transform = info.texture_transform()?;
    Some(UvTransform {
        offset: transform.offset(),
        rotation: transform.rotation(),
        scale: transform.scale(),
    })
}

fn transform_point(matrix: [[f32; 4]; 4], point: [f32; 3]) -> Vec3 {
    animation_transform_point(matrix, Vec3::new(point[0], point[1], point[2]))
}

fn transform_normal(matrix: [[f32; 4]; 4], normal: [f32; 3]) -> Vec3 {
    animation_transform_normal(matrix, Vec3::new(normal[0], normal[1], normal[2]))
}

fn collect_animations(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
) -> Vec<AnimationClip> {
    document
        .animations()
        .filter_map(|animation| {
            let mut duration_seconds = 0.0_f32;
            let mut channels = Vec::new();
            for channel in animation.channels() {
                let Some(imported) = import_channel(&channel, buffers) else {
                    continue;
                };
                duration_seconds = duration_seconds.max(
                    imported
                        .sampler
                        .inputs
                        .iter()
                        .copied()
                        .fold(0.0_f32, f32::max),
                );
                channels.push(imported);
            }
            if channels.is_empty() {
                None
            } else {
                Some(AnimationClip {
                    name: animation.name().map_or_else(
                        || format!("animation_{}", animation.index()),
                        ToOwned::to_owned,
                    ),
                    duration_seconds,
                    channels,
                })
            }
        })
        .collect()
}

fn import_channel(
    channel: &gltf::animation::Channel<'_>,
    buffers: &[gltf::buffer::Data],
) -> Option<AnimationChannel> {
    let property = match channel.target().property() {
        gltf::animation::Property::Translation => AnimatedProperty::Translation,
        gltf::animation::Property::Rotation => AnimatedProperty::Rotation,
        gltf::animation::Property::Scale => AnimatedProperty::Scale,
        gltf::animation::Property::MorphTargetWeights => return None,
    };
    let interpolation = match channel.sampler().interpolation() {
        gltf::animation::Interpolation::Linear => Interpolation::Linear,
        gltf::animation::Interpolation::Step => Interpolation::Step,
        gltf::animation::Interpolation::CubicSpline => return None,
    };
    let reader = channel.reader(|buffer| buffers.get(buffer.index()).map(|data| data.0.as_slice()));
    let inputs = reader.read_inputs()?.collect::<Vec<_>>();
    let outputs: Vec<AnimationValue> = match reader.read_outputs()? {
        gltf::animation::util::ReadOutputs::Translations(values) => values
            .map(|value| AnimationValue::Vec3(Vec3::new(value[0], value[1], value[2])))
            .collect(),
        gltf::animation::util::ReadOutputs::Scales(values) => values
            .map(|value| AnimationValue::Vec3(Vec3::new(value[0], value[1], value[2])))
            .collect(),
        gltf::animation::util::ReadOutputs::Rotations(values) => values
            .into_f32()
            .map(|value| {
                AnimationValue::Rotation(
                    Quaternion::new(value[0], value[1], value[2], value[3]).normalized(),
                )
            })
            .collect(),
        gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => return None,
    };
    if inputs.is_empty() || outputs.is_empty() {
        return None;
    }
    Some(AnimationChannel {
        target_node: channel.target().node().index(),
        property,
        sampler: AnimationSampler {
            inputs,
            outputs,
            interpolation,
        },
    })
}

#[cfg(feature = "textures")]
fn load_textures(
    path: &Path,
    _options: &MeshLoadOptions,
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
) -> Result<Vec<Texture>> {
    // glTF/GLB embeds or references encoded images directly. Decode them here instead of
    // using `gltf::import_images`, which intentionally leaves texture-container extensions
    // such as EXT_texture_webp to applications.
    document
        .images()
        .map(|image| decode_gltf_texture(path, image, buffers))
        .collect()
}

#[cfg(feature = "textures")]
fn decode_gltf_texture(
    path: &Path,
    image: gltf::image::Image<'_>,
    buffers: &[gltf::buffer::Data],
) -> Result<Texture> {
    let (texture_path, bytes) = encoded_image_bytes(path, image, buffers)?;
    decode_texture(&texture_path, &bytes)
}

#[cfg(feature = "textures")]
fn encoded_image_bytes(
    path: &Path,
    image: gltf::image::Image<'_>,
    buffers: &[gltf::buffer::Data],
) -> Result<(PathBuf, Vec<u8>)> {
    match image.source() {
        gltf::image::Source::View { view, .. } => {
            let parent = buffers.get(view.buffer().index()).ok_or_else(|| {
                Error::parse(
                    path,
                    None,
                    format!("image {} references missing buffer", image.index()),
                )
            })?;
            let begin = view.offset();
            let end = begin + view.length();
            let bytes = parent.0.get(begin..end).ok_or_else(|| {
                Error::parse(
                    path,
                    None,
                    format!("image {} buffer view is out of bounds", image.index()),
                )
            })?;
            Ok((image_path(path, image.index(), None), bytes.to_vec()))
        }
        gltf::image::Source::Uri { uri, .. } => {
            let texture_path = image_path(path, image.index(), Some(uri));
            let bytes = read_image_uri(path, uri)?;
            Ok((texture_path, bytes))
        }
    }
}

#[cfg(feature = "textures")]
fn read_image_uri(path: &Path, uri: &str) -> Result<Vec<u8>> {
    if let Some(data) = uri.strip_prefix("data:") {
        let (_, encoded) = data.split_once(',').ok_or_else(|| {
            Error::texture_decode(image_path(path, 0, None), "malformed data URI")
        })?;
        use base64::Engine as _;
        return base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|err| Error::texture_decode(image_path(path, 0, None), err.to_string()));
    }

    let uri_path = if let Some(rest) = uri.strip_prefix("file://") {
        PathBuf::from(rest)
    } else if let Some(rest) = uri.strip_prefix("file:") {
        PathBuf::from(rest)
    } else if uri.contains(':') {
        return Err(Error::parse(
            path,
            None,
            format!("unsupported image URI scheme in {uri}"),
        ));
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(decode_uri_path(uri))
    };

    std::fs::read(&uri_path).map_err(|source| Error::io(uri_path, source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{sample_mesh_animation, AnimatedProperty};
    use std::{fs, time::SystemTime};

    #[test]
    fn uv_transform_applies_scale_offset_rotation() {
        // Identity transform leaves UVs untouched.
        let identity = UvTransform {
            offset: [0.0, 0.0],
            rotation: 0.0,
            scale: [1.0, 1.0],
        };
        assert_eq!(identity.apply([0.25, 0.75]), [0.25, 0.75]);

        // Scale then offset.
        let scaled = UvTransform {
            offset: [0.1, 0.2],
            rotation: 0.0,
            scale: [2.0, 3.0],
        };
        let [u, v] = scaled.apply([0.5, 0.5]);
        assert!((u - 1.1).abs() < 1e-6, "u = {u}");
        assert!((v - 1.7).abs() < 1e-6, "v = {v}");

        // 90 degree counter-clockwise rotation maps (1, 0) -> (0, -1).
        let rotated = UvTransform {
            offset: [0.0, 0.0],
            rotation: std::f32::consts::FRAC_PI_2,
            scale: [1.0, 1.0],
        };
        let [u, v] = rotated.apply([1.0, 0.0]);
        assert!(u.abs() < 1e-6, "u = {u}");
        assert!((v + 1.0).abs() < 1e-6, "v = {v}");
    }

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

    #[cfg(feature = "textures")]
    #[test]
    fn loads_embedded_glb_textures_without_opt_in() {
        // glTF/GLB embeds its images; default options must decode them without the
        // caller opting into material textures the way OBJ requires.
        let path = Path::new("examples/assets/gltf/box_textured.glb");
        let mesh = load_gltf(path, &MeshLoadOptions::default()).unwrap();

        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.faces.is_empty());
        assert!(!mesh.tex_coords.is_empty(), "textured box carries UVs");
        assert!(!mesh.textures.is_empty(), "embedded image must decode");

        // Every base-color texture reference must resolve to a loaded texture index.
        let mut resolved = 0usize;
        for material in &mesh.materials {
            if let Some(index) = material.diffuse_texture.as_ref().and_then(|t| t.index) {
                assert!(
                    mesh.textures.get(index).is_some(),
                    "material {} references missing texture {index}",
                    material.name
                );
                resolved += 1;
            }
        }
        assert!(resolved > 0, "textured box must resolve a base-color map");
    }

    #[cfg(feature = "textures")]
    #[test]
    fn loads_ext_texture_webp_texture_source() {
        let dir = std::env::temp_dir().join(format!(
            "ratatui-3dmesh-gltf-webp-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let bin_path = dir.join("mesh.bin");
        let gltf_path = dir.join("scene.gltf");

        let mut webp = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut webp)
            .encode(&[255, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        use base64::Engine as _;
        let webp_data = base64::engine::general_purpose::STANDARD.encode(webp);

        let mut bin = Vec::new();
        for value in [
            0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, // positions
            0.0, 0.0, 1.0, 0.0, 0.0, 1.0, // texcoords
        ] {
            bin.extend_from_slice(&value.to_le_bytes());
        }
        fs::write(&bin_path, bin).unwrap();
        fs::write(
            &gltf_path,
            format!(
                r#"{{
  "asset": {{ "version": "2.0" }},
  "extensionsUsed": ["EXT_texture_webp"],
  "extensionsRequired": ["EXT_texture_webp"],
  "buffers": [{{ "uri": "mesh.bin", "byteLength": 60 }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 }},
    {{ "buffer": 0, "byteOffset": 36, "byteLength": 24, "target": 34962 }}
  ],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0] }},
    {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC2" }}
  ],
  "images": [{{ "uri": "data:image/webp;base64,{webp_data}", "mimeType": "image/webp" }}],
  "textures": [{{ "extensions": {{ "EXT_texture_webp": {{ "source": 0 }} }} }}],
  "materials": [{{ "pbrMetallicRoughness": {{ "baseColorTexture": {{ "index": 0 }} }} }}],
  "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0, "TEXCOORD_0": 1 }}, "material": 0, "mode": 4 }}] }}],
  "nodes": [{{ "mesh": 0 }}],
  "scenes": [{{ "nodes": [0] }}],
  "scene": 0
}}"#
            ),
        )
        .unwrap();

        let mesh = load_gltf(&gltf_path, &MeshLoadOptions::default()).unwrap();
        assert_eq!(mesh.textures.len(), 1);
        assert_eq!(mesh.textures[0].width, 1);
        assert_eq!(mesh.textures[0].height, 1);
        assert_eq!(mesh.textures[0].rgba.as_ref(), &[255, 0, 0, 255]);
        assert_eq!(
            mesh.materials[0]
                .diffuse_texture
                .as_ref()
                .and_then(|texture| texture.index),
            Some(0)
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn loads_glb_node_animation() {
        // BoxAnimated exercises node TRS animation without skinning.
        let path = Path::new("examples/assets/gltf/box_animated.glb");
        let mesh = load_gltf(path, &MeshLoadOptions::default()).unwrap();

        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.animations.is_empty(), "must import animation clips");
        assert!(
            mesh.animations[0].channel_count() > 0,
            "clip must carry channels"
        );

        let sampled = sample_mesh_animation(&mesh, 0, 0.5, true).unwrap();
        assert_eq!(sampled.vertices.len(), mesh.vertices.len());
    }

    #[test]
    fn loads_glb_skinned_animation() {
        // Fox is a skinned mesh with multiple clips: it exercises JOINTS_0/WEIGHTS_0
        // CPU skinning end to end.
        let path = Path::new("examples/assets/gltf/fox.glb");
        let mesh = load_gltf(path, &MeshLoadOptions::default()).unwrap();

        assert!(!mesh.skins.is_empty(), "fox must load skin bindings");
        assert!(
            mesh.animations.len() >= 2,
            "fox ships several animation clips, got {}",
            mesh.animations.len()
        );

        // Skinning at a non-zero time must actually displace vertices from bind pose.
        let sampled = sample_mesh_animation(&mesh, 0, 0.3, true).unwrap();
        assert_eq!(sampled.vertices.len(), mesh.vertices.len());
        assert_ne!(
            sampled.vertices, mesh.vertices,
            "skinned pose must differ from bind pose"
        );
    }

    #[test]
    fn loads_generated_translation_animation() {
        let dir = std::env::temp_dir().join(format!(
            "ratatui-3dmesh-gltf-animation-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let bin_path = dir.join("anim.bin");
        let gltf_path = dir.join("scene.gltf");

        let mut bin = Vec::new();
        for value in [
            0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, // positions
            0.0, 1.0, // input times
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, // translation outputs
        ] {
            bin.extend_from_slice(&value.to_le_bytes());
        }
        fs::write(&bin_path, bin).unwrap();
        fs::write(
            &gltf_path,
            r#"{
  "asset": { "version": "2.0" },
  "buffers": [{ "uri": "anim.bin", "byteLength": 68 }],
  "bufferViews": [
    { "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 },
    { "buffer": 0, "byteOffset": 36, "byteLength": 8 },
    { "buffer": 0, "byteOffset": 44, "byteLength": 24 }
  ],
  "accessors": [
    { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0] },
    { "bufferView": 1, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0], "max": [1] },
    { "bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC3" }
  ],
  "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "mode": 4 }] }],
  "nodes": [{ "name": "animated-node", "mesh": 0 }],
  "scenes": [{ "nodes": [0] }],
  "scene": 0,
  "animations": [{
    "name": "Move",
    "samplers": [{ "input": 1, "output": 2, "interpolation": "LINEAR" }],
    "channels": [{ "sampler": 0, "target": { "node": 0, "path": "translation" } }]
  }]
}"#,
        )
        .unwrap();

        let mesh = load_gltf(&gltf_path, &MeshLoadOptions::default()).unwrap();
        assert_eq!(mesh.animations.len(), 1);
        assert!(!mesh.flip_texture_v);
        assert_eq!(mesh.animations[0].name, "Move");
        assert_eq!(mesh.animations[0].duration_seconds, 1.0);
        assert_eq!(mesh.animations[0].channel_count(), 1);
        assert_eq!(
            mesh.animations[0].channels[0].property,
            AnimatedProperty::Translation
        );
        assert_eq!(mesh.animation_nodes.len(), 1);
        assert_eq!(
            mesh.animation_nodes[0].vertex_ranges[0],
            MeshRange::new(0, 3)
        );

        let sampled = sample_mesh_animation(&mesh, 0, 1.0, false).unwrap();
        assert_eq!(sampled.vertices[0], Vec3::new(1.0, 0.0, 0.0));

        let _ = fs::remove_dir_all(dir);
    }
}
