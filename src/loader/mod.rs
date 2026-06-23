use std::path::Path;

use crate::{model::Mesh, Error, Result};

#[cfg(feature = "mtl")]
pub mod mtl;
#[cfg(feature = "obj")]
pub mod obj;
#[cfg(feature = "stl")]
pub mod stl;

/// Load a mesh by file extension.
pub fn load(path: &Path) -> Result<Mesh> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        #[cfg(feature = "obj")]
        "obj" => obj::load_obj(path),
        #[cfg(feature = "stl")]
        "stl" => stl::load_stl(path),
        _ => Err(Error::UnsupportedFormat {
            path: path.to_path_buf(),
        }),
    }
}
