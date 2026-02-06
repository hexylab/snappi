use crate::config::{AppSettings, ExportFormat, QualityPreset, RecordingMeta};
use crate::export::presets::EncodingParams;
use anyhow::Result;
use std::process::Command;

pub fn export(
    recording_id: &str,
    format: &ExportFormat,
    quality: &QualityPreset,
    settings: &AppSettings,
) -> Result<String> {
    let recording_dir = dirs::video_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Snappi")
        .join("recordings")
        .join(recording_id);

    let meta_path = recording_dir.join("meta.json");
    let meta_str = std::fs::read_to_string(&meta_path)?;
    let meta: RecordingMeta = serde_json::from_str(&meta_str)?;

    let params = EncodingParams::from_preset(quality, meta.screen_width, meta.screen_height);

    let output_dir = std::path::PathBuf::from(&settings.output.save_directory);
    std::fs::create_dir_all(&output_dir)?;

    let output_path = match format {
        ExportFormat::Mp4 => output_dir.join(format!("{}.mp4", recording_id)),
        ExportFormat::Gif => output_dir.join(format!("{}.gif", recording_id)),
        ExportFormat::WebM => output_dir.join(format!("{}.webm", recording_id)),
    };

    let ffmpeg = find_ffmpeg()?;
    let frames_dir = recording_dir.join("frames");

    match format {
        ExportFormat::Mp4 => {
            encode_mp4(&ffmpeg, &frames_dir, &output_path, &params, &recording_dir)?;
        }
        ExportFormat::Gif => {
            encode_gif(&ffmpeg, &frames_dir, &output_path, &params)?;
        }
        ExportFormat::WebM => {
            encode_webm(&ffmpeg, &frames_dir, &output_path, &params, &recording_dir)?;
        }
    }

    Ok(output_path.to_string_lossy().to_string())
}

fn find_ffmpeg() -> Result<String> {
    // Try bundled ffmpeg first (next to exe)
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    if let Some(ref dir) = exe_dir {
        let bundled = dir.join("ffmpeg").join("ffmpeg.exe");
        if bundled.exists() {
            return Ok(bundled.to_string_lossy().to_string());
        }
        // Also check next to the exe directly
        let beside = dir.join("ffmpeg.exe");
        if beside.exists() {
            return Ok(beside.to_string_lossy().to_string());
        }
    }

    // Try system PATH
    if Command::new("ffmpeg").arg("-version").output().is_ok() {
        return Ok("ffmpeg".to_string());
    }

    // Try winget install location
    if let Some(local_app) = std::env::var_os("LOCALAPPDATA") {
        let winget_dir = std::path::PathBuf::from(local_app)
            .join("Microsoft")
            .join("WinGet")
            .join("Packages");
        if winget_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&winget_dir) {
                for entry in entries.flatten() {
                    if entry.file_name().to_string_lossy().contains("FFmpeg") {
                        // Search for ffmpeg.exe inside this package
                        if let Some(path) = find_ffmpeg_in_dir(&entry.path()) {
                            return Ok(path);
                        }
                    }
                }
            }
        }
    }

    // Try common install locations
    let common_paths = [
        r"C:\ffmpeg\bin\ffmpeg.exe",
        r"C:\tools\ffmpeg\bin\ffmpeg.exe",
        r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
    ];
    for path in &common_paths {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }

    Err(anyhow::anyhow!(
        "FFmpeg not found. Please install FFmpeg or place it in the ffmpeg/ directory."
    ))
}

fn find_ffmpeg_in_dir(dir: &std::path::Path) -> Option<String> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.file_name().map(|n| n == "ffmpeg.exe").unwrap_or(false) {
                return Some(path.to_string_lossy().to_string());
            }
            if path.is_dir() {
                if let Some(found) = find_ffmpeg_in_dir(&path) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn encode_mp4(
    ffmpeg: &str,
    frames_dir: &std::path::Path,
    output: &std::path::Path,
    params: &EncodingParams,
    recording_dir: &std::path::Path,
) -> Result<()> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-y", "-framerate"])
        .arg(params.fps.to_string())
        .args(["-i"])
        .arg(frames_dir.join("frame_%08d.png").to_string_lossy().to_string())
        .args(["-c:v", "libx264"])
        .args(["-crf"])
        .arg(params.crf.to_string())
        .args(["-preset", "medium"])
        .args(["-pix_fmt", "yuv420p"])
        .args(["-movflags", "+faststart"]);

    // Add audio if available
    let audio_path = recording_dir.join("audio.wav");
    if audio_path.exists() {
        cmd.args(["-i"])
            .arg(audio_path.to_string_lossy().to_string())
            .args(["-c:a", "aac", "-b:a", "128k"]);
    }

    if let (Some(w), Some(h)) = (params.width, params.height) {
        cmd.args(["-vf"]).arg(format!("scale={}:{}", w, h));
    }

    cmd.arg(output.to_string_lossy().to_string());

    let result = cmd.output()?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(anyhow::anyhow!("FFmpeg MP4 encoding failed: {}", stderr));
    }

    Ok(())
}

fn encode_gif(
    ffmpeg: &str,
    frames_dir: &std::path::Path,
    output: &std::path::Path,
    params: &EncodingParams,
) -> Result<()> {
    let palette_path = output.with_extension("palette.png");
    let fps = params.fps.min(15); // GIF typically lower FPS
    let width = params.width.unwrap_or(640).min(640);

    // Pass 1: Generate palette
    Command::new(ffmpeg)
        .args(["-y", "-framerate"])
        .arg(fps.to_string())
        .args(["-i"])
        .arg(frames_dir.join("frame_%08d.png").to_string_lossy().to_string())
        .args(["-vf"])
        .arg(format!(
            "scale={}:-1:flags=lanczos,palettegen",
            width
        ))
        .arg(palette_path.to_string_lossy().to_string())
        .output()?;

    // Pass 2: Generate GIF with palette
    Command::new(ffmpeg)
        .args(["-y", "-framerate"])
        .arg(fps.to_string())
        .args(["-i"])
        .arg(frames_dir.join("frame_%08d.png").to_string_lossy().to_string())
        .args(["-i"])
        .arg(palette_path.to_string_lossy().to_string())
        .args(["-lavfi"])
        .arg(format!(
            "scale={}:-1:flags=lanczos[x];[x][1:v]paletteuse",
            width
        ))
        .arg(output.to_string_lossy().to_string())
        .output()?;

    // Clean up palette
    let _ = std::fs::remove_file(&palette_path);

    Ok(())
}

fn encode_webm(
    ffmpeg: &str,
    frames_dir: &std::path::Path,
    output: &std::path::Path,
    params: &EncodingParams,
    recording_dir: &std::path::Path,
) -> Result<()> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-y", "-framerate"])
        .arg(params.fps.to_string())
        .args(["-i"])
        .arg(frames_dir.join("frame_%08d.png").to_string_lossy().to_string())
        .args(["-c:v", "libvpx-vp9"])
        .args(["-crf"])
        .arg(params.crf.to_string())
        .args(["-b:v", "0"]);

    let audio_path = recording_dir.join("audio.wav");
    if audio_path.exists() {
        cmd.args(["-i"])
            .arg(audio_path.to_string_lossy().to_string())
            .args(["-c:a", "libopus"]);
    }

    if let (Some(w), Some(h)) = (params.width, params.height) {
        cmd.args(["-vf"]).arg(format!("scale={}:{}", w, h));
    }

    cmd.arg(output.to_string_lossy().to_string());

    let result = cmd.output()?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(anyhow::anyhow!("FFmpeg WebM encoding failed: {}", stderr));
    }

    Ok(())
}
