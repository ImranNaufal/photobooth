use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotCoordinate {
    pub index: usize,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateConfig {
    pub id: String,
    pub name: String,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub overlay_png: String,
    pub is_strip_2x6: bool,
    pub slots: Vec<SlotCoordinate>,
}

impl TemplateConfig {
    pub fn default_templates() -> Vec<TemplateConfig> {
        vec![
            TemplateConfig {
                id: "default-4r-3pose".to_string(),
                name: "4R Postcard (3 Poses)".to_string(),
                canvas_width: 1800,
                canvas_height: 1200,
                overlay_png: "assets/templates/default_4r_overlay.png".to_string(),
                is_strip_2x6: false,
                slots: vec![
                    SlotCoordinate {
                        index: 0,
                        x: 60,
                        y: 60,
                        width: 810,
                        height: 1080,
                    },
                    SlotCoordinate {
                        index: 1,
                        x: 930,
                        y: 60,
                        width: 810,
                        height: 510,
                    },
                    SlotCoordinate {
                        index: 2,
                        x: 930,
                        y: 630,
                        width: 810,
                        height: 510,
                    },
                ],
            },
            TemplateConfig {
                id: "default-2x6-strip".to_string(),
                name: "2x6 Photo Strip (3 Poses - Auto Duplicate & Cut)".to_string(),
                canvas_width: 1200,
                canvas_height: 1800,
                overlay_png: "assets/templates/default_strip_overlay.png".to_string(),
                is_strip_2x6: true,
                slots: vec![
                    SlotCoordinate {
                        index: 0,
                        x: 50,
                        y: 60,
                        width: 500,
                        height: 480,
                    },
                    SlotCoordinate {
                        index: 1,
                        x: 50,
                        y: 580,
                        width: 500,
                        height: 480,
                    },
                    SlotCoordinate {
                        index: 2,
                        x: 50,
                        y: 1100,
                        width: 500,
                        height: 480,
                    },
                ],
            },
        ]
    }

    pub fn load_from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Vec<TemplateConfig>> {
        let content = fs::read_to_string(path)?;
        let templates: Vec<TemplateConfig> = serde_json::from_str(&content)?;
        Ok(templates)
    }

    pub fn save_to_file<P: AsRef<Path>>(
        templates: &[TemplateConfig],
        path: P,
    ) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(templates)?;
        fs::write(path, content)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn find_by_id<'a>(templates: &'a [TemplateConfig], id: &str) -> Option<&'a TemplateConfig> {
        templates.iter().find(|t| t.id == id)
    }
}
