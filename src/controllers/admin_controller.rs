use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tracing::{error, info, warn};

use crate::models::AppConfig;
use crate::services::{
    CameraProvider, Canon700dCamera, DnpRx1hsPrinter, LaptopWebcamCamera, MockCamera, MockPrinter,
    PrinterProvider, UpdaterService, VersionInfo,
};
use crate::{AppWindow, GalleryPhotoItem};

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
        allow_guest_layout_choice: bool,
        auto_print: bool,
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
                cfg.allow_guest_layout_choice = allow_guest_layout_choice;
                cfg.auto_print = auto_print;

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
                    w.set_allow_guest_layout_choice(allow_guest_layout_choice);
                    w.set_auto_print_enabled(auto_print);
                    w.set_status_message("Settings saved & hardware reloaded!".into());
                }
            });
        });
    }

    pub fn handle_test_camera(&self) {
        let camera_provider = self.camera_provider.clone();
        let window_weak = self.window.clone();

        tokio::spawn(async move {
            let test_path = PathBuf::from("output").join("raw").join("test_capture.jpg");
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
            let mut test_path = PathBuf::from("output").join("raw").join("test_capture.jpg");

            // Look for the most recently captured photo in output/raw or output/composites
            let search_dirs = [PathBuf::from("output").join("raw"), PathBuf::from("output").join("composites")];
            let mut recent_files = Vec::new();

            for dir in &search_dirs {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            if let Some(ext) = path.extension() {
                                if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("png") {
                                    if let Ok(meta) = entry.metadata() {
                                        if let Ok(mod_time) = meta.modified() {
                                            recent_files.push((mod_time, path));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some((_, latest_path)) = recent_files.iter().max_by_key(|(time, _)| *time) {
                test_path = latest_path.clone();
                info!("Selected latest captured image for test print: {:?}", test_path);
            }

            if !test_path.exists() {
                // Generate a dummy test image if not exists
                let mock = MockCamera::new();
                let _ = mock.capture_photo(&test_path, 1);
            }

            let prn = printer_provider.read().await;
            let filename = test_path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "photo.jpg".into());

            match prn.print_image(&test_path, false, 1) {
                Ok(_) => {
                    info!("Printer test dispatched successfully: {:?}", test_path);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_status_message(
                                format!("Print OK! Dispatched 1 sheet ({}) to DS-RX1", filename).into(),
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

    /// Loads recent captured Raw & Composite images into the Admin Reprint Picker
    pub fn handle_load_gallery(&self) {
        let window_weak = self.window.clone();
        tokio::spawn(async move {
            let mut entries_with_time = Vec::new();
            for (dir_str, kind) in [("output/composites", "Composite"), ("output/raw", "Raw")] {
                if let Ok(entries) = std::fs::read_dir(dir_str) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            if let Some(ext) = path.extension() {
                                if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("png") {
                                    let mod_time = entry
                                        .metadata()
                                        .and_then(|m| m.modified())
                                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                                    entries_with_time.push((mod_time, kind, path));
                                }
                            }
                        }
                    }
                }
            }

            entries_with_time.sort_by(|a, b| b.0.cmp(&a.0));
            entries_with_time.truncate(15);

            let items: Vec<(String, String, String, bool)> = entries_with_time
                .into_iter()
                .map(|(_, kind, path)| {
                    let filename = path
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let path_str = path.to_string_lossy().to_string();
                    let is_strip = if kind == "Composite" {
                        image::image_dimensions(&path)
                            .map(|(w, h)| w == 1200 && h == 1800)
                            .unwrap_or(false)
                    } else {
                        false
                    };
                    (filename, path_str, kind.to_string(), is_strip)
                })
                .collect();

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = window_weak.upgrade() {
                    let slint_items: Vec<GalleryPhotoItem> = items
                        .into_iter()
                        .map(|(filename, path, kind, is_strip)| GalleryPhotoItem {
                            filename: filename.into(),
                            path: path.into(),
                            kind: kind.into(),
                            is_strip,
                        })
                        .collect();
                    let model = Rc::new(slint::VecModel::from(slint_items));
                    w.set_gallery_items(slint::ModelRc::from(model));
                }
            });
        });
    }

    /// Prints a specific photo picked from the Admin Gallery / Reprint Picker
    pub fn handle_print_gallery_item(&self, file_path: String, is_strip_2x6: bool) {
        let printer_provider = self.printer_provider.clone();
        let window_weak = self.window.clone();

        tokio::spawn(async move {
            let p = PathBuf::from(&file_path);
            let filename = p
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "photo.jpg".into());

            let _ = slint::invoke_from_event_loop({
                let window_weak = window_weak.clone();
                let filename = filename.clone();
                move || {
                    if let Some(w) = window_weak.upgrade() {
                        w.set_status_message(format!("🖨️ Printing {}...", filename).into());
                    }
                }
            });

            let prn = printer_provider.read().await;
            match prn.print_image(Path::new(&file_path), is_strip_2x6, 1) {
                Ok(_) => {
                    info!("Admin reprinted selected file: {:?}", file_path);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_status_message(format!("✅ Reprint sent: {}", filename).into());
                        }
                    });
                }
                Err(e) => {
                    error!("Admin reprint failed: {:?}", e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_status_message(format!("⚠️ Reprint error: {}", e).into());
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
