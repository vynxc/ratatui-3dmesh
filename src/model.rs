use std::{path::Path, sync::Arc};

use ratatui::style::Color;

use crate::{
    animation::{AnimationClip, AnimationNode, SkinBinding},
    loader, Error, Result,
};

/// A small 3D vector type used for mesh geometry and camera math.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vec3 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
}

impl Vec3 {
    /// Create a vector.
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Dot product.
    #[must_use]
    pub fn dot(self, rhs: Self) -> f32 {
        self.x.mul_add(rhs.x, self.y.mul_add(rhs.y, self.z * rhs.z))
    }

    /// Cross product.
    #[must_use]
    pub fn cross(self, rhs: Self) -> Self {
        Self {
            x: self.y.mul_add(rhs.z, -(self.z * rhs.y)),
            y: self.z.mul_add(rhs.x, -(self.x * rhs.z)),
            z: self.x.mul_add(rhs.y, -(self.y * rhs.x)),
        }
    }

    /// Euclidean length.
    #[must_use]
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// Normalized vector, or zero when the vector is too small.
    #[must_use]
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len <= f32::EPSILON {
            Self::default()
        } else {
            self / len
        }
    }

    /// Rotate by Euler angles in radians, applied x then y then z.
    #[must_use]
    pub fn rotate_euler(self, rotation: Self) -> Self {
        let (sx, cx) = rotation.x.sin_cos();
        let (sy, cy) = rotation.y.sin_cos();
        let (sz, cz) = rotation.z.sin_cos();

        let mut v = Self::new(self.x, self.y * cx - self.z * sx, self.y * sx + self.z * cx);
        v = Self::new(v.x * cy + v.z * sy, v.y, -v.x * sy + v.z * cy);
        Self::new(v.x * cz - v.y * sz, v.x * sz + v.y * cz, v.z)
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl std::ops::AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl std::ops::Div<f32> for Vec3 {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs)
    }
}

/// A small 2D vector type used for texture coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vec2 {
    /// Horizontal texture coordinate.
    pub u: f32,
    /// Vertical texture coordinate.
    pub v: f32,
}

impl Vec2 {
    /// Create a texture coordinate.
    #[must_use]
    pub const fn new(u: f32, v: f32) -> Self {
        Self { u, v }
    }
}

/// Axis-aligned mesh bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Bounds {
    /// Minimum corner.
    pub min: Vec3,
    /// Maximum corner.
    pub max: Vec3,
}

impl Bounds {
    /// Compute bounds from vertices.
    #[must_use]
    pub fn from_vertices(vertices: &[Vec3]) -> Option<Self> {
        let first = *vertices.first()?;
        let mut bounds = Self {
            min: first,
            max: first,
        };
        for &v in &vertices[1..] {
            bounds.min.x = bounds.min.x.min(v.x);
            bounds.min.y = bounds.min.y.min(v.y);
            bounds.min.z = bounds.min.z.min(v.z);
            bounds.max.x = bounds.max.x.max(v.x);
            bounds.max.y = bounds.max.y.max(v.y);
            bounds.max.z = bounds.max.z.max(v.z);
        }
        Some(bounds)
    }

    /// Center point.
    #[must_use]
    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Largest dimension.
    #[must_use]
    pub fn diameter(self) -> f32 {
        let size = self.max - self.min;
        size.x.max(size.y).max(size.z)
    }

    /// Bounding sphere radius based on all three dimensions.
    #[must_use]
    pub fn radius(self) -> f32 {
        ((self.max - self.min).length() * 0.5).max(0.0001)
    }
}

/// Reference to a texture image on disk or inside a loaded mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextureRef {
    /// Source path as resolved by the loader.
    pub path: std::path::PathBuf,
    /// Loaded texture index inside [`Mesh::textures`], when available.
    pub index: Option<usize>,
}

impl TextureRef {
    /// Create a texture reference with no loaded index.
    #[must_use]
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            path: path.into(),
            index: None,
        }
    }
}

