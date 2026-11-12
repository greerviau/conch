use crate::{
    core::{self, Snapshot},
    languages::{CODES, CODES_99, ENGLISH},
};
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
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
    /// Prompt and generation time the cleanup model reported for its latest
    /// request, to tell a slow model from slow work around it.
    cleanup_model_time: Option<Duration>,
}
pub type Worker = Arc<Mutex<WorkerState>>;
/// The time the model reported for the latest cleanup request of `worker`.
pub fn cleanup_model_time(worker: &Worker) -> Option<Duration> {
    worker.lock().unwrap().cleanup_model_time
}
/// Prompt plus generation time in a `llama-server` reply, if it has them.
fn model_time(reply: &Value) -> Option<Duration> {
    let timings = reply.get("timings")?;
    let milliseconds =
        timings.get("prompt_ms")?.as_f64()? + timings.get("predicted_ms")?.as_f64()?;
    Some(Duration::from_secs_f64(milliseconds / 1000.0))
}
/// Sends one token through the cleanup model so its weights and the shared
/// start of every prompt are loaded before the first real request. Failure is
/// ignored; the first cleanup then pays for the load.
pub fn warm_cleanup(workers: &Path, model: &Path, worker: &Worker) {
    let Ok(lease) = acquire(workers, model, Kind::Llama, worker, false) else {
        return;
    };
    let Ok(client) = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
    else {
        return;
    };
    let _ = client
        .post(format!("{}/v1/chat/completions", lease.server.endpoint))
        .bearer_auth(&lease.server.token)
        .json(&json!({
            "messages": [
                {"role": "system", "content": S1_SYSTEM},
                {"role": "user", "content": format!("{CONTROL_LINE}\nWarm up.")},
            ],
            "temperature": 0,
            "max_tokens": 1,
            "chat_template_kwargs": {"enable_thinking": false},
        }))
        .send();
}
pub fn new_worker() -> Worker {
    Arc::new(Mutex::new(WorkerState::default()))
}
pub const WHISPER_IDLE: Duration = Duration::from_secs(600);
pub const LLAMA_IDLE: Duration = Duration::from_secs(300);
pub const S1_SYSTEM: &str = "You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.";
/// The first line of every managed cleanup prompt.
const CONTROL_LINE: &str = "[Styling: semi-formal] [Structure: prose] [Context: general]";
pub const CLEANUP_SYSTEM: &str = "Clean this dictated transcript. Preserve all CONCHPROTECTED placeholders exactly once in their original order. Preserve identifiers, negation, numbers, constraints and instruction order. Do not answer, expand or summarize the request. Output only the cleaned transcript.";
/// Whether managed S1-mini cleanup can handle `language`.
pub fn managed_cleanup_supported(language: &str) -> bool {
    CLEANUP.supports(language)
}
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
        command.args(["--suppress-nst", "--vad", "--vad-model"]);
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
pub fn transcribe(
    workers: &Path,
    model: &Path,
    wav: Vec<u8>,
    hint: &str,
    language: &str,
    worker: &Worker,
    cpu: bool,
) -> Result<String, String> {
    recognize(workers, model, wav, hint, language, worker, cpu).map(|r| r.text)
}
/// What one recognizer request returned.
pub struct Recognition {
    pub text: String,
    /// The language the recognizer detected, only when the request asked for `auto`.
    pub language: Option<String>,
}
pub fn recognize(
    workers: &Path,
    model: &Path,
    wav: Vec<u8>,
    hint: &str,
    language: &str,
    worker: &Worker,
    cpu: bool,
) -> Result<Recognition, String> {
    let detecting = language == crate::languages::AUTO;
    let lease = acquire(workers, model, Kind::Whisper, worker, cpu)?;
    let (url, token) = (&lease.server.endpoint, &lease.server.token);
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|_| "Cannot initialize recognizer IPC")?;
    let mut form = reqwest::blocking::multipart::Form::new()
        .part(
            "file",
            reqwest::blocking::multipart::Part::bytes(wav).file_name("recording.wav"),
        )
        .text(
            "response_format",
            // Only the verbose format names the language the recognizer used.
            if detecting { "verbose_json" } else { "json" },
        )
        .text("temperature", "0")
        .text("language", language.to_owned());
    if detecting {
        // The probabilities cost a second pass over the audio, and only the language is needed.
        form = form.text("no_language_probabilities", "true");
    }
    if !hint.is_empty() {
        form = form.text("prompt", hint.to_owned());
    }
    let value: Value = client
        .post(format!("{url}/inference"))
        .bearer_auth(token)
        .multipart(form)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .map_err(|_| "Local recognition failed or timed out")?;
    let text = value["text"]
        .as_str()
        .map(flatten_segments)
        .ok_or("Recognizer returned invalid output")?;
    let language = value["language"]
        .as_str()
        .and_then(crate::languages::code_for_name)
        .filter(|_| detecting)
        .map(String::from);
    Ok(Recognition { text, language })
}
/// The recognizer of one recording. With Auto-detect, the first chunk that has speech is
/// recognized with detection and every later chunk of the recording reuses the language it
/// found, because a short chunk is often misdetected. A speculative call recognizes only the
/// start of a chunk still being spoken, so it uses the language but never sets it.
pub struct ChunkRecognizer {
    workers: PathBuf,
    model: PathBuf,
    hint: String,
    worker: Worker,
    cpu: bool,
    configured: String,
    locked: Mutex<Option<String>>,
}
impl ChunkRecognizer {
    pub fn new(
        language: &str,
        workers: &Path,
        model: &Path,
        hint: &str,
        worker: &Worker,
        cpu: bool,
    ) -> Self {
        Self {
            workers: workers.into(),
            model: model.into(),
            hint: hint.into(),
            worker: worker.clone(),
            cpu,
            configured: language.into(),
            locked: Mutex::new(None),
        }
    }
    /// Recognizes a chunk that is final. The first one with text fixes the language.
    pub fn transcribe(&self, wav: Vec<u8>) -> Result<String, String> {
        self.run(wav, false)
    }
    /// Recognizes audio that may be extended or replaced, so it never fixes the language.
    pub fn transcribe_speculative(&self, wav: Vec<u8>) -> Result<String, String> {
        self.run(wav, true)
    }
    fn run(&self, wav: Vec<u8>, speculative: bool) -> Result<String, String> {
        let language = self
            .locked
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| self.configured.clone());
        let recognition = recognize(
            &self.workers,
            &self.model,
            wav,
            &self.hint,
            &language,
            &self.worker,
            self.cpu,
        )?;
        if self.configured == crate::languages::AUTO
            && !speculative
            && !recognition.text.trim().is_empty()
            && let Some(found) = recognition.language
        {
            self.locked.lock().unwrap().get_or_insert(found);
        }
        Ok(recognition.text)
    }
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
/// Enhanced cleanup of one part of a dictation. A part that Basic cleanup
/// empties, such as "um, uh", contributes no text instead of failing the
/// whole dictation.
pub fn enhanced_part(request: CleanupRequest<'_>) -> Result<String, String> {
    let cleanup = &request.snapshot.cleanup;
    if cleanup.enhanced != "disabled" && !request.raw.contains("CONCHPROTECTED") {
        let masked = core::mask(request.raw, request.snapshot);
        if core::basic(&masked.text, cleanup, &request.snapshot.language)
            .trim()
            .is_empty()
        {
            return Ok(String::new());
        }
    }
    enhanced(request)
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
    if cleanup.enhanced == "managed" && !managed_cleanup_supported(&snapshot.language) {
        return Err("Managed cleanup supports English only; using Basic".into());
    }
    if worker.lock().unwrap().cancelled {
        return Err("Cancelled".into());
    }
    if raw.contains("CONCHPROTECTED") {
        return Err("Reserved placeholder in transcript; using Basic".into());
    }
    let masked = core::mask(raw, snapshot);
    let basic = core::basic(&masked.text, cleanup, &snapshot.language);
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
            format!("{CONTROL_LINE}\n{basic}"),
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
    worker.lock().unwrap().cleanup_model_time = model_time(&value);
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
    /// Shown in the interface.
    pub label: &'static str,
    pub note: &'static str,
    /// The worker that runs the model. Voice detection has none: the recognizer loads it.
    pub engine: Option<Kind>,
    /// ISO codes of the languages the model recognizes.
    pub languages: &'static [&'static str],
}
impl Model {
    /// Whether the model recognizes `language`, an ISO code or `auto`. Only a
    /// model with more than one language can detect the language.
    pub fn supports(&self, language: &str) -> bool {
        if language == crate::languages::AUTO {
            return self.languages.len() > 1;
        }
        self.languages.contains(&language)
    }
}
/// What the interface shows for a model, with whether it is on disk.
#[derive(serde::Serialize)]
pub struct ModelView {
    pub id: &'static str,
    pub label: &'static str,
    pub note: &'static str,
    pub size: u64,
    pub engine: Option<Kind>,
    pub languages: &'static [&'static str],
    pub ready: bool,
}
pub fn catalog(data: &Path) -> Vec<ModelView> {
    MODELS
        .iter()
        .map(|m| ModelView {
            id: m.id,
            label: m.label,
            note: m.note,
            size: m.size,
            engine: m.engine,
            languages: m.languages,
            ready: data.join(m.name).is_file(),
        })
        .collect()
}
/// Recognizers the user can choose between: the English-only Whisper models,
/// then the multilingual ones, each smallest first. Larger models make fewer
/// errors and take longer to recognize a chunk.
pub const RECOGNIZERS: [Model; 7] = [
    Model {
        id: "tiny.en",
        name: "ggml-tiny.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-tiny.en.bin",
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        size: 77704715,
        label: "Tiny (English)",
        note: "Fastest",
        engine: Some(Kind::Whisper),
        languages: ENGLISH,
    },
    Model {
        id: "base.en",
        name: "ggml-base.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.en.bin",
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        size: 147964211,
        label: "Base (English)",
        note: "Balanced",
        engine: Some(Kind::Whisper),
        languages: ENGLISH,
    },
    Model {
        id: "small.en",
        name: "ggml-small.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.en.bin",
        sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
        size: 487614201,
        label: "Small (English)",
        note: "Most accurate",
        engine: Some(Kind::Whisper),
        languages: ENGLISH,
    },
    Model {
        id: "tiny",
        name: "ggml-tiny.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-tiny.bin",
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        size: 77691713,
        label: "Tiny (multilingual)",
        note: "Fastest",
        engine: Some(Kind::Whisper),
        languages: &CODES_99,
    },
    Model {
        id: "base",
        name: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.bin",
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        size: 147951465,
        label: "Base (multilingual)",
        note: "Balanced",
        engine: Some(Kind::Whisper),
        languages: &CODES_99,
    },
    Model {
        id: "small",
        name: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.bin",
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        size: 487601967,
        label: "Small (multilingual)",
        note: "Accurate",
        engine: Some(Kind::Whisper),
        languages: &CODES_99,
    },
    Model {
        id: "large-v3-turbo",
        name: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size: 574041195,
        label: "Large Turbo (multilingual)",
        note: "Most accurate",
        engine: Some(Kind::Whisper),
        languages: &CODES,
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
    label: "Voice detection (Silero)",
    note: "",
    engine: None,
    languages: &[],
};
pub const CLEANUP: Model = Model {
    id: "s1-mini",
    name: "s1-mini-q4_k_m.gguf",
    url: "https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf",
    sha256: "3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634",
    size: 484219808,
    label: "S1-mini by Superwhisper",
    note: "",
    engine: Some(Kind::Llama),
    languages: ENGLISH,
};
pub const MODELS: [Model; 9] = [
    RECOGNIZERS[0],
    RECOGNIZERS[1],
    RECOGNIZERS[2],
    RECOGNIZERS[3],
    RECOGNIZERS[4],
    RECOGNIZERS[5],
    RECOGNIZERS[6],
    VAD,
    CLEANUP,
];
pub fn recognizer(id: &str) -> Option<Model> {
    RECOGNIZERS.iter().find(|m| m.id == id).copied()
}
/// The smallest recognition model that supports `language`.
pub fn smallest_supporting(language: &str) -> Option<Model> {
    RECOGNIZERS
        .iter()
        .filter(|m| m.supports(language))
        .min_by_key(|m| m.size)
        .copied()
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
    fn a_part_that_basic_cleanup_empties_cleans_to_nothing_without_a_worker() {
        let dir = tempfile::tempdir().unwrap();
        let mut snapshot = crate::core::Snapshot::default();
        snapshot.cleanup.enhanced = "managed".into();
        let part = |raw: &str, snapshot: &crate::core::Snapshot| {
            enhanced_part(CleanupRequest {
                workers: dir.path(),
                data: dir.path(),
                snapshot,
                raw,
                worker: &new_worker(),
                cpu: true,
            })
        };
        assert_eq!(part("um, uh", &snapshot), Ok(String::new()));
        // A part with words reaches the worker, which is not installed here.
        assert!(part("rename the helper", &snapshot).is_err());
        // Disabled cleanup stays an error for every part.
        snapshot.cleanup.enhanced = "disabled".into();
        assert!(part("um, uh", &snapshot).is_err());
    }
    #[test]
    fn the_model_time_adds_prompt_and_generation_and_is_absent_without_timings() {
        let reply = json!({"timings": {"prompt_ms": 12.5, "predicted_ms": 100.25}});
        assert_eq!(model_time(&reply).map(|t| t.as_millis()), Some(112));
        assert_eq!(model_time(&json!({"choices": []})), None);
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
        reply = os.path.join(here, "reply.json")
        if os.path.exists(reply):
            body = open(reply, "rb").read()
        else:
            body = json.dumps({"text": str(os.getpid())}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(body)
http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
"#;
        const LLAMA_SCRIPT: &str = r#"#!/usr/bin/env python3
import http.server, json, os, sys
port = int(sys.argv[sys.argv.index("--port") + 1])
here = os.path.dirname(os.path.abspath(__file__))
class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
    def do_POST(self):
        request = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        if self.path.endswith("/tokenize"):
            body = {"tokens": [1, 2, 3]}
        else:
            with open(os.path.join(here, "last-chat.json"), "wb") as saved:
                saved.write(request)
            user = json.loads(request)["messages"][-1]["content"]
            body = {
                "choices": [{"message": {"content": user.split("\n", 1)[-1]}}],
                "timings": {"prompt_ms": 12.5, "predicted_ms": 100.25},
            }
        data = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(data)
http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
"#;
        /// Installs a `llama-server` that echoes the text after the control
        /// line, with fixed timings, and the managed cleanup model file.
        pub fn install_cleanup(dir: &Path) {
            let exe = dir.join("llama-server");
            std::fs::write(&exe, LLAMA_SCRIPT).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(dir.join(super::CLEANUP.name), b"model").unwrap();
        }
        pub fn last_chat(dir: &Path) -> String {
            String::from_utf8_lossy(&std::fs::read(dir.join("last-chat.json")).unwrap())
                .into_owned()
        }
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
        /// Makes the fake answer with this JSON until the next call.
        pub fn reply(dir: &Path, json: &str) {
            std::fs::write(dir.join("reply.json"), json).unwrap();
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
    #[test]
    fn enhanced_cleanup_keeps_the_time_the_model_reported() {
        let dir = tempfile::tempdir().unwrap();
        fake::install_cleanup(dir.path());
        let mut snapshot = crate::core::Snapshot::default();
        snapshot.cleanup.enhanced = "managed".into();
        let worker = new_worker();
        assert_eq!(cleanup_model_time(&worker), None);
        let cleaned = enhanced(CleanupRequest {
            workers: dir.path(),
            data: dir.path(),
            snapshot: &snapshot,
            raw: "rename the helper",
            worker: &worker,
            cpu: true,
        });
        shutdown(dir.path());
        assert_eq!(cleaned, Ok("rename the helper".into()));
        assert_eq!(
            cleanup_model_time(&worker).map(|time| time.as_millis()),
            Some(112)
        );
    }
    #[cfg(unix)]
    #[test]
    fn warming_the_cleanup_model_sends_one_token_with_the_cleanup_system_prompt() {
        let dir = tempfile::tempdir().unwrap();
        fake::install_cleanup(dir.path());
        let model = dir.path().join(CLEANUP.name);
        warm_cleanup(dir.path(), &model, &new_worker());
        let request = fake::last_chat(dir.path());
        shutdown(dir.path());
        assert!(request.contains("\"max_tokens\":1"), "{request}");
        assert!(request.contains(S1_SYSTEM), "{request}");
        assert!(request.contains("[Styling: semi-formal]"), "{request}");
    }
    #[cfg(unix)]
    fn run(dir: &Path, worker: &Worker) -> String {
        transcribe(
            dir,
            &dir.join("model.bin"),
            vec![0; 64],
            "",
            "en",
            worker,
            true,
        )
        .unwrap()
    }
    #[cfg(unix)]
    #[test]
    fn recognizer_receives_the_vocabulary_hint_as_its_initial_prompt() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let model = dir.path().join("model.bin");
        transcribe(
            dir.path(),
            &model,
            vec![0; 64],
            "",
            "en",
            &new_worker(),
            true,
        )
        .unwrap();
        let without = fake::last_request(dir.path());
        assert!(!without.contains("name=\"prompt\""));
        let hint = "Vocabulary: podGC, maxGroups.";
        transcribe(
            dir.path(),
            &model,
            vec![0; 64],
            hint,
            "en",
            &new_worker(),
            true,
        )
        .unwrap();
        let with = fake::last_request(dir.path());
        assert!(with.contains("name=\"prompt\""));
        assert!(with.contains(hint));
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn a_request_never_overrides_the_window_or_the_decoding_of_the_recognizer() {
        // A per-request `audio_ctx` made the worker drop most of the text of a
        // later request (see `docs/validation/latency.md`), so none is sent.
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let model = dir.path().join("model.bin");
        for seconds in [2, 31] {
            transcribe(
                dir.path(),
                &model,
                silence(seconds),
                "",
                "en",
                &new_worker(),
                true,
            )
            .unwrap();
            let request = fake::last_request(dir.path());
            for field in ["audio_ctx", "no_timestamps", "temperature_inc"] {
                assert!(
                    !request.contains(&format!("name=\"{field}\"")),
                    "{seconds} s audio sent {field}"
                );
            }
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
            "en",
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
        assert_eq!(
            ids,
            [
                "tiny.en",
                "base.en",
                "small.en",
                "tiny",
                "base",
                "small",
                "large-v3-turbo"
            ]
        );
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
    /// Sizes and SHA-256 values from the Hugging Face listing of `ggerganov/whisper.cpp`
    /// at the pinned revision, checked when each row was added.
    #[test]
    fn multilingual_whisper_rows_match_the_pinned_listing() {
        let listing = [
            (
                "tiny",
                "ggml-tiny.bin",
                77691713,
                "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
            ),
            (
                "base",
                "ggml-base.bin",
                147951465,
                "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
            ),
            (
                "small",
                "ggml-small.bin",
                487601967,
                "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
            ),
            (
                "large-v3-turbo",
                "ggml-large-v3-turbo-q5_0.bin",
                574041195,
                "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
            ),
        ];
        for (id, name, size, sha256) in listing {
            let model = recognizer(id).unwrap_or_else(|| panic!("{id} is missing"));
            assert_eq!(model.name, name);
            assert_eq!(model.size, size, "{id}");
            assert_eq!(model.sha256, sha256, "{id}");
            assert_eq!(
                model.url,
                format!(
                    "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/{name}"
                )
            );
            assert_eq!(model.engine, Some(Kind::Whisper));
        }
    }
    /// `n_vocab` in the ggml header decides the language count: 51865 is 99 languages and
    /// 51866 is 100. Cantonese (`yue`, id 99) exists only in the 100-language models, so
    /// on the others its id selects the translate token. Read from the pinned files.
    #[test]
    fn only_large_v3_turbo_lists_cantonese_and_the_other_multilingual_models_list_99_languages() {
        use crate::languages::LANGUAGES;
        let all: Vec<_> = LANGUAGES.iter().map(|l| l.code).collect();
        assert_eq!(all.last(), Some(&"yue"));
        for model in RECOGNIZERS {
            match model.id {
                id if id.ends_with(".en") => assert_eq!(model.languages, ["en"], "{id}"),
                "large-v3-turbo" => {
                    assert_eq!(model.languages, all, "{}", model.id);
                    assert!(model.supports("yue"));
                }
                id => {
                    assert_eq!(model.languages, &all[..99], "{id}");
                    assert!(!model.supports("yue"), "{id}");
                    assert!(model.supports("fr") && model.supports("auto"), "{id}");
                }
            }
        }
    }
    #[test]
    fn the_smallest_model_for_a_language_is_the_smallest_download_that_supports_it() {
        assert_eq!(smallest_supporting("fr").unwrap().id, "tiny");
        assert_eq!(smallest_supporting("auto").unwrap().id, "tiny");
        assert_eq!(smallest_supporting("yue").unwrap().id, "large-v3-turbo");
        assert!(smallest_supporting("xx").is_none());
    }
    #[cfg(unix)]
    #[test]
    fn the_recognizer_receives_the_language_with_each_request() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let model = dir.path().join("model.bin");
        for language in ["fr", "auto"] {
            transcribe(
                dir.path(),
                &model,
                vec![0; 64],
                "",
                language,
                &new_worker(),
                true,
            )
            .unwrap();
            let request = fake::last_request(dir.path());
            assert!(
                request.contains(&format!("name=\"language\"\r\n\r\n{language}\r\n")),
                "{request}"
            );
        }
        shutdown(dir.path());
    }
    fn cleanup_request(
        language: &str,
        enhanced: &str,
        endpoint: &str,
        dir: &Path,
    ) -> Result<String, String> {
        let mut snapshot = Snapshot {
            language: language.into(),
            ..Default::default()
        };
        snapshot.cleanup.enhanced = enhanced.into();
        snapshot.cleanup.endpoint = endpoint.into();
        enhanced_cleanup(CleanupRequest {
            workers: dir,
            data: dir,
            snapshot: &snapshot,
            raw: "hello there",
            worker: &new_worker(),
            cpu: true,
        })
    }
    use super::enhanced as enhanced_cleanup;
    const ENGLISH_ONLY: &str = "Managed cleanup supports English only; using Basic";
    /// Answers one chat-completions request on a loopback port, as a local cleanup server would.
    fn serve_cleanup(answer: &'static str) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!(
            "http://127.0.0.1:{}/v1/chat/completions",
            listener.local_addr().unwrap().port()
        );
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = vec![];
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(str::to_owned)
                        })
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                assert!(n > 0, "request ended early");
            }
            let body = json!({"choices": [{"message": {"content": answer}}]}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        (endpoint, thread)
    }
    /// Behavior table row "Managed S1-mini cleanup".
    #[test]
    fn managed_cleanup_is_available_in_english() {
        let dir = tempfile::tempdir().unwrap();
        assert!(managed_cleanup_supported("en"));
        // English proceeds to the worker, which is missing here.
        let error = cleanup_request("en", "managed", "", dir.path()).unwrap_err();
        assert!(
            error.starts_with("Bundled inference worker is missing"),
            "{error}"
        );
    }
    #[test]
    fn managed_cleanup_falls_back_to_basic_with_a_status_in_another_language() {
        let dir = tempfile::tempdir().unwrap();
        for language in ["de", "pt", "yue"] {
            assert!(!managed_cleanup_supported(language), "{language}");
            let error = cleanup_request(language, "managed", "", dir.path()).unwrap_err();
            assert_eq!(error, ENGLISH_ONLY, "{language}");
        }
    }
    #[test]
    fn managed_cleanup_falls_back_to_basic_with_a_status_in_auto_detect() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!managed_cleanup_supported("auto"));
        let error = cleanup_request("auto", "managed", "", dir.path()).unwrap_err();
        assert_eq!(error, ENGLISH_ONLY);
    }
    /// Behavior table row "Local or remote cleanup endpoint". A local endpoint answers in
    /// every column. A remote one needs HTTPS and the user's confirmation, which no language
    /// changes, so the test checks that language never produces the English-only status.
    fn local_endpoint_answers(language: &str) {
        let dir = tempfile::tempdir().unwrap();
        let (endpoint, server) = serve_cleanup("Hello there.");
        let result = cleanup_request(language, "local", &endpoint, dir.path());
        server.join().unwrap();
        assert_eq!(result.unwrap(), "Hello there.", "{language}");
    }
    #[test]
    fn a_local_cleanup_endpoint_answers_in_english() {
        local_endpoint_answers("en");
    }
    #[test]
    fn a_local_cleanup_endpoint_answers_in_another_language() {
        local_endpoint_answers("de");
    }
    #[test]
    fn a_local_cleanup_endpoint_answers_in_auto_detect() {
        local_endpoint_answers("auto");
    }
    #[test]
    fn a_remote_cleanup_endpoint_is_judged_by_its_own_rules_in_every_language() {
        let dir = tempfile::tempdir().unwrap();
        for language in ["en", "de", "auto"] {
            let error = cleanup_request(language, "remote", "https://example.com/v1", dir.path())
                .unwrap_err();
            assert_eq!(error, "Remote cleanup is disabled", "{language}");
        }
    }
    fn language_field(request: &str, value: &str) -> bool {
        request.contains(&format!("name=\"language\"\r\n\r\n{value}\r\n"))
    }
    #[cfg(unix)]
    #[test]
    fn an_auto_request_asks_for_the_detected_language_and_other_requests_do_not() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        fake::reply(dir.path(), r#"{"text":"guten tag","language":"german"}"#);
        let model = dir.path().join("model.bin");
        let auto = recognize(
            dir.path(),
            &model,
            vec![0; 64],
            "",
            "auto",
            &new_worker(),
            true,
        )
        .unwrap();
        assert_eq!(auto.text, "guten tag");
        assert_eq!(auto.language.as_deref(), Some("de"));
        let request = fake::last_request(dir.path());
        assert!(language_field(&request, "auto"), "{request}");
        assert!(request.contains("name=\"response_format\"\r\n\r\nverbose_json\r\n"));
        assert!(request.contains("name=\"no_language_probabilities\"\r\n\r\ntrue\r\n"));
        let fixed = recognize(
            dir.path(),
            &model,
            vec![0; 64],
            "",
            "fr",
            &new_worker(),
            true,
        )
        .unwrap();
        assert_eq!(fixed.language, None);
        let request = fake::last_request(dir.path());
        assert!(request.contains("name=\"response_format\"\r\n\r\njson\r\n"));
        assert!(!request.contains("no_language_probabilities"));
        shutdown(dir.path());
    }
    #[cfg(unix)]
    fn chunk_recognizer(dir: &Path, language: &str) -> ChunkRecognizer {
        ChunkRecognizer::new(
            language,
            dir,
            &dir.join("model.bin"),
            "",
            &new_worker(),
            true,
        )
    }
    #[cfg(unix)]
    #[test]
    fn a_recording_reuses_the_language_its_first_chunk_with_speech_detected() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let recording = chunk_recognizer(dir.path(), "auto");
        let send = |reply: &str| {
            fake::reply(dir.path(), reply);
            let text = recording.transcribe(vec![0; 64]).unwrap();
            (text, fake::last_request(dir.path()))
        };
        // A chunk without speech detects nothing worth keeping.
        let (text, request) = send(r#"{"text":"","language":"french"}"#);
        assert_eq!(text, "");
        assert!(language_field(&request, "auto"));
        let (text, request) = send(r#"{"text":"guten tag","language":"german"}"#);
        assert_eq!(text, "guten tag");
        assert!(language_field(&request, "auto"));
        // Later chunks reuse German even when the recognizer would now say French.
        let (_, request) = send(r#"{"text":"ok","language":"french"}"#);
        assert!(language_field(&request, "de"), "{request}");
        let (_, request) = send(r#"{"text":"ok","language":"french"}"#);
        assert!(language_field(&request, "de"), "{request}");
        // The next recording starts with detection again.
        let next = chunk_recognizer(dir.path(), "auto");
        fake::reply(dir.path(), r#"{"text":"salut","language":"french"}"#);
        next.transcribe(vec![0; 64]).unwrap();
        assert!(language_field(&fake::last_request(dir.path()), "auto"));
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn a_speculative_chunk_uses_the_language_but_never_sets_it() {
        // A speculation recognizes only the start of a chunk that is still being spoken, which
        // is too short to detect from reliably.
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let recording = chunk_recognizer(dir.path(), "auto");
        let run = |reply: &str, speculative: bool| {
            fake::reply(dir.path(), reply);
            if speculative {
                recording.transcribe_speculative(vec![0; 64]).unwrap();
            } else {
                recording.transcribe(vec![0; 64]).unwrap();
            }
            fake::last_request(dir.path())
        };
        let request = run(r#"{"text":"salut","language":"french"}"#, true);
        assert!(language_field(&request, "auto"));
        let request = run(r#"{"text":"salut","language":"french"}"#, true);
        assert!(
            language_field(&request, "auto"),
            "a speculation must not lock"
        );
        let request = run(r#"{"text":"guten tag","language":"german"}"#, false);
        assert!(language_field(&request, "auto"));
        let request = run(r#"{"text":"ok","language":"french"}"#, true);
        assert!(
            language_field(&request, "de"),
            "a speculation reuses the lock"
        );
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn only_a_chunk_with_text_and_a_known_language_sets_the_language() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let recording = chunk_recognizer(dir.path(), "auto");
        let attempt = |reply: &str| {
            fake::reply(dir.path(), reply);
            let result = recording.transcribe(vec![0; 64]);
            (result, fake::last_request(dir.path()))
        };
        // Text that is only whitespace has no speech to detect from.
        let (result, _) = attempt("{\"text\":\" \\n \",\"language\":\"french\"}");
        assert_eq!(result.unwrap(), "");
        // The recognizer named a language that is not in the table.
        let (result, _) = attempt(r#"{"text":"qapla","language":"klingon"}"#);
        assert_eq!(result.unwrap(), "qapla");
        // The request failed.
        let (result, _) = attempt("not json");
        assert!(result.is_err());
        // Every one of those left the language open.
        let (_, request) = attempt(r#"{"text":"bonjour","language":"french"}"#);
        assert!(language_field(&request, "auto"), "{request}");
        let (_, request) = attempt(r#"{"text":"salut","language":"german"}"#);
        assert!(language_field(&request, "fr"), "{request}");
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn a_chosen_language_is_never_replaced_by_a_detected_one() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        fake::reply(dir.path(), r#"{"text":"hola","language":"german"}"#);
        let recording = chunk_recognizer(dir.path(), "es");
        for _ in 0..2 {
            recording.transcribe(vec![0; 64]).unwrap();
            assert!(language_field(&fake::last_request(dir.path()), "es"));
        }
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn the_recognizer_starts_without_a_fixed_language() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        run(dir.path(), &new_worker());
        let args = fake::arguments(dir.path());
        assert!(!args.iter().any(|a| a == "--language"), "{args:?}");
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn changing_the_language_reuses_the_recognizer_process() {
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let model = dir.path().join("model.bin");
        let first = transcribe(
            dir.path(),
            &model,
            vec![0; 64],
            "",
            "en",
            &new_worker(),
            true,
        )
        .unwrap();
        let second = transcribe(
            dir.path(),
            &model,
            vec![0; 64],
            "",
            "de",
            &new_worker(),
            true,
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(fake::starts(dir.path()), 1);
        shutdown(dir.path());
    }
    /// `g_lang` in `src/whisper.cpp` at the revision `scripts/build-runtimes.py` pins,
    /// as `code:name` in table order, with `_` for spaces.
    const WHISPER_TABLE: &str = concat!(
        "en:english zh:chinese de:german es:spanish ru:russian ko:korean fr:french ja:japanese ",
        "pt:portuguese tr:turkish pl:polish ca:catalan nl:dutch ar:arabic sv:swedish it:italian ",
        "id:indonesian hi:hindi fi:finnish vi:vietnamese he:hebrew uk:ukrainian el:greek ms:malay ",
        "cs:czech ro:romanian da:danish hu:hungarian ta:tamil no:norwegian th:thai ur:urdu ",
        "hr:croatian bg:bulgarian lt:lithuanian la:latin mi:maori ml:malayalam cy:welsh sk:slovak ",
        "te:telugu fa:persian lv:latvian bn:bengali sr:serbian az:azerbaijani sl:slovenian ",
        "kn:kannada et:estonian mk:macedonian br:breton eu:basque is:icelandic hy:armenian ",
        "ne:nepali mn:mongolian bs:bosnian kk:kazakh sq:albanian sw:swahili gl:galician ",
        "mr:marathi pa:punjabi si:sinhala km:khmer sn:shona yo:yoruba so:somali af:afrikaans ",
        "oc:occitan ka:georgian be:belarusian tg:tajik sd:sindhi gu:gujarati am:amharic ",
        "yi:yiddish lo:lao uz:uzbek fo:faroese ht:haitian_creole ps:pashto tk:turkmen nn:nynorsk ",
        "mt:maltese sa:sanskrit lb:luxembourgish my:myanmar bo:tibetan tl:tagalog mg:malagasy ",
        "as:assamese tt:tatar haw:hawaiian ln:lingala ha:hausa ba:bashkir jw:javanese ",
        "su:sundanese yue:cantonese"
    );
    #[test]
    fn the_language_table_is_the_whisper_cpp_table() {
        use crate::languages::LANGUAGES;
        let expected: Vec<_> = WHISPER_TABLE.split(' ').collect();
        assert_eq!(expected.len(), 100);
        let actual: Vec<_> = LANGUAGES
            .iter()
            .map(|l| {
                let (code, name) = (l.code, l.name.to_lowercase().replace(' ', "_"));
                format!("{code}:{name}")
            })
            .collect();
        assert_eq!(actual, expected);
        for language in LANGUAGES {
            assert!(
                language
                    .name
                    .split(' ')
                    .all(|word| word.starts_with(|c: char| c.is_ascii_uppercase())),
                "{}",
                language.name
            );
        }
        assert_eq!(LANGUAGES[0].name, "English");
        assert!(!LANGUAGES.iter().any(|l| l.code == "auto"));
    }
    #[test]
    fn every_model_declares_its_engine_and_languages() {
        use crate::languages::LANGUAGES;
        for model in RECOGNIZERS {
            assert!(!model.languages.is_empty(), "{}", model.id);
            assert_eq!(model.engine, Some(Kind::Whisper), "{}", model.id);
            assert!(
                !model.label.is_empty() && !model.note.is_empty(),
                "{}",
                model.id
            );
            let mut codes = model.languages.to_vec();
            codes.sort();
            codes.dedup();
            assert_eq!(codes.len(), model.languages.len(), "{}", model.id);
            for code in model.languages {
                assert!(LANGUAGES.iter().any(|l| l.code == *code), "{}", code);
            }
            if model.id.ends_with(".en") {
                assert_eq!(model.languages, ["en"], "{}", model.id);
            }
        }
        assert_eq!(CLEANUP.engine, Some(Kind::Llama));
        assert_eq!(CLEANUP.languages, ["en"]);
        assert_eq!(VAD.engine, None);
        assert!(VAD.languages.is_empty());
    }
    #[test]
    fn auto_is_valid_only_for_a_model_with_more_than_one_language() {
        for model in RECOGNIZERS {
            assert_eq!(
                model.supports("auto"),
                model.languages.len() > 1,
                "{}",
                model.id
            );
            assert!(model.supports("en"), "{}", model.id);
            assert!(!model.supports(""), "{}", model.id);
            assert!(!model.supports("xx"), "{}", model.id);
        }
        let english = recognizer("small.en").unwrap();
        assert!(!english.supports("fr"));
        assert!(!english.supports("auto"));
        let multilingual = Model {
            languages: &["en", "fr"],
            ..english
        };
        assert!(multilingual.supports("auto"));
        assert!(multilingual.supports("fr"));
        assert!(!multilingual.supports("de"));
    }
    #[test]
    fn the_catalog_gives_the_interface_each_models_metadata_and_install_state() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ggml-base.en.bin"), b"x").unwrap();
        let catalog = serde_json::to_value(catalog(dir.path())).unwrap();
        let ids: Vec<_> = catalog
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "tiny.en",
                "base.en",
                "small.en",
                "tiny",
                "base",
                "small",
                "large-v3-turbo",
                "silero-vad",
                "s1-mini"
            ]
        );
        assert_eq!(catalog[3]["label"], "Tiny (multilingual)");
        assert_eq!(catalog[3]["languages"].as_array().unwrap().len(), 99);
        assert_eq!(catalog[6]["languages"].as_array().unwrap().len(), 100);
        assert_eq!(
            catalog[0],
            json!({
                "id": "tiny.en",
                "label": "Tiny (English)",
                "note": "Fastest",
                "size": 77704715,
                "engine": "whisper",
                "languages": ["en"],
                "ready": false,
            })
        );
        assert_eq!(catalog[1]["ready"], true);
        assert_eq!(catalog[7]["engine"], Value::Null);
        assert_eq!(catalog[8]["engine"], "llama");
        assert_eq!(catalog[8]["label"], "S1-mini by Superwhisper");
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
                "en",
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
