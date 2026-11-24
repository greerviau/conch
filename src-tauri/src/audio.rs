use crate::{lock::Locked, log};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
pub const MAX_SECONDS: usize = 180;
pub const RATE: u32 = 16_000;
pub struct Captured {
    pub wav: Vec<u8>,
    pub samples: Vec<f32>,
}
fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|description| description.name().to_owned())
        .unwrap_or_default()
}
/// Names of the input devices the system offers, without duplicates.
pub fn input_names() -> Vec<String> {
    let mut names: Vec<String> = cpal::default_host()
        .input_devices()
        .map(|devices| devices.map(|device| device_name(&device)).collect())
        .unwrap_or_default();
    names.retain(|name| !name.is_empty());
    let mut seen = std::collections::HashSet::new();
    names.retain(|name| seen.insert(name.clone()));
    names
}
/// Index of the device `wanted` names. An empty or unplugged name yields
/// `None`, which means the system input.
fn pick_input(available: &[String], wanted: &str) -> Option<usize> {
    if wanted.is_empty() {
        return None;
    }
    available.iter().position(|name| name == wanted)
}
/// The device for `wanted`, and whether it is the device `wanted` asks for.
/// An empty or unplugged name yields the system input; only the unplugged
/// name yields `false`.
fn input_device(wanted: &str) -> Result<(cpal::Device, bool), String> {
    let host = cpal::default_host();
    if !wanted.is_empty() {
        let devices: Vec<cpal::Device> = host
            .input_devices()
            .map(|devices| devices.collect())
            .unwrap_or_default();
        let names: Vec<String> = devices.iter().map(device_name).collect();
        if let Some(index) = pick_input(&names, wanted) {
            let device = devices.into_iter().nth(index).expect("index from names");
            return Ok((device, true));
        }
    }
    host.default_input_device()
        .map(|device| (device, wanted.is_empty()))
        .ok_or_else(|| "No microphone is available".to_string())
}
fn system_input() -> Option<cpal::DeviceId> {
    cpal::default_host()
        .default_input_device()
        .and_then(|device| device.id().ok())
}
/// Whether the input stays open and paused between recordings. On macOS,
/// opening it took 250 ms to over a second, most of it spent finding a chosen
/// device among the inputs, and words spoken in that time were lost. A paused
/// stream starts again in under 100 ms, and the system shows the microphone
/// as off while it is paused. Other systems are untested with a paused
/// stream, so they open the input for each recording.
const KEEP: bool = cfg!(target_os = "macos");
/// The microphone input, kept open and paused between recordings where `KEEP` allows.
#[derive(Default)]
pub struct Microphone(Mutex<Slot>);
#[derive(Default)]
struct Slot {
    kept: Option<Open>,
    recording: bool,
}
/// An open input stream. Its callback feeds `sink` while a recording has one
/// installed and drops the audio otherwise.
struct Open {
    stream: cpal::Stream,
    sink: Arc<Mutex<Option<Sink>>>,
    failed: Arc<AtomicBool>,
    rate: f64,
    channels: usize,
    /// The Microphone setting the stream was opened for.
    wanted: String,
    /// The device, when the stream may be kept: it is the one `wanted` asks for.
    device: Option<cpal::DeviceId>,
}
impl Open {
    fn serves(&self, wanted: &str) -> bool {
        self.device.as_ref().is_some_and(|device| {
            let kept = KeptFor {
                wanted: self.wanted.clone(),
                device: device.clone(),
                failed: self.failed.load(Ordering::Relaxed),
            };
            reusable(&kept, wanted, system_input)
        })
    }
    /// Sends the audio to `buffer` from now on.
    fn listen(
        &self,
        buffer: &Arc<Mutex<Vec<f32>>>,
        level: &Arc<AtomicU32>,
        heard: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        *self.sink.locked() = Some(Sink {
            buffer: buffer.clone(),
            resampler: Resampler::new(self.rate),
            channels: self.channels,
            level: level.clone(),
            heard: heard.clone(),
        });
        self.stream
            .play()
            .map_err(|_| "Cannot start microphone; check system permission".into())
    }
}
impl Microphone {
    /// Opens the input for `wanted` and pauses it, so the next recording starts
    /// without opening it. Does nothing during a recording or when the kept
    /// stream already serves `wanted`.
    pub fn prepare(&self, wanted: &str) {
        if !KEEP {
            return;
        }
        let mut slot = self.0.locked();
        if slot.recording || slot.kept.as_ref().is_some_and(|open| open.serves(wanted)) {
            return;
        }
        slot.kept = None;
        match open(wanted) {
            Ok(open) if open.device.is_some() && open.stream.pause().is_ok() => {
                slot.kept = Some(open);
            }
            Ok(_) => (),
            Err(error) => log::warn("audio", &format!("Cannot prepare the microphone: {error}")),
        }
    }
    /// Starts sending the input for `wanted` to `buffer`, from the kept stream
    /// when it still serves `wanted`.
    fn start(
        &self,
        wanted: &str,
        buffer: &Arc<Mutex<Vec<f32>>>,
        level: &Arc<AtomicU32>,
        heard: &Arc<AtomicBool>,
    ) -> Result<Lease<'_>, String> {
        let started = Instant::now();
        let kept = {
            let mut slot = self.0.locked();
            slot.recording = true;
            slot.kept.take()
        };
        let mut lease = Lease {
            microphone: self,
            open: None,
        };
        let reused = kept
            .filter(|open| open.serves(wanted))
            .filter(|open| open.listen(buffer, level, heard).is_ok());
        let how = if reused.is_some() {
            "kept open"
        } else {
            "opened"
        };
        let open = match reused {
            Some(open) => open,
            None => {
                let open = open(wanted)?;
                open.listen(buffer, level, heard)?;
                open
            }
        };
        lease.open = Some(open);
        log::info(
            "audio",
            &format!(
                "Microphone started in {} ms ({how})",
                started.elapsed().as_millis()
            ),
        );
        Ok(lease)
    }
}
/// A recording's use of the input. Dropping it keeps the stream, paused, for
/// the next recording where `KEEP` allows, and closes it otherwise.
struct Lease<'a> {
    microphone: &'a Microphone,
    open: Option<Open>,
}
impl Lease<'_> {
    fn open(&self) -> &Open {
        self.open.as_ref().expect("a lease holds a stream")
    }
}
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let mut slot = self.microphone.0.locked();
        slot.recording = false;
        let Some(open) = self.open.take() else {
            return;
        };
        open.sink.locked().take();
        let paused = KEEP && open.stream.pause().is_ok();
        if slot.kept.is_none()
            && kept_after(
                paused,
                open.failed.load(Ordering::Relaxed),
                open.device.is_some(),
            )
        {
            slot.kept = Some(open);
        }
    }
}
/// Whether a stream a recording used is kept for the next one. One that did
/// not pause would leave the microphone on between recordings.
fn kept_after(paused: bool, failed: bool, known_device: bool) -> bool {
    KEEP && paused && !failed && known_device
}
/// Opens the input for `wanted`. The stream may already be running.
fn open(wanted: &str) -> Result<Open, String> {
    let (device, exact) = input_device(wanted)?;
    let config = device
        .default_input_config()
        .map_err(|_| "Microphone permission or format is unavailable")?;
    let sink = Arc::new(Mutex::new(None));
    let failed = Arc::new(AtomicBool::new(false));
    let on_error = {
        let failed = failed.clone();
        move |_| failed.store(true, Ordering::Relaxed)
    };
    let stream_config = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &stream_config, sink.clone(), on_error),
        cpal::SampleFormat::I16 => build::<i16>(&device, &stream_config, sink.clone(), on_error),
        cpal::SampleFormat::U16 => build::<u16>(&device, &stream_config, sink.clone(), on_error),
        _ => Err("Microphone sample format is unsupported".into()),
    }?;
    Ok(Open {
        stream,
        sink,
        failed,
        rate: config.sample_rate() as f64,
        channels: config.channels() as usize,
        wanted: wanted.into(),
        device: if exact { device.id().ok() } else { None },
    })
}
/// Records until `stop` fires. `ready` runs when the first audio that is not
/// silence arrives: a Bluetooth headset sends silence for up to a second after
/// its microphone starts. `observe` sees the whole recording so far about
/// every 100 ms while the audio callback waits, so it must return quickly.
pub fn capture(
    microphone: &Microphone,
    wanted: &str,
    stop: mpsc::Receiver<bool>,
    ready: impl FnOnce(),
    level: Arc<AtomicU32>,
    mut observe: impl FnMut(&[f32]),
) -> Result<Option<Captured>, String> {
    let buffer = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
        RATE as usize * MAX_SECONDS,
    )));
    let heard = Arc::new(AtomicBool::new(false));
    let started = Instant::now();
    let lease = microphone.start(wanted, &buffer, &level, &heard)?;
    let keep = record(
        &stop,
        &lease.open().failed,
        &heard,
        &buffer,
        || {
            log::info(
                "audio",
                &format!("First audio after {} ms", started.elapsed().as_millis()),
            );
            ready();
        },
        &mut observe,
    )?;
    drop(lease);
    if !heard.load(Ordering::Relaxed) {
        log::warn("audio", "The microphone sent only silence");
    }
    if !keep {
        return Ok(None);
    }
    let mut buffer = buffer.locked();
    if buffer.len() < RATE as usize / 5 {
        return Err("Recording is too short; record again".into());
    }
    Ok(Some(Captured {
        wav: wav(&buffer)?,
        samples: std::mem::take(&mut *buffer),
    }))
}
/// Waits for `stop`, showing `observe` the recording about every 100 ms, and
/// runs `ready` within a few milliseconds of `heard`. Returns whether to keep
/// the recording.
fn record(
    stop: &mpsc::Receiver<bool>,
    failed: &AtomicBool,
    heard: &AtomicBool,
    buffer: &Mutex<Vec<f32>>,
    ready: impl FnOnce(),
    mut observe: impl FnMut(&[f32]),
) -> Result<bool, String> {
    let mut ready = Some(ready);
    let start = Instant::now();
    let mut observed: Option<Instant> = None;
    loop {
        if failed.load(Ordering::Relaxed) {
            return Err("Microphone capture failed".into());
        }
        if start.elapsed().as_secs() >= MAX_SECONDS as u64 {
            return Ok(true);
        }
        if heard.load(Ordering::Relaxed)
            && let Some(ready) = ready.take()
        {
            ready();
        }
        if observed.is_none_or(|at| at.elapsed() >= Duration::from_millis(100)) {
            observe(&buffer.locked());
            observed = Some(Instant::now());
        }
        // Until the first audio, polls often so the indicator changes as it arrives.
        let wait = if ready.is_some() { 5 } else { 100 };
        match stop.recv_timeout(Duration::from_millis(wait)) {
            Ok(keep) => return Ok(keep),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(false),
            Err(mpsc::RecvTimeoutError::Timeout) => (),
        }
    }
}
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sink: Arc<Mutex<Option<Sink>>>,
    on_error: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |samples: &[T], _| deliver(&sink, samples),
            on_error,
            None,
        )
        .map_err(|_| "Cannot open microphone".into())
}
/// Where a recording's audio goes.
struct Sink {
    buffer: Arc<Mutex<Vec<f32>>>,
    resampler: Resampler,
    channels: usize,
    level: Arc<AtomicU32>,
    /// Set by the first audio that is not silence.
    heard: Arc<AtomicBool>,
}
/// Mixes `samples` to mono, resamples them into the recording's buffer, and
/// sets the level. Without a recording the audio is dropped.
fn deliver<T>(sink: &Mutex<Option<Sink>>, samples: &[T])
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    use cpal::Sample;
    let mut sink = sink.locked();
    let Some(Sink {
        buffer,
        resampler,
        channels,
        level,
        heard,
    }) = sink.as_mut()
    else {
        return;
    };
    let mut out = buffer.locked();
    let mut peak = 0.0f32;
    for frame in samples.chunks_exact(*channels) {
        let mono = frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / *channels as f32;
        resampler.push(mono, |sample| {
            if out.len() < RATE as usize * MAX_SECONDS {
                peak = peak.max(sample.abs());
                out.push(sample);
            }
        });
    }
    level.store((peak * 6.0).min(1.0).to_bits(), Ordering::Relaxed);
    if peak > 0.0 {
        heard.store(true, Ordering::Relaxed);
    }
}
/// Converts the microphone's mono frames at its own rate to samples at `RATE`.
struct Resampler {
    rate: f64,
    phase: f64,
    sum: f32,
    count: usize,
}
impl Resampler {
    fn new(rate: f64) -> Self {
        Self {
            rate,
            phase: 0.0,
            sum: 0.0,
            count: 0,
        }
    }
    /// Takes one frame and passes each sample that is now due to `emit`: the mean
    /// of the frames since the last one. A microphone slower than `RATE` owes more
    /// than one sample for a frame, and the frame is repeated for each.
    fn push(&mut self, frame: f32, mut emit: impl FnMut(f32)) {
        self.sum += frame;
        self.count += 1;
        self.phase += RATE as f64;
        if self.phase >= self.rate {
            let sample = self.sum / self.count as f32;
            while self.phase >= self.rate {
                self.phase -= self.rate;
                emit(sample);
            }
            self.sum = 0.0;
            self.count = 0;
        }
    }
}
/// A kept stream, as `reusable` sees it.
struct KeptFor {
    wanted: String,
    device: cpal::DeviceId,
    failed: bool,
}
/// Whether a kept stream can record for the Microphone setting `wanted`. A
/// chosen device keeps its identity while connected; a disconnection fails
/// the stream. The system input can change at any time, so `system` is asked.
fn reusable(kept: &KeptFor, wanted: &str, system: impl FnOnce() -> Option<cpal::DeviceId>) -> bool {
    !kept.failed
        && kept.wanted == wanted
        && (!wanted.is_empty() || system().as_ref() == Some(&kept.device))
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

#[cfg(test)]
mod tests {
    use super::*;
    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| (*n).into()).collect()
    }
    /// One second of a ramp from a microphone at `rate`, resampled.
    fn resampled(rate: usize) -> Vec<f32> {
        let mut resampler = Resampler::new(rate as f64);
        let mut out = vec![];
        for frame in 0..rate {
            resampler.push(frame as f32 / rate as f32, |sample| out.push(sample));
        }
        out
    }
    #[test]
    fn one_second_from_any_microphone_rate_becomes_one_second_at_the_recognizer_rate() {
        for rate in [8_000, 11_025, 16_000, 22_050, 44_100, 48_000, 96_000] {
            let out = resampled(rate);
            assert!(
                out.len().abs_diff(RATE as usize) <= 1,
                "{rate} Hz gave {} samples",
                out.len()
            );
            // The ramp still rises from start to end, so no audio was dropped or reordered.
            assert!(out.windows(2).all(|pair| pair[0] <= pair[1]));
            assert!(out[0] < 0.01 && out[out.len() - 1] > 0.99);
        }
    }
    #[test]
    fn a_faster_microphone_is_averaged_down_exactly_as_before() {
        let mut resampler = Resampler::new(48_000.0);
        let mut out = vec![];
        for frame in [0.0, 0.3, 0.6, 1.0, 1.0, 1.0] {
            resampler.push(frame, |sample| out.push(sample));
        }
        assert_eq!(out.len(), 2);
        assert!((out[0] - 0.3).abs() < 1e-6 && (out[1] - 1.0).abs() < 1e-6);
    }
    #[test]
    fn an_empty_choice_follows_the_system_input() {
        assert_eq!(pick_input(&names(&["Built-in", "USB"]), ""), None);
    }
    #[test]
    fn a_named_microphone_is_found_by_its_exact_name() {
        let available = names(&["Built-in", "USB", "USB 2"]);
        assert_eq!(pick_input(&available, "USB"), Some(1));
        assert_eq!(pick_input(&available, "USB 2"), Some(2));
    }
    #[test]
    fn an_unplugged_microphone_falls_back_to_the_system_input() {
        assert_eq!(pick_input(&names(&["Built-in"]), "USB"), None);
    }
    fn device(name: &str) -> cpal::DeviceId {
        cpal::DeviceId(cpal::default_host().id(), name.into())
    }
    fn kept(wanted: &str, name: &str) -> KeptFor {
        KeptFor {
            wanted: wanted.into(),
            device: device(name),
            failed: false,
        }
    }
    #[test]
    fn a_kept_chosen_microphone_serves_the_next_recording_without_finding_it_again() {
        let kept = kept("USB", "usb-uid");
        assert!(reusable(&kept, "USB", || panic!(
            "the system input is not asked"
        )));
        assert!(!reusable(&kept, "Built-in", || Some(device("usb-uid"))));
        assert!(!reusable(&kept, "", || Some(device("usb-uid"))));
    }
    #[test]
    fn a_kept_system_input_is_opened_again_after_the_system_switches_input() {
        let kept = kept("", "built-in-uid");
        assert!(reusable(&kept, "", || Some(device("built-in-uid"))));
        assert!(!reusable(&kept, "", || Some(device("usb-uid"))));
        assert!(!reusable(&kept, "", || None));
    }
    #[test]
    fn a_stream_kept_after_a_recording_must_be_paused_healthy_and_for_a_known_device() {
        assert_eq!(kept_after(true, false, true), KEEP);
        assert!(
            !kept_after(false, false, true),
            "a stream that did not pause would keep recording"
        );
        assert!(!kept_after(true, true, true));
        assert!(!kept_after(true, false, false));
    }
    #[test]
    fn a_stream_that_reported_an_error_is_never_reused() {
        let kept = KeptFor {
            failed: true,
            ..kept("USB", "usb-uid")
        };
        assert!(!reusable(&kept, "USB", || None));
    }
    #[test]
    fn a_recording_receives_mono_frames_and_audio_between_recordings_is_discarded() {
        let slot = Mutex::new(None);
        deliver(&slot, &[0.5f32, 0.5]);
        let buffer = Arc::new(Mutex::new(vec![]));
        let level = Arc::new(std::sync::atomic::AtomicU32::new(0));
        *slot.locked() = Some(Sink {
            buffer: buffer.clone(),
            resampler: Resampler::new(RATE as f64),
            channels: 2,
            level: level.clone(),
            heard: Arc::new(AtomicBool::new(false)),
        });
        deliver(&slot, &[0.2f32, 0.4, -0.1, -0.3]);
        let got = buffer.locked().clone();
        assert_eq!(got.len(), 2);
        assert!((got[0] - 0.3).abs() < 1e-6 && (got[1] + 0.2).abs() < 1e-6);
        let peak = f32::from_bits(level.load(std::sync::atomic::Ordering::Relaxed));
        assert!((peak - 1.0).abs() < 1e-6, "level {peak}");
    }
    #[test]
    fn silence_before_the_first_audio_is_not_heard() {
        let heard = Arc::new(AtomicBool::new(false));
        let slot = Mutex::new(Some(Sink {
            buffer: Arc::new(Mutex::new(vec![])),
            resampler: Resampler::new(RATE as f64),
            channels: 1,
            level: Arc::new(AtomicU32::new(0)),
            heard: heard.clone(),
        }));
        deliver(&slot, &[0.0f32; 160]);
        assert!(!heard.load(Ordering::Relaxed), "silence was taken as audio");
        deliver(&slot, &[0.001f32; 160]);
        assert!(heard.load(Ordering::Relaxed));
        deliver(&slot, &[0.0f32; 160]);
        assert!(
            heard.load(Ordering::Relaxed),
            "later silence undid the first audio"
        );
    }
    /// A `record` running on its own thread.
    struct Recording {
        stop: mpsc::Sender<bool>,
        /// Stands for the first audio arriving.
        heard: Arc<AtomicBool>,
        /// Whether `ready` ran.
        ready: Arc<AtomicBool>,
        thread: std::thread::JoinHandle<Result<bool, String>>,
    }
    fn recording() -> Recording {
        let (stop, receive) = mpsc::channel();
        let heard = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(AtomicBool::new(false));
        let thread = {
            let (heard, ready) = (heard.clone(), ready.clone());
            std::thread::spawn(move || {
                record(
                    &receive,
                    &AtomicBool::new(false),
                    &heard,
                    &Mutex::new(vec![]),
                    || ready.store(true, Ordering::Relaxed),
                    |_| (),
                )
            })
        };
        Recording {
            stop,
            heard,
            ready,
            thread,
        }
    }
    #[test]
    fn a_recording_is_ready_only_once_the_first_audio_arrives() {
        let Recording {
            stop,
            heard,
            ready,
            thread,
        } = recording();
        std::thread::sleep(Duration::from_millis(250));
        assert!(
            !ready.load(Ordering::Relaxed),
            "ready before any audio arrived"
        );
        let arrived = Instant::now();
        heard.store(true, Ordering::Relaxed);
        while !ready.load(Ordering::Relaxed) && arrived.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            ready.load(Ordering::Relaxed),
            "the first audio never made it ready"
        );
        assert!(
            arrived.elapsed() < Duration::from_millis(60),
            "ready {} ms after the first audio",
            arrived.elapsed().as_millis()
        );
        stop.send(true).unwrap();
        assert_eq!(thread.join().unwrap(), Ok(true));
    }
    #[test]
    fn waiting_for_the_first_audio_still_observes_the_recording_only_every_100_ms() {
        let (stop, receive) = mpsc::channel();
        let observed = std::cell::Cell::new(0);
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(350));
            stop.send(true).unwrap();
        });
        let kept = record(
            &receive,
            &AtomicBool::new(false),
            &AtomicBool::new(false),
            &Mutex::new(vec![]),
            || (),
            |_| observed.set(observed.get() + 1),
        );
        stopper.join().unwrap();
        assert_eq!(kept, Ok(true));
        assert!(
            (3..=5).contains(&observed.get()),
            "observed {} times in 350 ms",
            observed.get()
        );
    }
    #[test]
    fn a_recording_stopped_before_any_audio_never_becomes_ready() {
        let Recording {
            stop,
            ready,
            thread,
            ..
        } = recording();
        std::thread::sleep(Duration::from_millis(50));
        stop.send(true).unwrap();
        assert_eq!(thread.join().unwrap(), Ok(true));
        assert!(!ready.load(Ordering::Relaxed));
    }
}
