use axum::{
    body::Body,
    extract::Path,
    http::{header, Response, StatusCode},
    response::Html,
    routing::get,
    Router,
};
use std::io::{BufRead, BufReader};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path as StdPath, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::fs;
use tracing::{error, info, warn};

use crate::services::LoggerService;

#[derive(Clone)]
pub struct WebService {
    local_ip: IpAddr,
    port: u16,
    public_url: Arc<StdRwLock<Option<String>>>,
}

impl WebService {
    pub fn new(port: u16) -> Self {
        let local_ip = Self::detect_local_ip();
        info!(
            "WebService initialized with local IP: {}:{}",
            local_ip, port
        );
        Self {
            local_ip,
            port,
            public_url: Arc::new(StdRwLock::new(None)),
        }
    }

    /// Auto-detects primary local network IP (WiFi / Ethernet / Hotspot)
    pub fn detect_local_ip() -> IpAddr {
        if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
            // Connecting to a public IP populates socket local_addr without sending packets
            if socket.connect("8.8.8.8:80").is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    return addr.ip();
                }
            }
        }
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
    }

    pub fn get_share_url(&self, session_id: &str) -> String {
        if let Ok(guard) = self.public_url.read() {
            if let Some(ref base) = *guard {
                return format!("{}/s/{}", base.trim_end_matches('/'), session_id);
            }
        }
        format!("http://{}:{}/s/{}", self.local_ip, self.port, session_id)
    }

    pub fn is_public_tunnel_active(&self) -> bool {
        self.public_url
            .read()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    pub fn start_background_server(self: Arc<Self>) {
        let port = self.port;
        let public_url = self.public_url.clone();

        tokio::spawn(async move {
            let app = Router::new()
                .route("/", get(handle_root))
                .route("/logs", get(handle_logs_page))
                .route("/logs/raw", get(handle_logs_raw))
                .route("/api/logs", get(handle_logs_api))
                .route("/s/:id", get(handle_share_page))
                .route("/photo/:id", get(handle_photo))
                .route("/download/:id", get(handle_download))
                .route("/raw/:id/:pose", get(handle_raw_photo))
                .route("/raw_thumb/:id/:pose", get(handle_raw_thumb))
                .route("/raw_download/:id/:pose", get(handle_raw_download));

            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            info!("Starting Photobooth Web Server on http://0.0.0.0:{}", port);

            match tokio::net::TcpListener::bind(addr).await {
                Ok(listener) => {
                    // Launch Cloudflare Quick Tunnel supervisor so guests on 4G/5G can scan QR seamlessly
                    std::thread::spawn(move || {
                        run_cloudflared_supervisor(port, public_url);
                    });

                    if let Err(e) = axum::serve(listener, app).await {
                        error!("Photobooth Web Server crashed: {:?}", e);
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to bind Photobooth Web Server to port {}: {:?}",
                        port, e
                    );
                }
            }
        });
    }
}

fn find_cloudflared_binary() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("cloudflared.exe");
            if p.exists() {
                return Some(p);
            }
        }
    }
    let candidates = [
        StdPath::new("cloudflared.exe"),
        StdPath::new("tools/cloudflared/cloudflared.exe"),
        StdPath::new(r"C:\Program Files (x86)\cloudflared\cloudflared.exe"),
        StdPath::new(r"C:\Program Files\cloudflared\cloudflared.exe"),
    ];
    for c in &candidates {
        if c.exists() {
            return Some(c.to_path_buf());
        }
    }
    Some(PathBuf::from("cloudflared.exe"))
}

