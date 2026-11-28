use chrono::Utc;
use slint::Rgb8Pixel;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::controllers::PrintController;
use crate::models::{AppConfig, PhotoSession, SessionModel, TemplateConfig};
use crate::services::{AudioService, CameraProvider, CompositorService, QrService, WebService};
use crate::{AppState, AppWindow};

#[derive(Clone)]
struct ActiveSessionData {
    session_id: String,
    composite_path: PathBuf,
    is_strip_2x6: bool,
}

pub struct BoothController {
    window: slint::Weak<AppWindow>,
    config: Arc<RwLock<AppConfig>>,
    session_model: Arc<SessionModel>,
    camera_provider: Arc<RwLock<Box<dyn CameraProvider>>>,
    print_controller: Arc<PrintController>,
    templates: Arc<RwLock<Vec<TemplateConfig>>>,
    web_service: Arc<WebService>,
    active_session: Arc<TokioMutex<Option<ActiveSessionData>>>,
    is_session_running: Arc<AtomicBool>,
    is_reviewing: Arc<AtomicBool>,
    is_printing_busy: Arc<AtomicBool>,
}

impl BoothController {
    pub fn new(
        window: slint::Weak<AppWindow>,
        config: Arc<RwLock<AppConfig>>,
        session_model: Arc<SessionModel>,
        camera_provider: Arc<RwLock<Box<dyn CameraProvider>>>,
        print_controller: Arc<PrintController>,
        templates: Arc<RwLock<Vec<TemplateConfig>>>,
        web_service: Arc<WebService>,
    ) -> Self {
        Self {
            window,
            config,
            session_model,
            camera_provider,
            print_controller,
            templates,
            web_service,
            active_session: Arc::new(TokioMutex::new(None)),
            is_session_running: Arc::new(AtomicBool::new(false)),
            is_reviewing: Arc::new(AtomicBool::new(false)),
            is_printing_busy: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Persists template layout chosen by guest on Idle screen so backend uses the exact selection
    pub fn handle_select_template(&self, template_id: String) {
        let config_lock = self.config.clone();
        tokio::spawn(async move {
            let mut cfg = config_lock.write().await;
            if cfg.active_template_id != template_id {
                info!("Guest selected template layout: {}", template_id);
                cfg.active_template_id = template_id;
                if let Err(e) = cfg.save() {
                    warn!("Failed to persist guest template selection: {:?}", e);
                }
            }
        });
    }

    /// Spawns a background task that streams camera frames to Slint UI (~30 fps) when in Capture state
    pub fn start_live_view_loop(&self) {
        let window_weak = self.window.clone();
        let camera_provider = self.camera_provider.clone();
        let is_reviewing = self.is_reviewing.clone();

        tokio::spawn(async move {
            info!("Live View background loop started");
            loop {
                // If currently reviewing a captured photo, hold that photo on screen and do not overwrite with live feed
                if is_reviewing.load(Ordering::Relaxed) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }

                // Query the window state safely from UI thread
                let (should_stream, _) = {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let window_weak = window_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            let _ = tx.send(w.get_current_state() == AppState::Capture);
                        } else {
                            let _ = tx.send(false);
                        }
                    });
                    match rx.await {
                        Ok(res) => (res, ()),
                        Err(_) => (false, ()),
                    }
                };

                if should_stream {
                    let frame_res = {
                        let cam = camera_provider.read().await;
                        cam.fetch_live_view_frame()
                    };

                    match frame_res {
                        Ok((raw_bytes, width, height)) => {
                            let window_weak = window_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(w) = window_weak.upgrade() {
                                    let mut pixel_buffer =
                                        slint::SharedPixelBuffer::<Rgb8Pixel>::new(width, height);
                                    let dest_slice = pixel_buffer.make_mut_bytes();
                                    if dest_slice.len() == raw_bytes.len() {
                                        dest_slice.copy_from_slice(&raw_bytes);
                                        let img = slint::Image::from_rgb8(pixel_buffer);
                                        w.set_live_view_image(img);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            warn!("Live view frame fetch error: {:?}", e);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(33)).await;
                } else {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
            }
        });
    }

    /// Dispatches a full photo booth session
    pub fn handle_start_session(&self) {
        // Prevent concurrent session triggers
        if self
            .is_session_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            warn!("Session already running. Ignoring duplicate start request.");
            return;
        }

        let window_weak = self.window.clone();
        let config_lock = self.config.clone();
        let templates_lock = self.templates.clone();
        let camera_provider = self.camera_provider.clone();
        let session_model = self.session_model.clone();
        let active_session = self.active_session.clone();
        let is_running_flag = self.is_session_running.clone();
        let is_reviewing = self.is_reviewing.clone();
        let web_service = self.web_service.clone();

        tokio::spawn(async move {
            let session_id = Uuid::new_v4().to_string();
            info!("Starting new Photobooth Session: {}", session_id);

            // Fetch active template and settings
            let (template, countdown_seconds) = {
                let cfg = config_lock.read().await;
                let tmpl_list = templates_lock.read().await;
                let tmpl = tmpl_list
                    .iter()
                    .find(|t| t.id == cfg.active_template_id)
                    .cloned()
                    .unwrap_or_else(|| tmpl_list.first().cloned().unwrap());
                (tmpl, cfg.countdown_seconds)
            };

            let total_shots = template.slots.len() as u8;
            let slot_aspect = |idx: u8| -> f32 {
                template
                    .slots
                    .get(idx.saturating_sub(1) as usize)
                    .filter(|s| s.height > 0)
                    .map(|s| s.width as f32 / s.height as f32)
                    .unwrap_or(1.5)
            };
            let first_aspect = slot_aspect(1);

            // Switch UI state to Capture
            let _ = slint::invoke_from_event_loop({
                let window_weak = window_weak.clone();
                move || {
                    if let Some(w) = window_weak.upgrade() {
                        w.set_current_state(AppState::Capture);
                        w.set_current_shot(1);
                        w.set_total_shots(total_shots as i32);
                        w.set_active_slot_aspect(first_aspect);
                        w.set_countdown_value(countdown_seconds as i32);
                        w.set_status_message("GET READY!".into());
                    }
                }
            });

            // Brief pause before countdown starts
            tokio::time::sleep(Duration::from_millis(1000)).await;

            let mut captured_paths = Vec::new();

            for shot_idx in 1..=total_shots {
                is_reviewing.store(false, Ordering::SeqCst);
                let current_aspect = slot_aspect(shot_idx);
                let _ = slint::invoke_from_event_loop({
                    let window_weak = window_weak.clone();
                    move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_current_shot(shot_idx as i32);
                            w.set_active_slot_aspect(current_aspect);
                        }
                    }
                });

                // Countdown loop with Audio Beep cues
                for sec in (1..=countdown_seconds).rev() {
                    AudioService::play_countdown_beep();
                    let _ = slint::invoke_from_event_loop({
                        let window_weak = window_weak.clone();
                        move || {
                            if let Some(w) = window_weak.upgrade() {
                                w.set_countdown_value(sec as i32);
                            }
                        }
                    });
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }

                // Pause background LiveView polling while camera switches from EVF mirror to DSLR shutter!
                is_reviewing.store(true, Ordering::SeqCst);

                // Signal guest to hold pose while camera focuses and snaps
                let _ = slint::invoke_from_event_loop({
                    let window_weak = window_weak.clone();
                    move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_countdown_value(0);
                            w.set_status_message("📸 HOLD STILL & SMILE!".into());
                        }
                    }
                });

                let shot_dest = PathBuf::from("output")
                    .join("raw")
                    .join(format!("session_{}_pose_{}.jpg", session_id, shot_idx));

                // Capture photo from DSLR / Camera
                let cam = camera_provider.read().await;
                let capture_res = cam.capture_photo(&shot_dest, shot_idx);
                drop(cam);

                match capture_res {
                    Ok(path) => {
                        captured_paths.push(path.clone());

                        // Fire shutter sound & flash right as the real snap completes
                        AudioService::play_shutter_click();
                        is_reviewing.store(true, Ordering::SeqCst);

                        // Immediately reveal captured photo mirrored cleanly (matching Live View) under flash fade
                        if let Ok(img) = image::open(&path) {
                            let preview_img = img.resize(1280, 853, image::imageops::FilterType::Triangle);
                            let mirrored = image::imageops::flip_horizontal(&preview_img.to_rgb8());
                            let (w_img, h_img) = mirrored.dimensions();
                            let raw_bytes = mirrored.into_raw();

                            let _ = slint::invoke_from_event_loop({
                                let window_weak = window_weak.clone();
                                move || {
                                    if let Some(w) = window_weak.upgrade() {
                                        let mut pixel_buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(w_img, h_img);
                                        pixel_buffer.make_mut_bytes().copy_from_slice(&raw_bytes);
                                        let slint_img = slint::Image::from_rgb8(pixel_buffer);
                                        w.set_live_view_image(slint_img);
                                        w.set_flash_active(true);
                                        w.set_status_message("✨ GREAT SHOT! ✨".into());
                                    }
                                }
                            });
                        } else {
                            let _ = slint::invoke_from_event_loop({
                                let window_weak = window_weak.clone();
                                move || {
                                    if let Some(w) = window_weak.upgrade() {
                                        w.set_flash_active(true);
                                    }
                                }
                            });
                        }
                    }
                    Err(e) => {
                        error!("Failed to capture pose #{}: {:?}", shot_idx, e);
                    }
                }

                // Wait 300ms for flash animation fade
                tokio::time::sleep(Duration::from_millis(300)).await;
                let _ = slint::invoke_from_event_loop({
                    let window_weak = window_weak.clone();
                    move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_flash_active(false);
                        }
                    }
                });

                // Review pose freeze for 1800ms so user clearly sees their face
                tokio::time::sleep(Duration::from_millis(1800)).await;
            }

            is_reviewing.store(false, Ordering::SeqCst);

            // Rest DSLR mirror / Live View while processing & printing
            {
                let cam = camera_provider.read().await;
                cam.stop_live_view();
            }

            // Switch to Processing State
            let _ = slint::invoke_from_event_loop({
                let window_weak = window_weak.clone();
                move || {
                    if let Some(w) = window_weak.upgrade() {
                        w.set_current_state(AppState::Processing);
                        w.set_status_message("Rendering 300 DPI high-res composite...".into());
                    }
                }
            });

            // Run Compositing in blocking thread pool
            let session_id_clone = session_id.clone();
            let template_clone = template.clone();
            let compose_res = tokio::task::spawn_blocking(move || {
                CompositorService::compose(&captured_paths, &template_clone, &session_id_clone)
            })
            .await;

            match compose_res {
                Ok(Ok((composite_path, raw_rgb, comp_w, comp_h))) => {
                    info!(
                        "Composite created: {:?} ({}x{})",
                        composite_path, comp_w, comp_h
                    );

                    // Generate Real Mobile Download URL & QR Code
                    let share_url = web_service.get_share_url(&session_id);
                    info!("Guest download share URL: {}", share_url);
                    let qr_res = QrService::generate_qr_rgb8(&share_url, 300);

                    // Record session in SQLite database
                    let new_session = PhotoSession {
                        id: session_id.clone(),
                        created_at: Utc::now().to_rfc3339(),
                        template_id: template.id.clone(),
                        composite_path: composite_path.to_string_lossy().to_string(),
                        share_url: share_url.clone(),
                        upload_status: "PENDING".to_string(),
                        printed_copies: 0,
                    };

                    if let Err(e) = session_model.create_session(&new_session) {
                        error!("Failed to persist session to database: {:?}", e);
                    }

                    // Update Active Session state
                    {
                        let mut act = active_session.lock().await;
                        *act = Some(ActiveSessionData {
                            session_id: session_id.clone(),
                            composite_path: composite_path.clone(),
                            is_strip_2x6: template.is_strip_2x6,
                        });
                    }

                    // Update UI with Result and QR code
                    let _ = slint::invoke_from_event_loop({
                        let window_weak = window_weak.clone();
                        let raw_rgb = raw_rgb.clone();
                        let qr_opt = qr_res.ok();

                        move || {
                            if let Some(w) = window_weak.upgrade() {
                                // Composite Image
                                let mut comp_buffer =
                                    slint::SharedPixelBuffer::<Rgb8Pixel>::new(comp_w, comp_h);
                                let dest = comp_buffer.make_mut_bytes();
                                if dest.len() == raw_rgb.len() {
                                    dest.copy_from_slice(&raw_rgb);
                                    w.set_composite_result_image(slint::Image::from_rgb8(
                                        comp_buffer,
                                    ));
                                }

                                // QR Code Image
                                if let Some((qr_bytes, qr_w, qr_h)) = qr_opt {
                                    let mut qr_buffer =
                                        slint::SharedPixelBuffer::<Rgb8Pixel>::new(qr_w, qr_h);
                                    let qr_dest = qr_buffer.make_mut_bytes();
                                    if qr_dest.len() == qr_bytes.len() {
                                        qr_dest.copy_from_slice(&qr_bytes);
                                        w.set_qr_code_image(slint::Image::from_rgb8(qr_buffer));
                                    }
                                }

                                w.set_current_state(AppState::Result);
                                w.set_status_message("".into());
                                w.set_auto_print_enabled(false);
                                w.set_is_printing_busy(false);
                                w.set_auto_return_seconds(0);
                            }
                        }
                    });
                }
                _ => {
                    error!("Composition task failed!");
                    let _ = slint::invoke_from_event_loop({
                        let window_weak = window_weak.clone();
                        move || {
                            if let Some(w) = window_weak.upgrade() {
                                w.set_current_state(AppState::Idle);
                                w.set_status_message(
                                    "Error rendering photo. Please try again.".into(),
                                );
                            }
                        }
                    });
                    is_running_flag.store(false, Ordering::SeqCst);
                }
            }
        });
    }

    pub fn handle_print_current(&self) {
        if self
            .is_printing_busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            info!("Ignored duplicate print tap while printer is busy.");
            return;
        }

        let active_session = self.active_session.clone();
        let print_controller = self.print_controller.clone();
        let window_weak = self.window.clone();
        let is_printing_busy = self.is_printing_busy.clone();

        tokio::spawn(async move {
            let session_data = {
                let act = active_session.lock().await;
                act.clone()
            };

            if let Some(data) = session_data {
                let _ = slint::invoke_from_event_loop({
                    let window_weak = window_weak.clone();
                    move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_is_printing_busy(true);
                            w.set_status_message("🖨️ Sending photo to printer...".into());
                        }
                    }
                });

                let res = print_controller
                    .print_session(data.session_id, data.composite_path, data.is_strip_2x6, 1)
                    .await;

                let msg = match &res {
                    Ok(_) => "✅ Print sent to printer!".to_string(),
                    Err(e) => {
                        error!("Print error: {:?}", e);
                        format!("⚠️ Print failed: {}", e)
                    }
                };

                let _ = slint::invoke_from_event_loop({
                    let window_weak = window_weak.clone();
                    move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.set_status_message(msg.into());
                        }
                    }
                });

                tokio::time::sleep(Duration::from_secs(2)).await;
            }

            is_printing_busy.store(false, Ordering::SeqCst);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = window_weak.upgrade() {
                    w.set_is_printing_busy(false);
                }
            });
        });
    }

    pub fn handle_finish_session(&self) {
        let active_session = self.active_session.clone();
        let is_running_flag = self.is_session_running.clone();
        let window_weak = self.window.clone();

        tokio::spawn(async move {
            {
                let mut act = active_session.lock().await;
                *act = None;
            }
            is_running_flag.store(false, Ordering::SeqCst);

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = window_weak.upgrade() {
                    w.set_current_state(AppState::Idle);
                    w.set_status_message("".into());
                }
            });
        });
    }

    pub fn handle_retake_session(self: &Arc<Self>) {
        info!("Guest requested Retake Session");
        let active_session = self.active_session.clone();
        let is_running_flag = self.is_session_running.clone();
        let self_clone = self.clone();

        tokio::spawn(async move {
            {
                let mut act = active_session.lock().await;
                *act = None;
            }
            is_running_flag.store(false, Ordering::SeqCst);

            // Re-trigger start session directly
            self_clone.handle_start_session();
        });
    }
}
