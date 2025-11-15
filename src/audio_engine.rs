use rodio::Source;
use std::fs::File;
use std::path::Path;
use std::time::Duration;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct AudioDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    current_frame: u64,
}

impl AudioDecoder {
    pub fn new(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let hint = Hint::new();
        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();

        let probed =
            symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts)?;

        let format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("No audio track found")?;

        let track_id = track.id;
        let decoder_opts = DecoderOptions::default();

        let decoder = symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts)?;

        Ok(AudioDecoder {
            format,
            decoder,
            track_id,
            current_frame: 0,
        })
    }

    // pub fn seek(&mut self, time: Duration) -> Result<(), Error> {
    //     let seek_to = SeekTo::Time {
    //         time: Time::from(time),
    //         track_id: Some(self.track_id),
    //     };
    //     let seeked_to = self.format.seek(SeekMode::Accurate, seek_to)?;
    //     self.current_frame = seeked_to.actual_ts;
    //     Ok(())
    // }

    pub fn decode_next(&mut self) -> Result<Option<AudioBufferRef<'_>>, Error> {
        let packet = self.format.next_packet()?;

        if packet.track_id() == self.track_id {
            match self.decoder.decode(&packet)? {
                decoded => {
                    self.current_frame = packet.ts();
                    Ok(Some(decoded))
                }
            }
        } else {
            // Пропускаем пакеты других дорожек и декодируем следующий
            self.decode_next()
        }
    }

    pub fn duration(&self) -> Option<Duration> {
        let track = self
            .format
            .tracks()
            .iter()
            .find(|t| t.id == self.track_id)?;
        let time_base = track.codec_params.time_base?;
        let n_frames = track.codec_params.n_frames?;

        let time = time_base.calc_time(n_frames);
        Some(Duration::from_secs_f64(time.seconds as f64 + time.frac))
    }

    //     pub fn current_time(&self) -> Option<Duration> {
    //         let track = self
    //             .format
    //             .tracks()
    //             .iter()
    //             .find(|t| t.id == self.track_id)?;
    //         let time_base = track.codec_params.time_base?;
    //
    //         let time = time_base.calc_time(self.current_frame);
    //         Some(Duration::from_secs_f64(time.seconds as f64 + time.frac))
    //     }
}

// Адаптер для преобразования Symphonia AudioBuffer в Rodio Source
pub struct SymphoniaSource {
    decoder: AudioDecoder,
    current_buffer: Option<AudioBufferRef<'static>>,
    buffer_pos: usize,
    sample_rate: u32,
    channels: u16,
}

impl SymphoniaSource {
    pub fn new(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let decoder = AudioDecoder::new(path)?;

        // Получаем информацию о формате
        let sample_rate = decoder
            .decoder
            .codec_params()
            .sample_rate
            .ok_or("No sample rate")?;
        let channels = decoder
            .decoder
            .codec_params()
            .channels
            .ok_or("No channels info")?
            .count() as u16;

        Ok(SymphoniaSource {
            decoder,
            current_buffer: None,
            buffer_pos: 0,
            sample_rate,
            channels,
        })
    }

    fn fill_buffer(&mut self) -> Result<bool, Error> {
        if self.current_buffer.is_none() || self.buffer_pos >= self.get_buffer_len() {
            match self.decoder.decode_next()? {
                Some(buffer) => {
                    // Временное решение - преобразуем в 'static
                    let buffer = unsafe { std::mem::transmute(buffer) };
                    self.current_buffer = Some(buffer);
                    self.buffer_pos = 0;
                    Ok(true)
                }
                None => Ok(false), // Конец потока
            }
        } else {
            Ok(true)
        }
    }

    fn get_buffer_len(&self) -> usize {
        self.current_buffer
            .as_ref()
            .map(|b| b.frames() * b.spec().channels.count())
            .unwrap_or(0)
    }

    // ДОБАВЛЯЕМ ГЕТТЕРЫ
    //     pub fn decoder(&self) -> &AudioDecoder {
    //         &self.decoder
    //     }
    //
    //     pub fn decoder_mut(&mut self) -> &mut AudioDecoder {
    //         &mut self.decoder
    //     }
    //
    //     // Удобные методы для часто используемых операций
    //     pub fn current_time(&self) -> Option<Duration> {
    //         self.decoder.current_time()
    //     }

    pub fn duration(&self) -> Option<Duration> {
        self.decoder.duration()
    }

    // pub fn seek(&mut self, time: Duration) -> Result<(), Error> {
    //     self.decoder.seek(time)
    // }
}

impl Iterator for SymphoniaSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if !self.fill_buffer().ok()? {
            return None;
        }

        let buffer = self.current_buffer.as_ref()?;
        let channels = buffer.spec().channels.count();

        if self.buffer_pos >= buffer.frames() * channels {
            self.current_buffer = None;
            return self.next();
        }

        let frame = self.buffer_pos / channels;
        let channel = self.buffer_pos % channels;

        let sample = match buffer {
            AudioBufferRef::F32(buf) => buf.chan(channel)[frame],
            AudioBufferRef::S16(buf) => buf.chan(channel)[frame] as f32 / i16::MAX as f32,
            AudioBufferRef::S24(buf) => {
                // Для S24 используем ручное преобразование
                let sample_i24 = buf.chan(channel)[frame];
                // Преобразуем i24 в i32 (i24 хранится в младших 24 битах i32)
                let sample_i32 = sample_i24.0 as i32;
                sample_i32 as f32 / 8_388_607.0 // 2^23 - 1
            }
            AudioBufferRef::S32(buf) => buf.chan(channel)[frame] as f32 / i32::MAX as f32,
            AudioBufferRef::U8(buf) => (buf.chan(channel)[frame] as f32 - 128.0) / 128.0,
            _ => 0.0,
        };

        self.buffer_pos += 1;
        Some(sample)
    }
}

impl Source for SymphoniaSource {
    fn current_frame_len(&self) -> Option<usize> {
        None // Поток неизвестной длины
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.decoder.duration()
    }
}
