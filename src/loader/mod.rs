use std::path::{Path, PathBuf};

use crate::{model::Mesh, Error, Result};

#[cfg(feature = "gltf")]
pub mod gltf;

#[cfg(feature = "mtl")]
pub mod mtl;
#[cfg(feature = "obj")]
pub mod obj;
#[cfg(feature = "stl")]
pub mod stl;
#[cfg(feature = "textures")]
pub mod texture;

/// Options used while loading meshes and optional companion assets.
#[derive(Debug, Clone, Default)]
pub struct MeshLoadOptions {
    /// Texture to use for faces with UVs when no material texture is assigned.
    pub texture_override: Option<PathBuf>,
    /// Load textures referenced from MTL `map_Kd` entries.
    pub load_material_textures: bool,
    /// Treat missing or undecodable textures as errors instead of falling back.
    pub strict_textures: bool,
}

impl MeshLoadOptions {
    /// Use a manually supplied texture image for OBJ files that contain UVs.
    #[must_use]
    pub fn texture_override(mut self, path: impl Into<PathBuf>) -> Self {
        self.texture_override = Some(path.into());
        self
    }

    /// Enable or disable loading material texture maps.
    #[must_use]
    pub fn load_material_textures(mut self, enabled: bool) -> Self {
        self.load_material_textures = enabled;
        self
    }

    /// Configure whether texture loading failures should be hard errors.
    #[must_use]
    pub fn strict_textures(mut self, strict: bool) -> Self {
        self.strict_textures = strict;
        self
    }
}

/// Load a mesh by file extension.
///
/// # Errors
///
/// Returns an error when the file extension is unsupported or the selected loader cannot read or parse the mesh.
pub fn load(path: &Path) -> Result<Mesh> {
    load_with_options(path, &MeshLoadOptions::default())
}

/// Load a mesh by file extension with loader options.
///
/// # Errors
///
/// Returns an error when the file extension is unsupported or the selected loader cannot read the mesh or companion assets.
pub fn load_with_options(path: &Path, options: &MeshLoadOptions) -> Result<Mesh> {
    #[cfg(not(feature = "obj"))]
    let _ = options;
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        #[cfg(feature = "gltf")]
        "gltf" | "glb" => gltf::load_gltf(path, options),
        #[cfg(feature = "obj")]
        "obj" => obj::load_obj_with_options(path, options),
        #[cfg(feature = "stl")]
        "stl" => stl::load_stl(path),
        _ => Err(Error::UnsupportedFormat {
            path: path.to_path_buf(),
        }),
    }
}
