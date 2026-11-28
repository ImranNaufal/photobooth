use image::{imageops, ImageBuffer, Rgba, RgbaImage};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};

use crate::models::TemplateConfig;

pub struct CompositorService;

impl CompositorService {
    pub fn compose(
        photo_paths: &[PathBuf],
        template: &TemplateConfig,
        session_id: &str,
    ) -> anyhow::Result<(PathBuf, Vec<u8>, u32, u32)> {
        let canvas_w = template.canvas_width;
        let canvas_h = template.canvas_height;

        info!(
            "Starting composition for session {}: canvas {}x{}, {} slots, is_strip_2x6: {}",
            session_id,
            canvas_w,
            canvas_h,
            template.slots.len(),
            template.is_strip_2x6
        );

        // 1. Initialize blank canvas (solid white for print)
        let mut canvas: RgbaImage =
            ImageBuffer::from_pixel(canvas_w, canvas_h, Rgba([255, 255, 255, 255]));

        // 2. Crop and paste each photo into its corresponding slot
        for slot in &template.slots {
            if let Some(photo_path) = photo_paths.get(slot.index) {
                if !photo_path.exists() {
                    warn!(
                        "Photo file {:?} does not exist. Skipping slot {}.",
                        photo_path, slot.index
                    );
                    continue;
                }

                match image::open(photo_path) {
                    Ok(dyn_img) => {
                        let rgba_src = dyn_img.to_rgba8();
                        let sw = rgba_src.width();
                        let sh = rgba_src.height();

                        if sw == 0 || sh == 0 || slot.width == 0 || slot.height == 0 {
                            continue;
                        }

                        let target_ratio = slot.width as f32 / slot.height as f32;
                        let src_ratio = sw as f32 / sh as f32;

                        // Center Crop (Aspect Ratio Cover)
                        let (crop_x, crop_y, crop_w, crop_h) = if src_ratio > target_ratio {
                            let cw = ((sh as f32 * target_ratio).round() as u32).min(sw);
                            let cx = (sw.saturating_sub(cw)) / 2;
                            (cx, 0, cw, sh)
                        } else {
                            let ch = ((sw as f32 / target_ratio).round() as u32).min(sh);
                            let cy = (sh.saturating_sub(ch)) / 2;
                            (0, cy, sw, ch)
                        };

                        let cropped = imageops::crop_imm(
                            &rgba_src,
                            crop_x,
                            crop_y,
                            crop_w.min(sw.saturating_sub(crop_x)),
                            crop_h.min(sh.saturating_sub(crop_y)),
                        )
                        .to_image();

                        // Fast high-quality resize
                        let resized = imageops::resize(
                            &cropped,
                            slot.width,
                            slot.height,
                            imageops::FilterType::Triangle,
                        );

                        // Overlay onto left slot (and duplicate onto right half BEFORE overlay PNG for 2x6 strips)
                        imageops::overlay(&mut canvas, &resized, slot.x as i64, slot.y as i64);
                        if template.is_strip_2x6 && canvas_w >= 1200 {
                            let half_w = canvas_w / 2;
                            imageops::overlay(
                                &mut canvas,
                                &resized,
                                (slot.x + half_w) as i64,
                                slot.y as i64,
                            );
                        }
                    }
                    Err(e) => {
                        error!("Failed to open photo at {:?}: {:?}", photo_path, e);
                    }
                }
            }
        }

        // 3. Alpha-blend template overlay PNG on top (preserves crisp center cut-line on 2x6)
        let overlay_path = Path::new(&template.overlay_png);
        if overlay_path.exists() {
            match image::open(overlay_path) {
                Ok(overlay_dyn) => {
                    let overlay_rgba = overlay_dyn.to_rgba8();
                    imageops::overlay(&mut canvas, &overlay_rgba, 0, 0);
                    info!("Applied overlay PNG {:?}", overlay_path);
                }
                Err(e) => {
                    warn!("Failed to load overlay PNG {:?}: {:?}", overlay_path, e);
                }
            }
        }

        // 5. Convert to RGB8 and save as high-res 300 DPI JPEG
        let out_dir = Path::new("output/composites");
        fs::create_dir_all(out_dir)?;

        let dest_filename = format!("session_{}.jpg", session_id);
        let dest_path = out_dir.join(dest_filename);

        let rgb_canvas = image::DynamicImage::ImageRgba8(canvas).to_rgb8();

        let mut jpeg_bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, 95);
        encoder.encode_image(&rgb_canvas)?;

        let final_jpeg = Self::inject_jfif_300dpi(&jpeg_bytes);
        fs::write(&dest_path, &final_jpeg)?;

        let raw_bytes = rgb_canvas.into_raw();
        info!(
            "Composite saved successfully with 300 DPI JFIF header to {:?}",
            dest_path
        );

        Ok((dest_path, raw_bytes, canvas_w, canvas_h))
    }

    /// Injects or updates JFIF APP0 marker so dye-sub printer rasterizers read explicit 300 DPI
    pub fn inject_jfif_300dpi(data: &[u8]) -> Vec<u8> {
        let mut out = data.to_vec();
        if out.len() > 20 && out[0] == 0xFF && out[1] == 0xD8 && out[2] == 0xFF && out[3] == 0xE0 {
            if &out[6..11] == b"JFIF\0" {
                out[13] = 1; // Units: 1 = dots per inch (DPI)
                out[14] = 0x01; // Xdensity high byte (300)
                out[15] = 0x2C; // Xdensity low byte
                out[16] = 0x01; // Ydensity high byte (300)
                out[17] = 0x2C; // Ydensity low byte
                return out;
            }
        }
        if out.len() >= 2 && out[0] == 0xFF && out[1] == 0xD8 {
            let jfif_header: [u8; 18] = [
                0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x02, 0x01, 0x01, 0x2C,
                0x01, 0x2C, 0x00, 0x00,
            ];
            let mut with_jfif = Vec::with_capacity(out.len() + 18);
            with_jfif.extend_from_slice(&out[0..2]);
            with_jfif.extend_from_slice(&jfif_header);
            with_jfif.extend_from_slice(&out[2..]);
            return with_jfif;
        }
        out
    }
}
