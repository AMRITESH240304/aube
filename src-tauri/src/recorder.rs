use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed},
        mpsc, Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, SampleFormat, SizedSample, StreamConfig, SupportedStreamConfig,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{
    err,
    library::{self, Db},
    mixer::{Chunk, Mixer, Source, OUTPUT_RATE},
    stt::Transcriber,
    Result,
};

type Chunks = mpsc::Sender<Chunk>;

#[derive(Default)]
pub struct Recorder {
    session: Mutex<Option<Session>>,
}

struct Session {
    id: String,
    title: String,
    shared: Arc<Shared>,
    thread: JoinHandle<Result<()>>,
}

#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    paused: AtomicBool,
    frames: AtomicU64,
    peaks: [AtomicU32; 2],
    system_error: Mutex<Option<String>>,
    transcription_error: Mutex<Option<String>>,
}

impl Shared {
    fn take_level(&self, source: Source) -> f32 {
        self.peaks[source as usize].swap(0, Relaxed) as f32 / i16::MAX as f32
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    id: String,
    title: String,
    elapsed_secs: f64,
    paused: bool,
    mic_level: f32,
    system_level: f32,
    system_error: Option<String>,
    transcription_error: Option<String>,
}

fn build_stream<T>(
    device: &cpal::Device,
    config: StreamConfig,
    source: Source,
    tx: Chunks,
    shared: Arc<Shared>,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    let mut was_paused = false;
    device
        .build_input_stream(
            config,
            move |data: &[T], info: &cpal::InputCallbackInfo| {
                if shared.paused.load(Relaxed) {
                    was_paused = true;
                    return;
                }
                let mono: Vec<f32> = data
                    .chunks(channels)
                    .map(|frame| {
                        frame.iter().map(|&s| s.to_sample::<f32>()).sum::<f32>() / channels as f32
                    })
                    .collect();
                let peak = mono.iter().fold(0f32, |m, s| m.max(s.abs())).min(1.0);
                shared.peaks[source as usize].fetch_max((peak * i16::MAX as f32) as u32, Relaxed);
                let _ = tx.send(Chunk {
                    source,
                    samples: mono,
                    captured: info.timestamp().capture,
                    resumed: std::mem::take(&mut was_paused),
                });
            },
            |e| eprintln!("audio input error: {e}"),
            None,
        )
        .map_err(err)
}

fn open_stream(
    device: &cpal::Device,
    supported: SupportedStreamConfig,
    source: Source,
    tx: Chunks,
    shared: &Arc<Shared>,
) -> Result<(cpal::Stream, u32)> {
    let config = supported.config();
    let shared = shared.clone();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(device, config, source, tx, shared),
        SampleFormat::I16 => build_stream::<i16>(device, config, source, tx, shared),
        SampleFormat::I32 => build_stream::<i32>(device, config, source, tx, shared),
        SampleFormat::U16 => build_stream::<u16>(device, config, source, tx, shared),
        other => Err(format!("Unsupported audio format: {other}")),
    }?;
    stream.play().map_err(err)?;
    Ok((stream, supported.sample_rate()))
}

fn open_microphone(
    host: &cpal::Host,
    tx: Chunks,
    shared: &Arc<Shared>,
) -> Result<(cpal::Stream, u32)> {
    let device = host.default_input_device().ok_or("No microphone found.")?;
    let config = device.default_input_config().map_err(err)?;
    open_stream(&device, config, Source::Mic, tx, shared)
}

// An input stream on an output device is a Core Audio tap on macOS 14.2+
// and WASAPI loopback on Windows.
fn open_system_audio(
    host: &cpal::Host,
    tx: Chunks,
    shared: &Arc<Shared>,
) -> Result<(cpal::Stream, u32)> {
    #[cfg(target_os = "macos")]
    crate::permission::system_audio()?;
    let device = host
        .default_output_device()
        .ok_or("No speakers or headphones found.")?;
    // cpal only taps devices without inputs; otherwise it would record this device's mic.
    if cfg!(target_os = "macos") && device.supports_input() {
        return Err(
            "Your output device has its own microphone (like a USB headset). \
                    Switch sound output to speakers or Bluetooth headphones to record the call."
                .into(),
        );
    }
    let config = device.default_output_config().map_err(err)?;
    open_stream(&device, config, Source::System, tx, shared)
}

const OUTPUT_CHECK: Duration = Duration::from_secs(1);

struct SystemAudio {
    stream: Option<cpal::Stream>,
    device: Option<cpal::DeviceId>,
    checked: Instant,
}

impl SystemAudio {
    fn open(host: &cpal::Host, tx: &Chunks, shared: &Arc<Shared>, mixer: &mut Mixer) -> Self {
        let mut system = Self {
            stream: None,
            device: None,
            checked: Instant::now(),
        };
        system.reopen(host, tx, shared, mixer);
        system
    }

    fn follow_default_output(
        &mut self,
        host: &cpal::Host,
        tx: &Chunks,
        shared: &Arc<Shared>,
        mixer: &mut Mixer,
    ) {
        if self.checked.elapsed() < OUTPUT_CHECK {
            return;
        }
        self.checked = Instant::now();
        if default_output_id(host) != self.device {
            self.reopen(host, tx, shared, mixer);
        }
    }

    fn reopen(&mut self, host: &cpal::Host, tx: &Chunks, shared: &Arc<Shared>, mixer: &mut Mixer) {
        self.stream = None;
        self.device = default_output_id(host);
        let error = match open_system_audio(host, tx.clone(), shared) {
            Ok((stream, rate)) => {
                mixer.set_system_rate(rate);
                self.stream = Some(stream);
                None
            }
            Err(e) => Some(e),
        };
        *shared.system_error.lock().unwrap() = error;
    }
}