/// Loaded RGBA texture image.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    /// Source path.
    pub path: std::path::PathBuf,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Average RGB color, precomputed for wireframe/points texture fallback.
    pub average_color: [u8; 3],

    /// Packed RGBA8 pixels in row-major order.
    pub rgba: Arc<[u8]>,
}
fn average_rgba_color(rgba: &[u8]) -> [u8; 3] {
    let mut total = [0_u64; 3];
    let mut samples = 0_u64;
    for pixel in rgba.chunks_exact(4) {
        total[0] += u64::from(pixel[0]);
        total[1] += u64::from(pixel[1]);
        total[2] += u64::from(pixel[2]);
        samples += 1;
    }
    if samples == 0 {
        return [255, 255, 255];
    }
    [
        (total[0] / samples) as u8,
        (total[1] / samples) as u8,
        (total[2] / samples) as u8,
    ]
}

impl Texture {
    /// Create a loaded texture from RGBA8 pixels.
    #[must_use]
    pub fn new(
        path: impl Into<std::path::PathBuf>,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Self {
        let average_color = average_rgba_color(&rgba);
        Self {
            path: path.into(),
            width,
            height,
            rgba: Arc::from(rgba),
            average_color,
        }
    }
    /// Return the precomputed average RGB color for fast non-solid texture coloring.
    #[must_use]
    pub const fn average_color(&self) -> [u8; 3] {
        self.average_color
    }

    /// Sample the texture using normalized UV coordinates.
    #[must_use]
    pub fn sample_nearest(
        &self,
        uv: Vec2,
        wrap: crate::config::TextureWrap,
        flip_v: bool,
    ) -> [u8; 4] {
        if self.width == 0 || self.height == 0 || self.rgba.len() < 4 {
            return [255, 255, 255, 255];
        }
        let mut u = match wrap {
            crate::config::TextureWrap::Repeat => uv.u.rem_euclid(1.0),
            crate::config::TextureWrap::Clamp => uv.u.clamp(0.0, 1.0),
        };
        let mut v = match wrap {
            crate::config::TextureWrap::Repeat => uv.v.rem_euclid(1.0),
            crate::config::TextureWrap::Clamp => uv.v.clamp(0.0, 1.0),
        };
        if flip_v {
            v = 1.0 - v;
        }
        // Keep exact 1.0 on the last pixel for clamp mode.
        if matches!(wrap, crate::config::TextureWrap::Repeat) {
            u = u.fract();
            v = v.fract();
        }
        let x = (u * (self.width.saturating_sub(1)) as f32).round() as u32;
        let y = (v * (self.height.saturating_sub(1)) as f32).round() as u32;
        let idx = ((y as usize * self.width as usize) + x as usize) * 4;
        self.rgba
            .get(idx..idx + 4)
            .map_or([255, 255, 255, 255], |p| [p[0], p[1], p[2], p[3]])
    }
}

/// How a material's alpha channel is interpreted, mirroring glTF `alphaMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AlphaMode {
    /// Alpha is ignored; the surface is fully opaque.
    #[default]
    Opaque,
    /// Alpha acts as a hard cutout against [`Material::alpha_cutoff`].
    Mask,
    /// Alpha blends the surface over what is behind it.
    Blend,
}

/// Diffuse material metadata.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Material {
    /// Material name from the source file.
    pub name: String,
    /// Diffuse color as normalized RGB.
    pub diffuse: [f32; 3],
    /// Optional diffuse texture map (`map_Kd`).
    pub diffuse_texture: Option<TextureRef>,
    /// Base-color alpha factor in `[0, 1]`. glTF base-color alpha; `1.0` for OBJ.
    pub base_color_alpha: f32,
    /// How alpha is interpreted while rasterizing.
    pub alpha_mode: AlphaMode,
    /// Cutoff used when [`Self::alpha_mode`] is [`AlphaMode::Mask`].
    pub alpha_cutoff: f32,
    /// Render both faces, ignoring back-face culling (glTF `doubleSided`).
    pub double_sided: bool,
    /// Emissive color factor as normalized RGB. `[0, 0, 0]` disables emission.
    pub emissive: [f32; 3],
    /// Optional emissive texture map.
    pub emissive_texture: Option<TextureRef>,
    /// Material is unlit: shade with the flat base color, ignoring scene lighting
    /// (glTF `KHR_materials_unlit`). Defaults to `false`.
    pub unlit: bool,
}

