use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{error, info, warn};

pub trait PrinterProvider: Send + Sync {
    fn print_image(&self, image_path: &Path, is_strip_2x6: bool, copies: u8) -> anyhow::Result<()>;

    /// Queries the printer spooler status (e.g. Ready, Offline, Paper Out / Jam)
    fn check_status(&self, _is_strip_2x6: bool) -> anyhow::Result<String> {
        Ok("Ready".to_string())
    }
}

/// MockPrinter simulates hardware spooling by copying to output/mock_prints/
pub struct MockPrinter;

impl MockPrinter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockPrinter {
    fn default() -> Self {
        Self::new()
    }
}

impl PrinterProvider for MockPrinter {
    fn print_image(&self, image_path: &Path, is_strip_2x6: bool, copies: u8) -> anyhow::Result<()> {
        info!(
            "MockPrinter received print job: {:?}, strip_2x6: {}, copies: {}",
            image_path, is_strip_2x6, copies
        );

        // Simulate spooling delay
        sleep(Duration::from_millis(1500));

        let mock_out_dir = Path::new("output/mock_prints");
        fs::create_dir_all(mock_out_dir)?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for i in 1..=copies {
            let filename = format!(
                "PRINT_{}_copy{}_{}",
                now,
                i,
                image_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("output.jpg")
            );
            let dest = mock_out_dir.join(filename);
            fs::copy(image_path, &dest)?;
            info!("Mock print spooled to {:?}", dest);
        }

        Ok(())
    }
}

/// DnpRx1hsPrinter controls the physical DNP DS-RX1HS dye-sublimation photo printer
pub struct DnpRx1hsPrinter {
    pub default_printer_name: String,
    pub strip_preset_printer_name: String,
}

impl DnpRx1hsPrinter {
    pub fn new(default_printer: String, strip_preset: String) -> Self {
        Self {
            default_printer_name: default_printer,
            strip_preset_printer_name: strip_preset,
        }
    }

    fn find_sumatra_pdf() -> Option<PathBuf> {
        let candidates = [
            PathBuf::from("SumatraPDF.exe"),
            PathBuf::from(r"C:\Program Files\SumatraPDF\SumatraPDF.exe"),
            PathBuf::from(r"C:\Program Files (x86)\SumatraPDF\SumatraPDF.exe"),
        ];

        for c in candidates {
            if c.exists() {
                return Some(c);
            }
        }
        None
    }
}

impl PrinterProvider for DnpRx1hsPrinter {
    fn check_status(&self, is_strip_2x6: bool) -> anyhow::Result<String> {
        let target_printer = if is_strip_2x6 {
            &self.strip_preset_printer_name
        } else {
            &self.default_printer_name
        };

        let ps_cmd = format!(
            "$p = Get-CimInstance Win32_Printer -Filter \"Name = '{}'\" -ErrorAction SilentlyContinue; if ($null -eq $p) {{ 'NotFound' }} elseif ($p.WorkOffline) {{ 'Offline' }} elseif ($p.PrinterStatus -eq 1 -or $p.DetectedErrorState -ne 0) {{ 'Error: Paper Out or Jam' }} else {{ 'Ready' }}",
            target_printer
        );

        let output = Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps_cmd])
            .output();

        match output {
            Ok(out) => {
                let status = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if status.is_empty() || status == "NotFound" {
                    Ok("Ready".to_string()) // If printer not installed on dev machine, allow spool attempt
                } else {
                    Ok(status)
                }
            }
            Err(_) => Ok("Ready".to_string()),
        }
    }

    fn print_image(&self, image_path: &Path, is_strip_2x6: bool, copies: u8) -> anyhow::Result<()> {
        let target_printer = if is_strip_2x6 {
            &self.strip_preset_printer_name
        } else {
            &self.default_printer_name
        };

        // Query status first
        let status = self.check_status(is_strip_2x6)?;
        if status.contains("Offline") || status.contains("Error") {
            anyhow::bail!("Printer '{}' is not ready: {}", target_printer, status);
        }

        info!(
            "Dispatching print job to target printer '{}' for image {:?}",
            target_printer, image_path
        );

        let abs_path = fs::canonicalize(image_path)?;
        let abs_str = abs_path.to_string_lossy().to_string();

        let num_copies = copies.max(1);

        // Option A: Use SumatraPDF if installed
        if let Some(sumatra) = Self::find_sumatra_pdf() {
            info!("Found SumatraPDF at {:?}. Executing silent print.", sumatra);
            for _ in 0..num_copies {
                let status = Command::new(&sumatra)
                    .arg("-print-to")
                    .arg(target_printer)
                    .arg("-silent")
                    .arg(&abs_str)
                    .status()?;

                if !status.success() {
                    warn!("SumatraPDF exited with status: {:?}", status);
                }
            }
            return Ok(());
        }

        // Option B: Native Windows PowerShell print verb fallback
        info!("SumatraPDF not found. Using PowerShell PrintTo verb for silent printing.");
        for _ in 0..num_copies {
            let ps_cmd = format!(
                "Start-Process -FilePath '{}' -Verb PrintTo -ArgumentList '\"{}\"' -WindowStyle Hidden",
                abs_str, target_printer
            );

            let output = Command::new("powershell")
                .arg("-NoProfile")
                .arg("-Command")
                .arg(&ps_cmd)
                .output()?;

            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                error!("PowerShell print error: {}", err);
            }
        }

        Ok(())
    }
}
