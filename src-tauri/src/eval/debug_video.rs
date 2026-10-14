//! カメラの動きを確認するデバッグ動画。
//!
//! 元フレーム全体に次を描き込み、下端に区間タイムラインを付けて MP4 にする。
//! - ビューポート矩形（寄っている間は緑、全体表示は灰）
//! - カーソル（赤い点）
//! - クリック（水色の広がる輪）、スクロール（橙の輪）、キー入力（カーソル横の紫の四角）
//! - 画面の変化領域（桃色の細枠）と、いまのショットの主役（水色の細枠）
//! - タイムライン: 寄っている区間（緑）、アクティビティ（白=クリック / 紫=キー / 橙=スクロール / 水色=ドラッグ）、
//!   R5 で無視する録画冒頭・末尾（暗い赤）、現在時刻（赤い縦線）

use crate::camera::{self, spec, ActivityKind, CameraConfig, CameraTrack, Viewport};
use crate::config::RecordingEvent;
use crate::eval::metrics::zoomed_runs;
use crate::recording_data::RecordingData;
use anyhow::{anyhow, Context, Result};
use image::{Rgb, RgbImage};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const TIMELINE_HEIGHT: u32 = 48;
/// クリックの輪を表示する時間 (ms)
const RING_MS: u64 = 500;
/// キー入力の印を表示する時間 (ms)
const KEY_MARK_MS: u64 = 300;

const GREEN: Rgb<u8> = Rgb([40, 220, 90]);
const GRAY: Rgb<u8> = Rgb([160, 160, 160]);
const RED: Rgb<u8> = Rgb([240, 40, 40]);
const CYAN: Rgb<u8> = Rgb([40, 200, 240]);
const ORANGE: Rgb<u8> = Rgb([250, 140, 30]);
const MAGENTA: Rgb<u8> = Rgb([210, 60, 220]);
const WHITE: Rgb<u8> = Rgb([255, 255, 255]);
const PINK: Rgb<u8> = Rgb([255, 110, 180]);
/// 変化領域を表示する時間 (ms)
const CHANGE_MS: u64 = 600;
const TIMELINE_BG: Rgb<u8> = Rgb([30, 30, 30]);
const GUARD_BG: Rgb<u8> = Rgb([70, 25, 25]);

