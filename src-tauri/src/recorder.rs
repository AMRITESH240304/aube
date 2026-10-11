use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed},
        mpsc, Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, SampleFormat, SizedSample, StreamConfig, SupportedStreamConfig,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::{
    err,
    mixer::{self, Chunk, Mixer, Source, OUTPUT_RATE},
    stt::Transcriber,
    Result,
};

type Wav = hound::WavWriter<BufWriter<File>>;
type Chunks = mpsc::Sender<Chunk>;

pub(crate) const WAV_SPEC: hound::WavSpec = hound::WavSpec {
    channels: 1,
    sample_rate: OUTPUT_RATE,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
};

// Recordings are written as `<id>.partial` and renamed to `<id>.wav` on stop.
// A `.partial` left behind means the app died mid-recording.
const PARTIAL: &str = "partial";

#[derive(Default)]
pub struct Recorder {
    session: Mutex<Option<Session>>,
}

struct Session {
    id: String,
    title: String,
    dir: PathBuf,
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

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Meta {
    pub title: String,
    pub started_at: u64,
    pub recovered: bool,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    id: String,
    title: String,
    started_at: u64,
    duration_secs: f64,
    recovered: bool,
    transcribed: bool,
    path: PathBuf,
}

pub(crate) fn recordings_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_data_dir().map_err(err)?.join("recordings");
    fs::create_dir_all(&dir).map_err(err)?;
    Ok(dir)
}

pub(crate) fn write_meta(dir: &Path, id: &str, meta: &Meta) -> Result<()> {
    let json = serde_json::to_vec_pretty(meta).map_err(err)?;
    fs::write(dir.join(format!("{id}.json")), json).map_err(err)
}

pub(crate) fn read_meta(dir: &Path, id: &str) -> Option<Meta> {
    serde_json::from_slice(&fs::read(dir.join(format!("{id}.json"))).ok()?).ok()
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

fn write_samples(writer: &mut Wav, shared: &Shared, frames: &[[f32; 2]]) -> Result<()> {
    for &frame in frames {
        writer.write_sample(mixer::mix(frame)).map_err(err)?;
    }
    shared.frames.fetch_add(frames.len() as u64, Relaxed);
    Ok(())
}

// Owns the audio streams (not Send on every platform) and the file writer.
// Flushing every second keeps the file on disk if the process is killed.
fn open_writer(saved: &Path, partial: &Path, continuing: bool) -> Result<Wav> {
    if !continuing {
        return hound::WavWriter::create(partial, WAV_SPEC).map_err(err);
    }
    fs::rename(saved, partial).map_err(err)?;
    hound::WavWriter::append(partial).map_err(|e| {
        let _ = fs::rename(partial, saved);
        err(e)
    })
}

fn record(
    app: AppHandle,
    dir: PathBuf,
    id: String,
    continuing: bool,
    language: Option<String>,
    shared: Arc<Shared>,
    ready: mpsc::Sender<Result<()>>,
) -> Result<()> {
    let path = dir.join(format!("{id}.{PARTIAL}"));
    let host = cpal::default_host();
    let (tx, rx) = mpsc::channel();
    let setup = || -> Result<_> {
        let mic = open_microphone(&host, tx.clone(), &shared)?;
        let writer = open_writer(&dir.join(format!("{id}.wav")), &path, continuing)?;
        Ok((mic, writer))
    };
    let ((mic, mic_rate), mut writer) = match setup() {
        Ok(opened) => opened,
        Err(e) => {
            let _ = ready.send(Err(e.clone()));
            return Err(e);
        }
    };
    let recorded = writer.duration() as usize;
    shared.frames.store(recorded as u64, Relaxed);
    let mut mixer = Mixer::new(mic_rate, None);
    let mut system = SystemAudio::open(&host, &tx, &shared, &mut mixer);
    let _ = ready.send(Ok(()));

    // Recording goes on without a transcript if transcription can't start.
    let mut transcriber = Transcriber::new(app, id, language, recorded)
        .map_err(|e| *shared.transcription_error.lock().unwrap() = Some(e))
        .ok();
    let mut last_flush = Instant::now();
    while !shared.stop.load(Relaxed) {
        system.follow_default_output(&host, &tx, &shared, &mut mixer);
        let received = rx.recv_timeout(Duration::from_millis(100)).into_iter();
        for chunk in received.chain(rx.try_iter()) {
            mixer.push(chunk);
        }
        let samples = mixer.drain(false);
        write_samples(&mut writer, &shared, &samples)?;
        if let Some(transcriber) = &mut transcriber {
            transcriber.push(&samples);
            if let Some(e) = transcriber.error() {
                *shared.transcription_error.lock().unwrap() = Some(e);
            }
        }
        if last_flush.elapsed() >= Duration::from_secs(1) {
            writer.flush().map_err(err)?;
            last_flush = Instant::now();
        }
    }

    drop((mic, system, tx));
    for chunk in rx.try_iter() {
        mixer.push(chunk);
    }
    let samples = mixer.drain(true);
    write_samples(&mut writer, &shared, &samples)?;
    writer.finalize().map_err(err)?;
    if let Some(mut transcriber) = transcriber {
        transcriber.push(&samples);
        transcriber.finish(&dir)?;
    }
    Ok(())
}

// Rewrites the RIFF and data sizes from the real file length, so a
// recording cut off by a crash plays back up to its last written sample.
fn repair_wav(path: &Path) -> io::Result<()> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let mut header = [0u8; 256];
    let n = file.read(&mut header)?;
    let data = header[..n]
        .windows(4)
        .position(|w| w == b"data")
        .ok_or(io::ErrorKind::InvalidData)? as u64;
    let data_len = (file.metadata()?.len().saturating_sub(data + 8)) & !1;
    file.set_len(data + 8 + data_len)?;
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&((data + data_len) as u32).to_le_bytes())?;
    file.seek(SeekFrom::Start(data + 4))?;
    file.write_all(&(data_len as u32).to_le_bytes())
}

