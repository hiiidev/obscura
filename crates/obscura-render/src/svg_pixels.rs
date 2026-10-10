//! SVG-as-image decoding. No document script, file resolver or network client
//! is involved. Data images share a bounded budget across nested SVGs.
use super::{bounded_length, raster_image_dimensions, MAX_DECODE_BYTES};
use std::{
    borrow::Cow,
    io::Read,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_EMBEDDED_BYTES: usize = 32 * 1024 * 1024;
const MAX_XML_NODES: u32 = 100_000;
const MAX_XML_DEPTH: usize = 128;
const MAX_IMAGE_DEPTH: usize = 16;

pub(super) fn is_candidate(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x1f, 0x8b])
        || bytes
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(bytes)
            .iter()
            .find(|b| !b.is_ascii_whitespace())
            == Some(&b'<')
}

fn source(bytes: &[u8]) -> Option<Cow<'_, [u8]>> {
    if bytes.len() > MAX_SOURCE_BYTES {
        return None;
    }
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Some(Cow::Borrowed(bytes));
    }
    // usvg's default SVGZ reader has no decompressed-length limit.
    let mut output = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut output)
        .ok()?;
    (output.len() <= MAX_SOURCE_BYTES).then_some(Cow::Owned(output))
}

fn consume(remaining: &AtomicUsize, amount: usize) -> bool {
    remaining
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
            left.checked_sub(amount)
        })
        .is_ok()
}

struct Budget {
    encoded: AtomicUsize,
    raster: AtomicUsize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            encoded: AtomicUsize::new(MAX_EMBEDDED_BYTES),
            raster: AtomicUsize::new(MAX_DECODE_BYTES as usize),
        }
    }
}

fn embedded(
    mime: &str,
    data: Arc<Vec<u8>>,
    options: &usvg::Options,
    depth: usize,
    budget: &Arc<Budget>,
) -> Option<usvg::ImageKind> {
    if !consume(&budget.encoded, data.len()) {
        return None;
    }
    if matches!(mime, "image/svg+xml" | "text/plain") && is_candidate(&data) {
        return parse(&data, depth, budget).map(usvg::ImageKind::SVG);
    }
    // Check the decoded dimensions before resvg sees embedded raster bytes.
    // The default data resolver is used only after SVG has been excluded, so
    // it cannot recursively bypass the budgets or install a file resolver.
    let (width, height) = raster_image_dimensions(&data)?;
    if !consume(&budget.raster, bounded_length(width, height)?) {
        return None;
    }
    match mime {
        "image/jpg" | "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "text/plain" => {
            if is_candidate(&data) {
                return None;
            }
            (usvg::ImageHrefResolver::default_data_resolver())(mime, data, options)
        }
        _ => None,
    }
}

