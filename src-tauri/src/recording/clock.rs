use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 録画中の全スレッドが共有する時計。
///
/// フレーム・入力イベント・UI イベント・ウィンドウ矩形のタイムスタンプを
/// すべてこの時計で付けることで、スレッドごとの開始時刻のずれをなくす。
/// 一時停止中は時刻が進まないため、タイムスタンプは「一時停止を除いた
/// 実効録画時間」になり、一時停止をまたいでも音声（一時停止中は記録しない）と揃う。
pub struct RecordingClock {
    start: Instant,
    pause: Mutex<PauseState>,
}

#[derive(Default)]
struct PauseState {
    /// 確定済みの一時停止時間の合計
    total: Duration,
    /// 一時停止中ならその開始時刻
    since: Option<Instant>,
}

impl RecordingClock {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            pause: Mutex::new(PauseState::default()),
        }
    }

    /// 録画開始からの実効経過時間 (ms)。一時停止中は停止した時点の値を返す。
    pub fn now_ms(&self) -> u64 {
        let now = Instant::now();
        let paused = self
            .pause
            .lock()
            .map(|p| p.total + p.since.map(|s| now - s).unwrap_or_default())
            .unwrap_or_default();
        (now - self.start).saturating_sub(paused).as_millis() as u64
    }

    pub fn pause(&self) {
        if let Ok(mut p) = self.pause.lock() {
            if p.since.is_none() {
                p.since = Some(Instant::now());
            }
        }
    }

    pub fn resume(&self) {
        if let Ok(mut p) = self.pause.lock() {
            if let Some(since) = p.since.take() {
                p.total += since.elapsed();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_stops_while_paused() {
        let clock = RecordingClock::new();
        clock.pause();
        let t0 = clock.now_ms();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(clock.now_ms(), t0, "一時停止中は時刻が進まない");

        clock.resume();
        std::thread::sleep(Duration::from_millis(20));
        let t1 = clock.now_ms();
        assert!(t1 >= t0 + 15, "再開後は時刻が進む: t0={} t1={}", t0, t1);
        // 一時停止時間 (50ms) が含まれていれば t0 + 70 以上になる
        assert!(t1 < t0 + 60, "一時停止時間は含まれない: t0={} t1={}", t0, t1);
    }
}
