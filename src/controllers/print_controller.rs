use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::models::{AppConfig, SessionModel};
use crate::services::PrinterProvider;

pub struct PrintController {
    printer_provider: Arc<RwLock<Box<dyn PrinterProvider>>>,
    session_model: Arc<SessionModel>,
    _config: Arc<RwLock<AppConfig>>,
}

impl PrintController {
    pub fn new(
        printer_provider: Arc<RwLock<Box<dyn PrinterProvider>>>,
        session_model: Arc<SessionModel>,
        config: Arc<RwLock<AppConfig>>,
    ) -> Self {
        Self {
            printer_provider,
            session_model,
            _config: config,
        }
    }

    pub async fn print_session(
        &self,
        session_id: String,
        image_path: PathBuf,
        is_strip_2x6: bool,
        copies: u8,
    ) -> anyhow::Result<()> {
        info!(
            "Dispatching print for session {} to active printer provider",
            session_id
        );

        let provider = self.printer_provider.read().await;
        provider.print_image(&image_path, is_strip_2x6, copies)?;

        let session_model = self.session_model.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = session_model.increment_printed_copies(&session_id) {
                error!("Failed to increment printed copies in DB: {:?}", e);
            }
        });

        Ok(())
    }
}
