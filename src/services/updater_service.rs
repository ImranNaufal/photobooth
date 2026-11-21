use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub version: String,
    pub release_notes: String,
    pub download_url: String,
    pub sha256: Option<String>,
    pub published_at: String,
}

pub struct UpdaterService;

impl UpdaterService {
    /// Compares two semver strings (e.g. "0.2.0" vs "0.1.0", or "v0.2.0" vs "v0.1.0")
    pub fn is_newer_version(current: &str, remote: &str) -> bool {
        let parse = |v: &str| -> Vec<u32> {
            v.trim()
                .trim_start_matches('v')
                .trim_start_matches('V')
                .split('.')
                .filter_map(|s| s.parse::<u32>().ok())
                .collect()
        };

        let c = parse(current);
        let r = parse(remote);

        // Compare piece by piece (major, minor, patch)
        let max_len = c.len().max(r.len());
        for i in 0..max_len {
            let part_c = c.get(i).copied().unwrap_or(0);
            let part_r = r.get(i).copied().unwrap_or(0);
            if part_r > part_c {
                return true;
            } else if part_r < part_c {
                return false;
            }
        }
        false
    }

    /// Checks if a new version is available on the remote endpoint.
    /// Uses a short timeout (3s) so photobooth kiosks without internet never hang.
    pub async fn check_for_updates(
        current_version: &str,
        update_url: &str,
    ) -> anyhow::Result<Option<VersionInfo>> {
        info!("Checking for updates at {} (current: {})", update_url, current_version);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()?;

        let resp = match client.get(update_url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("Update check failed or offline: {:?}", e);
                return Ok(None);
            }
        };

        if !resp.status().is_success() {
            warn!("Update check server responded with status: {}", resp.status());
            return Ok(None);
        }

        let info: VersionInfo = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to parse version.json: {:?}", e);
                return Ok(None);
            }
        };

        if Self::is_newer_version(current_version, &info.version) {
            info!(
                "New version available: {} (current: {})",
                info.version, current_version
            );
            Ok(Some(info))
        } else {
            info!("Current version {} is up to date.", current_version);
            Ok(None)
        }
    }

    /// Downloads the update binary in chunks and updates progress callback (0.0 to 1.0).
    /// Upon completion, launches a detached Windows helper to swap the binary and restart.
    pub async fn download_and_install<F>(
        download_url: &str,
        progress_cb: F,
    ) -> anyhow::Result<()>
    where
        F: Fn(f32) + Send + Sync + 'static,
    {
        info!("Starting download of update from {}", download_url);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;

        let mut resp = client.get(download_url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to download update binary. HTTP Status: {}", resp.status());
        }

        let total_size = resp.content_length().unwrap_or(24_000_000);
        let mut downloaded: u64 = 0;

        let current_exe = std::env::current_exe()?;
        let current_dir = current_exe.parent().unwrap_or(Path::new("."));
        let new_exe = current_dir.join("Photobooth-RS.exe.new");

        let mut file = fs::File::create(&new_exe)?;
        while let Some(chunk) = resp.chunk().await? {
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            let progress = (downloaded as f32 / total_size as f32).min(1.0);
            progress_cb(progress);
        }
        file.flush()?;
        drop(file);

        info!("Update downloaded successfully to {:?}", new_exe);
        progress_cb(1.0);

        // Execute self-replace and restart via Windows helper script
        Self::execute_windows_restart(&current_exe, &new_exe)?;

        Ok(())
    }

    /// Spawns a detached Windows script that waits for the current process to terminate,
    /// swaps the new binary in place, restarts the application, and exits.
    fn execute_windows_restart(current_exe: &Path, new_exe: &Path) -> anyhow::Result<()> {
        let current_dir = current_exe.parent().unwrap_or(Path::new("."));
        let script_path = current_dir.join("update_restart.bat");

        let script_content = format!(
            r#"@echo off
timeout /t 2 /nobreak > nul
:loop
del "{current_exe}" > nul 2>&1
if exist "{current_exe}" (
    timeout /t 1 /nobreak > nul
    goto loop
)
move /y "{new_exe}" "{current_exe}" > nul 2>&1
start "" "{current_exe}"
del "%~f0"
"#,
            current_exe = current_exe.display(),
            new_exe = new_exe.display(),
        );

        fs::write(&script_path, script_content)?;
        info!("Created helper updater script at {:?}", script_path);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            const DETACHED_PROCESS: u32 = 0x00000008;

            std::process::Command::new("cmd.exe")
                .args(["/c", &script_path.to_string_lossy()])
                .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
                .spawn()?;
            
            info!("Launched updater script detached. Exiting current process for restart...");
            std::process::exit(0);
        }

        #[cfg(not(target_os = "windows"))]
        {
            anyhow::bail!("Self-update restart helper is currently only supported on Windows.");
        }
    }
}