/// デバッグ動画を `output` に書き出す。録画に frames が無い場合はエラー。
pub fn render(data: &RecordingData, track: &CameraTrack, output: &Path) -> Result<()> {
    if data.frame_count() == 0 || !data.frames_dir().is_dir() {
        return Err(anyhow!("frames が無い録画にはデバッグ動画を作れません: {}", data.dir.display()));
    }
    let first = image::open(data.frame_path(0))
        .with_context(|| format!("フレームを読めません: {}", data.frame_path(0).display()))?
        .to_rgb8();
    // yuv420p のため幅・高さを偶数にそろえる
    let out_w = (first.width() + 1) & !1;
    let out_h = (first.height() + TIMELINE_HEIGHT + 1) & !1;
    // 元フレームの px / キャプチャ領域座標
    let scale = first.width() as f64 / data.width() as f64;
    let duration_ms = track.time_ms(track.frames.len()).max(1);
    let timeline = render_timeline(data, track, out_w, out_h - first.height(), duration_ms);

    let ffmpeg = crate::export::encoder::find_ffmpeg()?;
    let mut child = Command::new(ffmpeg)
        .args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s"])
        .arg(format!("{}x{}", out_w, out_h))
        .args(["-framerate", &track.fps.to_string(), "-i", "-"])
        .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-pix_fmt", "yuv420p"])
        .arg(output)
        .stdin(Stdio::piped())
        .spawn()
        .context("ffmpeg を起動できません")?;
    let mut stdin = child.stdin.take().expect("stdin is piped");

    let (w, h) = (data.width() as f64, data.height() as f64);
    let shots = camera::plan(data, &CameraConfig::default());
    let mut current: Option<(u64, RgbImage)> = Some((0, first));
    for (k, state) in track.frames.iter().enumerate() {
        let t = track.time_ms(k);
        let frame_idx = data.frame_index_at(t);
        if current.as_ref().map(|(i, _)| *i) != Some(frame_idx) {
            match image::open(data.frame_path(frame_idx)) {
                Ok(img) => current = Some((frame_idx, img.to_rgb8())),
                Err(e) => log::warn!("Frame {} could not be loaded: {}", frame_idx, e),
            }
        }
        let mut canvas = RgbImage::new(out_w, out_h);
        if let Some((_, frame)) = &current {
            image::imageops::replace(&mut canvas, frame, 0, 0);
        }
        let frame_h = out_h - timeline.height();
        image::imageops::replace(&mut canvas, &timeline, 0, frame_h as i64);

        // 変化領域（直近の変化したフレーム）と、いまのショットの主役
        let ci = data.changes.partition_point(|c| c.t <= t);
        if let Some(c) = ci.checked_sub(1).map(|i| &data.changes[i]).filter(|c| c.t + CHANGE_MS > t) {
            for r in &c.rects {
                let r = r.rect;
                stroke_rect(&mut canvas, scale_rect(Viewport { x: r.l, y: r.t, width: r.width(), height: r.height() }, scale), 1.0, PINK);
            }
        }
        if let Some(s) = shots.iter().find(|s| s.start <= t && t < s.end) {
            let r = s.subject.inflate(2.0);
            stroke_rect(&mut canvas, scale_rect(Viewport { x: r.l, y: r.t, width: r.width(), height: r.height() }, scale), 2.0, CYAN);
        }

        // ビューポートとデッドゾーン
        let vp = scale_rect(state.viewport(w, h), scale);
        let zoomed = state.zoom >= spec::ZOOMED_THRESHOLD;
        stroke_rect(&mut canvas, vp, 3.0, if zoomed { GREEN } else { GRAY });

        // イベント
        for e in recent_events(&data.events, t, RING_MS) {
            let age = (t - e.timestamp()) as f64;
            match e {
                RecordingEvent::Click { x, y, .. } => {
                    let r = (10.0 + 30.0 * age / RING_MS as f64) * scale;
                    stroke_circle(&mut canvas, x * scale, y * scale, r, 3.0, CYAN);
                }
                RecordingEvent::Scroll { x, y, .. } => {
                    stroke_circle(&mut canvas, x * scale, y * scale, 14.0 * scale, 3.0, ORANGE);
                }
                _ => {}
            }
        }
        let cursor = camera::cursor_at(&data.events, t);
        if let Some((cx, cy)) = cursor {
            fill_circle(&mut canvas, cx * scale, cy * scale, 6.0 * scale.max(1.0), RED);
            let key_recent = recent_events(&data.events, t, KEY_MARK_MS)
                .any(|e| matches!(e, RecordingEvent::Key { .. }));
            if key_recent {
                let (kx, ky) = (cx * scale + 12.0, cy * scale + 12.0);
                fill_rect(&mut canvas, kx, ky, kx + 14.0, ky + 14.0, MAGENTA);
            }
        }

        // 現在時刻
        let x = t as f64 / duration_ms as f64 * out_w as f64;
        fill_rect(&mut canvas, x - 1.0, frame_h as f64, x + 1.0, out_h as f64, RED);

        stdin.write_all(canvas.as_raw()).context("ffmpeg への書き込みに失敗しました")?;
        if k % 300 == 0 {
            log::info!("debug video: frame {}/{}", k, track.frames.len());
        }
    }
    drop(stdin);
    let status = child.wait()?;
    if !status.success() {
        return Err(anyhow!("ffmpeg が失敗しました: {}", status));
    }
    Ok(())
}

/// 時刻 `t` 以前 `window_ms` 以内のイベント
fn recent_events(events: &[RecordingEvent], t: u64, window_ms: u64) -> impl Iterator<Item = &RecordingEvent> {
    let end = events.partition_point(|e| e.timestamp() <= t);
    let start = events.partition_point(|e| e.timestamp() + window_ms < t);
    events[start..end.max(start)].iter()
}

