#[cfg(windows)]
extern "system" {
    fn Beep(dwFreq: u32, dwDuration: u32) -> i32;
}

pub struct AudioService;

impl AudioService {
    /// Plays an audio cue for countdown ticks (3, 2, 1)
    pub fn play_countdown_beep() {
        std::thread::spawn(|| {
            #[cfg(windows)]
            unsafe {
                // 880 Hz (A5 pitch) for 150 milliseconds
                Beep(880, 150);
            }
        });
    }

    /// Plays an audio cue when camera shutter flashes
    pub fn play_shutter_click() {
        std::thread::spawn(|| {
            #[cfg(windows)]
            unsafe {
                // 1760 Hz (A6 pitch) for 220 milliseconds
                Beep(1760, 220);
            }
        });
    }
}
