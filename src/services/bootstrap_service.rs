use image::{ImageBuffer, Rgba};
use imageproc::rect::Rect;
use std::fs;
use std::path::Path;
use tracing::info;

use crate::models::{AppConfig, TemplateConfig, CONFIG_FILE_PATH};

pub struct BootstrapService;

impl BootstrapService {
    pub fn ensure_directories_and_assets() -> anyhow::Result<()> {
        // 1. Create directory structure
        let dirs = [
            "assets/templates",
            "output/raw",
            "output/composites",
            "output/mock_prints",
        ];

        for dir in dirs {
            fs::create_dir_all(dir)?;
        }
        info!("Ensured all core directories exist.");

        // 2. Ensure default config exists
        let config_path = Path::new(CONFIG_FILE_PATH);
        if !config_path.exists() {
            let default_cfg = AppConfig::default();
            default_cfg.save_to(config_path)?;
            info!("Created default config.json");
        }

        // 3. Ensure templates.json exists
        let templates_path = Path::new("assets/templates/templates.json");
        if !templates_path.exists() {
            let defaults = TemplateConfig::default_templates();
            TemplateConfig::save_to_file(&defaults, templates_path)?;
            info!("Created default assets/templates/templates.json");
        }

        // 4. Ensure default_4r_overlay.png exists
        let overlay_4r_path = Path::new("assets/templates/default_4r_overlay.png");
        if !overlay_4r_path.exists() {
            Self::generate_default_4r_overlay(overlay_4r_path)?;
            info!("Generated default 4R overlay PNG at {:?}", overlay_4r_path);
        }

        // 5. Ensure default_strip_overlay.png exists
        let overlay_strip_path = Path::new("assets/templates/default_strip_overlay.png");
        if !overlay_strip_path.exists() {
            Self::generate_default_strip_overlay(overlay_strip_path)?;
            info!(
                "Generated default 2x6 strip overlay PNG at {:?}",
                overlay_strip_path
            );
        }

        Ok(())
    }

    fn generate_default_4r_overlay(path: &Path) -> anyhow::Result<()> {
        let width = 1800u32;
        let height = 1200u32;
        let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0]));

        let white = Rgba([255, 255, 255, 255]);
        let gold = Rgba([234, 179, 8, 255]);
        let dark_slate = Rgba([15, 23, 42, 230]);

        // Draw outer aesthetic border
        Self::draw_rect_outline(&mut img, 16, 16, width - 32, height - 32, 4, gold);

        // Slots for 4R:
        // Slot 0: 60, 60, 810, 1080
        // Slot 1: 930, 60, 810, 510
        // Slot 2: 930, 630, 810, 510
        Self::draw_rect_outline(&mut img, 56, 56, 818, 1088, 6, white);
        Self::draw_rect_outline(&mut img, 926, 56, 818, 518, 6, white);
        Self::draw_rect_outline(&mut img, 926, 626, 818, 518, 6, white);

        // Draw an elegant bottom banner between x: 930..1740, y: 1140..1180
        Self::draw_filled_rect(&mut img, 930, 1146, 810, 32, dark_slate);
        Self::draw_rect_outline(&mut img, 930, 1146, 810, 32, 2, gold);

        // Subtle corner accents
        Self::draw_corner_accents(&mut img, 24, 24, 30, gold);
        Self::draw_corner_accents(&mut img, width - 54, 24, 30, gold);
        Self::draw_corner_accents(&mut img, 24, height - 54, 30, gold);
        Self::draw_corner_accents(&mut img, width - 54, height - 54, 30, gold);

        img.save(path)?;
        Ok(())
    }

    fn generate_default_strip_overlay(path: &Path) -> anyhow::Result<()> {
        let width = 1200u32;
        let height = 1800u32;
        let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0]));

        let white = Rgba([255, 255, 255, 255]);
        let rose = Rgba([244, 63, 94, 255]);
        let dark_slate = Rgba([15, 23, 42, 240]);
        let faint_cut = Rgba([148, 163, 184, 120]);

        // Draw strip layouts on both halves: left (0..600) and right (600..1200)
        for offset_x in [0u32, 600u32] {
            // Outer frame
            Self::draw_rect_outline(&mut img, offset_x + 16, 16, 568, height - 32, 3, rose);

            // Slots: (50, 60, 500, 480), (50, 580, 500, 480), (50, 1100, 500, 480)
            Self::draw_rect_outline(&mut img, offset_x + 46, 56, 508, 488, 5, white);
            Self::draw_rect_outline(&mut img, offset_x + 46, 576, 508, 488, 5, white);
            Self::draw_rect_outline(&mut img, offset_x + 46, 1096, 508, 488, 5, white);

            // Elegant footer branding card at bottom (y: 1610..1760)
            Self::draw_filled_rect(&mut img, offset_x + 50, 1610, 500, 150, dark_slate);
            Self::draw_rect_outline(&mut img, offset_x + 50, 1610, 500, 150, 2, rose);
        }

        // Faint vertical cut-line in center
        for y in (0..height).step_by(12) {
            for dy in 0..6 {
                if y + dy < height {
                    img.put_pixel(599, y + dy, faint_cut);
                    img.put_pixel(600, y + dy, faint_cut);
                }
            }
        }

        img.save(path)?;
        Ok(())
    }

    fn draw_rect_outline(
        img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        thickness: u32,
        color: Rgba<u8>,
    ) {
        let _rect = Rect::at(x as i32, y as i32).of_size(w, h);
        let max_x = (x + w).min(img.width());
        let max_y = (y + h).min(img.height());

        for px in x..max_x {
            for t in 0..thickness {
                if y + t < img.height() {
                    img.put_pixel(px, y + t, color);
                }
                if max_y > t + 1 && (max_y - 1 - t) < img.height() {
                    img.put_pixel(px, max_y - 1 - t, color);
                }
            }
        }

        for py in y..max_y {
            for t in 0..thickness {
                if x + t < img.width() {
                    img.put_pixel(x + t, py, color);
                }
                if max_x > t + 1 && (max_x - 1 - t) < img.width() {
                    img.put_pixel(max_x - 1 - t, py, color);
                }
            }
        }
    }

    fn draw_filled_rect(
        img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        color: Rgba<u8>,
    ) {
        let _rect = Rect::at(x as i32, y as i32).of_size(w, h);
        let max_x = (x + w).min(img.width());
        let max_y = (y + h).min(img.height());

        for py in y..max_y {
            for px in x..max_x {
                img.put_pixel(px, py, color);
            }
        }
    }

    fn draw_corner_accents(
        img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
        x: u32,
        y: u32,
        size: u32,
        color: Rgba<u8>,
    ) {
        let max_x = (x + size).min(img.width());
        let max_y = (y + size).min(img.height());

        for px in x..max_x {
            for py in y..max_y {
                if (px - x) + (py - y) <= size / 2 {
                    img.put_pixel(px, py, color);
                }
            }
        }
    }
}