/// 動画下端の静的なタイムライン（現在時刻の線以外）
fn render_timeline(data: &RecordingData, track: &CameraTrack, width: u32, height: u32, duration_ms: u64) -> RgbImage {
    let mut img = RgbImage::from_pixel(width, height, TIMELINE_BG);
    let x_of = |t: u64| t as f64 / duration_ms as f64 * width as f64;
    let h = height as f64;

    // R5: 無視する冒頭・末尾
    fill_rect(&mut img, 0.0, 0.0, x_of(spec::START_GUARD_MS), h, GUARD_BG);
    let end_guard = data.meta.duration_ms.saturating_sub(spec::END_GUARD_MS);
    fill_rect(&mut img, x_of(end_guard), 0.0, width as f64, h, GUARD_BG);

    // 寄っている区間
    for (s, e) in zoomed_runs(track) {
        fill_rect(&mut img, x_of(track.time_ms(s)), 4.0, x_of(track.time_ms(e)), h * 0.45, GREEN);
    }

    // アクティビティ
    for a in camera::extract_activities(&data.events, data.meta.duration_ms) {
        let color = match a.kind {
            ActivityKind::Click => WHITE,
            ActivityKind::Key => MAGENTA,
            ActivityKind::Scroll => ORANGE,
            ActivityKind::Drag => CYAN,
        };
        let x = x_of(a.t);
        fill_rect(&mut img, x, h * 0.55, x + 1.0, h - 4.0, color);
    }
    img
}

fn scale_rect(r: Viewport, s: f64) -> Viewport {
    Viewport { x: r.x * s, y: r.y * s, width: r.width * s, height: r.height * s }
}

/// `[x0, x1) x [y0, y1)` を塗る（画像外ははみ出し分を無視）
fn fill_rect(img: &mut RgbImage, x0: f64, y0: f64, x1: f64, y1: f64, color: Rgb<u8>) {
    let clamp = |v: f64, max: u32| v.round().clamp(0.0, max as f64) as u32;
    let (x0, x1) = (clamp(x0, img.width()), clamp(x1, img.width()));
    let (y0, y1) = (clamp(y0, img.height()), clamp(y1, img.height()));
    for y in y0..y1 {
        for x in x0..x1 {
            img.put_pixel(x, y, color);
        }
    }
}

fn stroke_rect(img: &mut RgbImage, r: Viewport, thickness: f64, color: Rgb<u8>) {
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width, r.y + r.height);
    fill_rect(img, x0, y0, x1, y0 + thickness, color);
    fill_rect(img, x0, y1 - thickness, x1, y1, color);
    fill_rect(img, x0, y0, x0 + thickness, y1, color);
    fill_rect(img, x1 - thickness, y0, x1, y1, color);
}

/// 中心からの距離が `[inner, outer]` の画素を塗る
fn paint_annulus(img: &mut RgbImage, cx: f64, cy: f64, inner: f64, outer: f64, color: Rgb<u8>) {
    let x0 = (cx - outer).floor().max(0.0) as u32;
    let y0 = (cy - outer).floor().max(0.0) as u32;
    let x1 = ((cx + outer).ceil().max(0.0) as u32).min(img.width());
    let y1 = ((cy + outer).ceil().max(0.0) as u32).min(img.height());
    for y in y0..y1 {
        for x in x0..x1 {
            let d = (x as f64 + 0.5 - cx).hypot(y as f64 + 0.5 - cy);
            if d >= inner && d <= outer {
                img.put_pixel(x, y, color);
            }
        }
    }
}

fn fill_circle(img: &mut RgbImage, cx: f64, cy: f64, r: f64, color: Rgb<u8>) {
    paint_annulus(img, cx, cy, 0.0, r, color);
}

fn stroke_circle(img: &mut RgbImage, cx: f64, cy: f64, r: f64, thickness: f64, color: Rgb<u8>) {
    paint_annulus(img, cx, cy, (r - thickness).max(0.0), r, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drawing_clips_to_image() {
        let mut img = RgbImage::new(10, 10);
        fill_rect(&mut img, -5.0, -5.0, 3.0, 3.0, WHITE);
        stroke_rect(&mut img, Viewport { x: 5.0, y: 5.0, width: 20.0, height: 20.0 }, 1.0, RED);
        fill_circle(&mut img, 0.0, 9.0, 4.0, GREEN);
        assert_eq!(*img.get_pixel(0, 0), WHITE);
        assert_eq!(*img.get_pixel(3, 3), Rgb([0, 0, 0]));
        assert_eq!(*img.get_pixel(6, 5), RED);
        assert_eq!(*img.get_pixel(0, 9), GREEN);
    }

    #[test]
    fn test_recent_events_window() {
        let key = |t| RecordingEvent::Key { t, key: "Character".into(), modifiers: vec![] };
        let events = vec![key(100), key(500), key(900), key(1500)];
        let ts: Vec<u64> = recent_events(&events, 1000, 500).map(RecordingEvent::timestamp).collect();
        assert_eq!(ts, vec![500, 900]);
    }
}
