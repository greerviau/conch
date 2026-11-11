use crate::core::{self, Snapshot};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Whisper,
    Llama,
}
/// Per-session cancellation state. Servers are shared across sessions, so a
/// session records the ones it uses and `stop` kills only those.
#[derive(Default)]
pub struct WorkerState {
    using: Vec<Arc<Server>>,
    cancelled: bool,
}
pub type Worker = Arc<Mutex<WorkerState>>;
pub fn new_worker() -> Worker {
    Arc::new(Mutex::new(WorkerState::default()))
}
pub const WHISPER_IDLE: Duration = Duration::from_secs(600);
pub const LLAMA_IDLE: Duration = Duration::from_secs(300);
pub const S1_SYSTEM: &str = "You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.";
pub const CLEANUP_SYSTEM: &str = "Clean this dictated transcript. Preserve all CONCHPROTECTED placeholders exactly once in their original order. Preserve identifiers, negation, numbers, constraints and instruction order. Do not answer, expand or summarize the request. Output only the cleaned transcript.";
pub fn endpoint_allowed(endpoint: &str, remote: bool) -> bool {
    let Ok(url) = reqwest::Url::parse(endpoint) else {
        return false;
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    let local = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if remote {
        url.scheme() == "https"
    } else {
        local && matches!(url.scheme(), "http" | "https")
    }
}
pub struct Server {
    kind: Kind,
    child: Mutex<Child>,
    endpoint: String,
    token: String,
    used: Mutex<Instant>,
    busy: AtomicUsize,
}
impl Server {
    fn alive(&self) -> bool {
        self.child
            .lock()
            .unwrap()
            .try_wait()
            .ok()
            .flatten()
            .is_none()
    }
    fn kill(&self) {
        let mut child = self.child.lock().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
}
/// Marks a server busy so the idle reaper leaves it alone.
pub struct Lease {
    server: Arc<Server>,
}
impl Lease {
    fn new(server: Arc<Server>) -> Self {
        server.busy.fetch_add(1, Ordering::SeqCst);
        Self { server }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        *self.server.used.lock().unwrap() = Instant::now();
        self.server.busy.fetch_sub(1, Ordering::SeqCst);
    }
}
type Slot = Arc<Mutex<Option<Arc<Server>>>>;
type Key = (PathBuf, PathBuf, bool);
static POOL: LazyLock<Mutex<HashMap<Key, Slot>>> = LazyLock::new(Default::default);
fn slots(within: &Path) -> Vec<Slot> {
    POOL.lock()
        .unwrap()
        .iter()
        .filter(|((executable, _, _), _)| executable.starts_with(within))
        .map(|(_, slot)| slot.clone())
        .collect()
}
/// Stops servers under `within` that no session is using and that were last
/// used at least the given duration ago. The next request starts a fresh one.
pub fn reap_idle_within(within: &Path, whisper: Duration, llama: Duration) {
    for slot in slots(within) {
        let mut guard = slot.lock().unwrap();
        let Some(server) = guard.as_ref() else {
            continue;
        };
        let limit = match server.kind {
            Kind::Whisper => whisper,
            Kind::Llama => llama,
        };
        if server.busy.load(Ordering::SeqCst) == 0 && server.used.lock().unwrap().elapsed() >= limit
        {
            server.kill();
            *guard = None;
        }
    }
}
pub fn reap_idle(within: &Path) {
    reap_idle_within(within, WHISPER_IDLE, LLAMA_IDLE);
}
/// Stops every pooled server under `within`. Called when the app exits.
pub fn shutdown(within: &Path) {
    for slot in slots(within) {
        if let Some(server) = slot.lock().unwrap().take() {
            server.kill();
        }
    }
}
/// Stops workers a previous Conch process left running after a crash or forced
/// quit. Workers are matched by their exact executable path.
#[cfg(unix)]
pub fn reap_orphans(workers: &Path) {
    for name in ["whisper-server", "llama-server"] {
        let pattern = format!(
            "^{} --host 127\\.0\\.0\\.1 ",
            regex::escape(&workers.join(name).to_string_lossy())
        );
        let _ = Command::new("/usr/bin/pkill")
            .args(["-f", &pattern])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
#[cfg(not(unix))]
pub fn reap_orphans(_: &Path) {}
pub fn stop(worker: &Worker) {
    let using = {
        let mut state = worker.lock().unwrap();
        state.cancelled = true;
        std::mem::take(&mut state.using)
    };
    for server in using {
        server.kill();
    }
}
/// Starts the worker for a recording while the user is still speaking, so the
/// model load overlaps the recording instead of following it.
pub fn preload(
    workers: &Path,
    model: &Path,
    kind: Kind,
    worker: &Worker,
    cpu: bool,
) -> Result<(), String> {
    acquire(workers, model, kind, worker, cpu).map(drop)
}
fn acquire(
    workers: &Path,
    model: &Path,
    kind: Kind,
    worker: &Worker,
    cpu: bool,
) -> Result<Lease, String> {
    let name = match kind {
        Kind::Whisper => "whisper-server",
        Kind::Llama => "llama-server",
    };
    let executable = workers.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if !executable.is_file() {
        return Err(
            "Bundled inference worker is missing; run the runtime build before packaging".into(),
        );
    }
    if !model.is_file() || (kind == Kind::Whisper && !vad_path(model).is_file()) {
        return Err("Download the required model in Settings first".into());
    }
    if worker.lock().unwrap().cancelled {
        return Err("Cancelled".into());
    }
    let slot = POOL
        .lock()
        .unwrap()
        .entry((executable.clone(), model.to_owned(), cpu))
        .or_default()
        .clone();
    let mut guard = slot.lock().unwrap();
    let server = match guard.as_ref().filter(|server| server.alive()) {
        Some(server) => server.clone(),
        None => {
            let server = Arc::new(launch(&executable, model, kind, worker, cpu)?);
            *guard = Some(server.clone());
            server
        }
    };
    let lease = Lease::new(server.clone());
    let mut state = worker.lock().unwrap();
    if state.cancelled {
        drop(state);
        server.kill();
        return Err("Cancelled".into());
    }
    state.using.push(server);
    Ok(lease)
}
fn launch(
    executable: &Path,
    model: &Path,
    kind: Kind,
    worker: &Worker,
    cpu: bool,
) -> Result<Server, String> {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "Cannot allocate inference port")?
        .local_addr()
        .map_err(|_| "Cannot allocate inference port")?
        .port();
    let token = uuid::Uuid::new_v4().to_string();
    let mut command = Command::new(executable);
    command
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--model",
        ])
        .arg(model)
        .args(["--threads", "4"]);
    if kind == Kind::Whisper {
        command.env("CONCH_WORKER_TOKEN", &token);
        command.args(["--language", "en", "--suppress-nst", "--vad", "--vad-model"]);
        command.arg(vad_path(model));
        if cpu {
            command.arg("--no-gpu");
        }
    } else {
        command.args([
            "--ctx-size",
            "4096",
            "--n-predict",
            "1024",
            "--temp",
            "0",
            "--reasoning",
            "off",
            "--chat-template-kwargs",
            r#"{"enable_thinking":false}"#,
            "--no-webui",
            "--api-key",
            &token,
        ]);
        if cpu {
            command.args(["--gpu-layers", "0"]);
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command
        .spawn()
        .map_err(|_| "Cannot launch inference worker")?;
    let server = Server {
        kind,
        child: Mutex::new(child),
        endpoint: format!("http://127.0.0.1:{port}"),
        token,
        used: Mutex::new(Instant::now()),
        busy: AtomicUsize::new(0),
    };
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(1))
        .build()
        .map_err(|_| "Cannot initialize local IPC")?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(45) {
        if worker.lock().unwrap().cancelled || !server.alive() {
            server.kill();
            return Err("Inference worker stopped or was cancelled".into());
        }
        if client
            .get(format!("{}/health", server.endpoint))
            .bearer_auth(&server.token)
            .send()
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(server);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    server.kill();
    Err("Inference worker startup timed out".into())
}
/// Whisper encodes a fixed 30 s window, so a short clip pays for the silence
/// that pads it. For audio up to this length `whisper_settings` shrinks the
/// window to the clip. Longer audio spans windows, which needs timestamps to
/// advance, so it keeps the defaults.
const SHORT_AUDIO_SECONDS: f32 = 30.0;
/// Extra encoder context beyond the clip, in 20 ms frames. A smaller margin
/// made `small.en` repeat itself past the end of 16 s clips (see
/// `docs/validation/latency.md`).
const CONTEXT_MARGIN: u32 = 200;
/// Length of a WAV in seconds, or `None` if it cannot be read.
fn wav_seconds(wav: &[u8]) -> Option<f32> {
    let reader = hound::WavReader::new(std::io::Cursor::new(wav)).ok()?;
    Some(reader.duration() as f32 / reader.spec().sample_rate as f32)
}
/// Request fields only `whisper-server` understands. They are measured as one
/// set: the window sized to the clip, no timestamp tokens, and no temperature
/// fallback, which re-decodes at random temperatures when the output looks bad.
fn whisper_settings(seconds: Option<f32>) -> Vec<(&'static str, String)> {
    let Some(seconds) = seconds.filter(|seconds| *seconds <= SHORT_AUDIO_SECONDS) else {
        return vec![];
    };
    let frames = ((seconds * 50.0).ceil() as u32 + CONTEXT_MARGIN).min(1500);
    vec![
        ("audio_ctx", frames.to_string()),
        ("no_timestamps", "true".into()),
        ("temperature_inc", "0".into()),
    ]
}
pub fn transcribe(
    workers: &Path,
    model: &Path,
    wav: Vec<u8>,
    hint: &str,
    worker: &Worker,
    cpu: bool,
) -> Result<String, String> {
    let lease = acquire(workers, model, Kind::Whisper, worker, cpu)?;
    let (url, token) = (&lease.server.endpoint, &lease.server.token);
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|_| "Cannot initialize recognizer IPC")?;
    let seconds = wav_seconds(&wav);
    let mut form = reqwest::blocking::multipart::Form::new()
        .part(
            "file",
            reqwest::blocking::multipart::Part::bytes(wav).file_name("recording.wav"),
        )
        .text("response_format", "json")
        .text("temperature", "0");
    if !hint.is_empty() {
        form = form.text("prompt", hint.to_owned());
    }
    for (field, value) in whisper_settings(seconds) {
        form = form.text(field, value);
    }
    let value: Value = client
        .post(format!("{url}/inference"))
        .bearer_auth(token)
        .multipart(form)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .map_err(|_| "Local recognition failed or timed out")?;
    value["text"]
        .as_str()
        .map(flatten_segments)
        .ok_or_else(|| "Recognizer returned invalid output".into())
}
/// The recognizer puts each segment of a long recording on its own line.
/// Dictated prose has no line breaks, so join the segments with one space.
pub fn flatten_segments(text: &str) -> String {
    static BREAKS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\s*[\r\n]\s*").unwrap());
    BREAKS.replace_all(text.trim(), " ").into_owned()
}
pub struct CleanupRequest<'a> {
    pub workers: &'a Path,
    pub data: &'a Path,
    pub snapshot: &'a Snapshot,
    pub raw: &'a str,
    pub worker: &'a Worker,
    pub cpu: bool,
}
pub fn enhanced(request: CleanupRequest<'_>) -> Result<String, String> {
    let CleanupRequest {
        workers,
        data,
        snapshot,
        raw,
        worker,
        cpu,
    } = request;
    let cleanup = &snapshot.cleanup;
    if cleanup.enhanced == "disabled" {
        return Err("Enhanced cleanup is disabled".into());
    }
    if worker.lock().unwrap().cancelled {
        return Err("Cancelled".into());
    }
    if raw.contains("CONCHPROTECTED") {
        return Err("Reserved placeholder in transcript; using Basic".into());
    }
    let masked = core::mask(raw, snapshot);
    let basic = core::basic(&masked.text, cleanup);
    if basic.trim().is_empty() {
        return Err("Nothing remains after Basic cleanup".into());
    }
    if basic.len() > 3000 {
        return Err("Enhanced input exceeds the conservative 3,000-byte limit; using Basic".into());
    }
    let mut lease = None;
    let (endpoint, token, system, user) = if cleanup.enhanced == "managed" {
        let held = acquire(workers, &data.join(CLEANUP.name), Kind::Llama, worker, cpu)?;
        let (url, key) = (held.server.endpoint.clone(), held.server.token.clone());
        lease = Some(held);
        (
            format!("{url}/v1/chat/completions"),
            key,
            S1_SYSTEM,
            format!("[Styling: semi-formal] [Structure: prose] [Context: general]\n{basic}"),
        )
    } else {
        let remote = cleanup.enhanced == "remote";
        if remote && !cleanup.remote_enabled {
            return Err("Remote cleanup is disabled".into());
        }
        if !endpoint_allowed(&cleanup.endpoint, remote) {
            return Err("Cleanup endpoint is not allowed".into());
        }
        let key = if remote {
            keyring::Entry::new("app.conch.desktop", &cleanup.endpoint)
                .map_err(|_| "Credential store unavailable")?
                .get_password()
                .unwrap_or_default()
        } else {
            String::new()
        };
        (cleanup.endpoint.clone(), key, CLEANUP_SYSTEM, basic.clone())
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Cannot initialize cleanup runtime")?;
    let value: Value = runtime.block_on(async {
        let operation = async {
            if worker.lock().unwrap().cancelled {return Err("Cancelled".to_string());}
            let mut builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(30));
            if cleanup.enhanced != "remote" {builder = builder.no_proxy();}
            let client = builder.build().map_err(|_| "Cannot initialize cleanup client")?;
            if cleanup.enhanced == "managed" {
                let tokenizer = endpoint.trim_end_matches("/v1/chat/completions").to_owned()+"/tokenize";
                let tokens: Value = client.post(tokenizer).bearer_auth(&token).json(&json!({"content":format!("{system}\n{user}")})).send().await.and_then(|r|r.error_for_status()).map_err(|_| "Local tokenization failed; using Basic")?.json().await.map_err(|_| "Invalid tokenizer response")?;
                if tokens["tokens"].as_array().is_none_or(|tokens|tokens.len() > 1000) {return Err("Managed cleanup input exceeds 1,000 tokens; using Basic".into());}
            }
            let mut request = client.post(endpoint).json(&json!({"model":cleanup.model,"messages":[{"role":"system","content":system},{"role":"user","content":user}],"temperature":0,"max_tokens":1024,"chat_template_kwargs":{"enable_thinking":false}}));
            if !token.is_empty() {request = request.bearer_auth(token);}
            let mut response = request.send().await.and_then(|r|r.error_for_status()).map_err(|_| "Enhanced cleanup failed or timed out; using Basic")?;
            let mut bytes = vec![];
            while let Some(chunk) = response.chunk().await.map_err(|_| "Invalid cleanup response")? {
                if bytes.len()+chunk.len() > 64*1024 {return Err("Cleanup response exceeds limit".into());}
                bytes.extend_from_slice(&chunk);
            }
            serde_json::from_slice(&bytes).map_err(|_| "Invalid cleanup response".into())
        };
        let cancelled = async {
            loop {if worker.lock().unwrap().cancelled {return;} tokio::time::sleep(Duration::from_millis(20)).await;}
        };
        tokio::select! {biased; _ = cancelled => Err("Cancelled".into()), result = operation => result}
    })?;
    let after = value["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("Invalid cleanup response")?;
    let result = core::validate_enhanced(&basic, after.trim(), &masked)
        .ok_or("Enhanced rewrite failed preservation checks; using Basic")?;
    drop(lease);
    Ok(result)
}
#[derive(Clone, Copy)]
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}
/// English recognizers the user can choose between, smallest first. Larger
/// models make fewer errors and take longer to recognize a chunk.
pub const RECOGNIZERS: [Model; 3] = [
    Model {
        id: "tiny.en",
        name: "ggml-tiny.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-tiny.en.bin",
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        size: 77704715,
    },
    Model {
        id: "base.en",
        name: "ggml-base.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.en.bin",
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        size: 147964211,
    },
    Model {
        id: "small.en",
        name: "ggml-small.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.en.bin",
        sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
        size: 487614201,
    },
];
/// Silero voice detection model. The recognizer runs only on the
/// parts of a chunk that contain speech, so silence and room noise after the
/// user stops talking cannot turn into words such as "Thank you."
pub const VAD: Model = Model {
    id: "silero-vad",
    name: "ggml-silero-v5.1.2.bin",
    url: "https://huggingface.co/ggml-org/whisper-vad/resolve/9ffd54a1e1ee413ddf265af9913beaf518d1639b/ggml-silero-v5.1.2.bin",
    sha256: "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf",
    size: 885098,
};
pub const CLEANUP: Model = Model {
    id: "s1-mini",
    name: "s1-mini-q4_k_m.gguf",
    url: "https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf",
    sha256: "3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634",
    size: 484219808,
};
pub const MODELS: [Model; 5] = [RECOGNIZERS[0], RECOGNIZERS[1], RECOGNIZERS[2], VAD, CLEANUP];
pub fn recognizer(id: &str) -> Option<Model> {
    RECOGNIZERS.iter().find(|m| m.id == id).copied()
}
/// The models to download so `id` works: the model itself, plus the voice
/// activity model when `id` is a recognizer. Models already on disk are left out.
pub fn missing(data: &Path, id: &str) -> Result<Vec<Model>, String> {
    let model = *MODELS.iter().find(|m| m.id == id).ok_or("Unknown model")?;
    let mut wanted = vec![model];
    if recognizer(id).is_some() {
        wanted.push(VAD);
    }
    wanted.retain(|m| !data.join(m.name).is_file());
    Ok(wanted)
}
/// The voice detection model sits next to the recognizer in the data directory.
fn vad_path(model: &Path) -> PathBuf {
    model.with_file_name(VAD.name)
}
pub fn download(data: &Path, model: Model, progress: impl Fn(u64, u64)) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    std::fs::create_dir_all(data).map_err(|_| "Cannot create model directory")?;
    let destination = data.join(model.name);
    let partial = data.join(format!("{}.partial", model.name));
    let result = (|| {
        let mut response = Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|_| "Cannot initialize downloader")?
            .get(model.url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| "Model download failed")?;
        let mut file = std::fs::File::create(&partial).map_err(|_| "Cannot write model")?;
        let mut hash = Sha256::new();
        let mut size = 0;
        let mut chunk = [0u8; 65536];
        loop {
            let n = response
                .read(&mut chunk)
                .map_err(|_| "Model download interrupted")?;
            if n == 0 {
                break;
            }
            size += n as u64;
            if size > model.size {
                return Err("Model download exceeds expected size".into());
            }
            hash.update(&chunk[..n]);
            file.write_all(&chunk[..n])
                .map_err(|_| "Cannot write model")?;
            progress(size, model.size);
        }
        if size != model.size || format!("{:x}", hash.finalize()) != model.sha256 {
            return Err("Model checksum mismatch".into());
        }
        file.sync_all().map_err(|_| "Cannot save model")?;
        std::fs::rename(&partial, &destination).map_err(|_| "Cannot install model")?;
        Ok(destination)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(partial);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizer_segment_breaks_become_single_spaces() {
        assert_eq!(
            flatten_segments(" Do not change it.\n Next sentence here.\r\n\n  Last one. "),
            "Do not change it. Next sentence here. Last one."
        );
        assert_eq!(flatten_segments("one line"), "one line");
    }
    fn silence(seconds: usize) -> Vec<u8> {
        crate::audio::wav(&vec![0.0; seconds * 16_000]).unwrap()
    }
    #[test]
    fn a_wav_reports_its_length_and_garbage_reports_none() {
        assert_eq!(wav_seconds(&silence(3)), Some(3.0));
        assert_eq!(wav_seconds(&[0; 64]), None);
    }
    #[test]
    fn short_audio_gets_a_window_that_covers_it_plus_the_margin() {
        let fields = whisper_settings(Some(5.0));
        assert!(fields.contains(&("audio_ctx", "450".into())), "{fields:?}");
        assert!(fields.contains(&("no_timestamps", "true".into())));
        assert!(fields.contains(&("temperature_inc", "0".into())));
    }
    #[test]
    fn the_window_never_exceeds_the_thirty_second_maximum() {
        let fields = whisper_settings(Some(29.0));
        assert!(fields.contains(&("audio_ctx", "1500".into())), "{fields:?}");
    }
    #[test]
    fn long_or_unreadable_audio_keeps_the_defaults() {
        assert!(whisper_settings(Some(31.0)).is_empty());
        assert!(whisper_settings(None).is_empty());
    }
    #[cfg(unix)]
    mod fake {
        use std::{os::unix::fs::PermissionsExt, path::Path};
        const SCRIPT: &str = r#"#!/usr/bin/env python3
import http.server, json, os, sys
port = int(sys.argv[sys.argv.index("--port") + 1])
here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, "starts.log"), "a") as log:
    log.write(str(os.getpid()) + "\n")
with open(os.path.join(here, "args.log"), "a") as log:
    log.write("\0".join(sys.argv[1:]) + "\n")
class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
    def do_POST(self):
        request = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        with open(os.path.join(here, "last-request.bin"), "wb") as saved:
            saved.write(request)
        body = json.dumps({"text": str(os.getpid())}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(body)
http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
"#;
        pub fn install(dir: &Path) {
            let exe = dir.join("whisper-server");
            std::fs::write(&exe, SCRIPT).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(dir.join("model.bin"), b"model").unwrap();
            std::fs::write(dir.join(super::VAD.name), b"vad").unwrap();
        }
        pub fn arguments(dir: &Path) -> Vec<String> {
            std::fs::read_to_string(dir.join("args.log"))
                .unwrap()
                .lines()
                .next_back()
                .unwrap()
                .split('\0')
                .map(String::from)
                .collect()
        }
        pub fn last_request(dir: &Path) -> String {
            String::from_utf8_lossy(&std::fs::read(dir.join("last-request.bin")).unwrap())
                .into_owned()
        }
        pub fn starts(dir: &Path) -> usize {
            std::fs::read_to_string(dir.join("starts.log"))
                .map(|log| log.lines().count())
                .unwrap_or(0)
        }
    }
    #[cfg(unix)]
    fn run(dir: &Path, worker: &Worker) -> String {
        transcribe(dir, &dir.join("model.bin"), vec![0; 64], "", worker, true).unwrap()
    }
    #[cfg(unix)]
    #[test]
    fn recognizer_receives_the_vocabulary_hint_as_its_initial_prompt() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let model = dir.path().join("model.bin");
        transcribe(dir.path(), &model, vec![0; 64], "", &new_worker(), true).unwrap();
        let without = fake::last_request(dir.path());
        assert!(!without.contains("name=\"prompt\""));
        let hint = "Vocabulary: podGC, maxGroups.";
        transcribe(dir.path(), &model, vec![0; 64], hint, &new_worker(), true).unwrap();
        let with = fake::last_request(dir.path());
        assert!(with.contains("name=\"prompt\""));
        assert!(with.contains(hint));
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn the_recognizer_gets_a_window_sized_to_short_audio_and_defaults_for_long() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let model = dir.path().join("model.bin");
        transcribe(dir.path(), &model, silence(2), "", &new_worker(), true).unwrap();
        let short = fake::last_request(dir.path());
        assert!(
            short.contains("name=\"audio_ctx\"\r\n\r\n300\r\n"),
            "{short:?}"
        );
        assert!(short.contains("name=\"no_timestamps\"\r\n\r\ntrue\r\n"));
        assert!(short.contains("name=\"temperature_inc\"\r\n\r\n0\r\n"));
        transcribe(dir.path(), &model, silence(31), "", &new_worker(), true).unwrap();
        let long = fake::last_request(dir.path());
        for field in ["audio_ctx", "no_timestamps", "temperature_inc"] {
            assert!(!long.contains(&format!("name=\"{field}\"")), "{field}");
        }
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn recognizer_starts_with_voice_detection_so_silence_yields_no_text() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        run(dir.path(), &new_worker());
        let args = fake::arguments(dir.path());
        assert!(args.iter().any(|a| a == "--vad"), "{args:?}");
        let model = args.iter().position(|a| a == "--vad-model").unwrap();
        assert_eq!(args[model + 1], dir.path().join(VAD.name).to_string_lossy());
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn recognition_without_the_voice_detection_model_asks_for_the_download() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        std::fs::remove_file(dir.path().join(VAD.name)).unwrap();
        let error = transcribe(
            dir.path(),
            &dir.path().join("model.bin"),
            vec![0; 64],
            "",
            &new_worker(),
            true,
        )
        .unwrap_err();
        assert!(error.contains("Download"), "{error}");
        assert_eq!(fake::starts(dir.path()), 0);
    }
    #[test]
    fn recognition_models_are_chosen_by_id_and_pinned() {
        let ids: Vec<_> = RECOGNIZERS.iter().map(|m| m.id).collect();
        assert_eq!(ids, ["tiny.en", "base.en", "small.en"]);
        assert_eq!(recognizer("small.en").unwrap().name, "ggml-small.en.bin");
        assert!(recognizer("large-v3").is_none());
        let mut all: Vec<_> = MODELS.iter().map(|m| m.id).collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), MODELS.len());
        for model in MODELS {
            assert_eq!(model.sha256.len(), 64, "{}", model.id);
            assert!(model.url.contains("/resolve/"), "{}", model.id);
            assert!(
                !model.url.contains("/resolve/main/"),
                "{} is not pinned",
                model.id
            );
        }
        assert_eq!(RECOGNIZERS[2].size, 487614201);
        assert_eq!(VAD.size, 885098);
    }
    #[test]
    fn downloading_a_recognizer_also_fetches_the_missing_voice_detection_model() {
        let dir = tempfile::tempdir().unwrap();
        let plan = |id: &str| -> Vec<&str> {
            missing(dir.path(), id)
                .unwrap()
                .iter()
                .map(|m| m.name)
                .collect()
        };
        assert_eq!(plan("small.en"), ["ggml-small.en.bin", VAD.name]);
        std::fs::write(dir.path().join("ggml-small.en.bin"), b"x").unwrap();
        assert_eq!(plan("small.en"), [VAD.name]);
        std::fs::write(dir.path().join(VAD.name), b"x").unwrap();
        assert!(plan("small.en").is_empty());
        assert_eq!(plan("tiny.en"), ["ggml-tiny.en.bin"]);
        assert_eq!(plan("s1-mini"), ["s1-mini-q4_k_m.gguf"]);
        assert!(missing(dir.path(), "huge").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn consecutive_recordings_reuse_one_recognizer_process() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let first = run(dir.path(), &new_worker());
        let second = run(dir.path(), &new_worker());
        assert_eq!(first, second);
        assert_eq!(fake::starts(dir.path()), 1);
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn preload_starts_the_recognizer_before_the_recording_ends() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let worker = new_worker();
        preload(
            dir.path(),
            &dir.path().join("model.bin"),
            Kind::Whisper,
            &worker,
            true,
        )
        .unwrap();
        assert_eq!(fake::starts(dir.path()), 1);
        run(dir.path(), &worker);
        assert_eq!(fake::starts(dir.path()), 1);
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn cancelling_kills_the_worker_in_use_and_the_next_recording_starts_a_fresh_one() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let cancelled = new_worker();
        let first = run(dir.path(), &cancelled);
        stop(&cancelled);
        assert_eq!(
            transcribe(
                dir.path(),
                &dir.path().join("model.bin"),
                vec![0; 64],
                "",
                &cancelled,
                true
            )
            .unwrap_err(),
            "Cancelled"
        );
        let second = run(dir.path(), &new_worker());
        assert_ne!(first, second);
        assert_eq!(fake::starts(dir.path()), 2);
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn startup_reaps_workers_left_behind_by_a_crashed_process() {
        // `reap_orphans` matches a command line that starts with the worker
        // path, so the stand-in must be a compiled program. A script shows its
        // interpreter first, and `pkill` would not match it.
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("orphan.rs");
        std::fs::write(
            &source,
            "fn main() { loop { std::thread::sleep(std::time::Duration::from_secs(1)); } }",
        )
        .unwrap();
        let worker = dir.path().join("whisper-server");
        assert!(
            Command::new("rustc")
                .arg(&source)
                .arg("-o")
                .arg(&worker)
                .status()
                .unwrap()
                .success()
        );
        let mut orphan = Command::new(&worker)
            .args(["--host", "127.0.0.1", "--port", "1", "--model", "m"])
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(orphan.try_wait().unwrap().is_none());
        reap_orphans(dir.path());
        assert!(orphan.wait().is_ok_and(|status| !status.success()));
    }
    #[cfg(unix)]
    #[test]
    fn idle_workers_are_stopped_and_restart_on_demand() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        run(dir.path(), &new_worker());
        reap_idle_within(dir.path(), Duration::ZERO, Duration::ZERO);
        run(dir.path(), &new_worker());
        assert_eq!(fake::starts(dir.path()), 2);
        shutdown(dir.path());
    }
    #[test]
    fn remote_requires_https_and_local_requires_loopback() {
        assert!(endpoint_allowed(
            "http://127.0.0.1:8080/v1/chat/completions",
            false
        ));
        assert!(!endpoint_allowed("http://example.com", true));
        assert!(!endpoint_allowed("https://example.com", false));
        assert!(!endpoint_allowed("https://user:secret@example.com", true));
    }
    #[test]
    fn rejects_changed_constraints_and_corrupt_placeholders() {
        let snapshot = Snapshot {
            mappings: vec![core::Mapping {
                phrase: "pod gee see".into(),
                replacement: "podGC".into(),
                enabled: true,
                repository: None,
            }],
            ..Default::default()
        };
        let masked = core::mask("do not change pod gee see before 12", &snapshot);
        assert!(
            core::validate_enhanced(
                &masked.text,
                "Do change CONCHPROTECTED0X before 12.",
                &masked
            )
            .is_none()
        );
        assert!(
            core::validate_enhanced(
                &masked.text,
                "Do not change CONCHPROTECTED0X before 13.",
                &masked
            )
            .is_none()
        );
        assert!(
            core::validate_enhanced(&masked.text, "Do not change before 12.", &masked).is_none()
        );
        assert_eq!(
            core::validate_enhanced(
                &masked.text,
                "Do not change CONCHPROTECTED0X before 12.",
                &masked
            )
            .unwrap(),
            "Do not change podGC before 12."
        );
    }
}
