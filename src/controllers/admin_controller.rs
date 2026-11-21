use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tracing::{error, info, warn};

use crate::models::AppConfig;
use crate::services::{
    CameraProvider, Canon700dCamera, DnpRx1hsPrinter, LaptopWebcamCamera, MockCamera, MockPrinter,
    PrinterProvider, UpdaterService, VersionInfo,
};
use crate::AppWindow;

pub struct AdminController {
    window: slint::Weak<AppWindow>,
    config: Arc<RwLock<AppConfig>>,
    camera_provider: Arc<RwLock<Box<dyn CameraProvider>>>,
    printer_provider: Arc<RwLock<Box<dyn PrinterProvider>>>,
    pending_update: Arc<TokioMutex<Option<VersionInfo>>>,
}

impl AdminController {
    pub fn new(
        window: slint::Weak<AppWindow>,
        config: Arc<RwLock<AppConfig>>,
        camera_provider: Arc<RwLock<Box<dyn CameraProvider>>>,
        printer_provider: Arc<RwLock<Box<dyn PrinterProvider>>>,
    ) -> Self {
        Self {
            window,
            config,
            camera_provider,
            printer_provider,
            pending_update: Arc::new(TokioMutex::new(None)),
        }
    }

    pub fn handle_unlock(&self, entered_pin: &str) {
        let config_clone = self.config.clone();
        let window_weak = self.window.clone();
        let pin = entered_pin.to_string();

        tokio::spawn(async move {
            let correct_pin = {
                let cfg = config_clone.read().await;
                cfg.admin_pin.clone()
            };

            let is_match = pin == correct_pin;
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = window_weak.upgrade() {
                    if is_match {
                        w.set_admin_unlocked(true);
                        w.set_status_message("Admin unlocked successfully".into());
                    } else {
                        w.set_admin_unlocked(false);
                        w.set_status_message("Incorrect PIN! Try again.".into());
                    }
                }
            });
        });
    }

    pub fn handle_save_settings(
        &self,
        is_hardware_mode: bool,
        template_id: String,
        printer_name: String,
        countdown_sec: i32,
    ) {
        let config_clone = self.config.clone();
        let camera_provider = self.camera_provider.clone();
        let printer_provider = self.printer_provider.clone();
        let window_weak = self.window.clone();

        tokio::spawn(async move {
            {
                let mut cfg = config_clone.write().await;
                cfg.hardware_mode = is_hardware_mode;
                cfg.active_template_id = template_id.clone();
                cfg.printer_name = printer_name.clone();
                cfg.countdown_seconds = countdown_sec.clamp(1, 10) as u8;

                if let Err(e) = cfg.save() {
                    error!("Failed to save config: {:?}", e);
                }
            }

            let digicam_url = {
                let cfg = config_clone.read().await;
                cfg.digicam_url.clone()
            };

            let strip_preset = {
                let cfg = config_clone.read().await;
                cfg.printer_strip_preset_name.clone()
            };

            // Hot-swap camera provider
            {
                let mut cam = camera_provider.write().await;
                if is_hardware_mode {
                    info!(
                        "Hot-swapping CameraProvider to Canon700dCamera ({})",
                        digicam_url
                    );
                    *cam = Box::new(Canon700dCamera::new(digicam_url));
                } else {
                    info!("Hot-swapping CameraProvider to LaptopWebcamCamera");
                    *cam = Box::new(LaptopWebcamCamera::new());
                }
            }

            // Hot-swap printer provider
            {
                let mut prn = printer_provider.write().await;
                if is_hardware_mode {
                    info!(
                        "Hot-swapping PrinterProvider to DnpRx1hsPrinter ({})",
                        printer_name
                    );
                    *prn = Box::new(DnpRx1hsPrinter::new(printer_name, strip_preset));
                } else {
                    info!("Hot-swapping PrinterProvider to MockPrinter");
                    *prn = Box::new(MockPrinter::new());
                }
            }

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = window_weak.upgrade() {
                    w.set_is_hardware_mode(is_hardware_mode);
                    w.set_active_template_id(template_id.into());
                    w.set_countdown_seconds_setting(countdown_sec);
                    w.set_status_message("Settings saved & hardware reloaded!".into());
                }
            });
        });
    }

    pub fn handle_test_camera(&self) {
        let camera_provider = self.camera_provider.clone();
        let window_weak = self.window.clone();

        tokio::spawn(async move {
            let test_path = PathBuf::from("output/raw/test_capture.jpg");
            let cam = camera_provider.read().await;
            let healthy = cam.is_healthy();
            match cam.capture_photo(&test_path, 1) {
                Ok(path) => {
                    info!("Camera test captured to {:?}, healthy: {}", path, healthy);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            let msg = if healthy {
                                "Camera capture test succeeded (Live Hardware/Webcam)".to_string()
                            } else {
                                "Simulated test photo captured (Hardware/Webcam not connected)".to_string()
                            };
                            w.set_status_message(msg.into());
                        }
                    });
                }
                Err(e) => {
                    error!("Camera test failed: {:?}", e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_status_message(format!("Camera test failed: {}", e).into());
                        }
                    });
                }
            }
        });
    }

    pub fn handle_test_printer(&self) {
        let printer_provider = self.printer_provider.clone();
        let window_weak = self.window.clone();

        tokio::spawn(async move {
            let test_path = PathBuf::from("output/raw/test_capture.jpg");
            if !test_path.exists() {
                // Generate a dummy test image if not exists
                let mock = MockCamera::new();
                let _ = mock.capture_photo(&test_path, 1);
            }

            let prn = printer_provider.read().await;
            let spooler_status = prn.check_status(false).unwrap_or_else(|_| "Unknown".into());
            info!("Printer spooler probe status: {}", spooler_status);

            match prn.print_image(&test_path, false, 1) {
                Ok(_) => {
                    info!("Printer test dispatched successfully");
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_status_message(
                                format!("Printer OK! Spooler: {}", spooler_status).into(),
                            );
                        }
                    });
                }
                Err(e) => {
                    error!("Printer test failed: {:?}", e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_status_message(format!("Printer test error: {}", e).into());
                        }
                    });
                }
            }
        });
    }

    /// User explicitly clicked "Check for Updates" in Admin panel
    pub fn handle_check_for_updates(&self) {
        let config_clone = self.config.clone();
        let window_weak = self.window.clone();
        let pending_update = self.pending_update.clone();

        tokio::spawn(async move {
            let _ = slint::invoke_from_event_loop({
                let w = window_weak.clone();
                move || {
                    if let Some(w) = w.upgrade() {
                        w.set_is_checking_update(true);
                        w.set_update_status_text("Checking internet for updates...".into());
                    }
                }
            });

            let (app_ver, update_url) = {
                let cfg = config_clone.read().await;
                (crate::models::APP_VERSION.to_string(), cfg.update_check_url.clone())
            };

            match UpdaterService::check_for_updates(&app_ver, &update_url).await {
                Ok(Some(info)) => {
                    let mut lock = pending_update.lock().await;
                    *lock = Some(info.clone());

                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_is_checking_update(false);
                            w.set_update_available(true);
                            w.set_update_version_string(format!("v{}", info.version).into());
                            w.set_update_status_text(
                                format!("v{} ready to install", info.version).into(),
                            );
                        }
                    });
                }
                Ok(None) => {
                    let mut lock = pending_update.lock().await;
                    *lock = None;

                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_is_checking_update(false);
                            w.set_update_available(false);
                            w.set_update_status_text("Your system is running the latest version.".into());
                        }
                    });
                }
                Err(e) => {
                    warn!("Check for updates encountered error: {:?}", e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_is_checking_update(false);
                            w.set_update_status_text("Offline or server unreachable.".into());
                        }
                    });
                }
            }
        });
    }

    /// Quiet background check on application startup
    pub fn check_quiet_update(&self) {
        let config_clone = self.config.clone();
        let window_weak = self.window.clone();
        let pending_update = self.pending_update.clone();

        tokio::spawn(async move {
            // Wait 2 seconds after boot so UI opens first
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            let (app_ver, update_url) = {
                let cfg = config_clone.read().await;
                (crate::models::APP_VERSION.to_string(), cfg.update_check_url.clone())
            };

            if let Ok(Some(info)) = UpdaterService::check_for_updates(&app_ver, &update_url).await {
                let mut lock = pending_update.lock().await;
                *lock = Some(info.clone());

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = window_weak.upgrade() {
                        w.set_update_available(true);
                        w.set_update_version_string(format!("v{}", info.version).into());
                        w.set_update_status_text(
                            format!("v{} ready to install", info.version).into(),
                        );
                    }
                });
            }
        });
    }

    /// User clicked "UPDATE & RESTART"
    pub fn handle_apply_update(&self) {
        let window_weak = self.window.clone();
        let pending_update = self.pending_update.clone();

        tokio::spawn(async move {
            let update_info = {
                let lock = pending_update.lock().await;
                lock.clone()
            };

            if let Some(info) = update_info {
                let win = window_weak.clone();
                let progress_cb = move |progress: f32| {
                    let win = win.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = win.upgrade() {
                            w.set_update_progress(progress);
                        }
                    });
                };

                if let Err(e) =
                    UpdaterService::download_and_install(&info.download_url, progress_cb).await
                {
                    error!("Failed to download and install update: {:?}", e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_update_status_text(format!("Update failed: {}", e).into());
                        }
                    });
                }
            }
        });
    }
}
