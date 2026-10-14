use anyhow::Result;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::clock::RecordingClock;

/// 録音する音
#[derive(Debug, Clone, Copy)]
pub enum AudioSource {
    /// PC から出ている音（既定の出力デバイスの WASAPI loopback）
    System,
    /// 既定の入力デバイス（マイク）
    Microphone,
}

impl AudioSource {
    /// 録画中に書き出す WAV。停止時に `finalize_audio` が audio.wav にまとめる
    pub fn file_name(self) -> &'static str {
        match self {
            AudioSource::System => "audio_system.wav",
            AudioSource::Microphone => "audio_mic.wav",
        }
    }
}

/// WAV ヘッダー (44 bytes) より大きければ音が入っている
fn has_samples(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.len() > 44).unwrap_or(false)
}

/// 録音した音を audio.wav にまとめる。PC の音とマイクの両方があれば FFmpeg でミックスする。
/// ミックスできなければ PC の音だけを使う（録画自体は失敗させない）。
pub fn finalize_audio(dir: &Path) -> Result<()> {
    let out = dir.join("audio.wav");
    let system = dir.join(AudioSource::System.file_name());
    let mic = dir.join(AudioSource::Microphone.file_name());
    let sources: Vec<&Path> = [system.as_path(), mic.as_path()]
        .into_iter()
        .filter(|p| has_samples(p))
        .collect();

    match sources.as_slice() {
        [] => {}
        [only] => std::fs::rename(only, &out)?,
        _ => {
            if let Err(e) = mix_wavs(&system, &mic, &out) {
                log::warn!("PCの音とマイクをミックスできません。PCの音だけを使います: {:#}", e);
                std::fs::rename(&system, &out)?;
            }
        }
    }
    let _ = std::fs::remove_file(&system);
    let _ = std::fs::remove_file(&mic);
    Ok(())
}

fn mix_wavs(a: &Path, b: &Path, out: &Path) -> Result<()> {
    let ffmpeg = crate::export::encoder::find_ffmpeg()?;
    let mut cmd = std::process::Command::new(ffmpeg);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    // amix は入力数で割って音量を下げるので、volume で戻す
    let result = cmd
        .args(["-y", "-i"])
        .arg(a)
        .arg("-i")
        .arg(b)
        .args([
            "-filter_complex",
            "amix=inputs=2:duration=longest:dropout_transition=0,volume=2",
            "-c:a",
            "pcm_f32le",
        ])
        .arg(out)
        .output()?;
    if !result.status.success() {
        anyhow::bail!("FFmpeg amix failed: {}", String::from_utf8_lossy(&result.stderr));
    }
    Ok(())
}

pub fn capture_audio(
    source: AudioSource,
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    output_dir: &Path,
    clock: Arc<RecordingClock>,
) -> Result<()> {
    let audio_path = output_dir.join(source.file_name());
    log::info!("Audio capture thread started: {:?}", source);

    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    // 出力デバイスに対して input stream を作ると WASAPI loopback になる
    let device = match source {
        AudioSource::System => host.default_output_device(),
        AudioSource::Microphone => host.default_input_device(),
    };
    let device = match device {
        Some(dev) => dev,
        None => {
            log::warn!("No audio device found for {:?}, skipping audio capture", source);
            return Ok(());
        }
    };

    let config = match source {
        AudioSource::System => device.default_output_config(),
        AudioSource::Microphone => device.default_input_config(),
    };
    let config = match config {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to get audio config for {:?}: {}", source, e);
            return Ok(());
        }
    };

    log::info!(
        "Audio device for {:?}: {} ({:?})",
        source,
        device.name().unwrap_or_default(),
        config
    );
    let spec = hound_spec_from_config(&config);
    let writer = Arc::new(std::sync::Mutex::new(
        hound_writer(&audio_path, spec)?
    ));

    let err_fn = |err: cpal::StreamError| {
        log::error!("Audio stream error: {}", err);
    };

    use cpal::traits::StreamTrait;
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            input_callback::<f32>(writer.clone(), is_running.clone(), is_paused.clone(), clock.clone()),
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            input_callback::<i16>(writer.clone(), is_running.clone(), is_paused.clone(), clock.clone()),
            err_fn,
            None,
        )?,
        _ => {
            log::warn!("Unsupported audio sample format");
            return Ok(());
        }
    };

    stream.play()?;

    while is_running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    drop(stream);
    if let Ok(writer) = Arc::try_unwrap(writer) {
        if let Ok(mut writer) = writer.into_inner() {
            // 最後が無音だった分も埋めて、映像の長さに揃える
            let _ = writer.pad_to(clock.now_ms());
            log::info!("Audio captured for {:?}: {} bytes", source, writer.data_len);
            let _ = writer.finalize();
        }
    }

    log::info!("Audio capture stopped: {:?}", source);
    Ok(())
}

/// 届いた音を WAV に書くコールバック。届く前の欠けは録画の時計に合わせて無音で埋める
fn input_callback<S: Sample + cpal::SizedSample>(
    writer: Arc<Mutex<WavWriter>>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    clock: Arc<RecordingClock>,
) -> impl FnMut(&[S], &cpal::InputCallbackInfo) + Send + 'static {
    move |data: &[S], _: &cpal::InputCallbackInfo| {
        if !running.load(Ordering::SeqCst) || paused.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut writer) = writer.lock() {
            // このデータの先頭が録画の時計で何 ms にあたるか
            let rate = writer.spec.sample_rate as u64;
            let frames = data.len() as u64 / writer.spec.channels.max(1) as u64;
            let start_ms = clock.now_ms().saturating_sub(frames * 1000 / rate.max(1));
            let _ = writer.pad_to(start_ms);
            for &sample in data {
                let _ = writer.write_sample(sample);
            }
        }
    }
}

