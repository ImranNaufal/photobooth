fn main() {
    slint_build::compile("ui/app.slint").expect("Failed to compile Slint UI");

    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        println!("cargo:rerun-if-changed=assets/icons/app.rc");
        println!("cargo:rerun-if-changed=assets/icons/app_icon.ico");

        let ico_path = std::path::Path::new("assets/icons/app_icon.ico");
        let rc_path = std::path::Path::new("assets/icons/app.rc");

        if ico_path.exists() && rc_path.exists() {
            let out_dir = std::env::var("OUT_DIR").unwrap();
            let res_path = format!("{}/app_icon.o", out_dir);

            let status = std::process::Command::new("windres")
                .args(["assets/icons/app.rc", "-O", "coff", "-o", &res_path])
                .status();

            if let Ok(s) = status {
                if s.success() {
                    println!("cargo:rustc-link-arg={}", res_path);
                }
            }
        }
    }
}