fn default_output_id(host: &cpal::Host) -> Option<cpal::DeviceId> {
    host.default_output_device()
        .and_then(|device| device.id().ok())
}

fn record(
    app: AppHandle,
    id: String,
    recorded: usize,
    language: Option<String>,
    shared: Arc<Shared>,
    ready: mpsc::Sender<Result<()>>,
) -> Result<()> {
    let host = cpal::default_host();
    let (tx, rx) = mpsc::channel();
    let (mic, mic_rate) = match open_microphone(&host, tx.clone(), &shared) {
        Ok(opened) => opened,
        Err(e) => {
            let _ = ready.send(Err(e.clone()));
            return Err(e);
        }
    };
    shared.frames.store(recorded as u64, Relaxed);
    let mut mixer = Mixer::new(mic_rate, None);
    let mut system = SystemAudio::open(&host, &tx, &shared, &mut mixer);
    let _ = ready.send(Ok(()));

    // Recording goes on without a transcript if transcription can't start.
    let mut transcriber = Transcriber::new(app.clone(), id.clone(), language, recorded)
        .map_err(|e| *shared.transcription_error.lock().unwrap() = Some(e))
        .ok();
    while !shared.stop.load(Relaxed) {
        system.follow_default_output(&host, &tx, &shared, &mut mixer);
        let received = rx.recv_timeout(Duration::from_millis(100)).into_iter();
        for chunk in received.chain(rx.try_iter()) {
            mixer.push(chunk);
        }
        let samples = mixer.drain(false);
        shared.frames.fetch_add(samples.len() as u64, Relaxed);
        if let Some(transcriber) = &mut transcriber {
            transcriber.push(&samples);
            if let Some(e) = transcriber.error() {
                *shared.transcription_error.lock().unwrap() = Some(e);
            }
        }
    }

    drop((mic, system, tx));
    for chunk in rx.try_iter() {
        mixer.push(chunk);
    }
    let samples = mixer.drain(true);
    let frames = shared.frames.fetch_add(samples.len() as u64, Relaxed) + samples.len() as u64;
    app.state::<Db>()
        .set_duration(&id, frames as f64 / OUTPUT_RATE as f64)?;
    if let Some(mut transcriber) = transcriber {
        transcriber.push(&samples);
        transcriber.finish()?;
    }
    Ok(())
}

pub fn finish(recorder: &Recorder) -> Result<String> {
    let session = recorder
        .session
        .lock()
        .unwrap()
        .take()
        .ok_or("Nothing is recording.")?;
    session.shared.stop.store(true, Relaxed);
    session
        .thread
        .join()
        .map_err(|_| "The recorder stopped unexpectedly.".to_string())??;
    Ok(session.id)
}

impl Recorder {
    pub fn elapsed(&self, id: &str) -> Option<f64> {
        let slot = self.session.lock().unwrap();
        let session = slot.as_ref().filter(|session| session.id == id)?;
        Some(session.shared.frames.load(Relaxed) as f64 / OUTPUT_RATE as f64)
    }
}

#[tauri::command]
pub fn start_recording(
    app: AppHandle,
    state: State<'_, Recorder>,
    db: State<'_, Db>,
    title: String,
    language: Option<String>,
    resume: Option<String>,
) -> Result<()> {
    let mut slot = state.session.lock().unwrap();
    if slot.is_some() {
        return Err("Already recording.".into());
    }

    let (id, title, recorded, started_at) = match resume {
        Some(id) => {
            let (title, duration_secs) = db.recording(&id)?;
            let recorded = (duration_secs * OUTPUT_RATE as f64) as usize;
            (id, title, recorded, None)
        }
        None => {
            let started_at = library::now();
            (started_at.to_string(), title, 0, Some(started_at))
        }
    };
    let shared = Arc::new(Shared::default());
    let (ready_tx, ready_rx) = mpsc::channel();

    let (thread_id, thread_shared) = (id.clone(), shared.clone());
    let thread = std::thread::spawn(move || {
        record(app, thread_id, recorded, language, thread_shared, ready_tx)
    });
    ready_rx
        .recv()
        .map_err(|_| "The recorder stopped unexpectedly.".to_string())??;

    if let Some(started_at) = started_at {
        db.create_recording(&id, &title, started_at)?;
    }
    *slot = Some(Session {
        id,
        title,
        shared,
        thread,
    });
    Ok(())
}

#[tauri::command]
pub fn stop_recording(state: State<'_, Recorder>) -> Result<String> {
    finish(&state)
}

#[tauri::command]
pub fn set_paused(state: State<'_, Recorder>, paused: bool) -> Result<()> {
    let slot = state.session.lock().unwrap();
    let session = slot.as_ref().ok_or("Nothing is recording.")?;
    session.shared.paused.store(paused, Relaxed);
    Ok(())
}

#[tauri::command]
pub fn recording_status(state: State<'_, Recorder>) -> Option<Status> {
    let slot = state.session.lock().unwrap();
    slot.as_ref().map(|s| Status {
        id: s.id.clone(),
        title: s.title.clone(),
        elapsed_secs: s.shared.frames.load(Relaxed) as f64 / OUTPUT_RATE as f64,
        paused: s.shared.paused.load(Relaxed),
        mic_level: s.shared.take_level(Source::Mic),
        system_level: s.shared.take_level(Source::System),
        system_error: s.shared.system_error.lock().unwrap().clone(),
        transcription_error: s.shared.transcription_error.lock().unwrap().clone(),
    })
}
