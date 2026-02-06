use crate::config::QualityPreset;

pub struct EncodingParams {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: u32,
    pub crf: u32,
}

impl EncodingParams {
    pub fn from_preset(preset: &QualityPreset, original_width: u32, original_height: u32) -> Self {
        match preset {
            QualityPreset::Social => Self {
                width: Some(1920),
                height: Some(1080),
                fps: 30,
                crf: 23,
            },
            QualityPreset::HighQuality => Self {
                width: Some(original_width),
                height: Some(original_height),
                fps: 60,
                crf: 18,
            },
            QualityPreset::Lightweight => Self {
                width: Some(1280),
                height: Some(720),
                fps: 24,
                crf: 30,
            },
        }
    }
}
