//! Pages written as a PDF file: what « Imprimer » and « Enregistrer en PDF » hand over.
//!
//! The file is written here, from the shapes [`super::render`] lays out: text stays text (it
//! can be selected and searched), lines stay sharp at any size. Text is set in Helvetica, which
//! every reader has, so no font is carried in the file.

use std::collections::HashMap;
use std::io::Write as _;

use base64::Engine as _;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use super::metrics::{latin1_byte, text_width, BASELINE, PT_TO_MM};
use super::render::{PageDrawing, Picture, Pictures, Rgb, Shape};

/// Points per millimetre.
const K: f64 = 72.0 / 25.4;

const WATERMARK: Rgb = Rgb::new(0xd9, 0x2d, 0x20);
/// How much of the page's diagonal the mark across it takes.
const WATERMARK_SPAN: f64 = 0.72;

/// The longest side a drawing (SVG) is turned into pixels at.
const SVG_PIXELS: f32 = 1200.0;

impl Picture {
    /// Read a picture file: PNG, JPEG, WebP, GIF, BMP, or an SVG drawing. `None` when it is
    /// none of these.
    pub fn decode(bytes: &[u8]) -> Option<Picture> {
        if let Ok(image) = image::load_from_memory(bytes) {
            let image = image.to_rgba8();
            return Some(Picture { width: image.width(), height: image.height(), rgba: image.into_raw() });
        }
        svg(bytes)
    }

    /// Read a picture written into its own address (`data:image/png;base64,…`).
    pub fn from_data_url(source: &str) -> Option<Picture> {
        Picture::decode(&data_url_bytes(source)?)
    }

    fn opaque(&self) -> bool {
        self.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255)
    }
}

/// The file a picture written into its own address (`data:image/png;base64,…`) holds.
pub fn data_url_bytes(source: &str) -> Option<Vec<u8>> {
    let (header, payload) = source.trim().strip_prefix("data:")?.split_once(',')?;
    if !header.ends_with(";base64") {
        return None;
    }
    base64::engine::general_purpose::STANDARD.decode(payload.trim()).ok()
}

fn svg(bytes: &[u8]) -> Option<Picture> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = SVG_PIXELS / size.width().max(size.height()).max(1.0);
    let (width, height) = ((size.width() * scale).ceil().max(1.0) as u32, (size.height() * scale).ceil().max(1.0) as u32);
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let plain = pixel.demultiply();
            [plain.red(), plain.green(), plain.blue(), plain.alpha()]
        })
        .collect();
    Some(Picture { width, height, rgba })
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing to memory does not fail.
    let _ = encoder.write_all(bytes);
    encoder.finish().unwrap_or_default()
}

/// The objects of the file being written, by number (from 1).
#[derive(Default)]
struct Objects(Vec<Vec<u8>>);

impl Objects {
    /// Keep a place for an object written later; its number.
    fn reserve(&mut self) -> usize {
        self.0.push(Vec::new());
        self.0.len()
    }

    fn set(&mut self, number: usize, body: impl Into<Vec<u8>>) {
        self.0[number - 1] = body.into();
    }

    fn add(&mut self, body: impl Into<Vec<u8>>) -> usize {
        let number = self.reserve();
        self.set(number, body);
        number
    }

    /// An object that carries bytes of its own, compressed.
    fn add_stream(&mut self, dictionary: &str, bytes: &[u8]) -> usize {
        let packed = deflate(bytes);
        let mut body = format!("<< {dictionary} /Filter /FlateDecode /Length {} >>\nstream\n", packed.len()).into_bytes();
        body.extend_from_slice(&packed);
        body.extend_from_slice(b"\nendstream");
        self.add(body)
    }
}

fn color(rgb: Rgb) -> String {
    format!("{:.3} {:.3} {:.3}", f64::from(rgb.r) / 255.0, f64::from(rgb.g) / 255.0, f64::from(rgb.b) / 255.0)
}

/// Text as PDF writes it in a page: the bytes of the Windows Latin-1 set, in hexadecimal.
fn hex_text(text: &str) -> String {
    text.chars().map(|c| format!("{:02X}", latin1_byte(c))).collect()
}

/// Text for the file's own description (its title): any character, as UTF-16.
fn hex_utf16(text: &str) -> String {
    std::iter::once(0xfeffu16).chain(text.encode_utf16()).map(|unit| format!("{unit:04X}")).collect()
}

