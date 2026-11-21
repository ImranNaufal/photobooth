# Master PRD: Ultra-Light Windows Photobooth (Pure Rust + Slint MVC)

## 1. Executive Summary & Core Objectives
Build a commercial-grade, ultra-lightweight (<40MB RAM), 60fps native Windows Photobooth kiosk application (similar to dslrBooth / LumaBooth) using **100% Pure Rust** and the **Slint UI framework**. 
- **Zero WebView / Browser Overhead:** Strictly no Electron, Tauri, or WebView2. All UI rendering and animations run natively on the GPU via Slint (`renderer-skia` or `renderer-femtovg`).
- **Strict MVC Architecture:** Clear separation between Declarative Views (`.slint`), Business Logic Controllers (`src/controllers/`), Data Models (`src/models/`), and Hardware/Processing Services (`src/services/`).
- **Dual Hardware Modes:**
  1. **Mock Mode (Default for Dev):** Works out-of-the-box on any Windows PC without physical booth hardware. Uses webcam (or procedural RGB frames) for Live View and saves printed outputs to disk.
  2. **Production Hardware Mode:** Targets **Canon EOS 700D** (via digiCamControl local HTTP/MJPEG server at `http://localhost:5513`) and **DNP DS-RX1HS** (via Windows silent printing / SumatraPDF CLI or Win32 GDI Spooler, supporting 4R and 2x6 Strip Cut).

---

## 2. Exact `Cargo.toml` & `build.rs` Specification

### `Cargo.toml`
~~~toml
[package]
name = "photobooth-rs"
version = "0.1.0"
edition = "2021"
build = "build.rs"