impl Material {
    /// Create a material with a white diffuse color and opaque, one-sided defaults.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            diffuse: [1.0, 1.0, 1.0],
            diffuse_texture: None,
            base_color_alpha: 1.0,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            emissive: [0.0, 0.0, 0.0],
            emissive_texture: None,
            unlit: false,
        }
    }

    /// Convert diffuse color to Ratatui RGB.
    #[must_use]
    pub fn color(&self) -> Color {
        let [r, g, b] = self.diffuse;
        Color::Rgb(
            (r.clamp(0.0, 1.0) * 255.0).round() as u8,
            (g.clamp(0.0, 1.0) * 255.0).round() as u8,
            (b.clamp(0.0, 1.0) * 255.0).round() as u8,
        )
    }

    /// Whether the material emits any light (non-zero emissive factor or a map).
    #[must_use]
    pub fn is_emissive(&self) -> bool {
        self.emissive_texture.is_some() || self.emissive.iter().any(|&c| c > 0.0)
    }
}

/// A polygonal face. Indices reference [`Mesh::vertices`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Face {
    /// Triangle or polygon indices. The renderer triangulates fans at draw time.
    pub indices: Vec<usize>,
    /// Optional texture-coordinate indices aligned with [`Self::indices`].
    pub tex_coord_indices: Vec<Option<usize>>,
    /// Optional normal indices aligned with [`Self::indices`].
    pub normal_indices: Vec<Option<usize>>,
    /// Optional source/material name.
    pub material: Option<String>,
    /// Optional face normal from file, otherwise computed from vertices.
    pub normal: Option<Vec3>,
}

impl Face {
    /// Create a face from vertex indices.
    #[must_use]
    pub fn new(indices: Vec<usize>) -> Self {
        let len = indices.len();
        Self {
            indices,
            tex_coord_indices: vec![None; len],
            normal_indices: vec![None; len],
            material: None,
            normal: None,
        }
    }

    /// Create a face with explicit texture-coordinate and normal indices.
    #[must_use]
    pub fn with_attributes(
        indices: Vec<usize>,
        tex_coord_indices: Vec<Option<usize>>,
        normal_indices: Vec<Option<usize>>,
    ) -> Self {
        Self {
            indices,
            tex_coord_indices,
            normal_indices,
            material: None,
            normal: None,
        }
    }
}

/// Loaded renderable mesh.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mesh {
    /// Source/display name.
    pub name: String,
    /// Vertex positions.
    pub vertices: Vec<Vec3>,
    /// Original bind-pose vertex positions used for animation sampling.
    pub bind_vertices: Vec<Vec3>,
    /// Texture coordinates.
    pub tex_coords: Vec<Vec2>,
    /// Vertex normals from OBJ files.
    pub normals: Vec<Vec3>,
    /// Original bind-pose vertex normals used for animation sampling.
    pub bind_normals: Vec<Vec3>,
    /// Faces/polygons.
    pub faces: Vec<Face>,
    /// Materials referenced by faces.
    pub materials: Vec<Material>,
    /// Loaded textures referenced by materials or manual overrides.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub textures: Vec<Texture>,
    /// Texture used when a face has UVs but no material texture.
    pub default_texture: Option<TextureRef>,
    /// Whether texture V coordinates should be flipped during sampling.
    pub flip_texture_v: bool,
    /// Embedded animation clips. glTF/GLB may populate this; OBJ leaves it empty.
    pub animations: Vec<AnimationClip>,
    /// Scene-node metadata for applying animation to flattened vertices.
    pub animation_nodes: Vec<AnimationNode>,
    /// glTF skin bindings for CPU joint animation.
    pub skins: Vec<SkinBinding>,
    /// Cached bounds.
    pub bounds: Bounds,
}

impl Mesh {
    /// Build a mesh from parts and compute bounds.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyMesh`] when vertices or faces are empty.
    pub fn new(
        name: impl Into<String>,
        vertices: Vec<Vec3>,
        faces: Vec<Face>,
        materials: Vec<Material>,
    ) -> Result<Self> {
        Self::with_attributes(name, vertices, Vec::new(), Vec::new(), faces, materials)
    }

