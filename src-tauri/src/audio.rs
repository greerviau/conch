use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
pub const MAX_SECONDS: usize = 180;
pub const RATE: u32 = 16_000;
pub struct Captured {
    pub wav: Vec<u8>,
    pub samples: Vec<f32>,
}
/// Records until `stop` fires. `observe` sees the whole recording so far about
/// every 100 ms while the audio callback waits, so it must return quickly.
pub fn capture(
    stop: mpsc::Receiver<bool>,
    ready: impl FnOnce(),
    level: Arc<std::sync::atomic::AtomicU32>,
    mut observe: impl FnMut(&[f32]),
) -> Result<Option<Captured>, String> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("No microphone is available")?;
    let config = device
        .default_input_config()
        .map_err(|_| "Microphone permission or format is unavailable")?;
    let channels = config.channels() as usize;
    let rate = config.sample_rate() as f64;
    let buffer = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
        RATE as usize * MAX_SECONDS,
    )));
    let error = Arc::new(Mutex::new(false));
    let make = |format| -> Result<cpal::Stream, String> {
        let error = error.clone();
        let on_error = move |_| {
            *error.lock().unwrap() = true;
        };
        match format {
            cpal::SampleFormat::F32 => build::<f32>(
                &device,
                &config.clone().into(),
                buffer.clone(),
                channels,
                rate,
                on_error,
                level.clone(),
            ),
            cpal::SampleFormat::I16 => build::<i16>(
                &device,
                &config.clone().into(),
                buffer.clone(),
                channels,
                rate,
                on_error,
                level.clone(),
            ),
            cpal::SampleFormat::U16 => build::<u16>(
                &device,
                &config.clone().into(),
                buffer.clone(),
                channels,
                rate,
                on_error,
                level.clone(),
            ),
            _ => Err("Microphone sample format is unsupported".into()),
        }
    };
    let stream = make(config.sample_format())?;
    stream
        .play()
        .map_err(|_| "Cannot start microphone; check system permission")?;
    ready();
    let start = Instant::now();
    let keep = loop {
        if *error.lock().unwrap() {
            return Err("Microphone capture failed".into());
        }
        if start.elapsed().as_secs() >= MAX_SECONDS as u64 {
            break true;
        }
        observe(&buffer.lock().unwrap());
        match stop.recv_timeout(Duration::from_millis(100)) {
            Ok(keep) => break keep,
            Err(mpsc::RecvTimeoutError::Disconnected) => break false,
            Err(mpsc::RecvTimeoutError::Timeout) => (),
        }
    };
    drop(stream);
    if !keep {
        return Ok(None);
    }
    let mut buffer = buffer.lock().unwrap();
    if buffer.len() < RATE as usize / 5 {
        return Err("Recording is too short; record again".into());
    }
    Ok(Some(Captured {
        wav: wav(&buffer)?,
        samples: std::mem::take(&mut *buffer),
    }))
}
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    channels: usize,
    rate: f64,
    on_error: impl FnMut(cpal::StreamError) + Send + 'static,
    level: Arc<std::sync::atomic::AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    use cpal::Sample;
    let mut phase = 0.0;
    let mut sum = 0.0;
    let mut count = 0usize;
    device
        .build_input_stream(
            config,
            move |samples: &[T], _| {
                let mut out = buffer.lock().unwrap();
                let mut peak = 0.0f32;
                for frame in samples.chunks_exact(channels) {
                    sum +=
                        frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / channels as f32;
                    count += 1;
                    phase += RATE as f64;
                    if phase >= rate {
                        phase -= rate;
                        if out.len() < RATE as usize * MAX_SECONDS {
                            let sample = sum / count as f32;
                            peak = peak.max(sample.abs());
                            out.push(sample);
                        }
                        sum = 0.0;
                        count = 0;
                    }
                }
                level.store(
                    (peak * 6.0).min(1.0).to_bits(),
                    std::sync::atomic::Ordering::Relaxed,
                );
            },
            on_error,
            None,
        )
        .map_err(|_| "Cannot open microphone".into())
}
pub fn wav(samples: &[f32]) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(vec![]);
    {
        let mut writer = hound::WavWriter::new(
            &mut cursor,
            hound::WavSpec {
                channels: 1,
                sample_rate: RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .map_err(|_| "Cannot encode recording")?;
        for sample in samples {
            writer
                .write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .map_err(|_| "Cannot encode recording")?;
        }
        writer.finalize().map_err(|_| "Cannot finish recording")?;
    }
    Ok(cursor.into_inner())
}
