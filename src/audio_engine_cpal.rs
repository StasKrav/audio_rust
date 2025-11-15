// audio_engine_cpal.rs
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, SampleFormat, SampleRate, StreamConfig,
};
use rubato::{
    Resampler, SincFixedIn, WindowFunction,
};
use symphonia::core::{
    audio::{AudioBufferRef, Signal},
    codecs::DecoderOptions,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

pub struct HighQualityPlayer {
    stream: Option<cpal::Stream>,
    device: cpal::Device,
    config: StreamConfig,
    audio_data: Arc<Mutex<AudioData>>,
    sample_rate: u32,
}

struct AudioData {
    samples: VecDeque<f32>,
    is_playing: bool,
    volume: f32,
    needs_resample: bool,
    target_sample_rate: u32,
}

pub struct CpalSymphoniaSource {
    path: std::path::PathBuf,
    duration: Option<Duration>,
    sample_rate: u32,
    channels: usize,
}

impl CpalSymphoniaSource {
    pub fn new(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let file = std::fs::File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            hint.with_extension(extension);
        }
        
        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();
        
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)?;
        
        let format = probed.format;
        let track = format
            .default_track()
            .ok_or("No audio track found")?;
        
        let _decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())?;
        
        // Получаем информацию о треке
        let codec_params = &track.codec_params;
        let sample_rate = codec_params.sample_rate.unwrap_or(44100);
        let channels = codec_params.channels.unwrap_or(symphonia::core::audio::Channels::FRONT_LEFT | symphonia::core::audio::Channels::FRONT_RIGHT).count();
        
        // Вычисляем длительность
        let duration = if let (Some(n_frames), Some(rate)) = (codec_params.n_frames, codec_params.sample_rate) {
            Some(Duration::from_secs_f64(n_frames as f64 / rate as f64))
        } else {
            None
        };
        
        Ok(Self {
            path: path.to_path_buf(),
            duration,
            sample_rate,
            channels,
        })
    }
    
    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }
    
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    
    pub fn channels(&self) -> usize {
        self.channels
    }
    
    pub fn decode_to_buffer(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let file = std::fs::File::open(&self.path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        
        let mut hint = Hint::new();
        if let Some(extension) = self.path.extension().and_then(|ext| ext.to_str()) {
            hint.with_extension(extension);
        }
        
        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();
        
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)?;
        
        let mut format = probed.format;
        let track = format.default_track().ok_or("No audio track found")?;
        
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())?;
        
        let mut samples = Vec::new();
        
        // Декодируем весь файл в память для высококачественного воспроизведения
        while let Ok(packet) = format.next_packet() {
            match decoder.decode(&packet) {
                Ok(decoded) => {
                    match decoded {
                        AudioBufferRef::F32(buf) => {
                            for frame in buf.chan(0) {
                                samples.push(*frame);
                            }
                            if buf.spec().channels.count() > 1 {
                                for frame in buf.chan(1) {
                                    samples.push(*frame);
                                }
                            }
                        }
                        AudioBufferRef::S16(buf) => {
                            for frame in buf.chan(0) {
                                samples.push(*frame as f32 / 32768.0);
                            }
                            if buf.spec().channels.count() > 1 {
                                for frame in buf.chan(1) {
                                    samples.push(*frame as f32 / 32768.0);
                                }
                            }
                        }
                        AudioBufferRef::S24(buf) => {
                            // Исправляем конвертацию i24 - используем прямой доступ к байтам
                            for frame in buf.chan(0) {
                                // Конвертируем i24 в i32, затем в f32
                                let bytes = frame.to_ne_bytes();
                                let sample_i32 = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]);
                                samples.push(sample_i32 as f32 / 8388608.0);
                            }
                            if buf.spec().channels.count() > 1 {
                                for frame in buf.chan(1) {
                                    let bytes = frame.to_ne_bytes();
                                    let sample_i32 = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]);
                                    samples.push(sample_i32 as f32 / 8388608.0);
                                }
                            }
                        }
                        AudioBufferRef::S32(buf) => {
                            for frame in buf.chan(0) {
                                samples.push(*frame as f32 / 2147483648.0);
                            }
                            if buf.spec().channels.count() > 1 {
                                for frame in buf.chan(1) {
                                    samples.push(*frame as f32 / 2147483648.0);
                                }
                            }
                        }
                        _ => {
                            // Конвертируем другие форматы
                            let converted = decoded.make_equivalent::<f32>();
                            for frame in converted.chan(0) {
                                samples.push(*frame);
                            }
                            if converted.spec().channels.count() > 1 {
                                for frame in converted.chan(1) {
                                    samples.push(*frame);
                                }
                            }
                        }
                    }
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) => {
                    // Пропускаем битые пакеты, продолжаем декодирование
                    continue;
                }
                Err(e) => return Err(Box::new(e)),
            }
        }
        
        Ok(samples)
    }
}

impl HighQualityPlayer {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("No output device available")?;
        
        // Пытаемся получить поддержку высоких sample rates
        let mut supported_configs = device.supported_output_configs()?;
        
        let mut best_config = None;
        while let Some(config) = supported_configs.next() {
            if config.sample_format() == SampleFormat::F32 {
                // Предпочитаем 96kHz или 192kHz для Hi-Res
                if config.max_sample_rate().0 >= 96000 {
                    best_config = Some(config);
                    break;
                } else if best_config.is_none() {
                    best_config = Some(config);
                }
            }
        }
        