fn parse(bytes: &[u8], depth: usize, budget: &Arc<Budget>) -> Option<usvg::Tree> {
    if depth > MAX_IMAGE_DEPTH {
        return None;
    }
    let bytes = source(bytes)?;
    // Charge expanded SVGZ separately. Plain source is already bounded and
    // embedded bytes are charged by the resolver before reaching this call.
    if matches!(bytes, Cow::Owned(_)) && !consume(&budget.encoded, bytes.len()) {
        return None;
    }
    let xml = std::str::from_utf8(&bytes).ok()?;
    let document = usvg::roxmltree::Document::parse_with_options(
        xml,
        usvg::roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: MAX_XML_NODES,
            ..Default::default()
        },
    )
    .ok()?;
    let root = document.root_element();
    if root.tag_name().name() != "svg"
        || root.tag_name().namespace() != Some("http://www.w3.org/2000/svg")
    {
        return None;
    }
    if root
        .descendants()
        .any(|node| node.ancestors().take(MAX_XML_DEPTH + 1).count() > MAX_XML_DEPTH)
    {
        return None;
    }
    let shared = Arc::clone(budget);
    let options = usvg::Options {
        default_size: usvg::Size::from_wh(300.0, 150.0)?,
        font_family: "Liberation Serif".into(),
        fontdb: crate::paint::svg_font_database(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_string: Box::new(|_, _| None),
            resolve_data: Box::new(move |mime, data, options| {
                embedded(mime, data, options, depth + 1, &shared)
            }),
        },
        ..Default::default()
    };
    // usvg 0.47 resolves root percentages against a 100x100 view box
    // without a viewBox. External SVG images instead default each missing or
    // percentage intrinsic axis independently to 300x150 CSS pixels.
    let mut replacements = Vec::new();
    let mut missing = String::new();
    if root.attribute("viewBox").is_none() {
        for (name, fallback) in [("width", "300"), ("height", "150")] {
            match root.attribute_node(name) {
                None => missing.push_str(&format!(" {name}=\"{fallback}\"")),
                Some(attribute) if attribute.value().trim().strip_suffix('%')
                    .and_then(|value| value.parse::<f64>().ok())
                    .is_some_and(|value| value.is_finite() && value >= 0.0) => {
                    // Replace the parser-validated whole attribute range; do
                    // not infer quote/value offsets or duplicate attributes.
                    replacements.push((attribute.range(), format!("{name}=\"{fallback}\"")));
                }
                _ => {}
            }
        }
    }
    let tree = if !missing.is_empty() || !replacements.is_empty() {
        let name_start = root.range().start.checked_add(1)?;
        let name_end = name_start + xml.as_bytes().get(name_start..)?.iter()
            .position(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'/' | b'>'))?;
        replacements.push((name_end..name_end, missing));
        replacements.sort_by_key(|(range, _)| range.start);
        let mut normalized = xml.to_owned();
        for (range, value) in replacements.into_iter().rev() {
            normalized.replace_range(range, &value);
        }
        let normalized = usvg::roxmltree::Document::parse_with_options(
            &normalized,
            usvg::roxmltree::ParsingOptions {
                allow_dtd: true,
                nodes_limit: MAX_XML_NODES,
                ..Default::default()
            },
        ).ok()?;
        usvg::Tree::from_xmltree(&normalized, &options).ok()?
    } else {
        usvg::Tree::from_xmltree(&document, &options).ok()?
    };
    tree_dimensions(&tree)?;
    Some(tree)
}

fn tree_dimensions(tree: &usvg::Tree) -> Option<(u32, u32)> {
    let size = tree.size();
    let (width, height) = (size.width().ceil() as u32, size.height().ceil() as u32);
    bounded_length(width, height)?;
    Some((width, height))
}

pub(super) fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    tree_dimensions(&parse(bytes, 0, &Arc::new(Budget::default()))?)
}