/// What draws one page, as PDF operators. `images` names the pictures the file carries.
fn content(page: &PageDrawing, images: &HashMap<&str, usize>) -> String {
    let mut ops = String::new();
    let top = |y: f64| (page.height - y) * K;
    for shape in &page.shapes {
        match shape {
            Shape::Rect { x, y, w, h, fill, stroke, dashed } => {
                let paint = match (fill, stroke) {
                    (Some(_), Some(_)) => "B",
                    (Some(_), None) => "f",
                    (None, Some(_)) => "S",
                    (None, None) => continue,
                };
                ops.push_str("q ");
                if let Some(fill) = fill {
                    ops.push_str(&format!("{} rg ", color(*fill)));
                }
                if let Some((stroke, width)) = stroke {
                    ops.push_str(&format!("{} RG {:.3} w ", color(*stroke), width * K));
                    if *dashed {
                        ops.push_str("[3 2] 0 d ");
                    }
                }
                ops.push_str(&format!("{:.2} {:.2} {:.2} {:.2} re {paint} Q\n", x * K, top(y + h), w * K, h * K));
            }
            Shape::Line { x1, y1, x2, y2, color: stroke, width } => {
                ops.push_str(&format!("q {} RG {:.3} w {:.2} {:.2} m {:.2} {:.2} l S Q\n", color(*stroke), width * K, x1 * K, top(*y1), x2 * K, top(*y2)));
            }
            Shape::Text { x, y, text, size, bold, color: ink } => {
                let baseline = y + BASELINE * size * PT_TO_MM;
                let font = if *bold { "F2" } else { "F1" };
                ops.push_str(&format!("BT /{font} {size:.2} Tf {} rg {:.2} {:.2} Td <{}> Tj ET\n", color(*ink), x * K, top(baseline), hex_text(text)));
            }
            Shape::Image { x, y, w, h, source } => {
                if let Some(number) = images.get(source.as_str()) {
                    ops.push_str(&format!("q {:.2} 0 0 {:.2} {:.2} {:.2} cm /Im{number} Do Q\n", w * K, h * K, x * K, top(y + h)));
                }
            }
            Shape::Watermark { text } => {
                let (width, height) = (page.width * K, page.height * K);
                // Sized to span most of the diagonal, from the top left to the bottom right.
                let unit = text_width(text, 1.0, true) * K;
                if unit <= 0.0 {
                    continue;
                }
                let size = WATERMARK_SPAN * width.hypot(height) / unit;
                let angle = -(height.atan2(width));
                let (cos, sin) = (angle.cos(), angle.sin());
                let along = unit * size / 2.0;
                // Half the height of a capital, so the word is centred on the diagonal.
                let down = 0.36 * size;
                let (x, y) = (width / 2.0 - along * cos + down * sin, height / 2.0 - along * sin - down * cos);
                ops.push_str(&format!(
                    "q /GS1 gs BT /F2 {size:.2} Tf {} rg {cos:.5} {sin:.5} {:.5} {cos:.5} {x:.2} {y:.2} Tm <{}> Tj ET Q\n",
                    color(WATERMARK),
                    -sin,
                    hex_text(text)
                ));
            }
            // Editor marks: never on paper.
            Shape::Badge { .. } | Shape::Placeholder { .. } => {}
        }
    }
    ops
}

