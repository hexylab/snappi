use crate::config::QualityPreset;

const CANVAS_PADDING: u32 = 128; // 64px each side

pub struct EncodingParams {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub fps: u32,
    pub crf: u32,
}

impl EncodingParams {
    pub fn from_preset(preset: &QualityPreset, original_width: u32, original_height: u32) -> Self {
        let (fps, crf) = match preset {
            QualityPreset::Social => (30, 23),
            QualityPreset::HighQuality => (60, 18),
            QualityPreset::Lightweight => (24, 30),
        };
        let (w, h) = match preset {
            QualityPreset::Social => fit_aspect(original_width, original_height, 1920, 1080),
            QualityPreset::HighQuality => (even(original_width), even(original_height)),
            QualityPreset::Lightweight => fit_aspect(original_width, original_height, 1280, 720),
        };
        Self {
            width: Some(w),
            height: Some(h),
            canvas_width: w + CANVAS_PADDING,
            canvas_height: h + CANVAS_PADDING,
            fps,
            crf,
        }
    }
}

/// 元の縦横比を保ったまま、長辺 `long`・短辺 `short` の枠に収まる最大サイズを返す。
/// 縦長の元映像では長辺が高さ側になる。yuv420p でエンコードするため偶数に丸める。
fn fit_aspect(src_w: u32, src_h: u32, long: u32, short: u32) -> (u32, u32) {
    if src_w == 0 || src_h == 0 {
        return (long, short);
    }
    let (box_w, box_h) = if src_h > src_w { (short, long) } else { (long, short) };
    let scale = (box_w as f64 / src_w as f64).min(box_h as f64 / src_h as f64);
    (
        even((src_w as f64 * scale).round() as u32),
        even((src_h as f64 * scale).round() as u32),
    )
}

/// 偶数に切り下げる（最小 2）。
fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_social_keeps_16_9() {
        let p = EncodingParams::from_preset(&QualityPreset::Social, 3840, 2160);
        assert_eq!((p.width, p.height), (Some(1920), Some(1080)));
    }

    #[test]
    fn test_social_keeps_16_10_without_stretch() {
        let p = EncodingParams::from_preset(&QualityPreset::Social, 1920, 1200);
        assert_eq!((p.width, p.height), (Some(1728), Some(1080)));
        assert_eq!((p.canvas_width, p.canvas_height), (1728 + CANVAS_PADDING, 1080 + CANVAS_PADDING));
    }

    #[test]
    fn test_lightweight_fits_window_aspect() {
        // 4:3 のウィンドウ
        let p = EncodingParams::from_preset(&QualityPreset::Lightweight, 800, 600);
        assert_eq!((p.width, p.height), (Some(960), Some(720)));
    }

    #[test]
    fn test_portrait_uses_long_side_for_height() {
        let p = EncodingParams::from_preset(&QualityPreset::Social, 1080, 1920);
        assert_eq!((p.width, p.height), (Some(1080), Some(1920)));
    }

    #[test]
    fn test_sizes_are_even() {
        // 奇数サイズのウィンドウ録画
        let p = EncodingParams::from_preset(&QualityPreset::HighQuality, 1001, 777);
        assert_eq!((p.width, p.height), (Some(1000), Some(776)));
        let p = EncodingParams::from_preset(&QualityPreset::Social, 1001, 777);
        assert_eq!(p.width.unwrap() % 2, 0);
        assert_eq!(p.height.unwrap() % 2, 0);
    }
}
