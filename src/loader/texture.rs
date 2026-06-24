use std::{fs, path::Path};

use crate::{model::Texture, Error, Result};

/// Load an image texture from disk, sniffing the image format from bytes.
pub fn load_texture(path: &Path) -> Result<Texture> {
    let bytes = fs::read(path).map_err(|err| Error::io(path, err))?;
    decode_texture(path, &bytes)
}

/// Decode image bytes into RGBA8 texture data.
pub fn decode_texture(path: &Path, bytes: &[u8]) -> Result<Texture> {
    let format = image::guess_format(bytes)
        .map_err(|err| Error::texture_decode(path, format!("unknown image format: {err}")))?;
    let image = image::load_from_memory_with_format(bytes, format)
        .map_err(|err| Error::texture_decode(path, err.to_string()))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(Texture::new(path, width, height, rgba.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_png_by_content_not_extension() {
        let mut bytes = Vec::new();
        let pixels = [
            255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        {
            let encoder = image::codecs::png::PngEncoder::new(&mut bytes);
            image::ImageEncoder::write_image(
                encoder,
                &pixels,
                2,
                2,
                image::ColorType::Rgba8.into(),
            )
            .unwrap();
        }
        let texture = decode_texture(Path::new("wrong.jpg"), &bytes).unwrap();
        assert_eq!(texture.width, 2);
        assert_eq!(texture.height, 2);
        assert_eq!(
            texture.sample_nearest(
                crate::model::Vec2::new(0.0, 0.0),
                crate::config::TextureWrap::Clamp,
                false
            ),
            [255, 0, 0, 255]
        );
    }
}
