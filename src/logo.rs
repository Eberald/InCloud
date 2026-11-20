//! The InCloud logo: a stylised cloud lit by a star, with bands for the
//! H II / PDR / molecular layers. One SVG, rendered for the window icon and the header.

const CLOUD: &str = "M34 98 H96 A22 22 0 0 0 98 54 A21 21 0 1 0 64 36 A18 18 0 0 0 40 52 A23 23 0 0 0 34 98 Z";

fn svg(background: bool) -> String {
    let bg = if background { "<rect width=\"128\" height=\"128\" rx=\"28\" fill=\"#101935\"/>" } else { "" };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 128 128\" width=\"128\" height=\"128\">\
<defs>\
<linearGradient id=\"sky\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\"><stop offset=\"0\" stop-color=\"#3fd0ff\"/><stop offset=\"0.55\" stop-color=\"#2f6bff\"/><stop offset=\"1\" stop-color=\"#8a3ffc\"/></linearGradient>\
<clipPath id=\"c\"><path d=\"{CLOUD}\"/></clipPath>\
</defs>\
{bg}\
<g clip-path=\"url(#c)\">\
<rect x=\"0\" y=\"0\" width=\"128\" height=\"128\" fill=\"url(#sky)\"/>\
<path d=\"M10 128 L128 10 L128 40 L40 128 Z\" fill=\"#ffb000\" fill-opacity=\"0.55\"/>\
<path d=\"M40 128 L128 40 L128 70 L70 128 Z\" fill=\"#ff3d8b\" fill-opacity=\"0.45\"/>\
</g>\
<path d=\"{CLOUD}\" fill=\"none\" stroke=\"#ffffff\" stroke-width=\"4\" stroke-linejoin=\"round\"/>\
<g fill=\"#ffd23f\">\
<circle cx=\"24\" cy=\"26\" r=\"9\"/>\
<path d=\"M24 6 L27 20 L24 46 L21 20 Z\"/>\
<path d=\"M4 26 L18 23 L44 26 L18 29 Z\"/>\
</g>\
</svg>"
    )
}

/// The logo as RGBA pixels (unmultiplied), `size` × `size`.
pub fn rgba(size: u32, background: bool) -> Vec<u8> {
    let opt = usvg::Options::default();
    let Ok(tree) = usvg::Tree::from_str(&svg(background), &opt) else { return vec![0; (size * size * 4) as usize] };
    let Some(mut pix) = resvg::tiny_skia::Pixmap::new(size, size) else { return vec![0; (size * size * 4) as usize] };
    let s = size as f32 / 128.0;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(s, s), &mut pix.as_mut());
    // tiny-skia stores premultiplied alpha
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for p in pix.pixels() {
        let c = p.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    out
}