fn extract_trycloudflare_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == ':' || c == '/'))
        .unwrap_or(rest.len());
    let candidate = rest[..end].trim_end_matches('/');
    if candidate.ends_with(".trycloudflare.com") && candidate.len() > "https://.trycloudflare.com".len() {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn run_cloudflared_supervisor(port: u16, public_url: Arc<StdRwLock<Option<String>>>) {
    #[cfg(target_os = "windows")]
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    loop {
        let Some(bin) = find_cloudflared_binary() else {
            return;
        };

        // Kill any orphaned cloudflared.exe from previous sessions
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill")
                .args(["/F", "/IM", "cloudflared.exe"])
                .creation_flags(CREATE_NO_WINDOW)
                .status();
        }

        let target = format!("http://127.0.0.1:{}", port);
        let mut cmd = Command::new(&bin);
        cmd.args(["tunnel", "--no-autoupdate", "--url", &target])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        match cmd.spawn() {
            Ok(mut child) => {
                info!("Spawned cloudflared tunnel process for {}", target);
                if let Some(stderr) = child.stderr.take() {
                    let reader = BufReader::new(stderr);
                    for line_res in reader.lines() {
                        if let Ok(line) = line_res {
                            if let Some(url) = extract_trycloudflare_url(&line) {
                                info!("🌐 Public QR Tunnel ready: {}", url);
                                if let Ok(mut guard) = public_url.write() {
                                    *guard = Some(url);
                                }
                            }
                        }
                    }
                }
                let _ = child.wait();
                warn!("cloudflared tunnel exited; clearing public URL and retrying in 5s...");
                if let Ok(mut guard) = public_url.write() {
                    *guard = None;
                }
            }
            Err(e) => {
                warn!("cloudflared unavailable ({:?}); QR uses local WiFi IP fallback", e);
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

async fn handle_root() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Photobooth Kiosk</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0F172A; color: #F8FAFC; text-align: center; padding: 40px 20px; }
        .card { max-width: 400px; margin: 0 auto; background: #1E293B; border-radius: 24px; padding: 32px; border: 1px solid #334155; }
        h1 { font-size: 24px; margin-bottom: 8px; color: #A5B4FC; }
        p { color: #94A3B8; font-size: 15px; }
    </style>
</head>
<body>
    <div class="card">
        <h1>📸 Photobooth Kiosk</h1>
        <p>Your photos will appear here after your session!</p>
    </div>
</body>
</html>"#,
    )
}

async fn handle_share_page(Path(id): Path<String>) -> Html<String> {
    let sanitized_id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();

    // Check which raw pose files exist for this session
    let mut raw_cards = String::new();
    for pose in 1..=3u8 {
        let raw_path = PathBuf::from("output/raw").join(format!("session_{}_pose_{}.jpg", sanitized_id, pose));
        if raw_path.exists() {
            raw_cards.push_str(&format!(
                r#"<div class="raw-card">
                    <img src="/raw_thumb/{id}/{pose}" alt="Pose {pose}" loading="lazy">
                    <a href="/raw_download/{id}/{pose}" class="btn-sm" download="pose_{pose}_{id}.jpg">⬇ Pose {pose}</a>
                </div>"#,
                id = sanitized_id,
                pose = pose
            ));
        }
    }

    let raw_section = if raw_cards.is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="raw-section">
                <h2>Individual Shots</h2>
                <div class="raw-grid">{cards}</div>
            </div>"#,
            cards = raw_cards
        )
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
    <title>Your Photobooth Memory</title>
    <style>
        * {{ box-sizing: border-box; margin: 0; padding: 0; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #0F172A;
            color: #F8FAFC;
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
            padding: 20px;
        }}
        .container {{
            width: 100%;
            max-width: 480px;
            background: #1E293B;
            border-radius: 28px;
            padding: 24px;
            box-shadow: 0 20px 40px rgba(0,0,0,0.4);
            border: 1px solid #334155;
            text-align: center;
        }}
        .header {{ margin-bottom: 20px; }}
        .header h1 {{
            font-size: 22px;
            font-weight: 800;
            background: linear-gradient(135deg, #818CF8 0%, #F472B6 100%);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }}
        .header p {{ color: #94A3B8; font-size: 14px; margin-top: 4px; }}
        .preview-wrapper {{
            border-radius: 16px;
            overflow: hidden;
            background: #0B0F19;
            margin-bottom: 20px;
            border: 2px solid #334155;
        }}
        .preview-wrapper img {{
            width: 100%;
            height: auto;
            display: block;
        }}
        .btn {{
            display: inline-block;
            width: 100%;
            padding: 16px 24px;
            font-size: 16px;
            font-weight: 700;
            color: #ffffff;
            text-decoration: none;
            background: linear-gradient(135deg, #6366F1 0%, #EC4899 100%);
            border-radius: 16px;
            box-shadow: 0 8px 24px rgba(99, 102, 241, 0.4);
            transition: transform 0.15s ease, opacity 0.15s ease;
        }}
        .btn:active {{ transform: scale(0.98); opacity: 0.9; }}
        .raw-section {{
            margin-top: 24px;
            padding-top: 20px;
            border-top: 1px solid #334155;
            text-align: left;
        }}
        .raw-section h2 {{
            font-size: 14px;
            text-transform: uppercase;
            letter-spacing: 1px;
            color: #94A3B8;
            margin-bottom: 12px;
            text-align: center;
        }}
        .raw-grid {{
            display: grid;
            grid-template-columns: repeat(3, 1fr);
            gap: 10px;
        }}
        .raw-card {{
            background: #0F172A;
            border-radius: 12px;
            padding: 6px;
            border: 1px solid #334155;
            text-align: center;
        }}
        .raw-card img {{
            width: 100%;
            height: 80px;
            object-fit: cover;
            border-radius: 8px;
            margin-bottom: 6px;
            display: block;
        }}
        .btn-sm {{
            display: block;
            padding: 6px 4px;
            font-size: 11px;
            font-weight: 700;
            color: #E2E8F0;
            background: #334155;
            border-radius: 8px;
            text-decoration: none;
        }}
        .footer {{
            margin-top: 20px;
            color: #64748B;
            font-size: 12px;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>✨ YOUR PHOTO IS READY! ✨</h1>
            <p>Save your print composite or individual DSLR shots below</p>
        </div>
        <div class="preview-wrapper">
            <img src="/photo/{id}" alt="Photobooth Composite Photo" loading="lazy">
        </div>
        <a href="/download/{id}" class="btn" download="photobooth_{id}.jpg">
            📥 Save Composite Photo
        </a>
        {raw_section}
        <div class="footer">
            Photobooth RS &bull; 300 DPI Original Quality
        </div>
    </div>
</body>
</html>"#,
        id = sanitized_id,
        raw_section = raw_section
    );

    Html(html)
}

async fn handle_raw_thumb(Path((id, pose)): Path<(String, String)>) -> Result<Response<Body>, StatusCode> {
    let sanitized_id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if !matches!(pose.as_str(), "1" | "2" | "3") {
        return Err(StatusCode::BAD_REQUEST);
    }

    let path = PathBuf::from("output/raw").join(format!("session_{}_pose_{}.jpg", sanitized_id, pose));
    if !path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let thumb_res = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let img = image::open(&path)?;
        let resized = img.resize(480, 320, image::imageops::FilterType::Triangle);
        let mut buf = std::io::Cursor::new(Vec::new());
        resized.write_to(&mut buf, image::ImageFormat::Jpeg)?;
        Ok(buf.into_inner())
    })
    .await;

    match thumb_res {
        Ok(Ok(data)) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/jpeg")
            .header(header::CACHE_CONTROL, "public, max-age=86400")
            .body(Body::from(data))
            .unwrap()),
        _ => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn handle_raw_photo(Path((id, pose)): Path<(String, String)>) -> Result<Response<Body>, StatusCode> {
    let sanitized_id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if !matches!(pose.as_str(), "1" | "2" | "3") {
        return Err(StatusCode::BAD_REQUEST);
    }

    let path = PathBuf::from("output/raw").join(format!("session_{}_pose_{}.jpg", sanitized_id, pose));
    if !path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    match fs::read(&path).await {
        Ok(data) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/jpeg")
            .header(header::CACHE_CONTROL, "public, max-age=86400")
            .body(Body::from(data))
            .unwrap()),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn handle_raw_download(Path((id, pose)): Path<(String, String)>) -> Result<Response<Body>, StatusCode> {
    let sanitized_id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if !matches!(pose.as_str(), "1" | "2" | "3") {
        return Err(StatusCode::BAD_REQUEST);
    }

    let path = PathBuf::from("output/raw").join(format!("session_{}_pose_{}.jpg", sanitized_id, pose));
    if !path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    match fs::read(&path).await {
        Ok(data) => {
            let filename = format!("attachment; filename=\"pose_{}_{}.jpg\"", pose, sanitized_id);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header(header::CONTENT_DISPOSITION, filename)
                .body(Body::from(data))
                .unwrap())
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn handle_photo(Path(id): Path<String>) -> Result<Response<Body>, StatusCode> {
    let sanitized_id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();

    let path = PathBuf::from("output/composites").join(format!("session_{}.jpg", sanitized_id));
    if !path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    match fs::read(&path).await {
        Ok(data) => {
            let response = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header(header::CACHE_CONTROL, "public, max-age=86400")
                .body(Body::from(data))
                .unwrap();
            Ok(response)
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn handle_download(Path(id): Path<String>) -> Result<Response<Body>, StatusCode> {
    let sanitized_id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();

    let path = PathBuf::from("output/composites").join(format!("session_{}.jpg", sanitized_id));
    if !path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    match fs::read(&path).await {
        Ok(data) => {
            let filename = format!("attachment; filename=\"photobooth_{}.jpg\"", sanitized_id);
            let response = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header(header::CONTENT_DISPOSITION, filename)
                .body(Body::from(data))
                .unwrap();
            Ok(response)
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn handle_logs_api() -> Response<Body> {
    let logs = LoggerService::get_recent_logs();
    let json = serde_json::to_string(&logs).unwrap_or_else(|_| "[]".into());
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(json))
        .unwrap()
}

async fn handle_logs_raw() -> Response<Body> {
    match LoggerService::read_full_log_file() {
        Ok(content) => Response::builder()
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .header(header::CONTENT_DISPOSITION, "attachment; filename=\"photobooth.log\"")
            .body(Body::from(content))
            .unwrap(),
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("No log file found on kiosk."))
            .unwrap(),
    }
}

async fn handle_logs_page() -> Html<String> {
    let raw_logs = LoggerService::get_recent_logs();
    let mut log_html = String::new();

    for line in &raw_logs {
        let (color, bg) = if line.contains("ERROR") || line.contains("error") {
            ("#f87171", "rgba(239, 68, 68, 0.15)")
        } else if line.contains("WARN") || line.contains("warn") {
            ("#fbbf24", "rgba(245, 158, 11, 0.12)")
        } else if line.contains("INFO") {
            ("#38bdf8", "transparent")
        } else {
            ("#94a3b8", "transparent")
        };

        let escaped = line
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");

        log_html.push_str(&format!(
            r#"<div style="color: {}; background: {}; padding: 3px 8px; border-radius: 4px; font-family: 'Consolas', 'Courier New', monospace; font-size: 13px; line-height: 1.45; word-break: break-all;">{}</div>"#,
            color, bg, escaped
        ));
    }

    if log_html.is_empty() {
        log_html.push_str(r#"<div style="color: #64748b; padding: 12px; font-style: italic;">No logs recorded yet.</div>"#);
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Photobooth RS - Remote Diagnostics & Logs</title>
    <style>
        body {{
            margin: 0; padding: 20px;
            background: #0B0F19; color: #F1F5F9;
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
        }}
        .container {{ max-width: 1000px; margin: 0 auto; }}
        .header {{
            display: flex; justify-content: space-between; align-items: center;
            background: #111827; border: 1px solid #1F2937;
            padding: 18px 24px; border-radius: 16px; margin-bottom: 16px;
        }}
        .title h1 {{ margin: 0; font-size: 20px; font-weight: 800; color: #818CF8; }}
        .title p {{ margin: 4px 0 0; font-size: 13px; color: #94A3B8; }}
        .actions a, .actions button {{
            display: inline-flex; align-items: center; gap: 6px;
            background: #312E81; color: #E0E7FF; text-decoration: none;
            padding: 8px 16px; border-radius: 999px; font-size: 13px; font-weight: 600;
            border: 1px solid #4338CA; cursor: pointer; transition: all 0.2s;
        }}
        .actions a:hover, .actions button:hover {{ background: #4338CA; }}
        .badge {{
            display: inline-block; padding: 2px 8px; border-radius: 12px;
            font-size: 11px; font-weight: 700; background: #064E3B; color: #6EE7B7;
            margin-left: 8px;
        }}
        .log-box {{
            background: #090D16; border: 1px solid #1E293B; border-radius: 16px;
            padding: 16px; min-height: 480px; max-height: 75vh; overflow-y: auto;
            box-shadow: 0 10px 25px -5px rgba(0, 0, 0, 0.5);
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <div class="title">
                <h1>📸 Photobooth RS Diagnostics <span class="badge">LIVE</span></h1>
                <p>Real-time terminal stream from remote kiosk • Auto-refreshes every 3s</p>
            </div>
            <div class="actions">
                <button onclick="location.reload()">🔄 Refresh</button>
                <a href="/logs/raw">📥 Download .log</a>
            </div>
        </div>
        <div class="log-box" id="logBox">
            {}
        </div>
    </div>
    <script>
        // Auto-scroll to the bottom of the log viewer
        const logBox = document.getElementById('logBox');
        logBox.scrollTop = logBox.scrollHeight;

        // Auto reload log every 3 seconds
        setInterval(async () => {{
            try {{
                const res = await fetch('/api/logs');
                if (res.ok) {{
                    const logs = await res.json();
                    let html = '';
                    for (const line of logs) {{
                        let color = '#94a3b8';
                        let bg = 'transparent';
                        if (line.includes('ERROR') || line.includes('error')) {{
                            color = '#f87171'; bg = 'rgba(239, 68, 68, 0.15)';
                        }} else if (line.includes('WARN') || line.includes('warn')) {{
                            color = '#fbbf24'; bg = 'rgba(245, 158, 11, 0.12)';
                        }} else if (line.includes('INFO')) {{
                            color = '#38bdf8';
                        }}
                        const esc = line.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
                        html += `<div style="color: ${{color}}; background: ${{bg}}; padding: 3px 8px; border-radius: 4px; font-family: monospace; font-size: 13px; line-height: 1.45; word-break: break-all;">${{esc}}</div>`;
                    }}
                    const isAtBottom = (logBox.scrollHeight - logBox.scrollTop - logBox.clientHeight) < 40;
                    logBox.innerHTML = html;
                    if (isAtBottom) {{ logBox.scrollTop = logBox.scrollHeight; }}
                }}
            }} catch (e) {{}}
        }}, 3000);
    </script>
</body>
</html>"#,
        log_html
    );

    Html(html)
}
