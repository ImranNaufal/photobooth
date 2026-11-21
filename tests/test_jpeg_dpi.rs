use image::{ImageBuffer, Rgb};
use std::fs;

#[test]
fn test_default_jpeg_dpi_headers() {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(100, 100);
    let temp_path = std::env::temp_dir().join("test_dpi.jpg");
    img.save(&temp_path).unwrap();

    let bytes = fs::read(&temp_path).unwrap();
    let _ = fs::remove_file(temp_path);

    // JPEG SOI marker is 0xFF, 0xD8
    assert_eq!(bytes[0], 0xFF);
    assert_eq!(bytes[1], 0xD8);

    // If JFIF APP0 is present, bytes[2] and bytes[3] should be 0xFF, 0xE0, and bytes[6..10] should be b"JFIF"
    let mut has_jfif_300_dpi = false;
    if bytes.len() > 18 && bytes[2] == 0xFF && bytes[3] == 0xE0 {
        if &bytes[6..10] == b"JFIF" {
            let units = bytes[13];
            let x_density = u16::from_be_bytes([bytes[14], bytes[15]]);
            let y_density = u16::from_be_bytes([bytes[16], bytes[17]]);
            println!(
                "JFIF found: units={}, x_density={}, y_density={}",
                units, x_density, y_density
            );
            if units == 1 && x_density == 300 && y_density == 300 {
                has_jfif_300_dpi = true;
            }
        }
    }

    println!("has_jfif_300_dpi: {}", has_jfif_300_dpi);
    assert!(
        !has_jfif_300_dpi,
        "Confirmed: Default image::save creates baseline JPEG without 300 DPI JFIF density tags!"
    );
}
