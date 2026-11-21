use qrcode::QrCode;
use tracing::info;

pub struct QrService;

impl QrService {
    pub fn generate_qr_rgb8(url: &str, min_dimension: u32) -> anyhow::Result<(Vec<u8>, u32, u32)> {
        let code = QrCode::new(url.as_bytes())?;
        let rendered_img = code
            .render::<image::Rgb<u8>>()
            .min_dimensions(min_dimension, min_dimension)
            .build();

        let (width, height) = rendered_img.dimensions();
        let raw_bytes = rendered_img.into_raw();

        info!(
            "Generated QR code for URL '{}' at {}x{}",
            url, width, height
        );
        Ok((raw_bytes, width, height))
    }
}
