//! The same byte signatures for local files, downloads and channel attachments.
pub fn media_type(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if head.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

/// Vision parts use static PNG/JPEG across providers. Original GIF/WebP bytes
/// remain the caller's download/send asset; decoding here never replaces them.
pub fn for_model(bytes: &[u8]) -> Result<kanzei_harness::ToolImage, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::io::Cursor;
    let media_type = media_type(bytes).ok_or("Unsupported image bytes")?;
    let converted;
    let (media_type, pixels) = if matches!(media_type, "image/gif" | "image/webp") {
        // Bound decoded frame memory as well as compressed download size.
        const MAX_DECODE_BYTES: u64 = 64 * 1024 * 1024;
        const MAX_PREVIEW_BYTES: usize = 16 * 1024 * 1024;
        let mut reader = ::image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|error| error.to_string())?;
        let mut limits = ::image::Limits::default();
        limits.max_alloc = Some(MAX_DECODE_BYTES);
        reader.limits(limits);
        let frame = reader
            .decode()
            .map_err(|error| format!("Cannot decode static image preview: {error}"))?;
        let mut png = Cursor::new(Vec::new());
        frame
            .write_to(&mut png, ::image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        converted = png.into_inner();
        if converted.len() > MAX_PREVIEW_BYTES {
            return Err("Static image preview exceeds 16 MiB".into());
        }
        ("image/png", converted.as_slice())
    } else {
        (media_type, bytes)
    };
    Ok(kanzei_harness::ToolImage {
        media_type: media_type.into(),
        data: STANDARD.encode(pixels),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    #[test]
    fn animated_sources_have_decodable_static_previews_without_changing_originals() {
        let mut original = Vec::new();
        {
            let mut encoder = ::image::codecs::gif::GifEncoder::new(&mut original);
            encoder
                .encode_frames([
                    ::image::Frame::new(::image::RgbaImage::from_pixel(
                        2,
                        2,
                        ::image::Rgba([255, 0, 0, 255]),
                    )),
                    ::image::Frame::new(::image::RgbaImage::from_pixel(
                        2,
                        2,
                        ::image::Rgba([0, 0, 255, 255]),
                    )),
                ])
                .unwrap();
        }
        let before = original.clone();
        let preview = for_model(&original).unwrap();
        assert_eq!(preview.media_type, "image/png");
        let decoded = ::image::load_from_memory(&STANDARD.decode(preview.data).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (2, 2));
        assert!(decoded.pixels().all(|pixel| pixel.0 == [255, 0, 0, 255]));
        assert_eq!(original, before);
        let mut oversized = original.clone();
        oversized[6..10].copy_from_slice(&[255, 255, 255, 255]);
        assert!(
            for_model(&oversized).is_err(),
            "decoded canvas must respect the memory budget"
        );
        assert!(for_model(b"GIF89a broken").is_err());
        assert!(for_model(b"<html>not image</html>").is_err());
    }
}
