use std::{collections::HashMap, fs, path::Path};

use crate::{
    model::{Material, TextureRef},
    Error, Result,
};

/// Load Wavefront MTL diffuse materials from disk.
///
/// # Errors
///
/// Returns an error when the file cannot be read or contains malformed supported material statements.
pub fn load_mtl(path: &Path) -> Result<Vec<Material>> {
    let text = fs::read_to_string(path).map_err(|err| Error::io(path, err))?;
    parse_mtl(path, &text)
}

/// Parse Wavefront MTL content.
///
/// # Errors
///
/// Returns an error when supported statements such as `newmtl` or `Kd` are malformed.
pub fn parse_mtl(path: &Path, text: &str) -> Result<Vec<Material>> {
    let mut materials = Vec::new();
    let mut current: Option<Material> = None;
    let base = path.parent().unwrap_or_else(|| Path::new("."));

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
                current = Some(Material::new(name));
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
            "map_Kd" => {
                if let Some(texture_path) = parse_texture_path(parts.collect::<Vec<_>>()) {
                    if let Some(material) = current.as_mut() {
                        material.diffuse_texture = Some(TextureRef::new(base.join(texture_path)));
                    }
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

fn parse_texture_path(tokens: Vec<&str>) -> Option<String> {
    let mut path_tokens = Vec::new();
    let mut skip = 0usize;
    for token in tokens {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        if token.starts_with('-') {
            skip = match token {
                "-blendu" | "-blendv" | "-boost" | "-mm" | "-o" | "-s" | "-t" => 1,
                "-texres" | "-clamp" | "-bm" | "-imfchan" | "-type" => 1,
                _ => 0,
            };
            if matches!(token, "-mm" | "-o" | "-s" | "-t") {
                skip = 2;
            }
            continue;
        }
        path_tokens.push(token);
    }
    (!path_tokens.is_empty()).then(|| path_tokens.join(" ").trim_matches('"').to_string())
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

    #[test]
    fn parses_diffuse_texture_map() {
        let materials = parse_mtl(
            Path::new("assets/model.mtl"),
            "newmtl skin\nKd 1 1 1\nmap_Kd -s 1 1 texture dir/base color.png\n",
        )
        .unwrap();
        assert_eq!(
            materials[0].diffuse_texture.as_ref().unwrap().path,
            Path::new("assets/texture dir/base color.png")
        );
    }
}