[dependencies]
slint = { version = "1.8", features = ["backend-winit", "renderer-skia"] }
tokio = { version = "1.40", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
image = "0.25"
imageproc = "0.25"
rayon = "1.10"
reqwest = { version = "0.12", features = ["json", "stream", "blocking"] }
rusqlite = { version = "0.32", features = ["bundled"] }
qrcode = "0.14"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.10", features = ["v4"] }
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"

[build-dependencies]
slint-build = "1.8"
~~~

### `build.rs`
~~~rust
fn main() {
    slint_build::compile("ui/app.slint").expect("Failed to compile Slint UI");
}
~~~

---

## 3. Strict MVC Directory Structure

~~~text
photobooth-rs/
├── Cargo.toml
├── build.rs
├── assets/
│   ├── templates/
│   │   ├── default_4r_overlay.png      # Auto-generated on first boot if missing
│   │   ├── default_strip_overlay.png   # Auto-generated on first boot if missing
│   │   └── templates.json              # Template definitions & slot coordinates
├── output/
│   ├── raw/                            # Captured raw high-res photos per session
│   ├── composites/                     # Final merged 1800x1200 (300 DPI) photos
│   └── mock_prints/                    # Simulated printer output in Mock Mode
├── ui/                                 # [VIEW LAYER] 100% Declarative Slint UI + Animations
│   ├── theme.slint                     # Colors, typography, gradients, animation timings
│   ├── app.slint                       # Main Window, State Router, Callbacks & Global Properties
│   ├── components/
│   │   ├── animated_button.slint       # Touch button with scale & glow transitions
│   │   └── flash_overlay.slint         # Camera shutter white flash animation
│   └── views/
│       ├── idle_view.slint             # Attract screen with pulsing CTA animation
│       ├── capture_view.slint          # Live View video frame + animated 3-2-1 countdown
│       ├── processing_view.slint       # Smooth spinner & compositing status
│       ├── result_view.slint           # Final composite preview, QR code card & print status
│       └── admin_modal.slint           # PIN pad & Hardware/Mock configuration panel
└── src/
    ├── main.rs                         # Entry point: initializes DB, default assets, and MVC wiring
    ├── models/                         # [MODEL LAYER] Data Structs, Config & SQLite Persistence
    │   ├── mod.rs
    │   ├── config_model.rs             # AppConfig struct & JSON load/save (`config.json`)
    │   ├── session_model.rs            # PhotoSession struct & SQLite CRUD + offline upload queue
    │   └── template_model.rs           # TemplateConfig & SlotCoordinate structs
    ├── controllers/                    # [CONTROLLER LAYER] Bridges Slint Callbacks <-> Models/Services
    │   ├── mod.rs
    │   ├── booth_controller.rs         # FSM transitions, countdown timer loops, Live View frame push
    │   ├── admin_controller.rs         # PIN validation, mode switching (Mock <-> Hardware), hardware test
    │   └── print_controller.rs         # Dispatches print jobs & updates UI print status
    └── services/                       # [SERVICE LAYER] Hardware Abstraction & Heavy Computation
        ├── mod.rs
        ├── bootstrap_service.rs        # Auto-creates folders, SQLite tables, and sample PNG overlays
        ├── camera_service.rs           # `CameraProvider` trait (`MockCamera` & `Canon700dCamera`)
        ├── printer_service.rs          # `PrinterProvider` trait (`MockPrinter` & `DnpRx1hsPrinter`)
        ├── compositor_service.rs       # Multi-threaded crop, resize, 2x6 duplication & PNG alpha-blend
        └── qr_service.rs               # Generates QR code directly into `slint::Image`
~~~

---

## 4. Detailed Layer Specifications

### A. VIEW LAYER (`ui/*.slint`) - Visuals & 60fps Native Animations
1. **Window & Kiosk Mode:**
   - Default resolution: `1920x1080` (responsive layout).
   - Modern Dark Glassmorphism theme: background `#0F172A`, cards `#1E293B` with `border-radius: 24px`, border `#334155`, primary accent `#6366F1` (Indigo) to `#EC4899` (Pink) gradient.
2. **State Routing (`AppState` enum in `app.slint`):**
   - `enum AppState { Idle, Capture, Processing, Result, Admin }`
3. **Required Native Animations:**
   - **Idle CTA Pulse:** The "TOUCH TO START" button uses a continuous `sin(animation-tick())` or state-based `animate` loop to gently pulse scale/glow.
   - **Countdown Number Pop:** When `countdown_number` changes (`3` -> `2` -> `1`), animate `opacity` (`1.0 -> 0.2`) and `scale` (`1.4 -> 1.0`) over `800ms` with `easing: ease-out`.
   - **Shutter Flash (`flash_overlay.slint`):** When `trigger_flash` is set to `true`, a full-screen white rectangle (`#FFFFFF`) animates `opacity` from `1.0` to `0.0` over `300ms` (`easing: ease-out`).
   - **Hidden Admin Trigger:** A transparent touch zone (`100x100px`) in the top-right corner. Tapping it 5 times within 3 seconds opens `admin_modal.slint` (PIN default: `1234`).
4. **Slint Root Properties & Callbacks (`app.slint`):**
   - `in-out property <AppState> current_state: AppState.Idle;`
   - `in-out property <image> live_view_image;`
   - `in-out property <image> composite_result_image;`
   - `in-out property <image> qr_code_image;`
   - `in-out property <int> countdown_value: 0;`
   - `in-out property <int> current_shot: 1;`
   - `in-out property <int> total_shots: 3;`
   - `in-out property <bool> flash_active: false;`
   - `in-out property <string> status_message: "";`
   - `in-out property <bool> is_hardware_mode: false;`
   - `in-out property <string> active_template_id: "default-4r-3pose";`
   - `callback start_session();`
   - `callback print_current_session();`
   - `callback finish_session();`
   - `callback admin_unlock(string /* pin */);`
   - `callback save_settings(bool /* is_hardware_mode */, string /* template_id */, string /* printer_name */, int /* countdown_sec */);`
   - `callback test_camera();`
   - `callback test_printer();`

---

### B. MODEL LAYER (`src/models/`)
1. **`config_model.rs` (`AppConfig`):**
   - Fields:
     - `hardware_mode: bool` (default: `false` -> Mock Mode)
     - `digicam_url: String` (default: `"http://127.0.0.1:5513"`)
     - `printer_name: String` (default: `"DS-RX1"`)
     - `printer_strip_preset_name: String` (default: `"DS-RX1-2x6"`)
     - `active_template_id: String` (default: `"default-4r-3pose"`)
     - `countdown_seconds: u8` (default: `3`)
     - `result_timeout_seconds: u8` (default: `15`)
     - `admin_pin: String` (default: `"1234"`)
     - `auto_print: bool` (default: `true`)
   - Persisted automatically to `config.json` in the working directory.

2. **`template_model.rs` (`TemplateConfig` & `SlotCoordinate`):**
   - Standard 4R print resolution at 300 DPI is **`1800 x 1200` pixels** (Landscape) or **`1200 x 1800` pixels** (Portrait / 2x6 Strip Dual Canvas).
   - Default `assets/templates/templates.json` structure:
~~~json
[
  {
    "id": "default-4r-3pose",
    "name": "4R Postcard (3 Poses)",
    "canvas_width": 1800,
    "canvas_height": 1200,
    "overlay_png": "assets/templates/default_4r_overlay.png",
    "is_strip_2x6": false,
    "slots": [
      { "index": 0, "x": 60, "y": 60, "width": 810, "height": 1080 },
      { "index": 1, "x": 930, "y": 60, "width": 810, "height": 510 },
      { "index": 2, "x": 930, "y": 630, "width": 810, "height": 510 }
    ]
  },
  {
    "id": "default-2x6-strip",
    "name": "2x6 Photo Strip (3 Poses - Auto Duplicate & Cut)",
    "canvas_width": 1200,
    "canvas_height": 1800,
    "overlay_png": "assets/templates/default_strip_overlay.png",
    "is_strip_2x6": true,
    "slots": [
      { "index": 0, "x": 50, "y": 60, "width": 500, "height": 480 },
      { "index": 1, "x": 50, "y": 580, "width": 500, "height": 480 },
      { "index": 2, "x": 50, "y": 1100, "width": 500, "height": 480 }
    ]
  }
]
~~~
   - **Note on `is_strip_2x6: true`:** The compositor places photos into the left half (`0..600` width), applies the `600x1800` strip overlay, and automatically duplicates the entire left half (`600x1800`) onto the right half (`x: 600..1200`) so the DNP DS-RX1HS 2x6 cutter produces two identical strips in one print!

3. **`session_model.rs` (`PhotoSession` & SQLite):**
   - Creates `booth_sessions.db` with table `sessions`:
     - `id TEXT PRIMARY KEY`, `created_at TEXT`, `template_id TEXT`, `composite_path TEXT`, `share_url TEXT`, `upload_status TEXT` (`PENDING`, `UPLOADED`, `FAILED`), `printed_copies INTEGER`.

---

### C. SERVICE LAYER (`src/services/`)
1. **`bootstrap_service.rs` (Zero-Friction First Run):**
   - On startup, creates `assets/templates/`, `output/raw/`, `output/composites/`, and `output/mock_prints/`.
   - If `default_4r_overlay.png` or `default_strip_overlay.png` does not exist, programmatically generates stylish transparent PNG overlays (using `image` and `imageproc`) with cutout transparent slots, decorative borders, and a footer banner so the user can test compositing immediately without needing external Photoshop files.

2. **`camera_service.rs` (`CameraProvider` Trait):**
   - `pub trait CameraProvider: Send + Sync`:
     - `fn fetch_live_view_frame(&self) -> anyhow::Result<(Vec<u8>, u32, u32)>;` (Returns raw RGB8 pixels, horizontally mirrored).
     - `fn capture_photo(&self, dest_path: &Path, shot_index: u8) -> anyhow::Result<PathBuf>;`
   - **`MockCamera` Implementation:**
     - `fetch_live_view_frame`: Generates a smooth, animated 60fps `640x480` RGB8 synthetic camera feed (moving gradient + live timestamp/crosshair pattern) so Live View works even on a desktop PC or VM without a webcam.
     - `capture_photo`: Generates a high-resolution `1920x1280` RGB image with distinct colors and visual pose markers (`POSE #1`, `POSE #2`, `POSE #3`) and saves to `output/raw/`.
   - **`Canon700dCamera` (digiCamControl HTTP Integration):**
     - `fetch_live_view_frame`: Fetches frame from `http://127.0.0.1:5513/liveview.jpg` (or MJPEG endpoint), decodes via `image`, flips horizontally (`image::imageops::flip_horizontal`), and returns RGB8 bytes.
     - `capture_photo`: Calls `GET http://127.0.0.1:5513/?SLC=Capture`, polls/watches the digiCamControl session download directory for the new `.jpg`, and moves/copies it to `dest_path`.

3. **`compositor_service.rs` (High-Speed Image Engine):**
   - Takes `Vec<PathBuf>` (raw photos) and `&TemplateConfig`.
   - Creates an `RgbaImage` canvas (`canvas_width x canvas_height`).
   - For each `SlotCoordinate`:
     - Loads the corresponding raw image.
     - Performs **Center-Crop (Aspect Ratio Cover)**: calculates target aspect ratio (`slot.width / slot.height`), crops the center of the source image without stretching, and resizes to `(slot.width, slot.height)` using `FilterType::Triangle` (fast & sharp).
     - Overlays the cropped photo onto `(slot.x, slot.y)`.
   - Alpha-blends `overlay_png` on top.
   - If `template.is_strip_2x6 == true`, copies the left half `[0..600, 0..1800]` to `[600..1200, 0..1800]`.
   - Saves final JPG to `output/composites/session_<uuid>.jpg` and returns both the `PathBuf` and an in-memory RGB8 buffer for instant Slint UI display.

4. **`printer_service.rs` (`PrinterProvider` Trait):**
   - `pub trait PrinterProvider: Send + Sync`:
     - `fn print_image(&self, image_path: &Path, is_strip_2x6: bool, copies: u8) -> anyhow::Result<()>;`
   - **`MockPrinter` Implementation:**
     - Copies `image_path` to `output/mock_prints/PRINT_<timestamp>.jpg` after a simulated 2-second spool delay.
   - **`DnpRx1hsPrinter` Implementation:**
     - Selects `printer_strip_preset_name` if `is_strip_2x6` is true, else `printer_name`.
     - Executes silent Windows printing via `SumatraPDF.exe -print-to "<printer>" -silent "<image_path>"` if `SumatraPDF.exe` is present, or falls back to PowerShell/Win32 print verb invocation (`Start-Process -FilePath "<image_path>" -Verb PrintTo -ArgumentList '"<printer>"' -WindowStyle Hidden`).

5. **`qr_service.rs`:**
   - Takes a URL string (e.g., `https://booth.local/s/<session_id>`) and uses the `qrcode` crate to rasterize a crisp `300x300` RGB8 buffer, converting it to `slint::Image::from_rgb8(SharedPixelBuffer)`.

---

### D. CONTROLLER LAYER (`src/controllers/`)
1. **`booth_controller.rs`:**
   - Holds `slint::Weak<AppWindow>`, `Arc<RwLock<AppConfig>>`, `Arc<SessionModel>`, and active Services.
   - Spawns a background Tokio task for **Zero-Copy Live View Streaming** (~30 FPS) whenever `current_state == AppState::Capture`. Uses `slint::invoke_from_event_loop` to construct `slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice` and assign `window.set_live_view_image(...)`.
   - Manages the **Capture Sequence Task**:
     - Loops `shot_index` from `1..=total_shots`.
     - Counts down `countdown_seconds` down to `1` (1-second intervals, updating `window.set_countdown_value`).
     - At `0`: triggers `window.set_flash_active(true)`, invokes `camera.capture_photo(...)`, shows the captured frame for `1500ms`, resets flash, and starts the next shot.
     - After all shots are captured: switches state to `AppState::Processing`, runs `compositor_service` + `qr_service` on `tokio::task::spawn_blocking`, updates `composite_result_image` and `qr_code_image`, triggers auto-print if enabled, and switches to `AppState::Result`.
     - Runs a 15-second auto-return timer on `AppState::Result` to reset back to `AppState::Idle`.

2. **`admin_controller.rs`:**
   - Verifies PIN on `admin_unlock`.
   - Saves updated settings (`hardware_mode`, `active_template_id`, `printer_name`, `countdown_seconds`) to `config.json` and hot-swaps the active `CameraProvider` and `PrinterProvider` dynamically without restarting the app.

---

## 5. Execution Rules for AI Coding Agent (Antigravity)
1. **Complete & Compilable Code Only:** Do NOT leave `todo!()`, `unimplemented!()`, or placeholder comments. Every file must be written in full.
2. **Thread Safety with Slint:** Remember that `slint::AppWindow` is NOT `Send`. Always pass `window.as_weak()` into Tokio background threads and update UI properties exclusively inside `slint::invoke_from_event_loop(move || { ... })`.
3. **Zero External Asset Dependencies:** Ensure `bootstrap_service.rs` programmatically creates `config.json`, `templates.json`, and the two transparent PNG overlays (`default_4r_overlay.png` and `default_strip_overlay.png`) on startup so `cargo run` works on the very first run with zero missing-file errors.
4. **Build Verification:** After generating all files, run `cargo check` and `cargo build` to verify there are no Slint syntax errors or Rust borrow-checker issues, and fix any compiler warnings/errors autonomously.