    /// Build a mesh with OBJ attributes and compute bounds.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyMesh`] when vertices or faces are empty.
    pub fn with_attributes(
        name: impl Into<String>,
        vertices: Vec<Vec3>,
        tex_coords: Vec<Vec2>,
        normals: Vec<Vec3>,
        faces: Vec<Face>,
        materials: Vec<Material>,
    ) -> Result<Self> {
        if vertices.is_empty() || faces.is_empty() {
            return Err(Error::EmptyMesh);
        }
        let bounds = Bounds::from_vertices(&vertices).ok_or(Error::EmptyMesh)?;
        let bind_vertices = vertices.clone();
        let bind_normals = normals.clone();
        Ok(Self {
            name: name.into(),
            vertices,
            bind_vertices,
            tex_coords,
            normals,
            bind_normals,
            faces,
            materials,
            textures: Vec::new(),
            default_texture: None,
            flip_texture_v: true,
            animations: Vec::new(),
            animation_nodes: Vec::new(),
            skins: Vec::new(),
            bounds,
        })
    }

    /// Load a mesh from a path, using enabled format features.
    ///
    /// # Errors
    ///
    /// Returns an error when loading, parsing, or format dispatch fails.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        loader::load(path.as_ref())
    }

    /// Load a mesh with extra loader options such as texture override.
    ///
    /// # Errors
    ///
    /// Returns an error when loading, parsing, format dispatch, or strict companion asset loading fails.
    pub fn load_with_options(
        path: impl AsRef<Path>,
        options: loader::MeshLoadOptions,
    ) -> Result<Self> {
        loader::load_with_options(path.as_ref(), &options)
    }

    /// Load a mesh and attach a texture for OBJ files with UVs but no MTL.
    ///
    /// # Errors
    ///
    /// Returns an error when mesh loading fails or strict texture decoding fails.
    #[cfg(feature = "textures")]
    pub fn load_textured(path: impl AsRef<Path>, texture_path: impl AsRef<Path>) -> Result<Self> {
        Self::load_with_options(
            path,
            loader::MeshLoadOptions::default().texture_override(texture_path.as_ref()),
        )
    }

    /// Find a material by name.
    #[must_use]
    pub fn material(&self, name: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.name == name)
    }

    /// Return a normalized copy centered around the origin and roughly radius 1.
    #[must_use]
    pub fn normalized(&self) -> Self {
        let center = self.bounds.center();
        let radius = self.bounds.radius();
        let vertices = self
            .vertices
            .iter()
            .map(|&v| (v - center) / radius)
            .collect::<Vec<_>>();
        let bounds = Bounds::from_vertices(&vertices).unwrap_or(self.bounds);
        let bind_vertices = self
            .bind_vertices
            .iter()
            .map(|&v| (v - center) / radius)
            .collect::<Vec<_>>();
        Self {
            name: self.name.clone(),
            vertices,
            bind_vertices,
            tex_coords: self.tex_coords.clone(),
            normals: self.normals.clone(),
            bind_normals: self.bind_normals.clone(),
            faces: self.faces.clone(),
            materials: self.materials.clone(),
            textures: self.textures.clone(),
            default_texture: self.default_texture.clone(),
            flip_texture_v: self.flip_texture_v,
            animations: self.animations.clone(),
            animation_nodes: self.animation_nodes.clone(),
            skins: self.skins.clone(),
            bounds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_center_and_radius() {
        let b =
            Bounds::from_vertices(&[Vec3::new(-1.0, 0.0, 1.0), Vec3::new(3.0, 2.0, 1.0)]).unwrap();
        assert_eq!(b.center(), Vec3::new(1.0, 1.0, 1.0));
        assert!(b.radius() > 2.0);
    }

    #[test]
    fn normalizes_mesh() {
        let mesh = Mesh::new(
            "tri",
            vec![
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(12.0, 0.0, 0.0),
                Vec3::new(10.0, 2.0, 0.0),
            ],
            vec![Face::new(vec![0, 1, 2])],
            vec![],
        )
        .unwrap();
        let normalized = mesh.normalized();
        assert!(normalized.bounds.radius() <= 1.01);
        assert!(normalized.animations.is_empty());
        assert!(normalized.animation_nodes.is_empty());
    }
}