pub fn recover_interrupted(app: &AppHandle) -> Result<()> {
    let dir = recordings_dir(app)?;
    for entry in fs::read_dir(&dir).map_err(err)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(PARTIAL) {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else {
            continue;
        };
        if let Err(e) = repair_wav(&path) {
            eprintln!("could not recover {}: {e}", path.display());
            continue;
        }
        fs::rename(&path, dir.join(format!("{id}.wav"))).map_err(err)?;
        if let Some(mut meta) = read_meta(&dir, &id) {
            meta.recovered = true;
            write_meta(&dir, &id, &meta)?;
        }
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
    let result = session
        .thread
        .join()
        .map_err(|_| "The recorder stopped unexpectedly.".to_string())?;
    let partial = session.dir.join(format!("{}.{PARTIAL}", session.id));
    fs::rename(&partial, session.dir.join(format!("{}.wav", session.id))).map_err(err)?;
    result.map(|()| session.id)
}

#[tauri::command]
pub fn start_recording(
    app: AppHandle,
    state: State<'_, Recorder>,
    title: String,
    language: Option<String>,
    resume: Option<String>,
) -> Result<()> {
    let mut slot = state.session.lock().unwrap();
    if slot.is_some() {
        return Err("Already recording.".into());
    }

    let dir = recordings_dir(&app)?;
    let continuing = resume.is_some();
    let (id, title, new_meta) = match resume {
        Some(id) => {
            let meta = read_meta(&dir, &id).ok_or("This recording no longer exists.")?;
            (id, meta.title, None)
        }
        None => {
            let started_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(err)?
                .as_millis() as u64;
            let meta = Meta {
                title: title.clone(),
                started_at,
                ..Default::default()
            };
            (started_at.to_string(), title, Some(meta))
        }
    };
    let shared = Arc::new(Shared::default());
    let (ready_tx, ready_rx) = mpsc::channel();

    let (thread_dir, thread_id, thread_shared) = (dir.clone(), id.clone(), shared.clone());
    let thread = std::thread::spawn(move || {
        record(
            app,
            thread_dir,
            thread_id,
            continuing,
            language,
            thread_shared,
            ready_tx,
        )
    });
    ready_rx
        .recv()
        .map_err(|_| "The recorder stopped unexpectedly.".to_string())??;

    if let Some(meta) = &new_meta {
        write_meta(&dir, &id, meta)?;
    }
    *slot = Some(Session {
        id,
        title,
        dir,
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

#[tauri::command]
pub fn list_recordings(app: AppHandle) -> Result<Vec<Recording>> {
    let dir = recordings_dir(&app)?;
    let mut recordings: Vec<Recording> = fs::read_dir(&dir)
        .map_err(err)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("wav"))
        .filter_map(|path| {
            let id = path.file_stem()?.to_str()?.to_string();
            let meta = read_meta(&dir, &id).unwrap_or_else(|| Meta {
                title: "Untitled recording".into(),
                ..Default::default()
            });
            let duration_secs = hound::WavReader::open(&path)
                .map(|r| r.duration() as f64 / r.spec().sample_rate as f64)
                .unwrap_or(0.0);
            Some(Recording {
                transcribed: crate::transcripts::transcript_path(&dir, &id).exists(),
                title: meta.title,
                started_at: meta.started_at,
                recovered: meta.recovered,
                duration_secs,
                path,
                id,
            })
        })
        .collect();
    recordings.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(recordings)
}

#[tauri::command]
pub fn rename_recording(app: AppHandle, id: String, title: String) -> Result<()> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Give the recording a name.".into());
    }
    let dir = recordings_dir(&app)?;
    let mut meta = read_meta(&dir, &id).ok_or("This recording no longer exists.")?;
    meta.title = title.to_string();
    write_meta(&dir, &id, &meta)
}
