#![windows_subsystem = "windows"]

mod controllers;
mod models;
mod services;

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use controllers::{AdminController, BoothController, PrintController};
use models::{AppConfig, SessionModel, TemplateConfig, CONFIG_FILE_PATH, DEFAULT_DB_PATH};
use services::{
    BootstrapService, CameraProvider, Canon700dCamera, DnpRx1hsPrinter, LaptopWebcamCamera,
    MockPrinter, PrinterProvider, WebService,
};

// Include generated Slint modules
slint::include_modules!();

fn main() -> anyhow::Result<()> {
    // 1. Initialize dual logging (local rolling file in logs/photobooth.log + in-memory ring buffer)
    let make_writer = services::LoggerService::init();
    tracing_subscriber::fmt()
        .with_writer(make_writer)
        .with_ansi(false)
        .init();

    info!("Starting Photobooth-RS Kiosk System (with persistent & remote web logging)...");

    // 2. Initialize Tokio Runtime for background hardware, web server, and heavy computation
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _rt_guard = rt.enter();

    // 3. Bootstrap directories and sample assets
    BootstrapService::ensure_directories_and_assets()?;

    // 4. Load Models & Persistence
    let config = AppConfig::load_or_default(CONFIG_FILE_PATH);
    let session_model = SessionModel::open(DEFAULT_DB_PATH)?;
    let templates = TemplateConfig::load_from_file("assets/templates/templates.json")
        .unwrap_or_else(|_| TemplateConfig::default_templates());

    // 5. Initialize Embedded Local Web Server (Port 8080) for Real Guest Mobile Downloads
    let web_service = Arc::new(WebService::new(8080));
    web_service.clone().start_background_server();

    // 6. Initialize Hardware / Mock / Laptop Webcam Providers
    let camera_provider: Box<dyn CameraProvider> = if config.hardware_mode {
        info!("Hardware Mode Active: Initializing Canon 700D Camera Provider");
        Box::new(Canon700dCamera::new(config.digicam_url.clone()))
    } else {
        info!("Test/Dev Mode: Initializing Laptop Webcam Camera Provider");
        Box::new(LaptopWebcamCamera::new())
    };

    let printer_provider: Box<dyn PrinterProvider> = if config.hardware_mode {
        info!("Hardware Mode Active: Initializing DNP DS-RX1HS Printer Provider");
        Box::new(DnpRx1hsPrinter::new(
            config.printer_name.clone(),
            config.printer_strip_preset_name.clone(),
        ))
    } else {
        info!("Mock Mode Active: Initializing Mock Disk Spool Printer Provider");
        Box::new(MockPrinter::new())
    };

    // Shared thread-safe state wrappers
    let shared_config = Arc::new(RwLock::new(config.clone()));
    let shared_session_model = Arc::new(session_model);
    let shared_camera = Arc::new(RwLock::new(camera_provider));
    let shared_printer = Arc::new(RwLock::new(printer_provider));
    let shared_templates = Arc::new(RwLock::new(templates));

    // 7. Initialize Slint Window
    let window = AppWindow::new()?;
    let weak_window = window.as_weak();

    // Set initial window properties from configuration
    window.set_is_hardware_mode(config.hardware_mode);
    window.set_active_template_id(config.active_template_id.clone().into());
    window.set_printer_name(config.printer_name.clone().into());
    window.set_countdown_seconds_setting(config.countdown_seconds as i32);
    window.set_app_version(format!("v{}", config.app_version).into());
    window.set_current_state(AppState::Idle);

    // 8. Instantiate Controllers
    let print_controller = Arc::new(PrintController::new(
        shared_printer.clone(),
        shared_session_model.clone(),
        shared_config.clone(),
    ));

    let booth_controller = Arc::new(BoothController::new(
        weak_window.clone(),
        shared_config.clone(),
        shared_session_model.clone(),
        shared_camera.clone(),
        print_controller.clone(),
        shared_templates.clone(),
        web_service.clone(),
    ));

    let admin_controller = Arc::new(AdminController::new(
        weak_window.clone(),
        shared_config.clone(),
        shared_camera.clone(),
        shared_printer.clone(),
    ));

    // 9. Wire Slint Callbacks to Controllers
    {
        let bc = booth_controller.clone();
        window.on_start_session(move || {
            bc.handle_start_session();
        });
    }

    {
        let bc = booth_controller.clone();
        window.on_print_current_session(move || {
            bc.handle_print_current();
        });
    }

    {
        let bc = booth_controller.clone();
        window.on_finish_session(move || {
            bc.handle_finish_session();
        });
    }

    {
        let bc = booth_controller.clone();
        window.on_retake_session(move || {
            bc.handle_retake_session();
        });
    }

    {
        let ac = admin_controller.clone();
        window.on_admin_unlock(move |pin| {
            ac.handle_unlock(pin.as_str());
        });
    }

    {
        let ac = admin_controller.clone();
        window.on_save_settings(move |is_hw, tmpl_id, prn_name, countdown_sec| {
            ac.handle_save_settings(
                is_hw,
                tmpl_id.to_string(),
                prn_name.to_string(),
                countdown_sec,
            );
        });
    }

    {
        let ac = admin_controller.clone();
        window.on_test_camera(move || {
            ac.handle_test_camera();
        });
    }

    {
        let ac = admin_controller.clone();
        window.on_test_printer(move || {
            ac.handle_test_printer();
        });
    }

    {
        let ac = admin_controller.clone();
        window.on_check_for_updates(move || {
            ac.handle_check_for_updates();
        });
    }

    {
        let ac = admin_controller.clone();
        window.on_apply_update(move || {
            ac.handle_apply_update();
        });
    }

    // Silent background check for updates on startup if enabled
    if config.auto_check_update {
        admin_controller.check_quiet_update();
    }

    // 10. Start background Live View stream loop
    booth_controller.start_live_view_loop();

    info!("Photobooth MVC initialized. Running Slint window event loop...");

    // 11. Run main UI loop
    window.run()?;

    info!("Photobooth application terminated normally.");
    Ok(())
}
