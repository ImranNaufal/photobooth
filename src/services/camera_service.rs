use image::{imageops, ImageBuffer, Rgb, RgbImage};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::Camera;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant, SystemTime};
use tracing::{info, warn};

pub trait CameraProvider: Send + Sync {
    /// Returns (raw_rgb8_bytes, width, height), horizontally mirrored for user-facing selfie mirror
    fn fetch_live_view_frame(&self) -> anyhow::Result<(Vec<u8>, u32, u32)>;

    /// Captures high-res photo and saves it to dest_path
    fn capture_photo(&self, dest_path: &Path, shot_index: u8) -> anyhow::Result<PathBuf>;

    /// Health check to verify camera/server connectivity
    fn is_healthy(&self) -> bool {
        true
    }
}

/// MockCamera provides procedural animated RGB8 frames for testing without camera hardware
pub struct MockCamera {
    frame_counter: AtomicU64,
}

impl MockCamera {
    pub fn new() -> Self {
        Self {
            frame_counter: AtomicU64::new(0),
        }
    }
}

impl Default for MockCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraProvider for MockCamera {
    fn fetch_live_view_frame(&self) -> anyhow::Result<(Vec<u8>, u32, u32)> {
        let width = 640u32;
        let height = 480u32;
        let frame_num = self.frame_counter.fetch_add(1, Ordering::Relaxed);

        let mut img: RgbImage = ImageBuffer::new(width, height);

        // Animated color wave based on timestamp and coordinates
        let t = (frame_num as f32 * 0.05).sin();
        let center_x = width / 2;
        let center_y = height / 2;

        let r_base = ((t * 40.0) + 120.0).clamp(0.0, 255.0) as u8;
        let g_base = 70u8;
        let b_base = (((-t) * 40.0) + 140.0).clamp(0.0, 255.0) as u8;

        for y in 0..height {
            let dy = (y as i32 - center_y as i32).abs() as u32;
            for x in 0..width {
                let dx = (x as i32 - center_x as i32).abs() as u32;

                // Subtle studio vignette
                let dist = ((dx * dx + dy * dy) as f32).sqrt();
                let vignette = (1.0 - (dist / 400.0)).clamp(0.25, 1.0);

                let r = ((r_base as f32 + (x as f32 / 10.0)) * vignette) as u8;
                let g = (g_base as f32 * vignette) as u8;
                let b = ((b_base as f32 + (y as f32 / 10.0)) * vignette) as u8;

                img.put_pixel(x, y, Rgb([r, g, b]));
            }
        }

        // Draw center crosshair
        let cross_color = Rgb([255, 255, 255]);
        for x in (center_x.saturating_sub(40))..=(center_x + 40).min(width - 1) {
            img.put_pixel(x, center_y, cross_color);
        }
        for y in (center_y.saturating_sub(40))..=(center_y + 40).min(height - 1) {
            img.put_pixel(center_x, y, cross_color);
        }

        // Draw animated scanning line
        let scan_y = ((frame_num * 4) % (height as u64)) as u32;
        let scan_color = Rgb([255, 215, 0]);
        for x in 0..width {
            img.put_pixel(x, scan_y, scan_color);
        }

        // Horizontal mirror for selfie view
        let mirrored = imageops::flip_horizontal(&img);
        let raw_bytes = mirrored.into_raw();

        Ok((raw_bytes, width, height))
    }