/// Write `pages` as a PDF file. `pictures` holds the ones the pages show; `title` is what a
/// reader displays as the document's name.
pub fn write(pages: &[PageDrawing], pictures: &Pictures, title: &str) -> Vec<u8> {
    let mut objects = Objects::default();
    let catalog = objects.reserve();
    let tree = objects.reserve();
    let regular = objects.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
    let bold = objects.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>");
    let faint = objects.add("<< /Type /ExtGState /ca 0.16 /CA 0.16 >>");

    // Each picture goes in the file once, however many pages show it.
    let mut images: HashMap<&str, usize> = HashMap::new();
    for shape in pages.iter().flat_map(|page| &page.shapes) {
        let Shape::Image { source, .. } = shape else { continue };
        let Some(picture) = pictures.get(source).filter(|_| !images.contains_key(source.as_str())) else { continue };
        let size = format!("/Width {} /Height {} /BitsPerComponent 8", picture.width, picture.height);
        let mask = if picture.opaque() {
            String::new()
        } else {
            let alpha: Vec<u8> = picture.rgba.chunks_exact(4).map(|pixel| pixel[3]).collect();
            format!(" /SMask {} 0 R", objects.add_stream(&format!("/Type /XObject /Subtype /Image {size} /ColorSpace /DeviceGray"), &alpha))
        };
        let rgb: Vec<u8> = picture.rgba.chunks_exact(4).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect();
        images.insert(source, objects.add_stream(&format!("/Type /XObject /Subtype /Image {size} /ColorSpace /DeviceRGB{mask}"), &rgb));
    }
    let xobjects: String = images.values().map(|number| format!("/Im{number} {number} 0 R ")).collect();
    let resources = format!("<< /Font << /F1 {regular} 0 R /F2 {bold} 0 R >> /ExtGState << /GS1 {faint} 0 R >> /XObject << {xobjects}>> >>");

    let mut kids = Vec::new();
    for page in pages {
        let drawn = objects.add_stream("", content(page, &images).as_bytes());
        kids.push(objects.add(format!(
            "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 {:.2} {:.2}] /Contents {drawn} 0 R /Resources {resources} >>",
            page.width * K,
            page.height * K
        )));
    }
    let kid_refs: String = kids.iter().map(|number| format!("{number} 0 R ")).collect();
    objects.set(tree, format!("<< /Type /Pages /Kids [{kid_refs}] /Count {} >>", kids.len()));
    objects.set(catalog, format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
    let info = objects.add(format!("<< /Title <{}> /Producer (Motodesk) >>", hex_utf16(title)));

    // The four bytes above 127 on the second line tell a reader the file is not plain text.
    let mut file = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.0.iter().enumerate() {
        offsets.push(file.len());
        file.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        file.extend_from_slice(body);
        file.extend_from_slice(b"\nendobj\n");
    }
    let table = file.len();
    file.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for offset in &offsets {
        file.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    file.extend_from_slice(format!("trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{table}\n%%EOF\n", offsets.len() + 1).as_bytes());
    file
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use flate2::read::ZlibDecoder;
    use serde_json::{json, Value};

    use super::super::render::{document, draw, Mode};
    use super::super::sample::{demo_shop, sample_data, LOGO_SOURCE};
    use super::super::schema::{Element, Fit, ImageEl, Kind, Layout};
    use super::super::starters::starter_layout;
    use super::super::TemplateKind;
    use super::*;

    fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
        haystack[from..].windows(needle.len()).position(|window| window == needle).map(|at| at + from)
    }

    /// What every stream of the file holds once unpacked, in the order they are written.
    fn streams(file: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut at = 0;
        while let Some(start) = find(file, b"stream\n", at) {
            let end = find(file, b"\nendstream", start).expect("a stream ends");
            let mut plain = Vec::new();
            ZlibDecoder::new(&file[start + 7..end]).read_to_end(&mut plain).expect("a stream unpacks");
            out.push(plain);
            at = end + 10;
        }
        out
    }

    #[test]
    fn the_file_is_one_a_reader_can_follow() {
        let layout = starter_layout(TemplateKind::Facture);
        let data = sample_data(TemplateKind::Facture, &demo_shop(), 30);
        let file = write(&document(&layout, &data, &Pictures::new(), None), &Pictures::new(), "Facture FA-2026-C1-00042 — Léa");
        assert!(file.starts_with(b"%PDF-1.4\n"));
        assert!(file.ends_with(b"%%EOF\n"));

        // The table at the end says where each object starts: each of them is there.
        let table = find(&file, b"xref\n", 0).expect("a table of objects");
        let announced: usize = String::from_utf8_lossy(&file[find(&file, b"startxref\n", table).unwrap() + 10..]).lines().next().unwrap().parse().unwrap();
        assert_eq!(announced, table);
        let lines: Vec<String> = String::from_utf8_lossy(&file[table..]).lines().map(str::to_string).collect();
        let count: usize = lines[1].strip_prefix("0 ").unwrap().parse().unwrap();
        for number in 1..count {
            let offset: usize = lines[2 + number][..10].parse().unwrap();
            assert!(file[offset..].starts_with(format!("{number} 0 obj\n").as_bytes()), "object {number} is not at {offset}");
        }
        let text = String::from_utf8_lossy(&file);
        assert!(text.contains("/Count 1") && text.contains("/MediaBox [0 0 595.28 841.89]"), "one A4 page");
        assert!(text.contains("/BaseFont /Helvetica-Bold") && text.contains("/WinAnsiEncoding"));
        // The title keeps its accents and its dash.
        assert!(text.contains(&format!("/Title <{}>", hex_utf16("Facture FA-2026-C1-00042 — Léa"))));
    }

    #[test]
    fn text_is_written_as_text_with_its_accents_and_its_euro_sign() {
        let layout = starter_layout(TemplateKind::Facture);
        let data = sample_data(TemplateKind::Facture, &demo_shop(), 30);
        let file = write(&document(&layout, &data, &Pictures::new(), None), &Pictures::new(), "Facture");
        let page = String::from_utf8(streams(&file).remove(0)).unwrap();
        assert!(page.contains(&format!("<{}> Tj", hex_text("FACTURE"))));
        assert!(page.contains(&format!("<{}> Tj", hex_text("Léa Martin"))));
        assert_eq!(hex_text("é€ "), "E98020");
        assert!(page.contains(&hex_text("55,68\u{a0}€")));
        assert!(page.contains("/F2 16.00 Tf"), "the title is bold, 16 points");
        // The band at the top is filled with the shop's navy.
        assert!(page.contains("0.118 0.227 0.373 rg"));
        assert!(!page.contains("/GS1 gs"), "no mark on an original");
    }

    #[test]
    fn a_copy_carries_its_mark_on_every_page() {
        let layout = starter_layout(TemplateKind::Facture);
        let mut data = sample_data(TemplateKind::Facture, &demo_shop(), 30);
        data["items"] = Value::Array((1..=30).map(|n| json!({ "label": format!("Ligne {n}") })).collect());
        let file = write(&document(&layout, &data, &Pictures::new(), Some("Duplicata")), &Pictures::new(), "Facture");
        assert!(String::from_utf8_lossy(&file).contains("/Count 3"));
        let pages = streams(&file);
        assert_eq!(pages.len(), 3);
        for (index, page) in pages.iter().enumerate() {
            let page = String::from_utf8_lossy(page);
            assert!(page.contains("/GS1 gs BT /F2") && page.contains(&format!("<{}> Tj ET Q", hex_text("DUPLICATA"))), "page {}", index + 1);
            assert!(page.contains(&hex_text(&format!("Page {} / 3", index + 1))));
        }
    }

    #[test]
    fn a_picture_goes_in_the_file_once_with_its_transparency() {
        // Two by one pixels: an opaque red one, a half transparent blue one.
        let picture = Picture { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 128] };
        let mut pictures = Pictures::new();
        pictures.insert(LOGO_SOURCE.to_string(), picture);
        let mut logo = Element::new_image(10.0, 10.0, 1);
        logo.kind = Kind::Image(ImageEl { src: "{{shop.logo_url}}".into(), fit: Fit::Contain });
        let mut again = logo.clone();
        again.id = "again".into();
        let layout = Layout { elements: vec![logo, again], ..Layout::default() };
        let page = draw(&layout, &json!({ "shop": { "logo_url": LOGO_SOURCE } }), &pictures, Mode::Print, 0);

        let file = write(&[page], &pictures, "Logo");
        let text = String::from_utf8_lossy(&file);
        assert_eq!(text.matches("/ColorSpace /DeviceRGB").count(), 1, "shown twice, carried once");
        assert_eq!(text.matches("/ColorSpace /DeviceGray").count(), 1);
        let unpacked = streams(&file);
        assert_eq!(unpacked[0], [255, 128], "the opacity of each pixel");
        assert_eq!(unpacked[1], [255, 0, 0, 0, 0, 255], "their colours");
        assert_eq!(String::from_utf8_lossy(&unpacked[2]).matches(" Do Q").count(), 2);

        // An opaque picture needs no mask.
        pictures.insert(LOGO_SOURCE.to_string(), Picture { width: 1, height: 1, rgba: vec![1, 2, 3, 255] });
        let page = draw(&layout, &json!({ "shop": { "logo_url": LOGO_SOURCE } }), &pictures, Mode::Print, 0);
        assert!(!String::from_utf8_lossy(&write(&[page], &pictures, "Logo")).contains("/SMask"));
    }

    #[test]
    fn pictures_are_read_from_files_and_from_their_own_address() {
        // The smallest PNG there is: one transparent pixel.
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let picture = Picture::from_data_url(&format!("data:image/png;base64,{png}")).expect("a PNG");
        assert_eq!((picture.width, picture.height, picture.rgba.len()), (1, 1, 4));
        assert!(Picture::from_data_url("data:text/plain,bonjour").is_none());
        assert!(Picture::from_data_url("https://exemple.fr/logo.png").is_none());
        assert!(Picture::decode(b"pas une image").is_none());

        let drawing = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 10"><rect width="20" height="10" fill="#1976d2"/></svg>"##;
        let picture = Picture::decode(drawing).expect("an SVG drawing");
        assert_eq!((picture.width, picture.height), (1200, 600));
        assert_eq!(&picture.rgba[..4], [0x19, 0x76, 0xd2, 255]);
    }
}
