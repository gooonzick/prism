//! Renders an SVG to a PNG with a transparent background, for the app icon:
//! `cargo run --example render_icon -- assets/icon.svg icon.png 1024`.

use resvg::{tiny_skia, usvg};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, src, out, size] = args.as_slice() else {
        eprintln!("usage: render_icon <in.svg> <out.png> <size>");
        std::process::exit(2);
    };
    let size: u32 = size.parse().expect("size is a number of pixels");
    let svg = std::fs::read(src).expect("the SVG reads");
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default()).expect("the SVG parses");
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("a non-zero size");
    let scale = size as f32 / tree.size().width();
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.save_png(out).expect("the PNG writes");
}