fn hound_spec_from_config(config: &cpal::SupportedStreamConfig) -> wav_spec::WavSpec {
    wav_spec::WavSpec {
        channels: config.channels(),
        sample_rate: config.sample_rate().0,
        bits_per_sample: match config.sample_format() {
            cpal::SampleFormat::F32 => 32,
            cpal::SampleFormat::I16 => 16,
            _ => 16,
        },
        sample_format: match config.sample_format() {
            cpal::SampleFormat::F32 => wav_spec::SampleFormat::Float,
            _ => wav_spec::SampleFormat::Int,
        },
    }
}

/// Minimal WAV writer to avoid hound dependency
mod wav_spec {
    #[derive(Clone, Copy)]
    pub struct WavSpec {
        pub channels: u16,
        pub sample_rate: u32,
        pub bits_per_sample: u16,
        pub sample_format: SampleFormat,
    }

    #[derive(Clone, Copy)]
    pub enum SampleFormat {
        Float,
        Int,
    }
}

struct WavWriter {
    file: std::io::BufWriter<std::fs::File>,
    spec: wav_spec::WavSpec,
    data_len: u32,
}

fn hound_writer(path: &Path, spec: wav_spec::WavSpec) -> Result<WavWriter> {
    let file = std::fs::File::create(path)?;
    let mut writer = WavWriter {
        file: std::io::BufWriter::new(file),
        spec,
        data_len: 0,
    };
    writer.write_header()?;
    Ok(writer)
}

impl WavWriter {
    fn write_header(&mut self) -> Result<()> {
        let bytes_per_sample = self.spec.bits_per_sample / 8;
        let block_align = self.spec.channels * bytes_per_sample;
        let byte_rate = self.spec.sample_rate * block_align as u32;
        let format_tag: u16 = match self.spec.sample_format {
            wav_spec::SampleFormat::Float => 3,
            wav_spec::SampleFormat::Int => 1,
        };

        // RIFF header
        self.file.write_all(b"RIFF")?;
        self.file.write_all(&0u32.to_le_bytes())?; // placeholder for file size
        self.file.write_all(b"WAVE")?;

        // fmt chunk
        self.file.write_all(b"fmt ")?;
        self.file.write_all(&16u32.to_le_bytes())?;
        self.file.write_all(&format_tag.to_le_bytes())?;
        self.file.write_all(&self.spec.channels.to_le_bytes())?;
        self.file.write_all(&self.spec.sample_rate.to_le_bytes())?;
        self.file.write_all(&byte_rate.to_le_bytes())?;
        self.file.write_all(&block_align.to_le_bytes())?;
        self.file.write_all(&self.spec.bits_per_sample.to_le_bytes())?;

        // data chunk header
        self.file.write_all(b"data")?;
        self.file.write_all(&0u32.to_le_bytes())?; // placeholder for data size

        Ok(())
    }

    fn bytes_per_frame(&self) -> u64 {
        self.spec.channels as u64 * (self.spec.bits_per_sample / 8) as u64
    }

    /// 録画の時計で `ms` までに書いたはずの長さに足りなければ、足りない分を無音で埋める。
    /// PC の音（loopback）は無音の間データが届かないので、埋めないと音が詰まって映像とずれる。
    /// コールバックの間隔の揺れでは埋めないよう、50ms 以上の欠けだけを埋める。
    fn pad_to(&mut self, ms: u64) -> Result<()> {
        let rate = self.spec.sample_rate as u64;
        let target = ms * rate / 1000;
        let written = self.data_len as u64 / self.bytes_per_frame().max(1);
        if target > written + rate / 20 {
            let zeros = vec![0u8; ((target - written) * self.bytes_per_frame()) as usize];
            self.file.write_all(&zeros)?;
            self.data_len += zeros.len() as u32;
        }
        Ok(())
    }

    fn write_sample<S: Sample>(&mut self, sample: S) -> Result<()> {
        sample.write(&mut self.file)?;
        self.data_len += std::mem::size_of::<S>() as u32;
        Ok(())
    }

    fn finalize(mut self) -> Result<()> {
        use std::io::Seek;
        let file_size = 36 + self.data_len;

        self.file.seek(std::io::SeekFrom::Start(4))?;
        self.file.write_all(&file_size.to_le_bytes())?;

        self.file.seek(std::io::SeekFrom::Start(40))?;
        self.file.write_all(&self.data_len.to_le_bytes())?;

        self.file.flush()?;
        Ok(())
    }
}

trait Sample {
    fn write<W: Write>(&self, writer: &mut W) -> Result<()>;
}

impl Sample for f32 {
    fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_all(&self.to_le_bytes())?;
        Ok(())
    }
}

impl Sample for i16 {
    fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_all(&self.to_le_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_len(path: &Path) -> u64 {
        std::fs::metadata(path).unwrap().len() - 44
    }

    #[test]
    fn test_pad_to_fills_gaps_with_silence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let spec = wav_spec::WavSpec {
            channels: 2,
            sample_rate: 1000,
            bits_per_sample: 16,
            sample_format: wav_spec::SampleFormat::Int,
        };
        let mut w = hound_writer(&path, spec).unwrap();
        // 届いた音が無いまま 500ms 経ったら、500 フレーム分の無音を書く
        w.pad_to(500).unwrap();
        assert_eq!(w.data_len, 500 * 4);
        // 50ms 未満の欠けはコールバックの揺れとみなして埋めない
        w.pad_to(540).unwrap();
        assert_eq!(w.data_len, 500 * 4);
        // 書いた長さより前の時刻なら何もしない
        w.pad_to(100).unwrap();
        assert_eq!(w.data_len, 500 * 4);
        w.finalize().unwrap();
        assert_eq!(read_len(&path), 500 * 4);
    }
}