    fn capture_photo(&self, dest_path: &Path, shot_index: u8) -> anyhow::Result<PathBuf> {
        let width = 1920u32;
        let height = 1280u32;
        let mut img: RgbImage = ImageBuffer::new(width, height);

        // Different background tone per pose
        let (bg_r, bg_g, bg_b) = match shot_index % 3 {
            1 => (59, 130, 246), // Vibrant Blue
            2 => (236, 72, 153), // Vibrant Pink
            _ => (16, 185, 129), // Vibrant Emerald
        };

        for y in 0..height {
            for x in 0..width {
                let fx = x as f32 / width as f32;
                let fy = y as f32 / height as f32;
                let r = (bg_r as f32 * (1.0 - fx * 0.3)) as u8;
                let g = (bg_g as f32 * (1.0 - fy * 0.3)) as u8;
                let b = (bg_b as f32 * (0.8 + fx * 0.2)) as u8;
                img.put_pixel(x, y, Rgb([r, g, b]));
            }
        }

        // Draw large stylized central pose banner
        let banner_color = Rgb([15, 23, 42]);
        let start_y = height / 2 - 140;
        let end_y = height / 2 + 140;
        for y in start_y..end_y {
            for x in 100..(width - 100) {
                img.put_pixel(x, y, banner_color);
            }
        }

        // Draw a distinct pattern for the pose
        let dot_color = Rgb([255, 255, 255]);
        let center_x = width / 2;
        let center_y = height / 2;
        let count = shot_index as i32;

        for i in 0..count {
            let offset_x = (i - count / 2) * 120;
            let cx = (center_x as i32 + offset_x) as u32;
            let cy = center_y;

            for dy in 0..30 {
                for dx in 0..30 {
                    if (dx as i32 - 15).pow(2) + (dy as i32 - 15).pow(2) <= 15 * 15 {
                        let px = cx.saturating_add(dx).min(width - 1);
                        let py = cy.saturating_add(dy).min(height - 1);
                        img.put_pixel(px, py, dot_color);
                    }
                }
            }
        }

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        img.save(dest_path)?;
        info!(
            "Mock camera captured pose #{} to {:?}",
            shot_index, dest_path
        );
        Ok(dest_path.to_path_buf())
    }
}

/// Canon700dCamera communicates via digiCamControl local REST / HTTP server
pub struct Canon700dCamera {
    base_url: String,
    client: reqwest::blocking::Client,
}

impl Canon700dCamera {
    pub fn new(base_url: String) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(2500))
            .build()
            .unwrap_or_default();

        Self { base_url, client }
    }
}

impl CameraProvider for Canon700dCamera {
    fn fetch_live_view_frame(&self) -> anyhow::Result<(Vec<u8>, u32, u32)> {
        let url = format!("{}/liveview.jpg", self.base_url.trim_end_matches('/'));
        let resp = self.client.get(&url).send()?;
        let bytes = resp.bytes()?;

        let img = image::load_from_memory(&bytes)?;
        let rgb_img = img.to_rgb8();
        let mirrored = imageops::flip_horizontal(&rgb_img);
        let (width, height) = mirrored.dimensions();
        let raw_bytes = mirrored.into_raw();

        Ok((raw_bytes, width, height))
    }

    fn is_healthy(&self) -> bool {
        let possible_cli = [
            Path::new(r"C:\Program Files (x86)\digiCamControl\CameraControlCmd.exe"),
            Path::new(r"C:\Program Files\digiCamControl\CameraControlCmd.exe"),
            Path::new(r"CameraControlCmd.exe"),
        ];
        if possible_cli.iter().any(|p| p.exists()) {
            return true;
        }

        let test_url = format!("{}/liveview.jpg", self.base_url.trim_end_matches('/'));
        self.client.head(&test_url).send().map_or(false, |r| {
            r.status().is_success() || r.status().as_u16() == 404
        })
    }

