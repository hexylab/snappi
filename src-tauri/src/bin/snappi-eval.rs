//! カメラの評価・デバッグ用 CLI (Issue #28)。
//!
//! ```text
//! cargo run --bin snappi-eval -- <recording_dir> [--metrics] [--debug-video <out.mp4>]
//! cargo run --bin snappi-eval -- <recording_dir> --draft-expect
//! cargo run --bin snappi-eval -- <recording_dir>... --table
//! cargo run --release --bin snappi-eval -- <recording_dir>... --changes
//! ```
//! オプションを省略した場合は `--metrics` とみなす。
//! `--draft-expect` は expect.json の下書きを標準出力に出す（description は既存の expect.json から引き継ぐ）。
//! `--table` は複数の録画のメトリクスを Markdown の表（仕様書のベースライン表の形式）で出す。
//! `--changes` は frames から changes.jsonl（画面の変化領域, Issue #39）を作り直す。
//! 録画でないディレクトリ（`testdata/recordings` 等）を渡すと、その直下の録画をすべて対象にする。

use snappi_lib::camera;
use snappi_lib::eval::expect::Expectation;
use snappi_lib::eval::{self, debug_video};
use snappi_lib::recording_data::RecordingData;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "使い方: snappi-eval <recording_dir> [--metrics] [--debug-video <out.mp4>]
       snappi-eval <recording_dir> --draft-expect
       snappi-eval <recording_dir>... --table
       snappi-eval <recording_dir>... --changes";

struct Args {
    dirs: Vec<PathBuf>,
    metrics: bool,
    debug_video: Option<PathBuf>,
    draft_expect: bool,
    table: bool,
    changes: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut dirs = Vec::new();
    let mut metrics = false;
    let mut debug_video = None;
    let mut draft_expect = false;
    let mut table = false;
    let mut changes = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--metrics" => metrics = true,
            "--debug-video" => {
                debug_video = Some(PathBuf::from(args.next().ok_or("--debug-video に出力先がありません")?));
            }
            "--draft-expect" => draft_expect = true,
            "--table" => table = true,
            "--changes" => changes = true,
            "-h" | "--help" => return Err(String::new()),
            a if a.starts_with('-') => return Err(format!("不明なオプション: {}", a)),
            a => dirs.push(PathBuf::from(a)),
        }
    }
    if dirs.is_empty() {
        return Err("録画ディレクトリを指定してください".into());
    }
    if table || changes {
        if metrics || debug_video.is_some() || draft_expect || (table && changes) {
            return Err("--table / --changes は他のオプションと同時に指定できません".into());
        }
    } else if dirs.len() > 1 {
        return Err(format!("余分な引数: {}", dirs[1].display()));
    }
    let metrics = metrics || (debug_video.is_none() && !draft_expect && !table && !changes);
    Ok(Args { dirs, metrics, debug_video, draft_expect, table, changes })
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("{}", msg);
            }
            eprintln!("{}", USAGE);
            return ExitCode::from(2);
        }
    };
    let result = if args.table {
        print_table(&args.dirs)
    } else if args.changes {
        generate_changes(&args.dirs)
    } else {
        run(&args)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("エラー: {:#}", e);
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> anyhow::Result<()> {
    let dir = &args.dirs[0];

    if args.draft_expect {
        let data = RecordingData::load(dir)?;
        let description = Expectation::load(dir)?.map(|e| e.description).unwrap_or_default();
        println!("{}", serde_json::to_string_pretty(&Expectation::draft(&data, &description))?);
        if !args.metrics && args.debug_video.is_none() {
            return Ok(());
        }
    }

    let (data, report) = eval::evaluate_dir(dir)?;

    if args.metrics {
        println!("録画: {}", dir.display());
        println!(
            "  {}x{} / {:.1}秒 / 元フレーム {} / イベント {} / モード {}",
            data.width(),
            data.height(),
            data.meta.duration_ms as f64 / 1000.0,
            data.frame_count(),
            data.events.len(),
            data.meta.recording_mode.as_deref().unwrap_or("display"),
        );
        println!("\nショット（ズーム強さ「標準」）:");
        for s in camera::plan(&data, &camera::CameraConfig::default()) {
            println!(
                "  {:>6}〜{:>6}ms 操作 {:>6}〜{:>6}ms  x{:.2}  中心 ({:.0}, {:.0})  主役 [{:.0}, {:.0}, {:.0}, {:.0}]",
                s.start, s.end, s.first, s.last, s.zoom, s.center.0, s.center.1,
                s.subject.l, s.subject.t, s.subject.r, s.subject.b
            );
        }
        println!("\nメトリクス ({}fps):", report.track.fps);
        for r in &report.metric_results {
            println!("  {}", r);
        }
        if !report.check_results.is_empty() {
            println!("\nexpect.json:");
            for c in &report.check_results {
                println!("  {} {}", if c.passed { "OK  " } else { "FAIL" }, c.description);
            }
        }
        println!("\n結果: {}", if report.passed() { "合格" } else { "不合格" });
    }

    if let Some(out) = &args.debug_video {
        debug_video::render(&data, &report.track, out)?;
        println!("デバッグ動画を出力しました: {}", out.display());
    }
    Ok(())
}

fn generate_changes(dirs: &[PathBuf]) -> anyhow::Result<()> {
    for dir in &table_dirs(dirs)? {
        snappi_lib::changes::generate(dir)?;
        let n = snappi_lib::changes::load(dir).len();
        println!("{}: 変化のあるフレーム {}", dir.display(), n);
    }
    Ok(())
}

/// `--table` / `--changes` の対象。meta.json の無いディレクトリ（testdata/recordings 等）はその直下の録画に展開する。
fn table_dirs(dirs: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for dir in dirs {
        if dir.join("meta.json").exists() {
            out.push(dir.clone());
        } else if dir.is_dir() {
            let mut children: Vec<PathBuf> = std::fs::read_dir(dir)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.join("meta.json").exists())
                .collect();
            children.sort();
            out.extend(children);
        }
    }
    Ok(out)
}

fn print_table(dirs: &[PathBuf]) -> anyhow::Result<()> {
    let mut rows = Vec::new();
    let mut header = None;
    for dir in &table_dirs(dirs)? {
        let (_, report) = eval::evaluate_dir(dir)?;
        header.get_or_insert_with(|| report.metric_results.iter().map(|r| r.id).collect::<Vec<_>>());
        let mut cells: Vec<String> = vec![format!("`{}`", dir_name(dir))];
        cells.extend(report.metric_results.iter().map(|r| mark(&r.value, r.passed)));
        let passed = report.check_results.iter().filter(|c| c.passed).count();
        let total = report.check_results.len();
        cells.push(mark(&format!("{}/{}", passed, total), passed == total));
        rows.push(cells);
    }
    let header = header.unwrap_or_default();
    println!("| シナリオ | {} | expect.json |", header.join(" | "));
    println!("|---|{}---|", "---|".repeat(header.len()));
    for cells in rows {
        println!("| {} |", cells.join(" | "));
    }
    Ok(())
}

fn mark(value: &str, passed: bool) -> String {
    if passed {
        value.to_string()
    } else {
        format!("**{}** ✗", value)
    }
}

fn dir_name(dir: &Path) -> String {
    dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned())
}
