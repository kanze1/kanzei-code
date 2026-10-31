pub(crate) fn image(value: &str) -> Result<String, String> {
    use base64::Engine;
    let code = qrcode::QrCode::new(value.as_bytes()).map_err(|e| e.to_string())?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(240, 240)
        .build();
    Ok(format!(
        "data:image/svg+xml;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(svg)
    ))
}
