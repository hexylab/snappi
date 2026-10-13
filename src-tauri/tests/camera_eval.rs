//! testdata/recordings/* の全シナリオに M1〜M7 と expect.json を適用する統合テスト (Issue #28)。

use snappi_lib::eval::{self, EVAL_FPS};
use std::path::PathBuf;

fn recording_dirs() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../testdata/recordings");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{} を読めません: {}", root.display(), e))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("meta.json").exists())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "評価用の録画がありません: {}", root.display());
    dirs
}

/// すべての評価用録画が読み込め、カメラトラック・メトリクス・expect.json を計算できること
#[test]
fn all_recordings_can_be_evaluated() {
    for dir in recording_dirs() {
        let (data, report) = eval::evaluate_dir(&dir)
            .unwrap_or_else(|e| panic!("{} の評価に失敗: {:#}", dir.display(), e));
        assert_eq!(report.track.fps, EVAL_FPS);
        assert_eq!(
            report.track.frames.len(),
            data.output_frame_times(EVAL_FPS).len(),
            "{}: カメラトラックの長さが出力フレーム数と一致しない",
            dir.display()
        );
        assert!(dir.join("expect.json").exists(), "{} に expect.json がありません", dir.display());
        assert!(!report.check_results.is_empty(), "{} の expect.json にチェックがありません", dir.display());
    }
}

/// すべての評価用録画がメトリクス基準と expect.json を満たすこと
#[test]
fn all_recordings_meet_camera_spec() {
    let mut failures = Vec::new();
    for dir in recording_dirs() {
        let (_, report) = eval::evaluate_dir(&dir).unwrap();
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        for r in report.metric_results.iter().filter(|r| !r.passed) {
            failures.push(format!("{}: {}", name, r));
        }
        for c in report.check_results.iter().filter(|c| !c.passed) {
            failures.push(format!("{}: expect.json {}", name, c.description));
        }
    }
    assert!(failures.is_empty(), "カメラ仕様を満たさない項目:\n{}", failures.join("\n"));
}
