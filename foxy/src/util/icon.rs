use iced::window::Icon;

pub fn load_icon_from_svg(svg_bytes: &[u8], size: u32) -> Result<Icon, String> {
    let tree = resvg::usvg::Tree::from_data(
        svg_bytes,
        &resvg::usvg::Options::default(),
    )
    .map_err(|e| format!("SVG parse error: {e}"))?;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)
        .ok_or("Не удалось создать pixmap")?;

    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );

    let rgba = pixmap.take();

    iced::window::icon::from_rgba(rgba, size, size)
        .map_err(|e| format!("Icon::from_rgba error: {e:?}"))
}