pub(super) fn decode(bytes: &[u8], destination: &mut [u8]) -> bool {
    let Some(tree) = parse(bytes, 0, &Arc::new(Budget::default())) else {
        return false;
    };
    let Some((width, height)) = tree_dimensions(&tree) else {
        return false;
    };
    if bounded_length(width, height) != Some(destination.len()) {
        return false;
    }
    let Some(mut pixels) = tiny_skia::Pixmap::new(width, height) else {
        return false;
    };
    let size = tree.size();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(
            width as f32 / size.width(),
            height as f32 / size.height(),
        ),
        &mut pixels.as_mut(),
    );
    // The browser upload path applies UNPACK_PREMULTIPLY_ALPHA_WEBGL itself.
    // Return straight alpha here, as for PNG/JPEG and ImageData sources.
    for (source, target) in pixels.pixels().iter().zip(destination.chunks_exact_mut(4)) {
        let pixel = source.demultiply();
        target.copy_from_slice(&[pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    const SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='1'><path fill='red' d='M0 0h1v1H0z'/><path fill='blue' fill-opacity='.5' d='M1 0h1v1H1z'/></svg>";
    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut writer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        writer.write_all(bytes).unwrap();
        writer.finish().unwrap()
    }
    fn data_url(mime: &str, bytes: &[u8]) -> String {
        use base64::Engine;
        format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }
    #[test]
    fn svg_and_svgz_render_real_straight_alpha_pixels() {
        for bytes in [SVG.as_bytes().to_vec(), gzip(SVG.as_bytes())] {
            assert_eq!(dimensions(&bytes), Some((2, 1)));
            let mut pixels = [99; 8];
            assert!(decode(&bytes, &mut pixels));
            assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
            assert_eq!(&pixels[4..7], &[0, 0, 255]);
            assert!((127..=128).contains(&pixels[7]));
        }
    }
    #[test]
    fn xml_prologs_comments_namespaces_and_default_viewport_are_supported() {
        for prefix in ["", "\u{feff}", "<!-- image -->", "<?xml version='1.0'?>"] {
            let bytes = format!("{prefix}{SVG}");
            assert!(is_candidate(bytes.as_bytes()));
            assert_eq!(dimensions(bytes.as_bytes()), Some((2, 1)));
        }
        assert_eq!(
            dimensions(b"<s:svg xmlns:s='http://www.w3.org/2000/svg' width='3' height='4'/>"),
            Some((3, 4))
        );
        assert_eq!(
            dimensions(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
            Some((300, 150))
        );
        for content in ["", "<rect width='2' height='1' fill='red'/>"] {
            for prefix in ["", "<?xml version='1.0'?><!-- fixture -->"] {
                let bytes = format!("{prefix}<s:svg xmlns:s='http://www.w3.org/2000/svg'>{content}</s:svg>");
                assert_eq!(dimensions(bytes.as_bytes()), Some((300, 150)));
            }
        }
        // Chromium's external-image natural size defaults each unspecified
        // intrinsic axis; a percentage does not supply an intrinsic length.
        for (attributes, expected) in [
            ("height='40'", (300, 40)),
            ("width='80'", (80, 150)),
            ("width='50%'", (300, 150)),
            ("height='50%'", (300, 150)),
            ("width='50%' height='50%'", (300, 150)),
            ("width='80' height='50%'", (80, 150)),
            ("width='50%' height='40'", (300, 40)),
            ("width = '5&#48;%' height = \"40\"", (300, 40)),
        ] {
            let bytes = format!("<?xml version='1.0'?><!-- fixture --><s:svg xmlns:s='http://www.w3.org/2000/svg' {attributes}><s:rect width='100%' height='100%' fill='red'/></s:svg>");
            assert_eq!(dimensions(bytes.as_bytes()), Some(expected), "{attributes}");
        }
        let bytes = b"<svg xmlns='http://www.w3.org/2000/svg'><rect width='100%' height='100%' fill='red'/></svg>";
        let mut pixels = vec![0; 300 * 150 * 4];
        assert!(decode(bytes, &mut pixels));
        for offset in [0, (300 * 150 - 1) * 4] {
            assert_eq!(&pixels[offset..offset + 4], &[255, 0, 0, 255]);
        }
    }
    #[test]
    fn invalid_or_oversized_inputs_preserve_the_destination() {
        for bytes in [
            b"<svg".to_vec(),
            b"<html/>".to_vec(),
            b"<svg xmlns='wrong'/>".to_vec(),
            b"<svg xmlns='http://www.w3.org/2000/svg' width='32768' height='1'/>".to_vec(),
            b"<svg xmlns='http://www.w3.org/2000/svg' width='4097' height='4096'/>".to_vec(),
            vec![0x1f, 0x8b, 1, 2, 3],
        ] {
            let mut pixels = [77; 8];
            assert!(!decode(&bytes, &mut pixels));
            assert_eq!(pixels, [77; 8]);
        }
        for length in [0, 4, 9] {
            let mut pixels = vec![77; length];
            assert!(!decode(SVG.as_bytes(), &mut pixels));
            assert!(pixels.iter().all(|v| *v == 77));
        }
    }
    #[test]
    fn decompression_xml_depth_and_node_limits_are_enforced() {
        assert!(source(&vec![b' '; MAX_SOURCE_BYTES + 1]).is_none());
        assert!(source(&gzip(&vec![b' '; MAX_SOURCE_BYTES + 1])).is_none());
        let deep = format!(
            "<svg xmlns='http://www.w3.org/2000/svg'>{}{}{}</svg>",
            "<g>".repeat(MAX_XML_DEPTH),
            "<path d='M0 0h1v1z'/>",
            "</g>".repeat(MAX_XML_DEPTH)
        );
        assert!(dimensions(deep.as_bytes()).is_none());
        let nodes = format!(
            "<svg xmlns='http://www.w3.org/2000/svg'>{}</svg>",
            "<g/>".repeat(MAX_XML_NODES as usize)
        );
        assert!(dimensions(nodes.as_bytes()).is_none());
    }
    #[test]
    fn embedded_svg_and_raster_data_images_render_without_external_loading() {
        let nested = data_url("image/svg+xml", SVG.as_bytes());
        let bytes=format!("<svg xmlns='http://www.w3.org/2000/svg' width='2' height='1'><image width='2' height='1' href='{nested}'/></svg>");
        let mut pixels = [0; 8];
        assert!(decode(bytes.as_bytes(), &mut pixels));
        assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([12, 34, 56, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let uri = data_url("image/png", &png.into_inner());
        let bytes=format!("<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'><image width='1' height='1' href='{uri}'/></svg>");
        let mut pixels = [0; 4];
        assert!(decode(bytes.as_bytes(), &mut pixels));
        assert_eq!(pixels, [12, 34, 56, 255]);
    }
    #[test]
    fn embedded_budgets_and_recursion_limit_cannot_be_bypassed() {
        let opts = usvg::Options::default();
        let data = Arc::new(SVG.as_bytes().to_vec());
        let budget = Arc::new(Budget::default());
        assert!(embedded(
            "image/svg+xml",
            data.clone(),
            &opts,
            MAX_IMAGE_DEPTH + 1,
            &budget
        )
        .is_none());
        budget.encoded.store(0, Ordering::Relaxed);
        assert!(embedded("image/svg+xml", data, &opts, 1, &budget).is_none());
        let counter = AtomicUsize::new(3);
        assert!(!consume(&counter, 4));
        assert_eq!(counter.load(Ordering::Relaxed), 3);
        assert!(consume(&counter, 3));
        assert!(!consume(&counter, 1));
        let mut writer = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(1, 1)
            .write_to(&mut writer, image::ImageFormat::Png)
            .unwrap();
        let data = Arc::new(writer.into_inner());
        let budget = Arc::new(Budget::default());
        budget.raster.store(0, Ordering::Relaxed);
        assert!(embedded("image/png", data.clone(), &opts, 1, &budget).is_none());
        assert!(embedded(
            "application/octet-stream",
            data,
            &opts,
            1,
            &Arc::new(Budget::default())
        )
        .is_none());
        let expanded = gzip(SVG.as_bytes());
        let budget = Arc::new(Budget::default());
        budget.encoded.store(1, Ordering::Relaxed);
        assert!(parse(&expanded, 0, &budget).is_none());
    }
    #[test]
    fn svg_text_uses_the_shared_embedded_font_database() {
        let svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'><text x='1' y='16' font-family='Liberation Sans' font-size='16'>Hi</text></svg>";
        let tree = parse(svg, 0, &Arc::new(Budget::default())).unwrap();
        assert!(tree.has_text_nodes());
        let mut pixels = vec![0; 40 * 20 * 4];
        assert!(decode(svg, &mut pixels));
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
        assert!(Arc::ptr_eq(
            &crate::paint::svg_font_database(),
            &crate::paint::svg_font_database()
        ));
    }
    #[test]
    fn external_images_never_read_a_local_file_even_inside_data_svg() {
        let file =
            std::env::temp_dir().join(format!("obscura-svg-upload-{}.png", std::process::id()));
        struct Remove(std::path::PathBuf);
        impl Drop for Remove {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _remove = Remove(file.clone());
        image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]))
            .save(&file)
            .unwrap();
        for href in [
            file.to_str().unwrap().to_owned(),
            format!("file://{}", file.display()),
            "https://example.invalid/image.png".into(),
        ] {
            let inner=format!("<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'><image width='1' height='1' href='{href}'/></svg>");
            let nested=format!("<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'><image width='1' height='1' href='{}'/></svg>",data_url("image/svg+xml",inner.as_bytes()));
            for bytes in [inner.as_bytes(), nested.as_bytes()] {
                let mut pixels = [99; 4];
                assert!(decode(bytes, &mut pixels));
                assert_eq!(pixels, [0; 4]);
            }
        }
    }
}
