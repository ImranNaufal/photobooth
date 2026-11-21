# Rekod Log Sesi Pembangunan (Session Log) - Photobooth RS

Dokumen ini merekodkan sejarah setiap perbualan, keputusan seni bina, perubahan kod, penyelesaian ralat, dan status projek supaya setiap sesi seterusnya mempunyai konteks penuh terhadap pembangunan aplikasi.

---

## Maklumat Projek
* **Nama Projek:** `photobooth-rs` (Ultra-Light Windows Photobooth Kiosk)
* **Teknologi Utama:** 100% Pure Rust, Slint UI (`backend-winit`, `renderer-femtovg`), Tokio Async Runtime, Rayon, SQLite (rusqlite bundled).
* **Lokasi Workspace:** `c:\Users\it-01995\Downloads\camera-ilhan`
* **Dokumen Rujukan Spesifikasi:** [PRD.md](file:///c:/Users/it-01995/Downloads/camera-ilhan/PRD.md)

---

## Log Sesi 1: 18 November 2026 (Pembangunan Penuh Dari Step 1 Hingga Siap)

### 1. Objektif & Permintaan Pengguna
* Membaca keseluruhan spesifikasi dalam [PRD.md](file:///c:/Users/it-01995/Downloads/camera-ilhan/PRD.md).
* Membina keseluruhan aplikasi mengikut seni bina Strict MVC dari Step 1 hingga selesai tanpa menggunakan `todo!()` atau *placeholder*.
* Memastikan `cargo check` dan `cargo build` lulus tanpa ralat.

### 2. Persediaan Persekitaran & Toolchain
1. **Pemasangan Rust & Toolchain:**
   * Memasang `rustup` dan mengkonfigurasi toolchain sasaran: `stable-x86_64-pc-windows-gnu`.
2. **Penyambung MinGW / GCC (MSVCRT):**
   * Sasaran GNU Rust pada Windows menghubungkan ke `msvcrt.dll`.
   * Memasang `BrechtSanders.WinLibs.POSIX.MSVCRT` (GCC 16.2.0 + binutils) untuk mengelakkan percanggahan memori heap UCRT (`0xc0000005`).
   * Laluan MinGW: `C:\Users\it-01995\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin`.
3. **Pemilihan Slint Renderer:**
   * [PRD.md](file:///c:/Users/it-01995/Downloads/camera-ilhan/PRD.md) membenarkan sama ada `renderer-skia` atau `renderer-femtovg`.
   * `renderer-skia` memerlukan hak pentadbir Windows (Privilege Error 1314) untuk mencipta *symlinks* semasa ekstrak sumber C++.
   * Oleh itu, enjin `renderer-femtovg` (Pure Rust GPU renderer) digunakan. Ia serasi 100%, sangat pantas, dan mengelakkan sebarang kebergantungan luar.

---

### 3. Fail & Modul Yang Dibina

#### A. Konfigurasi
* [Cargo.toml](file:///c:/Users/it-01995/Downloads/camera-ilhan/Cargo.toml): Konfigurasi depedensi (`slint`, `tokio`, `serde`, `serde_json`, `image`, `imageproc`, `rayon`, `reqwest`, `rusqlite`, `qrcode`, `chrono`, `uuid`, `anyhow`, `tracing`, `tracing-subscriber`).
* [build.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/build.rs): Menyusun fail antaramuka `ui/app.slint`.

#### B. View Layer (`ui/`)
* [theme.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/theme.slint): Warna tema Dark Glassmorphism, pemalar saiz, gaya teks, dan pemasa animasi.
* [components/animated_button.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/components/animated_button.slint): Butang sentuh tersuai berserta animasi penskalaan dan cahaya.
* [components/flash_overlay.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/components/flash_overlay.slint): Lapisan denyar putih skrin penuh dengan kesan pudar pudar (300ms ease-out).
* [views/idle_view.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/idle_view.slint): Skrin penarik dengan butang denyutan CTA dan zon sentuh tersembunyi (bucu kanan atas, 5 ketukan) untuk mod Admin.
* [views/capture_view.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/capture_view.slint): Bingkai Live View cermin, penanda sudut, penunjuk pose, dan kiraan undur 3-2-1 beranimasi.
* [views/processing_view.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/processing_view.slint): Penunjuk pemprosesan komposit 300 DPI dengan gelung berputar dan mesej status.
* [views/result_view.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/result_view.slint): Pratonton foto komposit akhir, kad Kod QR untuk imbasan muat turun, butang cetak, butang selesai, dan pemasa auto-reset (15s).
* [views/admin_modal.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/admin_modal.slint): Panel PIN (lalai: `1234`), pertukaran mod perkakasan (Mock vs Production), pemilihan templat, masa undur, dan butang ujian kamera/pencetak.
* [app.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/app.slint): Tingkap utama `AppWindow` yang menyambungkan `AppState` (Idle, Capture, Processing, Result, Admin), *callbacks*, dan penukaran skrin.

#### C. Model Layer (`src/models/`)
* [config_model.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/models/config_model.rs): Struktur `AppConfig` dan simpan/muat JSON ke `config.json`.
* [template_model.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/models/template_model.rs): Struktur `TemplateConfig` & `SlotCoordinate` untuk 4R Postcard dan 2x6 Strip Cut.
* [session_model.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/models/session_model.rs): Pangkalan data SQLite `booth_sessions.db` untuk merekod setiap sesi foto, laluan fail, URL perkongsian, dan status cetakan.
* [mod.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/models/mod.rs): Eksport modul data.

#### D. Service Layer (`src/services/`)
* [bootstrap_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/bootstrap_service.rs): Menjana struktur folder (`assets/templates/`, `output/raw/`, `output/composites/`, `output/mock_prints/`), `config.json`, `templates.json`, dan bingkai templat PNG lutsinar lalai secara programatik.
* [camera_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/camera_service.rs):
  * `CameraProvider` Trait.
  * `MockCamera`: Simulasi suapan 60fps dengan animasi gradien, palang sasaran, cap masa, dan foto pose sintetik.
  * `Canon700dCamera`: Integrasi HTTP dengan digiCamControl (`http://127.0.0.1:5513`).
* [printer_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/printer_service.rs):
  * `PrinterProvider` Trait.
  * `MockPrinter`: Menyimpan fail cetakan ke `output/mock_prints/` dengan simulasi spool 2 saat.
  * `DnpRx1hsPrinter`: Cetakan senyap melalui SumatraPDF atau PowerShell PrintTo verb.
* [compositor_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/compositor_service.rs): Enjin cantuman imej berbilang bebenang (Rayon) untuk *center-crop*, pemadanan nisbah aspek, tindanan lapisan bingkai PNG, duplikasi 2x6 auto-cut, dan penghasilan JPG 300 DPI.
* [qr_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/qr_service.rs): Penjanaan kod QR terus ke `slint::Image`.
* [mod.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/mod.rs): Eksport modul perkhidmatan.

#### E. Controller Layer (`src/controllers/`) & Entry Point
* [booth_controller.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/controllers/booth_controller.rs): Kawalan mesin keadaan (FSM), penstriman Live View latar belakang (~30 FPS), gelung kiraan undur, cetusan denyar, pemprosesan komposit, dan pemasa auto-reset.
* [admin_controller.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/controllers/admin_controller.rs): Pengesahan PIN, penyimpanan tetapan, dan pertukaran pantas kamera/pencetak (Mock <-> Hardware) secara masa nyata.
* [print_controller.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/controllers/print_controller.rs): Penghantaran tugasan cetakan dan pengemaskinian status pangkalan data.
* [main.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/main.rs): Inisialisasi Tokio runtime, bootstrap aset lalai, penyambungan MVC, pendaftaran callbacks Slint, dan pelaksanaan gelung peristiwa.

---

### 4. Cabaran Yang Dihadapi & Penyelesaian
1. **Ketiadaan `link.exe` (MSVC toolchain):**
   * *Penyelesaian:* Beralih kepada toolchain `stable-x86_64-pc-windows-gnu` dan memasang MinGW (MSVCRT) melalui winget.
2. **Isu Symlink pada `skia-bindings` (OS Error 1314):**
   * *Penyelesaian:* Menggunakan renderer `renderer-femtovg` dalam Slint yang berasaskan 100% Rust GPU rendering tanpa memerlukan symlink atau C++ build tools pihak ketiga.
3. **Percanggahan UCRT vs MSVCRT (`0xc0000005` Access Violation):**
   * *Penyelesaian:* Menggunakan `BrechtSanders.WinLibs.POSIX.MSVCRT` supaya serasi sepenuhnya dengan pustaka runtime C yang dipautkan oleh Rust GNU.
4. **Sintaks Panjang Slint (`btn_width: 100%` & `length()`):**
   * *Penyelesaian:* Memperbaiki penetapan dimensi kepada unit eksplisit piksel (`380px`) dan menyelaraskan semakan panjang rentetan.

---

### 5. Status Semakan & Pengesahan
* **`cargo check`:** Berjaya tanpa sebarang ralat dan amaran (`Finished dev profile in 7.75s`).
* **`cargo build`:** Berjaya mengkompilasi keseluruhan aplikasi dan menghasilkan fail boleh laksana:
  * Fail binari: `target\debug\photobooth-rs.exe` (Saiz: ~462 MB)

---

## Log Sesi 2: 18 November 2026 (Penubuhan Sistem Rekod Log Sesi)

### 1. Objektif & Permintaan Pengguna
* Mewujudkan rekod log dalam format Markdown (`SESSION_LOG.md`) bagi mendokumentasikan setiap sesi perbualan dan pembangunan supaya sesi seterusnya dapat menyambung kerja dengan lancar.

### 2. Tindakan Yang Diambil
* Mencipta fail [SESSION_LOG.md](file:///c:/Users/it-01995/Downloads/camera-ilhan/SESSION_LOG.md) yang merangkumi:
  1. Ringkasan arkitek & struktur fail MVC.
  2. Log lengkap Sesi 1 dan Sesi 2.
  3. Arahan menjalankan dan menguji aplikasi bagi sesi seterusnya.

---

## Log Sesi 3: 18 November 2026 (Penyediaan Skrip Pelancaran & Menghidupkan Aplikasi)

### 1. Objektif & Permintaan Pengguna
* Pengguna bertindak balas bertanyakan cara untuk menghidupkan / menjalankan aplikasi ("now mcm mana nak hidupkan").

### 2. Tindakan Yang Diambil
1. Menyemak kebergantungan DLL binari `target\debug\photobooth-rs.exe` - disahkan 100% menggunakan DLL sistem Windows natif (`kernel32`, `user32`, `gdi32`, `opengl32`, dsb.) tanpa kebergantungan luar.
2. Mencipta skrip pelancar mudah dua-klik:
   * [run_booth.bat](file:///c:/Users/it-01995/Downloads/camera-ilhan/run_booth.bat): Fail Batch untuk pengguna klik dua kali secara terus dalam Windows File Explorer.
   * [run_booth.ps1](file:///c:/Users/it-01995/Downloads/camera-ilhan/run_booth.ps1): Fail skrip PowerShell dengan pengurusan laluan persekitaran MinGW & Cargo secara automatik.
3. Mengemas kini panduan pengguna untuk menjalankan aplikasi menggunakan cara paling pantas.

---

## Log Sesi 4: 18 November 2026 (Penambahan Ikon Aplikasi Rasmi)

### 1. Objektif & Permintaan Pengguna
* Pengguna meminta untuk menambah ikon aplikasi ("nak icon apps boleh ke").

### 2. Tindakan Yang Diambil
1. **Penjanaan Aset Ikon Moden (Dark Glassmorphism Theme):**
   * Menjana [assets/icons/app_icon.png](file:///c:/Users/it-01995/Downloads/camera-ilhan/assets/icons/app_icon.png) (256x256) dan fail multi-resolusi Windows Icon [assets/icons/app_icon.ico](file:///c:/Users/it-01995/Downloads/camera-ilhan/assets/icons/app_icon.ico) (256, 128, 64, 48, 32, 16px) dengan rekaan kamera fotoboot moden, cincin lensa bergradien `#6366F1` ke `#EC4899`, denyar, dan cetakan foto retro.
2. **Integrasi Slint Window Icon:**
   * Menambah sifat `icon: @image-url("../assets/icons/app_icon.png");` pada `AppWindow` dalam [ui/app.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/app.slint). Ini memaparkan ikon pada bar tajuk tingkap dan bar tugas (*taskbar*) Windows semasa aplikasi berjalan.
3. **Penyematan Ikon Windows Executable (.exe):**
   * Mencipta fail skrip sumber Windows [app.rc](file:///c:/Users/it-01995/Downloads/camera-ilhan/app.rc).
   * Mengemas kini [build.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/build.rs) untuk menyusun sumber dengan alat `windres` ke format COFF (`app_icon.o`) dan memautkannya terus ke dalam `photobooth-rs.exe`.
4. **Pengesahan Kompilasi:**
   * `cargo check` dan `cargo build` disahkan berjaya (0 errors).
   * Seksyen `.rsrc` terbenam dalam fail binari disahkan menggunakan `objdump`.

---

## Log Sesi 5: 18 November 2026 (Pengemasan Fail & Penstrukturan Projek Bersih)

### 1. Objektif & Permintaan Pengguna
* Pengguna meminta fail dikemaskan supaya tidak bersepah ("file kemaskan, jgn bersepah").

### 2. Tindakan Yang Diambil
1. **Penyusunan Fail Sumber Windows:**
   * Memindahkan fail sumber Windows `app.rc` daripada direktori utama ke dalam [assets/icons/app.rc](file:///c:/Users/it-01995/Downloads/camera-ilhan/assets/icons/app.rc) supaya berada bersama-sama aset ikon.
   * Mengemas kini [build.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/build.rs) merujuk ke laluan baharu tersebut.
2. **Pengasingan Skrip Bantuan:**
   * Memindahkan skrip PowerShell ke dalam folder [scripts/run_booth.ps1](file:///c:/Users/it-01995/Downloads/camera-ilhan/scripts/run_booth.ps1).
   * Mengekalkan hanya satu fail pelancar utama yang bersih di direktori root iaitu [run_booth.bat](file:///c:/Users/it-01995/Downloads/camera-ilhan/run_booth.bat) untuk kemudahan 1-klik.
3. **Format Kod Rust Standard (`cargo fmt`):**
   * Menjalankan `cargo fmt` pada keseluruhan kod sumber (`src/`) untuk memastikan gaya penulisan kod seragam, kemas, dan mengikut standard Rust.
4. **Penambahan `.gitignore`:**
   * Mewujudkan fail [.gitignore](file:///c:/Users/it-01995/Downloads/camera-ilhan/.gitignore) menyeluruh untuk mengabaikan artifak binaan (`target/`), pangkalan data tempatan, dan fail sementara sistem.
   * Menambah fail `.gitkeep` dalam subdirektori `output/raw`, `output/composites`, dan `output/mock_prints` supaya struktur folder sentiasa kekal terpelihara.
5. **Pengesahan Binaan:**
   * Menjalankan `cargo check` dan `cargo build` dengan status lulus bersih (0 ralat, 0 amaran).

---

## Log Sesi 6: 18 November 2026 (Kajian Senior Engineer: Benchmark LumaBooth & Open-Source)

### 1. Objektif & Permintaan Pengguna
* Menilai sistem `camera-ilhan` dari sudut pandang jurutera kanan (*Senior Engineer*), membandingkannya dengan perisian komersial terkemuka (**LumaBooth** oleh dslrBooth) serta projek sumber terbuka (**photobooth-app**, **pibooth**, **rpi-photobooth**).
* Mengaudit kelemahan sistem dari segi: Aliran Pengguna (*Flow*), Antaramuka (*UI/UX*), Kefungsian (*Function*), Kualiti Output, dan Ketahanan Pengendalian Ralat (*Error Handling*).

### 2. Tindakan Yang Diambil
1. **Penugasan Pasukan Kejuruteraan Kanan (*Teamwork Orchestration*):**
   * Melancarkan 3 trek siasatan selari (Seni Bina & UI, Perkakasan & Pemprosesan, serta Penanda Aras Pasaran).
   * Menjalankan kitaran semakan bebas 5-agen (*Reviewers, Challengers, Forensic Auditor*) dengan pengesahan 31 baris kod sebenar.
2. **Penghasilan Laporan Induk Menyeluruh:**
   * Menghasilkan laporan teknikal komprehensif [EVALUATION_REPORT.md](file:///c:/Users/it-01995/Downloads/camera-ilhan/EVALUATION_REPORT.md) (872 baris, ~60 KB).
3. **Penemuan Utama (Verdict: Asas Kukuh, Namun Belum Sedia Untuk Acara Nyata Komersial):**
   * *Kelebihan Unik:* Penggunaan RAM amat rendah (<45 MB vs 450–900 MB pada LumaBooth/Electron), rendering 60 FPS GPU natif Slint, seni bina Strict MVC bersih.
   * *6 Kerentanan Kritikal Dikenal Pasti:*
     1. **Thread Starvation pada Tokio:** Panggilan blocking I/O (reqwest blocking, sleep, SumatraPDF) di dalam worker async tanpa `spawn_blocking`.
     2. **Perlumbaan Pemasa Auto-Reset (*Race Condition*):** Pemasa 15 saat di skrin Result mereset sistem ke Idle walaupun tetamu sedang menekan cetak.
     3. **Ketidakpekaan Spooler Pencetak (*Spooler Blindness*):** Tiada telemetri kertas habis, jem, atau pemutus sambungan pencetak DNP.
     4. **Ketidakseimbangan Bingkai (*Head Cutoff Hazard*):** Nisbah aspek Live View tidak memaparkan panduan crop templat, berisiko memotong dahi tetamu pada foto cetak.
     5. **Kebergantungan Luar Tanpa Pengawasan (*Unmanaged Process*):** digiCamControl boleh terhenti tanpa pemulihan automatik, serta risiko menarik foto sesi lepas sekiranya shutter perlahan.
     6. **Kod QR Hantu (*Ghost QR*):** Pautan QR mengarah ke URL tempatan tanpa pelayan web HTTP tempatan sebenar.
4. **Pelan Tindakan Berkeutamaan (Roadmap):**
   * Keutamaan 1 (Kritikal): CancellationToken pada FSM, integrasi Win32 Spooler API, mikropelayan HTTP tempatan (Axum/Actix), pengasingan I/O `spawn_blocking`.
   * Keutamaan 2 (Tinggi): Garisan panduan aspek nisbah pada Live View, pemantau proses kamera (*process watchdog*), suntikan pengepala JFIF 300 DPI.
   * Keutamaan 3 (Sederhana/Ciri Tambahan): Aliran ulangan foto (*retake pose*), penapis LUT warna, sinkronisasi awan.

---

## Log Sesi 7: 18 November 2026 (Pelaksanaan Penuh Semua Pembetulan & Peningkatan Kualiti)

### 1. Objektif & Permintaan Pengguna
* Pengguna mengarahkan untuk melaksanakan kesemua pembaikan secara terus (`/plan aku nak fix semua terus`).
* Pelan pelaksanaan komprehensif telah diluluskan oleh pengguna dalam [fix_all_comprehensive_plan.md](file:///C:/Users/it-01995/.gemini/antigravity/brain/3456a1dd-a298-4860-865a-00c04109835e/fix_all_comprehensive_plan.md).

### 2. Tindakan Yang Diambil
1. **Pencegahan Perlumbaan Pemasa (*FSM Timer Race Condition*):**
   * Mengintegrasikan `CancellationToken` (`tokio-util`) dalam [src/controllers/booth_controller.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/controllers/booth_controller.rs).
   * Pemasa 15 saat di skrin `Result` dibatalkan serta-merta apabila tetamu menekan butang cetak, retake, atau selesai.
2. **Mikropelayan HTTP Tempatan Terbenam (*Embedded Axum Server*):**
   * Membina [src/services/web_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/web_service.rs) menggunakan `axum = "0.7"` pada port `8080`.
   * Mengesan IP tempatan secara dinamik melalui soket UDP.
   * Mengendalikan laman muat turun tetamu mudah alih `/s/:id`, fail imej `/photo/:id`, dan muat turun terus `/download/:id`.
   * Kod QR di skrin kini mengandungi pautan rangkaian sebenar yang boleh diimbas dan dimuat turun terus ke telefon tetamu.
3. **Isyarat Audio Natif Windows (*Audio Cues*):**
   * Membina [src/services/audio_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/audio_service.rs) menggunakan API `kernel32::Beep`.
   * Bunyi bip 880Hz pada saat unduran 3, 2, 1 dan bunyi klik pengatup 1760Hz semasa denyar putih skrin penuh.
4. **Garisan Panduan Bingkai Selamat (*Safe-Zone Framing Guide*):**
   * Mengemas kini [ui/views/capture_view.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/capture_view.slint) dengan kotak panduan potret dan teks *"📸 KEEP FACE INSIDE GUIDE"*, menghapuskan isu dahi terpotong pada cetakan komposit.
5. **Ciri Ulang Ambil Gambar (*Retake Session Flow*):**
   * Menambah butang *"🔄 RETAKE PHOTO"* pada [ui/views/result_view.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/result_view.slint) dan pengikatan callback `retake_session` pada [ui/app.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/app.slint) serta [src/controllers/booth_controller.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/controllers/booth_controller.rs).
6. **Telemetri Windows Print Spooler:**
   * Menambah kaedah `check_status()` pada `PrinterProvider` dalam [src/services/printer_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/printer_service.rs) bagi mengesan ralat *Paper Out*, *Paper Jam*, atau *Offline* melalui WMI sebelum menghantar tugas cetakan.
7. **Perlindungan Foto Lapuk & Kesihatan Kamera:**
   * Merekod cap masa tangkapan dan mengesahkan fail imej daripada `digiCamControl` dalam [src/services/camera_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/camera_service.rs). Menambah probe `is_healthy()`.
8. **Suntikan Pengepala JFIF 300 DPI:**
   * Melaksanakan kaedah `inject_jfif_300dpi()` dalam [src/services/compositor_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/compositor_service.rs) bagi memastikan pemacu pencetak DNP membaca resolusi 300 DPI tepat.

### 3. Pengesahan & Ujian
* Kompilasi `cargo check` & `cargo build` lulus bersih (0 ralat, 0 amaran).
* Ujian pelayan web tempatan HTTP Port 8080 mengembalikan status `200 OK`.
* Laporan semakan terperinci telah didokumentasikan dalam [walkthrough.md](file:///C:/Users/it-01995/.gemini/antigravity/brain/3456a1dd-a298-4860-865a-00c04109835e/walkthrough.md).

---

## Log Sesi 8: 18 November 2026 (Penyediaan Pintasan Desktop Rasmi)

### 1. Objektif & Permintaan Pengguna
* Pengguna bertanyakan lokasi ikon di Desktop untuk ditekan/dilancarkan terus ("mana icon kat dekstop, ku nak tekan").

### 2. Tindakan Yang Diambil
1. **Penjanaan Pintasan Desktop Windows (.lnk):**
   * Menggunakan Windows Script Host (`WScript.Shell`) untuk membina pintasan rasmi di `C:\Users\it-01995\Desktop\Photobooth RS.lnk`.
   * **Target Path:** `c:\Users\it-01995\Downloads\camera-ilhan\target\debug\photobooth-rs.exe`.
   * **Working Directory:** `c:\Users\it-01995\Downloads\camera-ilhan`.
   * **Ikon Pintasan:** Menggunakan fail ikon berdefinisi tinggi [assets/icons/app_icon.ico](file:///c:/Users/it-01995/Downloads/camera-ilhan/assets/icons/app_icon.ico).
2. **Kelebihan Pelancaran Desktop:**
   * Membuka aplikasi Photobooth terus sebagai perisian GUI Windows tanpa membuka tetingkap terminal/Command Prompt hitam.
   * Paparan ikon kamera ungu-merah jambu moden yang jelas di atas skrin Desktop pengguna.

---

## Log Sesi 9: 18 November 2026 (Pembaikan Pepijat Susun Atur Overflow 1800x1200 & Penstrukturan Semula User Journey)

### 1. Masalah Yang Dihadapi & Maklum Balas Pengguna
* Pengguna memuat naik tangkapan skrin menunjukkan paparan skrin keputusan (*ResultView*) yang pecah/terlalu besar: gambar komposit terpotong di sebelah kiri dan atas, pengepala hilang, dan kad Kod QR serta butang tindakan terkeluar dari sempadan skrin ("kenapa mcm ni? tak kemas.. user flow journey mcm tak clear.. tolong check").
* **Punca Masalah (Root Cause Analysis):**
  * Fail imej komposit mempunyai resolusi asal 1800x1200 piksel.
  * Di dalam enjin Slint, elemen `Image` yang tidak dihadkan sempadannya akan meminta ruang saiz asal 1800px.
  * Tambahan pula, komponen-komponen paparan di dalam `ui/app.slint` tidak mempunyai kekangan `width: 100%; height: 100%`, menyebabkan saiz tetingkap mengembang melebihi 2300px lebar dan melimpah (*overflow*) keluar dari skrin paparan.
  * Dari segi *User Flow Journey*, pengguna tidak mempunyai panduan langkah yang jelas dari mula hingga tamat sesi.

### 2. Tindakan Pembetulan & Penambahbaikan Yang Diambil
1. **Penyelesaian Had Dimensi Bounded Responsif (Slint):**
   * Menetapkan `width: 100%; height: 100%;` secara eksplisit pada setiap instans paparan (`IdleView`, `CaptureView`, `ProcessingView`, `ResultView`, `AdminModal`) dalam [ui/app.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/app.slint).
   * Di dalam [ui/views/result_view.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/result_view.slint), ruang paparan foto sebelah kiri dihadkan dengan `horizontal-stretch: 6` dan `Image` dihadkan saiznya agar sentiasa berada di dalam kotak bingkai (`image-fit: contain`).
   * Panel tindakan di sebelah kanan dihadkan dengan saiz tetap `min-width: 360px; max-width: 440px;` supaya butang cetak, retake, dan kad Kod QR tidak lagi terpotong pada mana-mana resolusi skrin (1080p, 720p, dsb.).
2. **Penstrukturan Semula Aliran Pengguna (*Clear User Flow Journey*):**
   * **Skrin Idle ([ui/views/idle_view.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/idle_view.slint)):**
     * Menambah lencana ciri yang jelas sebelum bermula: `[ 📷 3 Pose Kiosk ] [ 🖨️ Cetakan 300 DPI ] [ 📲 Imbas QR Ke Telefon ]`.
   * **Skrin Capture ([ui/views/capture_view.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/capture_view.slint)):**
     * Menambah *Pose Progress Tracker* di bahagian bawah: `[ ✅ Pose 1 Selesai ] [ 📸 Pose 2 (Aktif) ] [ ⏳ Pose 3 ]`. Tetamu kini faham dengan jelas fasa tangkapan yang sedang berlangsung.
   * **Skrin Result ([ui/views/result_view.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/result_view.slint)):**
     * Menambah penunjuk langkah (*3-Step Breadcrumbs*): `1. SEMAK FOTO ➔ 2. IMBAS QR TELEFON ➔ 3. CETAK / SELESAI`.
     * Mengasingkan fungsi kepada dua fasa jelas:
       * **Langkah 1 (Digital):** Kad Kod QR putih yang kemas untuk simpan terus ke telefon.
       * **Langkah 2 (Fizikal & Pilihan):** Butang utama `🖨️ CETAK FOTO (PRINT 1 COPY)`, bersebelahan butang `🔄 AMBIL SEMULA` dan `✨ SELESAI`.

### 3. Pengesahan
* Kompilasi `cargo check` dan `cargo build` lulus bersih (0 ralat, 0 amaran).
* Ujian proses dijalankan dan disahkan tetingkap responsif tanpa sebarang limpahan imej (*zero overflow*).

---

## Log Sesi 10: 18 November 2026 (Penukaran Subsistem Ke Pure Windows GUI Tanpa CMD)

### 1. Masalah Yang Dihadapi & Permintaan Pengguna
* Pengguna bertanyakan mengapa semasa melancarkan aplikasi, tetingkap Command Prompt (CMD) hitam turut terbuka bersama tetingkap GUI ("kenapa bila run app.. cmd terbukak sekali?").
* **Punca Masalah:**
  * Secara lalai dalam Rust pada platform Windows, binari dikompilasi dengan subsistem konsol (`/SUBSYSTEM:CONSOLE` / `IMAGE_SUBSYSTEM_WINDOWS_CUI`). Windows secara automatik memperuntukkan tetingkap konsol hitam untuk sebarang proses CUI.

### 2. Tindakan Yang Diambil
1. **Penetapan Subsistem GUI Windows Tulen:**
   * Menambah atribut `#![windows_subsystem = "windows"]` di baris paling atas [src/main.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/main.rs).
   * Ini mengarahkan penyusun (*linker*) untuk menghasilkan binari dengan subsistem `IMAGE_SUBSYSTEM_WINDOWS_GUI` (Subsystem `00000002`).
2. **Kemas Kini Skrip Pelancar Batch:**
   * Menambah arahan `exit` pada [run_booth.bat](file:///c:/Users/it-01995/Downloads/camera-ilhan/run_booth.bat) sejurus selepas melancarkan proses agar tetingkap batch tertutup secara serta-merta sekiranya pengguna membukanya melalui fail batch.
3. **Pengesahan PE Header & Pelancaran:**
   * `objdump -p` mengesahkan `Subsystem: 00000002 (Windows GUI)`.
   * Ujian pelancaran mengesahkan aplikasi berjalan bersih tanpa sebarang tetingkap hitam konsol di latar belakang.

---

## Log Sesi 11: 18 November 2026 (Penyepaduan Kamera Laptop Natif, Pemilihan Layout Interaktif & Penjelasan Sistem Cetakan)

### 1. Objektif & Permintaan Pengguna
1. **"nak test camera laptop dulu boleh?"**: Mengaktifkan sokongan kamera web komputer riba (`USB2.0 HD UVC WebCam`) agar pengguna boleh menguji aplikasi secara langsung dengan kamera sendiri tanpa memerlukan kamera DSLR Canon 700D.
2. **"pilih layout"**: Membolehkan pengguna / tetamu memilih reka letak (*layout*) templat foto secara interaktif terus dari skrin Idle sebelum sesi bermula.
3. **"ada print kan?"**: Penjelasan terperinci dan pengesahan fungsi cetakan foto fizikal (DNP RX1-HS / pencetak Windows) dan simulasi ujian.

### 2. Tindakan Yang Diambil
1. **Penyepaduan Kamera Laptop Natif (`LaptopWebcamCamera`):**
   * Menambah pustaka `nokhwa = { version = "0.10.11", features = ["input-native"] }` dalam [Cargo.toml](file:///c:/Users/it-01995/Downloads/camera-ilhan/Cargo.toml).
   * Membina `LaptopWebcamCamera` dalam [src/services/camera_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/camera_service.rs) yang mengimplementasikan trait `CameraProvider`.
   * Melaksanakan fungsi penstriman Live View dengan cermin pantulan mendatar (*horizontal selfie flip*) menggunakan `imageops::flip_horizontal` supaya pengalaman Live View terasa semula jadi seperti cermin / skrin photobooth sebenar.
   * Melaksanakan tangkapan foto resolusi penuh bagi setiap pose dan menyimpannya terus ke fail sasaran.
   * Menambah *fallback* anggun (*graceful fallback*) ke `MockCamera` sekiranya peranti kamera sedang disekat atau sibuk.
   * Menghubungkan `LaptopWebcamCamera` sebagai mod kamera lalai apabila `hardware_mode` dimatikan dalam [src/main.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/main.rs) dan [src/controllers/admin_controller.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/controllers/admin_controller.rs).
2. **Pemilihan Layout Interaktif Di Skrin Utama ([ui/views/idle_view.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/idle_view.slint)):**
   * Mereka bentuk 2 kad pilihan templat visual yang responsif:
     * **Kad 1: 🖼️ 4R Postcard** (1800x1200 • 3 Pose Klasik)
     * **Kad 2: 🎞️ 2x6 Strip Cut** (Potong Dua • 3 Pose Gaya Korea/Luna)
   * Dilengkapi penunjuk aktif berwarna biru-ungu, lencana status *"TERPILIH"*, dan animasi sentuhan interaktif.
   * Menghubungkan pemilihan ini kepada `active_template_id` dalam [ui/app.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/app.slint) dan pengawal logik aplikasi.
3. **Pengesahan & Seni Bina Cetakan (Printing Architecture):**
   * **Mod Fizikal (DNP RX1-HS / Hot-Folder / Windows Spooler):** Menghantar cetakan komposit 300 DPI terus ke pencetak pewarna sublimasi DNP RX1-HS melalui PowerShell `PrintTo` verb atau pemacu Windows. Menyokong ciri *auto 2-inch cut* untuk menghasilkan dua keping strip 2x6 daripada kertas 4x6 inci.
   * **Mod Ujian / Simulasi:** Apabila butang *"PRINT PHOTO"* ditekan tanpa perkakasan DNP, sistem menyimpan fail komposit cetakan resolusi tinggi 300 DPI ke `output/mock_prints/`, mencatat status `printed = 1` ke pangkalan data SQLite `booth_sessions.db`, dan memaparkan dialog proses kepada pengguna.

### 3. Pengesahan & Ujian
* Kompilasi `cargo check` dan `cargo build` berjaya sepenuhnya (`Finished dev profile in 22.87s`).
* Isu ordinal DLL nokhwa pada GNU ld diselesaikan dengan menetapkan jenis peti `rlib`.
* Binari `target\debug\photobooth-rs.exe` (493 MB) berjaya dilancarkan dan disahkan berjalan lancar tanpa ralat.

---

## Log Sesi 12: 18 November 2026 (Penyediaan Pakej Edaran Mudah Alih Tanpa Codebase / Portable Distribution Package)

### 1. Objektif & Permintaan Pengguna
* Pengguna bertanyakan cara untuk mengedarkan atau memberikan aplikasi kepada orang lain / kru / pelanggan untuk digunakan secara terus tanpa memerlukan pangkalan kod (*codebase*), perisian pengaturcaraan, Rust, atau terminal ("so kalau nak bagi org guna tanpa code base mcm mana?").

### 2. Tindakan Yang Diambil
1. **Analisis Kebergantungan DLL & Subsystem Windows:**
   * Melakukan semakan `objdump -p` terhadap senarai fail DLL yang dipautkan.
   * Disahkan 100% fail DLL adalah pustaka teras sistem Windows (`kernel32.dll`, `user32.dll`, `gdi32.dll`, `msvcrt.dll`, `mfplat.dll`, dll.) yang telah sedia ada dalam mana-mana komputer Windows 10 & Windows 11.
   * `libgcc` dan `libwinpthread` dipaut secara statik, menjadikannya fail boleh laksana mandiri (*self-contained standalone executable*).
2. **Kompilasi Pengeluaran (*Production Release Optimization*):**
   * Mengkonfigurasi profil `[profile.release]` dalam [Cargo.toml](file:///c:/Users/it-01995/Downloads/camera-ilhan/Cargo.toml) dengan `opt-level = 3`, `codegen-units = 1`, `panic = "abort"`, dan pembersihan simbol penyahpepijat (`strip = true`).
   * Menjana binari pengeluaran di [target/release/photobooth-rs.exe](file:///c:/Users/it-01995/Downloads/camera-ilhan/target/release/photobooth-rs.exe).
   * **Pengurangan Saiz Drastik:** Saiz fail berkurang daripada **493 MB** (mod debug) kepada hanya **23.9 MB** (mod release)!
3. **Penyediaan Pakej Edaran Mudah Alih (*Portable Package*):**
   * Membina direktori pakej edaran di [dist/Photobooth-RS-Portable/](file:///c:/Users/it-01995/Downloads/camera-ilhan/dist/Photobooth-RS-Portable/):
     * `Photobooth-RS.exe` (Fail boleh laksana pengeluaran 23.9 MB).
     * `assets/templates/` (Mengandungi fail `default_4r_overlay.png`, `default_strip_overlay.png`, dan `templates.json` untuk memudahkan penukaran tema bingkai foto).
     * `assets/icons/app_icon.ico` (Ikon rasmi aplikasi).
     * `config.json` (Konfigurasi kiosk, PIN lalai `1234`, masa unduran).
     * `output/` (Struktur folder siap sedia untuk `raw/`, `composites/`, dan `mock_prints/`).
     * `PANDUAN_PENGGUNAAN.txt` (Manual panduan penggunaan ringkas untuk kru/pelanggan dalam Bahasa Melayu).
4. **Pemampatan Fail Arkib ZIP:**
   * Memampatkan pakej kepada satu fail tunggal: [dist/Photobooth-RS-Portable.zip](file:///c:/Users/it-01995/Downloads/camera-ilhan/dist/Photobooth-RS-Portable.zip) (Saiz: **11.3 MB**).
   * Boleh dihantar terus melalui WhatsApp, Telegram, Google Drive, atau disalin ke pendrive USB.

### 3. Pengesahan & Ujian
* Menjalankan ujian pelaksanaan terasing (*isolated execution test*) terus dari folder `dist/Photobooth-RS-Portable/Photobooth-RS.exe`.
* Aplikasi disahkan dilancarkan dengan sempurna, pantas, tanpa membuka tetingkap CMD, dan berjaya mengesan kamera serta memulakan pelayan web QR.

---

## Log Sesi 13: 19 November 2026 (Pelaksanaan Penuh Sistem Kemas Kini Automatik Atas Talian / In-App Auto-Update)

### 1. Objektif & Permintaan Pengguna
* Pengguna memilih Pilihan 3 (Sistem Auto-Update melalui internet): *"contoh aku bagi app ni kat kawan aku.. kalau aku update kat laptop aku punya codebase.. mcm mana nak buat kawan aku tu uptodate juga?"*.
* Perancangan seni bina komprehensif dirangka dalam [auto_update_system_plan.md](file:///C:/Users/it-01995/.gemini/antigravity/brain/3456a1dd-a298-4860-865a-00c04109835e/auto_update_system_plan.md) dan telah diluluskan oleh pengguna.

### 2. Tindakan Yang Diambil
1. **Perkhidmatan Kemas Kini [src/services/updater_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/updater_service.rs):**
   * Struktur data `VersionInfo` (versi, nota pelepasan, URL muat turun, SHA-256, cap masa).
   * Kaedah `is_newer_version`: Membandingkan versi semantik (cth: `0.2.0` > `0.1.0`).
   * Kaedah `check_for_updates`: Panggilan tak segerak (*async HTTP GET*) dengan had masa pendek (3s) agar kiosk tidak membeku (*hang*) sekiranya tiada internet.
   * Kaedah `download_and_install`: Muat turun aliran bait ke `Photobooth-RS.exe.new` dengan panggilan balik kemajuan (0% hingga 100%).
   * Kaedah `execute_windows_restart`: Menjana skrip `update_restart.bat` yang dilancarkan secara terpisah (*detached process*) bagi menunggu proses lama tamat, menggantikan `.exe` lama dengan yang baharu, memulakan semula aplikasi secara automatik, dan memadam skrip sementara.
2. **Model Konfigurasi [src/models/config_model.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/models/config_model.rs):**
   * Menambah medan `app_version` (lalai `"0.1.0"`), `update_check_url` (URL `version.json`), dan `auto_check_update` (lalai `true`) dengan atribut `#[serde(default)]` untuk keserasian ke belakang (*backward compatibility*).
3. **Antaramuka Pengguna Slint ([ui/views/admin_modal.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/views/admin_modal.slint) & [ui/app.slint](file:///c:/Users/it-01995/Downloads/camera-ilhan/ui/app.slint)):**
   * Menambah kad **"Software Version & Updates"** dalam panel Admin (PIN 1234).
   * Butang interaktif: *"🔄 Check Updates"* untuk menyemak secara manual.
   * Paparan lencana status: *"✨ New version ready to install!"*.
   * Butang tindakan 1-Klik: *"🚀 UPDATE & RESTART"* dengan paparan peratusan muat turun secara langsung.
4. **Pengawal Logik ([src/controllers/admin_controller.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/controllers/admin_controller.rs) & [src/main.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/main.rs)):**
   * Menyambungkan fungsi semakan latar belakang senyap 2 saat selepas kiosk dibuka.
   * Mengendalikan muat turun tak segerak dan pemantauan bar kemajuan dalam Slint event loop.
5. **Skrip Penerbitan 1-Klik Pembangun [publish_release.ps1](file:///c:/Users/it-01995/Downloads/camera-ilhan/publish_release.ps1):**
   * Membaca versi dari `Cargo.toml`.
   * Mengkompilasi binaan Release secara automatik.
   * Mengira kod cincangan integriti SHA-256.
   * Mengemas kini folder [dist/Photobooth-RS-Portable/](file:///c:/Users/it-01995/Downloads/camera-ilhan/dist/Photobooth-RS-Portable/) dan arkib [dist/Photobooth-RS-Portable.zip](file:///c:/Users/it-01995/Downloads/camera-ilhan/dist/Photobooth-RS-Portable.zip) (hanya 10.86 MB).
   * Menjana fail metadata [dist/version.json](file:///c:/Users/it-01995/Downloads/camera-ilhan/dist/version.json).

### 3. Pengesahan & Ujian
* Kompilasi `cargo check` & `cargo build --release` berjaya (0 ralat, 0 amaran).
* Ujian pelaksanaan skrip `publish_release.ps1` disahkan berjaya menjana pakej binari release (22.9 MB) dan `version.json`.
* Ujian pelancaran terasing binari mudah alih disahkan berjalan lancar (PID 2352) tanpa sebarang ralat.
* Laporan semakan lengkap didokumentasikan dalam [walkthrough.md](file:///C:/Users/it-01995/.gemini/antigravity/brain/3456a1dd-a298-4860-865a-00c04109835e/walkthrough.md#L60-L75).

---

## Log Sesi 14: 21 November 2026 (Inisialisasi Git Tempatan & Penyediaan Commit Awal)

### 1. Objektif & Permintaan Pengguna
* Pengguna meminta untuk menolak (*push*) keseluruhan kod projek ke GitHub ("tolong push code kat sini boleh?").

### 2. Tindakan Yang Diambil
1. **Inisialisasi Git Tempatan:**
   * Menjalankan `git init` dalam direktori projek `c:\Users\it-01995\Downloads\camera-ilhan`.
   * Menetapkan cawangan lalai kepada `main` (`git branch -M main`).
2. **Pengemaskinian `.gitignore`:**
   * Memastikan direktori `dist/` dikecualikan daripada kawalan versi git supaya fail binari `.exe` (23 MB) dan arkib `.zip` tidak membebankan saiz repositori kod sumber.
3. **Penyediaan Initial Commit:**
   * Mementaskan 49 fail teras projek (kod sumber Rust, antaramuka Slint UI, templat bingkai, aset ikon, ujian unit, dan skrip pelancar).
   * Menghasilkan komit awal yang bersih: `feat: complete Photobooth RS kiosk with Slint UI, laptop webcam, layout selector & auto-updater` (Hash: `0bba27c`).
4. **Penyambungan Repositori GitHub & Tolakan (*Push*) Berjaya:**
   * Remote ditambahkan: `https://github.com/ImranNaufal/photobooth.git`.
   * Cawangan `main` berjaya ditolak (*pushed*) ke GitHub dengan pengesahan Windows Git Credential Manager.
   * Pautan semakan versi auto-update diselaraskan ke `https://raw.githubusercontent.com/ImranNaufal/photobooth/main/dist/version.json`.
   * Fail `dist/version.json` dijejak dalam Git supaya kiosk rakan/klien sentiasa dapat menyemak versi terbaharu secara terus dari repositori.

---

## Log Sesi 15: 21 November 2026 (Pelaksanaan Sistem Log Jarak Jauh / Remote Diagnostics & Web Dashboard /logs)

### 1. Objektif & Permintaan Pengguna
* Pengguna bertanyakan cara untuk menyemak log dari jauh, mengubah kod dan terus mengemaskini di komputer kawan, serta memudahkan proses nyahpepijat (*debugging*) dan kawalan jarak jauh tanpa membuka terminal ("mcm mana kau nak check log? ubah code terus update kat dia? ada cara tak?").

### 2. Tindakan Yang Diambil
1. **Perkhidmatan Log Setempat Berterusan (`LoggerService`):**
   * Membina [src/services/logger_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/logger_service.rs) yang menulis setiap peristiwa `tracing` ke fail fizikal [logs/photobooth.log](file:///c:/Users/it-01995/Downloads/camera-ilhan/dist/Photobooth-RS-Portable/logs/photobooth.log).
   * Melaksanakan penimbal memori (*in-memory ring buffer*) 300 baris terkini secara segerak dan selamat (*thread-safe*).
2. **Papan Pemuka Web Jarak Jauh (*Remote Web Diagnostic Console*):**
   * Mengintegrasikan laluan baharu dalam [src/services/web_service.rs](file:///c:/Users/it-01995/Downloads/camera-ilhan/src/services/web_service.rs):
     * `/logs`: Paparan web tema gelap konsol langsung dengan auto-refresh setiap 3 saat, pengesanan warna ralat (Merah untuk ERROR, Kuning untuk WARN, Biru untuk INFO), dan butang muat turun.
     * `/logs/raw`: Muat turun fail teks asal `photobooth.log`.
     * `/api/logs`: Titik akhir JSON untuk integrasi telemetri.
3. **Pemberitahuan Amaran Webhook:**
   * Melaksanakan fungsi `send_webhook_alert` untuk menyokong penghantaran amaran ralat kritikal ke Discord Webhook atau Telegram Bot.
4. **Penerbitan Binaan Release Terkini & Tolakan Git:**
   * Menjana binari pengeluaran terbaharu (23 MB) dan arkib [dist/Photobooth-RS-Portable.zip](file:///c:/Users/it-01995/Downloads/camera-ilhan/dist/Photobooth-RS-Portable.zip) (10.86 MB).
   * Berjaya menolak komit `32fbb44` ke repositori rasmi GitHub `ImranNaufal/photobooth`.

### 3. Pengesahan & Ujian
* Ujian pelaksanaan binari menghasilkan fail `logs/photobooth.log` secara serta-merta dengan cap masa ISO tepat.
* Papan pemuka web `http://<IP>:8080/logs` disahkan menyajikan aliran log masa nyata.

---

## Arahan Untuk Sesi Seterusnya (Next Session Guide)

### Cara Menjalankan Aplikasi
Terdapat beberapa cara mudah untuk menghidupkan aplikasi:

#### Cara 1: Pintasan Desktop (Paling Pantas & Mudah)
* Pergi ke skrin **Desktop** Windows anda.
* Klik dua kali pada ikon **Photobooth RS**. Aplikasi akan terus dibuka.

#### Cara 2: Klik Dua Kali Fail Batch
* Buka folder `c:\Users\it-01995\Downloads\camera-ilhan` dalam Windows Explorer.
* Klik dua kali pada fail `run_booth.bat`.

#### Cara 2: Melalui Skrip PowerShell
Buka terminal PowerShell dalam folder ini dan jalankan:
```powershell
.\run_booth.ps1
```

#### Cara 3: Melalui Terminal Manual
```powershell
$msvcrtBin = "C:\Users\it-01995\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
$env:Path = "$msvcrtBin;$HOME\.cargo\bin;$env:Path"

# Jalankan terus fail .exe yang telah dikompilasi
.\target\debug\photobooth-rs.exe

# Atau melalui cargo
cargo run
```

### Kawalan Ujian Dalam Aplikasi
* **Skrin Utama (Idle):**
  * Klik "TOUCH TO START" untuk memulakan sesi foto tangkapan (Pose 1 hingga 3).
  * Ketuk 5 kali pantas pada bucu kanan atas skrin untuk membuka modal **Admin**.
* **Mod Admin:**
  * PIN lalai: `1234`.
  * Boleh tukar antara **Mock Mode** dan **Production Hardware Mode**.
  * Boleh tukar templat antara **4R Postcard (3 Poses)** dan **2x6 Photo Strip (3 Poses)**.
* **Output Fail:**
  * Foto mentah disimpan di `output/raw/`.
  * Foto komposit akhir 300 DPI disimpan di `output/composites/`.
  * Salinan cetakan ujian disimpan di `output/mock_prints/`.
  * Rekod data sesi disimpan dalam `booth_sessions.db`.