        let config_range = best_config
            .ok_or("No supported F32 config found")?
            .with_sample_rate(SampleRate(96000)); // Стараемся использовать 96kHz
        
        let config = StreamConfig {
            channels: 2, // Стерео
            sample_rate: config_range.sample_rate(),
            buffer_size: BufferSize::Fixed(4096), // Увеличенный буфер для качества
        };
        
        let sample_rate = config.sample_rate.0; // Сохраняем до перемещения config
        
        let audio_data = Arc::new(Mutex::new(AudioData {
            samples: VecDeque::new(),
            is_playing: false,
            volume: 1.0,
            needs_resample: false,
            target_sample_rate: sample_rate,
        }));
        
        Ok(Self {
            stream: None,
            device,
            config,
            audio_data: audio_data.clone(),
            sample_rate,
        })
    }
    
    pub fn play_source(&mut self, source: &CpalSymphoniaSource) -> Result<(), Box<dyn std::error::Error>> {
        self.stop();
        
        // Декодируем аудио в высоком качестве
        let samples = source.decode_to_buffer()?;
        
        let mut audio_data = self.audio_data.lock().unwrap();
        audio_data.samples.clear();
        
        // Ресемплинг если нужно
        let final_samples = if source.sample_rate() != self.sample_rate {
            self.resample_audio(&samples, source.sample_rate(), source.channels())?
        } else {
            samples
        };
        
        for sample in final_samples {
            audio_data.samples.push_back(sample);
        }
        
        audio_data.is_playing = true;
        audio_data.needs_resample = source.sample_rate() != self.sample_rate;
        audio_data.target_sample_rate = self.sample_rate;
        
        drop(audio_data);
        
        // Создаем аудио поток
        let audio_data_clone = self.audio_data.clone();
        let err_fn = |err| eprintln!("Audio stream error: {}", err);
        
        let stream = self.device.build_output_stream(
            &self.config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                Self::audio_callback(data, &audio_data_clone);
            },
            err_fn,
            None, // None timeout
        )?;
        
        stream.play()?;
        self.stream = Some(stream);
        
        Ok(())
    }
    
    fn audio_callback(data: &mut [f32], audio_data: &Arc<Mutex<AudioData>>) {
        let mut audio_data = audio_data.lock().unwrap();
        
        if !audio_data.is_playing {
            data.fill(0.0);
            return;
        }
        
        for sample in data.iter_mut() {
            if let Some(next_sample) = audio_data.samples.pop_front() {
                *sample = next_sample * audio_data.volume;
            } else {
                *sample = 0.0;
                audio_data.is_playing = false;
            }
        }
    }
    
    fn resample_audio(
        &self,
        samples: &[f32],
        source_rate: u32,
        channels: usize,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        if source_rate == self.sample_rate {
            return Ok(samples.to_vec());
        }
        
        // Используем высококачественный ресемплер
        let mut resampler = SincFixedIn::<f32>::new(
            self.sample_rate as f64 / source_rate as f64,
            2.0, // ratio
            rubato::SincInterpolationParameters {
                sinc_len: 256,
                f_cutoff: 0.95,
                interpolation: rubato::SincInterpolationType::Linear,
                window: WindowFunction::BlackmanHarris2,
                oversampling_factor: 256,
            },
            samples.len() / channels,
            channels,
        )?;
        
        // Преобразуем в формат для ресемплера
        let input: Vec<Vec<f32>> = (0..channels)
            .map(|ch| {
                samples
                    .chunks(channels)
                    .map(|frame| frame[ch])
                    .collect()
            })
            .collect();
        
        let output = resampler.process(&input, None)?;
        
        // Преобразуем обратно в интерливированный формат
        let mut result = Vec::with_capacity(output[0].len() * channels);
        for i in 0..output[0].len() {
            for ch in 0..channels {
                result.push(output[ch][i]);
            }
        }
        
        Ok(result)
    }
    
    pub fn pause(&mut self) {
        if let Some(stream) = &self.stream {
            let _ = stream.pause();
        }
        let mut audio_data = self.audio_data.lock().unwrap();
        audio_data.is_playing = false;
    }
    
    pub fn resume(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(stream) = &self.stream {
            stream.play()?;
        }
        let mut audio_data = self.audio_data.lock().unwrap();
        audio_data.is_playing = true;
        Ok(())
    }
    
    pub fn stop(&mut self) {
        if let Some(stream) = &self.stream {
            let _ = stream.pause();
        }
        let mut audio_data = self.audio_data.lock().unwrap();
        audio_data.is_playing = false;
        audio_data.samples.clear();
    }
    
    pub fn set_volume(&mut self, volume: f32) {
        let mut audio_data = self.audio_data.lock().unwrap();
        audio_data.volume = volume.max(0.0).min(1.0);
    }
    
    pub fn get_volume(&self) -> f32 {
        let audio_data = self.audio_data.lock().unwrap();
        audio_data.volume
    }
    
    pub fn is_playing(&self) -> bool {
        let audio_data = self.audio_data.lock().unwrap();
        audio_data.is_playing
    }
    
    pub fn samples_remaining(&self) -> usize {
        let audio_data = self.audio_data.lock().unwrap();
        audio_data.samples.len()
    }
}
