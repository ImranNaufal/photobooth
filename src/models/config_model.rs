use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::{error, info};

pub const CONFIG_FILE_PATH: &str = "config.json";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub hardware_mode: bool,
    pub digicam_url: String,
    pub printer_name: String,
    pub printer_strip_preset_name: String,
    pub active_template_id: String,
    pub countdown_seconds: u8,
    pub result_timeout_seconds: u8,
    pub admin_pin: String,
    pub auto_print: bool,
    #[serde(default = "default_app_version")]
    pub app_version: String,
    #[serde(default = "default_update_url")]
    pub update_check_url: String,
    #[serde(default = "default_true")]
    pub auto_check_update: bool,
    #[serde(default)]
    pub alert_webhook_url: String,
}

fn default_app_version() -> String {
    APP_VERSION.to_string()
}

fn default_update_url() -> String {
    "https://raw.githubusercontent.com/ImranNaufal/photobooth/main/dist/version.json".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hardware_mode: false,
            digicam_url: "http://127.0.0.1:5513".to_string(),
            printer_name: "DS-RX1".to_string(),
            printer_strip_preset_name: "DS-RX1-2x6".to_string(),
            active_template_id: "default-4r-3pose".to_string(),
            countdown_seconds: 3,
            result_timeout_seconds: 15,
            admin_pin: "1234".to_string(),
            auto_print: true,
            app_version: default_app_version(),
            update_check_url: default_update_url(),
            auto_check_update: true,
            alert_webhook_url: String::new(),
        }
    }
}

impl AppConfig {
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Self {
        let path_ref = path.as_ref();
        if path_ref.exists() {
            match fs::read_to_string(path_ref) {
                Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
                    Ok(mut cfg) => {
                        info!("Loaded configuration from {:?}", path_ref);
                        cfg.app_version = APP_VERSION.to_string();
                        return cfg;
                    }
                    Err(err) => {
                        error!("Failed to parse config file: {:?}. Using defaults.", err);
                    }
                },
                Err(err) => {
                    error!("Failed to read config file: {:?}. Using defaults.", err);
                }
            }
        }

        let default_cfg = Self::default();
        if let Err(err) = default_cfg.save_to(path_ref) {
            error!("Failed to save initial default config: {:?}", err);
        }
        default_cfg
    }

    pub fn save_to<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let mut to_save = self.clone();
        to_save.app_version = APP_VERSION.to_string();
        let json_str = serde_json::to_string_pretty(&to_save)?;
        fs::write(path.as_ref(), json_str)?;
        info!("Saved configuration to {:?}", path.as_ref());
        Ok(())
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(CONFIG_FILE_PATH)
    }
}
