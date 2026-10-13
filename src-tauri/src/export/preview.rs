//! プレビュー画面用のデータ (Issue #30)。
//!
//! プレビューは `<canvas>` で書き出しと同じ合成（背景・角丸・影・カーソル・クリックリング・キー表示）を
//! 再現する。そのための時系列・スタイル・カーソル画像を、書き出しと同じ計算
//! （`OutputStyle`・`EffectTimeline`・`Compositor` のスプライト）から作って渡す。
//! カメラはここに含めず、`get_camera_track` で別に求める。

use crate::config::defaults::OutputStyle;
use crate::config::{AppSettings, BackgroundConfig, QualityPreset};
use crate::engine::compositor::{self, Compositor, CursorKind};
use crate::export::encoder::EffectTimeline;
use crate::export::presets::EncodingParams;
use crate::recording_data::RecordingData;
use anyhow::Result;
use base64::Engine;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct PreviewData {
    /// キャプチャ領域の大きさ (px)
    pub width: u32,
    pub height: u32,
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub duration_ms: u64,
    /// 各フレームの録画時刻 (ms)。frames/frame_{番号:08}.{frame_ext}
    #[cfg_attr(feature = "ts-export", ts(type = "Array<number>"))]
    pub frame_times: Vec<u64>,
    /// フレーム画像の拡張子（新しい録画は "jpg"、旧録画は "png"）
    pub frame_ext: String,
    /// audio.wav に音声が入っているか
    pub has_audio: bool,
    /// カーソル位置（スムージング済み）。時刻順
    pub cursor: Vec<TimedPoint>,
    pub cursor_shapes: Vec<CursorShapeChange>,
    /// クリックリングを出すクリック。設定で無効なら空
    pub clicks: Vec<TimedPoint>,
    /// キー表示。設定で無効なら空
    pub keys: Vec<KeyBadge>,
    pub style: PreviewStyle,
    pub sprites: Vec<CursorSprite>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct TimedPoint {
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub t: u64,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct CursorShapeChange {
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub t: u64,
    pub kind: CursorKind,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct KeyBadge {
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub t: u64,
    pub keys: String,
}

/// 合成のスタイル。長さは出力画像（`output_width` x `output_height`）の px。
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct PreviewStyle {
    pub output_width: u32,
    pub output_height: u32,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub border_radius: u32,
    pub shadow_blur: f64,
    pub shadow_offset_y: f64,
    pub shadow_color: [u8; 4],
    pub background: BackgroundConfig,
    /// 倍率 1.0 のときのカーソル画像の幅（倍率に比例して大きくする）
    pub cursor_width: f64,
    pub click_ring_max_radius: f64,
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub click_ring_duration_ms: u64,
    pub click_ring_color: [u8; 4],
    pub click_ring_stroke_width: f64,
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub key_badge_duration_ms: u64,
    pub key_badge_height: u32,
    pub key_badge_margin_bottom: u32,
    pub key_badge_radius: u32,
    pub key_badge_color: [u8; 4],
    pub key_badge_char_width: u32,
    pub key_badge_padding: u32,
}

impl PreviewStyle {
    fn new(style: &OutputStyle) -> Self {
        Self {
            output_width: style.output_width,
            output_height: style.output_height,
            canvas_width: style.canvas_width,
            canvas_height: style.canvas_height,
            border_radius: style.border_radius,
            shadow_blur: style.shadow_blur,
            shadow_offset_y: style.shadow_offset_y,
            shadow_color: style.shadow_color,
            background: style.background.clone(),
            cursor_width: style.cursor_size_multiplier * compositor::CURSOR_BASE_SIZE as f64,
            click_ring_max_radius: style.click_ring_max_radius,
            click_ring_duration_ms: style.click_ring_duration_ms,
            click_ring_color: style.click_ring_color,
            click_ring_stroke_width: style.click_ring_stroke_width,
            key_badge_duration_ms: style.key_badge_duration_ms,
            key_badge_height: compositor::KEY_BADGE_HEIGHT,
            key_badge_margin_bottom: compositor::KEY_BADGE_MARGIN_BOTTOM,
            key_badge_radius: compositor::KEY_BADGE_RADIUS,
            key_badge_color: compositor::KEY_BADGE_COLOR,
            key_badge_char_width: compositor::KEY_BADGE_CHAR_WIDTH,
            key_badge_padding: compositor::KEY_BADGE_PADDING,
        }
    }
}

/// カーソル画像（PNG の data URL）とホットスポット（画像内の px）
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../src/lib/generated/"))]
pub struct CursorSprite {
    pub kind: CursorKind,
    pub data_url: String,
    pub hotspot_x: u32,
    pub hotspot_y: u32,
}

/// 画質プリセット `quality` で書き出すときと同じ見た目のプレビュー用データを作る。
pub fn preview_data(data: &RecordingData, settings: &AppSettings, quality: &QualityPreset) -> Result<PreviewData> {
    let params = EncodingParams::from_preset(quality, data.width(), data.height());
    let style = OutputStyle::from_settings(&params, settings);
    let timeline = EffectTimeline::new(data, settings, &style);
    let preview_style = PreviewStyle::new(&style);
    let compositor = Compositor::new(style, data.width(), data.height());

    let sprites = compositor
        .cursor_sprites()
        .into_iter()
        .map(|(kind, image, (hotspot_x, hotspot_y))| {
            let mut png = Vec::new();
            image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
            Ok(CursorSprite {
                kind,
                data_url: format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(&png)
                ),
                hotspot_x,
                hotspot_y,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let frame_ext = data
        .frame_path(0)
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "jpg".to_string());
    let has_audio = std::fs::metadata(data.dir.join("audio.wav"))
        .map(|m| m.len() > 44)
        .unwrap_or(false);

    Ok(PreviewData {
        width: data.width(),
        height: data.height(),
        duration_ms: data.meta.duration_ms,
        frame_times: data.frame_times.clone(),
        frame_ext,
        has_audio,
        cursor: timeline
            .cursor_positions
            .iter()
            .map(|&(t, x, y)| TimedPoint { t, x, y })
            .collect(),
        cursor_shapes: timeline
            .cursor_shapes
            .iter()
            .map(|&(t, kind)| CursorShapeChange { t, kind })
            .collect(),
        clicks: timeline
            .click_effects
            .iter()
            .map(|c| TimedPoint { t: c.start_ms, x: c.x, y: c.y })
            .collect(),
        keys: timeline
            .key_overlays
            .iter()
            .map(|k| KeyBadge { t: k.start_ms, keys: k.keys.clone() })
            .collect(),
        style: preview_style,
        sprites,
    })
}
