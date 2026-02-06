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
        match preset {
            QualityPreset::Social => {
                let w = 1920u32;
                let h = 1080u32;
                Self {
                    width: Some(w),
                    height: Some(h),
                    canvas_width: w + CANVAS_PADDING,
                    canvas_height: h + CANVAS_PADDING,
                    fps: 30,
                    crf: 23,
                }
            }
            QualityPreset::HighQuality => Self {
                width: Some(original_width),
                height: Some(original_height),
                canvas_width: original_width + CANVAS_PADDING,
                canvas_height: original_height + CANVAS_PADDING,
                fps: 60,
                crf: 18,
            },
            QualityPreset::Lightweight => {
                let w = 1280u32;
                let h = 720u32;
                Self {
                    width: Some(w),
                    height: Some(h),
                    canvas_width: w + CANVAS_PADDING,
                    canvas_height: h + CANVAS_PADDING,
                    fps: 24,
                    crf: 30,
                }
            }
        }
    }
}
