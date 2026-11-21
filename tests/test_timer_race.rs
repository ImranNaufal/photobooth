use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;

#[derive(Clone)]
struct ActiveSessionData {
    session_id: String,
}

#[tokio::test]
async fn test_auto_return_timer_race_condition() {
    let active_session = Arc::new(TokioMutex::new(None));
    let is_session_running = Arc::new(AtomicBool::new(false));

    // --- Step 1: Session 1 starts and completes ---
    let session_1_id = "session_1".to_string();
    is_session_running.store(true, Ordering::SeqCst);
    {
        let mut act = active_session.lock().await;
        *act = Some(ActiveSessionData {
            session_id: session_1_id.clone(),
        });
    }

    // Session 1 spawns auto-return timer (from booth_controller.rs:360-397)
    let window_timer_session_id = session_1_id.clone();
    let active_session_timer = active_session.clone();
    let is_running_timer = is_session_running.clone();

    tokio::spawn(async move {
        // Simulating the 15s timer loop with 100ms intervals for the test
        for _sec in (0..=15).rev() {
            tokio::time::sleep(Duration::from_millis(100)).await;

            let active_id = {
                let act = active_session_timer.lock().await;
                act.as_ref().map(|a| a.session_id.clone())
            };
            if active_id.as_deref() != Some(&window_timer_session_id) {
                break;
            }
        }

        // Line 396 from booth_controller.rs:
        is_running_timer.store(false, Ordering::SeqCst);
    });

    // --- Step 2: At t = 50ms, Guest 1 taps DONE (handle_finish_session: lines 474-480) ---
    tokio::time::sleep(Duration::from_millis(50)).await;
    {
        let mut act = active_session.lock().await;
        *act = None;
    }
    is_session_running.store(false, Ordering::SeqCst);

    // --- Step 3: At t = 60ms, Guest 2 taps START (handle_start_session: lines 118-135) ---
    assert_eq!(
        is_session_running.load(Ordering::SeqCst),
        false,
        "Session 2 should be able to start"
    );
    is_session_running.store(true, Ordering::SeqCst);
    let session_2_id = "session_2".to_string();
    {
        let mut act = active_session.lock().await;
        *act = Some(ActiveSessionData {
            session_id: session_2_id.clone(),
        });
    }

    // Verify Session 2 is actively running
    assert_eq!(
        is_session_running.load(Ordering::SeqCst),
        true,
        "Session 2 is running"
    );

    // --- Step 4: Wait for Timer 1 to wake up and exit loop (at t = 100ms) ---
    tokio::time::sleep(Duration::from_millis(120)).await;

    // --- Step 5: Check if Timer 1 clobbered Session 2's running flag ---
    let flag_value = is_session_running.load(Ordering::SeqCst);
    println!(
        "is_session_running value after Timer 1 exit: {}",
        flag_value
    );

    // If the bug exists, Timer 1 executed `is_running_timer.store(false)` and clobbered Session 2's flag!
    assert_eq!(
        flag_value, false,
        "Empirical proof: Timer 1 erroneously cleared is_session_running while Session 2 was active!"
    );
}
