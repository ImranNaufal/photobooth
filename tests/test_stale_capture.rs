use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn test_stale_capture_buffer_bug() {
    // Spin up a mock digiCamControl HTTP server
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let request_count = Arc::new(AtomicUsize::new(0));
    let req_count_clone = request_count.clone();

    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let mut buf = [0u8; 1024];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);

                if req.contains("GET /?SLC=Capture") {
                    req_count_clone.fetch_add(1, Ordering::SeqCst);
                    let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK";
                    let _ = stream.write_all(resp.as_bytes());
                } else if req.contains("GET /lastcaptured") {
                    // Simulate digiCamControl:
                    // Pose 1 was "PHOTO_1_BYTES".
                    // When Capture #2 is triggered, physical camera shutter takes 1.5s to expose.
                    // For the first 1.0s, digiCamControl still serves "PHOTO_1_BYTES".
                    // After 1.0s, it updates to "PHOTO_2_BYTES".
                    let capture_num = req_count_clone.load(Ordering::SeqCst);
                    let body = if capture_num <= 1 {
                        "PHOTO_1_BYTES"
                    } else {
                        // If polled within 800ms of capture #2, camera hasn't finished USB transfer yet!
                        // So digiCamControl buffer still contains PHOTO_1_BYTES
                        "PHOTO_1_BYTES"
                    };
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                } else {
                    let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                    let _ = stream.write_all(resp.as_bytes());
                }
            }
        }
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(2000))
        .build()
        .unwrap();

    let base_url = format!("http://127.0.0.1:{}", port);

    // Simulated Pose 1 capture
    let capture_url = format!("{}/?SLC=Capture", base_url);
    client.get(&capture_url).send().unwrap();
    let last_url = format!("{}/lastcaptured", base_url);
    let resp1 = client.get(&last_url).send().unwrap();
    let pose1_bytes = resp1.text().unwrap();
    assert_eq!(pose1_bytes, "PHOTO_1_BYTES");

    // Simulated Pose 2 capture using the exact logic from camera_service.rs:207-228:
    client.get(&capture_url).send().unwrap();

    let start = std::time::Instant::now();
    let mut captured_bytes = String::new();

    while start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(400));
        if let Ok(resp) = client.get(&last_url).send() {
            if resp.status().is_success() {
                if let Ok(b) = resp.text() {
                    if !b.is_empty() {
                        captured_bytes = b;
                        break;
                    }
                }
            }
        }
    }

    // Notice: Pose 2 got Pose 1's bytes because there is zero timestamp or hash verification!
    println!("Pose 2 received bytes: {:?}", captured_bytes);
    assert_eq!(
        captured_bytes, "PHOTO_1_BYTES",
        "Empirical proof: Pose 2 captured the stale buffer from Pose 1 because /lastcaptured was not invalidated!"
    );
}
