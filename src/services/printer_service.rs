use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{error, info};

pub trait PrinterProvider: Send + Sync {
    fn print_image(&self, image_path: &Path, is_strip_2x6: bool, copies: u8) -> anyhow::Result<()>;

    /// Queries the printer spooler status (e.g. Ready, Offline, Paper Out / Jam)
    #[allow(dead_code)]
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
}

impl PrinterProvider for DnpRx1hsPrinter {
    #[allow(dead_code)]
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
        let abs_path = if image_path.is_absolute() {
            image_path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(image_path)
        };

        if !abs_path.exists() {
            anyhow::bail!("Print image not found: {:?}", abs_path);
        }

        let target_printer = if is_strip_2x6 {
            &self.strip_preset_printer_name
        } else {
            &self.default_printer_name
        };

        info!(
            "Dispatching print job to target printer '{}' (fallback '{}') for image {:?}",
            target_printer, self.default_printer_name, abs_path
        );

        let clean_path = abs_path.to_string_lossy().replace('/', "\\");
        let num_copies = copies.max(1);

        // Native Windows .NET System.Drawing.Printing.PrintDocument
        // Directly spools raw bitmap to DNP printer driver without relying on external viewers
        let ps_script = format!(
            r#"
Add-Type -AssemblyName System.Drawing
$target = '{target_printer}'
$printers = Get-Printer | Select-Object -ExpandProperty Name
if ($printers -notcontains $target) {{
    $target = '{default_printer}'
}}
$img = [System.Drawing.Image]::FromFile('{clean_path}')
for ($i = 0; $i -lt {num_copies}; $i++) {{
    $pd = New-Object System.Drawing.Printing.PrintDocument
    $pd.PrinterSettings.PrinterName = $target
    $pd.DefaultPageSettings.Margins = New-Object System.Drawing.Printing.Margins(0, 0, 0, 0)
    $pd.OriginAtMargins = $false
    if ($img.Width -gt $img.Height) {{
        $pd.DefaultPageSettings.Landscape = $true
    }} else {{
        $pd.DefaultPageSettings.Landscape = $false
    }}
    $pd.add_PrintPage({{
        param($sender, $e)
        $e.Graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $e.Graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $e.Graphics.DrawImage($img, $e.PageBounds)
        $e.HasMorePages = $false
    }})
    $pd.Print()
    $pd.Dispose()
}}
$img.Dispose()
"#,
            target_printer = target_printer,
            default_printer = self.default_printer_name,
            clean_path = clean_path,
            num_copies = num_copies
        );

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let output = Command::new("powershell")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output()?;

            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                error!("Native print spooling error: {}", err);
                anyhow::bail!("Print error: {}", err);
            }
        }

        info!("Successfully sent print job to '{}'", target_printer);
        Ok(())
    }
}
