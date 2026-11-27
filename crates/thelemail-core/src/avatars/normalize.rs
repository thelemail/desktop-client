use std::io::Cursor;

use image::{ImageFormat as Codec, ImageReader, Limits, imageops::FilterType};
use resvg::{tiny_skia, usvg};

use super::policy::{self, ImageFormat};

pub const EDGE: u32 = 128;
const MAX_SIDE: u32 = 4096;
const MAX_ALLOC: u64 = 64 * 1024 * 1024;

fn codec(format: ImageFormat) -> Option<Codec> {
    match format {
        ImageFormat::Png => Some(Codec::Png),
        ImageFormat::Jpeg => Some(Codec::Jpeg),
        ImageFormat::Gif => Some(Codec::Gif),
        ImageFormat::Webp => Some(Codec::WebP),
        ImageFormat::Svg => None,
    }
}

fn encode(image: &image::RgbaImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), Codec::Png)
        .ok()?;
    Some(out)
}

pub fn photo(bytes: &[u8]) -> Option<Vec<u8>> {
    let format = codec(policy::photo(bytes)?)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let side = decoded.width().min(decoded.height()).clamp(1, EDGE);
    let scaled = decoded.resize_to_fill(side, side, FilterType::Lanczos3);
    encode(&scaled.to_rgba8())
}

fn sealed_options() -> usvg::Options<'static> {
    usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    }
}

pub fn logo(svg: &[u8]) -> Option<Vec<u8>> {
    if !policy::logo(svg) {
        return None;
    }
    let tree = usvg::Tree::from_data(svg, &sealed_options()).ok()?;
    let size = tree.size();
    let (w, h) = (size.width(), size.height());
    if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let scale = EDGE as f32 / w.max(h);
    let (pw, ph) = (
        ((w * scale).round() as u32).clamp(1, EDGE),
        ((h * scale).round() as u32).clamp(1, EDGE),
    );
    let mut pixmap = tiny_skia::Pixmap::new(pw, ph)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 120, 60, 255]));
        encode(&image).expect("png")
    }

    fn dimensions(bytes: &[u8]) -> (u32, u32) {
        let image = image::load_from_memory_with_format(bytes, Codec::Png).expect("decode");
        (image.width(), image.height())
    }

    #[test]
    fn a_large_photo_is_scaled_down_to_a_square() {
        assert_eq!(
            dimensions(&photo(&png(900, 600)).expect("photo")),
            (EDGE, EDGE)
        );
    }

    #[test]
    fn a_small_photo_is_never_upscaled() {
        assert_eq!(dimensions(&photo(&png(40, 64)).expect("photo")), (40, 40));
    }

    #[test]
    fn a_truncated_photo_is_refused() {
        let mut bytes = png(64, 64);
        bytes.truncate(bytes.len() / 2);
        assert_eq!(photo(&bytes), None);
        assert_eq!(photo(b"\x89PNG\r\n\x1a\nnot really"), None);
    }

    #[test]
    fn a_logo_is_rasterised_to_fit() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4000" height="2000" viewBox="0 0 4 2"><rect width="4" height="2" fill="red"/></svg>"#;
        assert_eq!(dimensions(&logo(svg).expect("logo")), (EDGE, EDGE / 2));
    }

    #[test]
    fn a_logo_that_references_files_is_never_rendered() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4"><image href="/etc/hosts" width="4" height="4"/></svg>"#;
        assert_eq!(logo(svg), None);
    }

    #[test]
    fn the_sealed_resolver_loads_nothing_even_past_the_filter() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 4 4"><image xlink:href="/etc/hosts" width="4" height="4"/></svg>"#;
        let tree = usvg::Tree::from_data(svg, &sealed_options()).expect("parse");
        assert!(!tree.root().has_children());
    }
}