    fn capture_photo(&self, dest_path: &Path, shot_index: u8) -> anyhow::Result<PathBuf> {
        let capture_start = SystemTime::now();

        // Resolve absolute destination path to guarantee it writes to the intended folder
        let abs_dest = if dest_path.is_absolute() {
            dest_path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(dest_path)
        };

        if let Some(parent) = abs_dest.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // 1. Direct Headless CLI Shutter (CameraControlCmd.exe)
        // Auto-connects directly to Canon DSLR over USB without needing GUI or web server!
        let possible_cli = [
            PathBuf::from(r"C:\Program Files (x86)\digiCamControl\CameraControlCmd.exe"),
            PathBuf::from(r"C:\Program Files\digiCamControl\CameraControlCmd.exe"),
            PathBuf::from(r"CameraControlCmd.exe"),
        ];

        for cli in &possible_cli {
            if cli.exists() {
                // Ensure no leftover or zombie CameraControlCmd processes hold the Canon USB device
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    let _ = std::process::Command::new("taskkill")
                        .args(["/F", "/IM", "CameraControlCmd.exe"])
                        .creation_flags(0x08000000)
                        .status();
                }

                // Digicamcontrol requires strict Windows backslashes without mixed forward slashes
                let clean_dest = abs_dest.to_string_lossy().replace('/', "\\");
                info!("Triggering Direct Canon Shutter via CLI: {:?} to {:?}", cli, clean_dest);

                // Remove previous file so we only accept fresh capture
                let _ = fs::remove_file(&abs_dest);

                let mut cmd = std::process::Command::new(cli);
                cmd.args(["/filename", &clean_dest, "/capture"]);
                cmd.stdin(std::process::Stdio::null());

                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
                }

                if let Ok(mut child) = cmd.spawn() {
                    let wait_start = Instant::now();
                    while wait_start.elapsed() < Duration::from_secs(20) {
                        if let Ok(Some(status)) = child.try_wait() {
                            info!("CameraControlCmd finished: {:?}", status);
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(250));
                    }
                    let _ = child.kill();
                }

                if abs_dest.exists() {
                    let size = abs_dest.metadata().map(|m| m.len()).unwrap_or(0);
                    if size > 10_000 {
                        info!(
                            "Direct Canon CLI captured high-res DSLR photo ({} bytes) for pose #{} to {:?}",
                            size, shot_index, abs_dest
                        );
                        return Ok(abs_dest);
                    }
                }
            }
        }

        // 2. Fallback: HTTP Web Server Mode
        info!(
            "Sending capture command to digiCamControl web server: {}",
            self.base_url
        );
        let capture_url = format!("{}/?SLC=Capture", self.base_url.trim_end_matches('/'));

        let resp = self.client.get(&capture_url).send();
        if let Err(e) = resp {
            warn!(
                "Capture request failed or timed out: {:?}. Checking download folder.",
                e
            );
        }

        // Wait up to 5 seconds for digiCamControl to save image or query lastcaptured
        let last_url = format!("{}/lastcaptured", self.base_url.trim_end_matches('/'));
        let start = Instant::now();

        while start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(400));

            if let Ok(resp) = self.client.get(&last_url).send() {
                if resp.status().is_success() {
                    if let Ok(bytes) = resp.bytes() {
                        if !bytes.is_empty() {
                            fs::write(&abs_dest, &bytes)?;
                            info!("Successfully saved captured photo to {:?}", abs_dest);
                            return Ok(abs_dest);
                        }
                    }
                }
            }
        }

        // Fallback: Check standard digiCamControl session directory if present
        let dcc_session_dir = Path::new(r"C:\Users\Public\Pictures\digiCamControl\Session1");
        if dcc_session_dir.exists() {
            if let Ok(entries) = fs::read_dir(dcc_session_dir) {
                let mut latest_file: Option<(PathBuf, SystemTime)> = None;
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension()
                        .map_or(false, |ext| ext.eq_ignore_ascii_case("jpg"))
                    {
                        if let Ok(metadata) = p.metadata() {
                            if let Ok(modified) = metadata.modified() {
                                // Guard against stale photo: must be created/modified after capture command
                                if modified
                                    >= capture_start
                                        .checked_sub(Duration::from_secs(1))
                                        .unwrap_or(capture_start)
                                {
                                    if latest_file.as_ref().map_or(true, |(_, m)| modified > *m) {
                                        latest_file = Some((p, modified));
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some((src, _)) = latest_file {
                    fs::copy(&src, &abs_dest)?;
                    info!(
                        "Copied photo from digiCamControl session to {:?}",
                        abs_dest
                    );
                    return Ok(abs_dest);
                }
            }
        }

        // If hardware is not reachable, fallback to Mock image generation to prevent crash
        warn!("Canon 700D hardware capture timed out. Falling back to synthetic capture for pose #{}.", shot_index);
        let mock = MockCamera::new();
        mock.capture_photo(&abs_dest, shot_index)
    }
}

/// LaptopWebcamCamera captures live frames from the laptop's built-in / USB webcam
pub struct LaptopWebcamCamera {
    camera: StdMutex<Option<Camera>>,
}

// Safety: Access to the underlying Camera is protected by a Mutex, ensuring mutual exclusion.
unsafe impl Send for LaptopWebcamCamera {}
unsafe impl Sync for LaptopWebcamCamera {}

impl LaptopWebcamCamera {
    pub fn new() -> Self {
        let devices = nokhwa::query(nokhwa::utils::ApiBackend::Auto).unwrap_or_default();
        info!("LaptopWebcamCamera: Detected {} camera device(s)", devices.len());
        for dev in &devices {
            info!("  -> Found camera [{:?}]: {}", dev.index(), dev.human_name());
        }

        let indices_to_try: Vec<CameraIndex> = if devices.is_empty() {
            vec![CameraIndex::Index(0)]
        } else {
            devices.iter().map(|d| d.index().clone()).collect()
        };

        let mut opened_camera: Option<Camera> = None;

        for idx in indices_to_try {
            // Attempt 1: Absolute highest resolution
            let req_high = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);
            let open_attempt = match Camera::new(idx.clone(), req_high) {
                Ok(mut c) => match c.open_stream() {
                    Ok(_) => {
                        info!("Webcam {:?} streaming at highest resolution ({:?})", idx, c.resolution());
                        Ok(c)
                    }
                    Err(e) => {
                        warn!("Webcam {:?} failed to stream at high-res: {:?}. Retrying with default format...", idx, e);
                        let req_def = RequestedFormat::new::<RgbFormat>(RequestedFormatType::None);
                        Camera::new(idx.clone(), req_def).and_then(|mut c2| {
                            c2.open_stream()?;
                            info!("Webcam {:?} streaming at default resolution ({:?})", idx, c2.resolution());
                            Ok(c2)
                        })
                    }
                },
                Err(e) => {
                    warn!("Webcam {:?} high-res format not supported: {:?}. Retrying with default format...", idx, e);
                    let req_def = RequestedFormat::new::<RgbFormat>(RequestedFormatType::None);
                    Camera::new(idx.clone(), req_def).and_then(|mut c2| {
                        c2.open_stream()?;
                        info!("Webcam {:?} streaming at default resolution ({:?})", idx, c2.resolution());
                        Ok(c2)
                    })
                }
            };

            match open_attempt {
                Ok(cam) => {
                    opened_camera = Some(cam);
                    break;
                }
                Err(err) => {
                    warn!("Failed to initialize camera {:?}: {:?}", idx, err);
                }
            }
        }

        if opened_camera.is_none() {
            warn!("No working webcam found. Operating in fallback Mock mode.");
        }

        Self {
            camera: StdMutex::new(opened_camera),
        }
    }
}

impl Default for LaptopWebcamCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraProvider for LaptopWebcamCamera {
    fn fetch_live_view_frame(&self) -> anyhow::Result<(Vec<u8>, u32, u32)> {
        let mut guard = self.camera.lock().unwrap();
        if let Some(cam) = guard.as_mut() {
            if let Ok(frame) = cam.frame() {
                if let Ok(rgb_img) = frame.decode_image::<RgbFormat>() {
                    let mirrored = imageops::flip_horizontal(&rgb_img);
                    let (w, h) = mirrored.dimensions();
                    return Ok((mirrored.into_raw(), w, h));
                }
            }
        }
        // Fallback to procedural animated frames if stream is unavailable
        let mock = MockCamera::new();
        mock.fetch_live_view_frame()
    }

    fn capture_photo(&self, dest_path: &Path, shot_index: u8) -> anyhow::Result<PathBuf> {
        let mut guard = self.camera.lock().unwrap();
        if let Some(cam) = guard.as_mut() {
            match cam.frame() {
                Ok(frame) => match frame.decode_image::<RgbFormat>() {
                    Ok(rgb_img) => {
                        let mirrored = imageops::flip_horizontal(&rgb_img);
                        if let Some(parent) = dest_path.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        mirrored.save(dest_path)?;
                        info!("Laptop webcam captured real frame for pose #{} to {:?}", shot_index, dest_path);
                        return Ok(dest_path.to_path_buf());
                    }
                    Err(e) => {
                        warn!("Webcam frame decode error: {:?}. Falling back to mock capture...", e);
                    }
                },
                Err(e) => {
                    warn!("Webcam capture frame error: {:?}. Falling back to mock capture...", e);
                }
            }
        } else {
            info!("No physical webcam active. Generating simulated photo #{}...", shot_index);
        }
        let mock = MockCamera::new();
        mock.capture_photo(dest_path, shot_index)
    }

    fn is_healthy(&self) -> bool {
        let guard = self.camera.lock().unwrap();
        guard.as_ref().map_or(false, |c| c.is_stream_open())
    }
}
