//! カメラの評価・デバッグ用 CLI (Issue #28)。
//!
//! ```text
//! cargo run --bin snappi-eval -- <recording_dir> [--metrics] [--debug-video <out.mp4>]
//! ```
//! オプションを省略した場合は `--metrics` とみなす。

use snappi_lib::eval::{self, debug_video};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "使い方: snappi-eval <recording_dir> [--metrics] [--debug-video <out.mp4>]";

struct Args {
    dir: PathBuf,
    metrics: bool,
    debug_video: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut dir = None;
    let mut metrics = false;
    let mut debug_video = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--metrics" => metrics = true,
            "--debug-video" => {
                debug_video = Some(PathBuf::from(args.next().ok_or("--debug-video に出力先がありません")?));
            }
            "-h" | "--help" => return Err(String::new()),
            a if a.starts_with('-') => return Err(format!("不明なオプション: {}", a)),
            a if dir.is_none() => dir = Some(PathBuf::from(a)),
            a => return Err(format!("余分な引数: {}", a)),
        }
    }
    let dir = dir.ok_or("録画ディレクトリを指定してください")?;
    Ok(Args { dir, metrics: metrics || debug_video.is_none(), debug_video })
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
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("エラー: {:#}", e);
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> anyhow::Result<()> {
    let (data, report) = eval::evaluate_dir(&args.dir)?;

    if args.metrics {
        println!("録画: {}", args.dir.display());
        println!(
            "  {}x{} / {:.1}秒 / 元フレーム {} / イベント {} / モード {}",
            data.width(),
            data.height(),
            data.meta.duration_ms as f64 / 1000.0,
            data.frame_count(),
            data.events.len(),
            data.meta.recording_mode.as_deref().unwrap_or("display"),
        );
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
