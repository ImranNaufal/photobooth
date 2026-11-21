pub mod audio_service;
pub mod bootstrap_service;
pub mod camera_service;
pub mod compositor_service;
pub mod logger_service;
pub mod printer_service;
pub mod qr_service;
pub mod updater_service;
pub mod web_service;

pub use audio_service::AudioService;
pub use bootstrap_service::BootstrapService;
pub use camera_service::{CameraProvider, Canon700dCamera, LaptopWebcamCamera, MockCamera};
pub use compositor_service::CompositorService;
pub use logger_service::LoggerService;
pub use printer_service::{DnpRx1hsPrinter, MockPrinter, PrinterProvider};
pub use qr_service::QrService;
pub use updater_service::{UpdaterService, VersionInfo};
pub use web_service::WebService;
