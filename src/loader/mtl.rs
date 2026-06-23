use std::{collections::HashMap, fs, path::Path};

use crate::{model::Material, Error, Result};

/// Load Wavefront MTL diffuse materials from disk.
pub fn load_mtl(path: &Path) -> Result<Vec<Material>> {
    let text = fs::read_to_string(path).map_err(|err| Error::io(path, err))?;
    parse_mtl(path, &text)
}

/// Parse Wavefront MTL content.
pub fn parse_mtl(path: &Path, text: &str) -> Result<Vec<Material>> {
    let mut materials = Vec::new();
    let mut current: Option<Material> = None;

    for (line_index, raw_line) in text.lines().enumerate() {
        let line_number = line_index + 1;
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next().unwrap_or_default() {
            "newmtl" => {
                if let Some(material) = current.take() {
                    materials.push(material);
                }
                let name = parts.collect::<Vec<_>>().join(" ");
                if name.is_empty() {
                    return Err(Error::parse(
                        path,
                        Some(line_number),
                        "newmtl requires a material name",
                    ));
                }
                current = Some(Material {
                    name,
                    diffuse: [1.0, 1.0, 1.0],
                });
            }
            "Kd" => {
                let values = parts
                    .map(str::parse::<f32>)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|_| {
                        Error::parse(path, Some(line_number), "Kd requires three float values")
                    })?;
                if values.len() < 3 {
                    return Err(Error::parse(
                        path,
                        Some(line_number),
                        "Kd requires three float values",
                    ));
                }
                if let Some(material) = current.as_mut() {
                    material.diffuse = [values[0], values[1], values[2]];
                }
            }
            _ => {}
        }
    }

    if let Some(material) = current {
        materials.push(material);
    }

    let mut deduped = HashMap::<String, Material>::new();
    for material in materials {
        deduped.insert(material.name.clone(), material);
    }
    Ok(deduped.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_diffuse_colors() {
        let materials = parse_mtl(Path::new("inline.mtl"), "newmtl red\nKd 1 0.25 0\n").unwrap();
        assert_eq!(materials[0].name, "red");
        assert_eq!(materials[0].diffuse, [1.0, 0.25, 0.0]);
    }
}
