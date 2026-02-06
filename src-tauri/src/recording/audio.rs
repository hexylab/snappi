use anyhow::Result;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub fn capture_audio(
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    output_dir: &Path,
) -> Result<()> {
    let audio_path = output_dir.join("audio.wav");
    log::info!("Audio capture thread started");

    // Use cpal to capture system audio via WASAPI loopback
    let host = cpal::traits::HostTrait::default_output_device(
        &cpal::default_host()
    );

    let device = match host {
        Some(dev) => dev,
        None => {
            log::warn!("No audio output device found, skipping audio capture");
            return Ok(());
        }
    };

    use cpal::traits::DeviceTrait;
    let config = match device.default_output_config() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to get audio config: {}", e);
            return Ok(());
        }
    };

    let spec = hound_spec_from_config(&config);
    let writer = Arc::new(std::sync::Mutex::new(
        hound_writer(&audio_path, spec)?
    ));

    let writer_clone = writer.clone();
    let running = is_running.clone();
    let paused = is_paused.clone();

    let err_fn = |err: cpal::StreamError| {
        log::error!("Audio stream error: {}", err);
    };

    use cpal::traits::StreamTrait;
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            use cpal::traits::DeviceTrait;
            device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if !running.load(Ordering::SeqCst) || paused.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Ok(mut writer) = writer_clone.lock() {
                        for &sample in data {
                            let _ = writer.write_sample(sample);
                        }
                    }
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            use cpal::traits::DeviceTrait;
            device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if !running.load(Ordering::SeqCst) || paused.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Ok(mut writer) = writer_clone.lock() {
                        for &sample in data {
                            let _ = writer.write_sample(sample);
                        }
                    }
                },
                err_fn,
                None,
            )?
        }
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
        if let Ok(writer) = writer.into_inner() {
            let _ = writer.finalize();
        }
    }

    log::info!("Audio capture stopped");
    Ok(())
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
