use crate::lock::Locked;
use crate::{
    core::{self, Snapshot},
    languages::{CODES, CODES_99, ENGLISH},
    log,
};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
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
    worker.locked().cleanup_model_time
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
/// The last lines a worker wrote to stderr, kept for the log when it fails.
#[derive(Default)]
struct Tail {
    lines: std::collections::VecDeque<String>,
}
/// Whether a line of worker output may reach the log. Workers can print the
/// text they process (the llama server does at debug verbosity), so only lines
/// from the recognizer's own libraries are kept, and a cleanup worker's output
/// never is.
fn loggable(kind: Kind, line: &str) -> bool {
    static LIBRARY: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^(?:ggml_|GGML_|whisper_init|whisper_model_load|whisper_backend|whisper_vad|error:|ERROR|fatal|terminate called)").unwrap()
    });
    kind == Kind::Whisper && LIBRARY.is_match(line.trim_start())
}
const TAIL_LINES: usize = 20;
const TAIL_WIDTH: usize = 300;
impl Tail {
    fn push(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let line = if line.chars().count() > TAIL_WIDTH {
            let mut cut: String = line.chars().take(TAIL_WIDTH - 3).collect();
            cut.push_str("...");
            cut
        } else {
            line.to_owned()
        };
        self.lines.push_back(line);
        while self.lines.len() > TAIL_LINES {
            self.lines.pop_front();
        }
    }
    fn text(&self) -> Option<String> {
        (!self.lines.is_empty()).then(|| self.lines.iter().cloned().collect::<Vec<_>>().join("\n"))
    }
}
pub struct Server {
    kind: Kind,
    /// The executable's file name, for the log.
    name: String,
    /// The model's file name, for the log.
    model: String,
    child: Mutex<Child>,
    /// The last lines the worker wrote to stderr.
    tail: Arc<Mutex<Tail>>,
    /// Set before Conch stops the worker, so its exit is not reported as a failure.
    killed: Arc<AtomicBool>,
    /// Set when the worker's stderr has ended.
    drained: Arc<AtomicBool>,
    endpoint: String,
    token: String,
    used: Mutex<Instant>,
    busy: AtomicUsize,
}
impl Server {
    fn alive(&self) -> bool {
        self.child.locked().try_wait().ok().flatten().is_none()
    }
    /// The log source for this worker's entries.
    fn source(&self) -> &'static str {
        match self.kind {
            Kind::Whisper => "recognizer",
            Kind::Llama => "cleanup",
        }
    }
    /// Stops the worker on purpose and logs the reason once. A worker that was
    /// already stopped is not logged again.
    fn stop_for(&self, reason: &str) {
        let first = !self.killed.swap(true, Ordering::SeqCst);
        self.kill();
        if first {
            log::info(
                self.source(),
                &format!("Stopped {} (model {}): {reason}", self.name, self.model),
            );
        }
    }
    fn kill(&self) {
        self.killed.store(true, Ordering::SeqCst);
        let mut child = self.child.locked();
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
        *self.server.used.locked() = Instant::now();
        self.server.busy.fetch_sub(1, Ordering::SeqCst);
    }
}
type Slot = Arc<Mutex<Option<Arc<Server>>>>;
type Key = (PathBuf, PathBuf, bool);
static POOL: LazyLock<Mutex<HashMap<Key, Slot>>> = LazyLock::new(Default::default);
fn slots(within: &Path) -> Vec<Slot> {
    POOL.locked()
        .iter()
        .filter(|((executable, _, _), _)| executable.starts_with(within))
        .map(|(_, slot)| slot.clone())
        .collect()
}
/// Stops servers under `within` that no session is using and that were last
/// used at least the given duration ago. The next request starts a fresh one.
pub fn reap_idle_within(within: &Path, whisper: Duration, llama: Duration) {
    for slot in slots(within) {
        let mut guard = slot.locked();
        let Some(server) = guard.as_ref() else {
            continue;
        };
        let limit = match server.kind {
            Kind::Whisper => whisper,
            Kind::Llama => llama,
        };
        if server.busy.load(Ordering::SeqCst) == 0 && server.used.locked().elapsed() >= limit {
            server.stop_for("it was idle");
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
        if let Some(server) = slot.locked().take() {
            server.stop_for("Conch is exiting or installing an update");
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
        let mut state = worker.locked();
        state.cancelled = true;
        std::mem::take(&mut state.using)
    };
    for server in using {
        server.stop_for("the recording was cancelled");
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
    if worker.locked().cancelled {
        return Err("Cancelled".into());
    }
    let slot = POOL
        .locked()
        .entry((executable.clone(), model.to_owned(), cpu))
        .or_default()
        .clone();
    let mut guard = slot.locked();
    let server = match guard.as_ref().filter(|server| server.alive()) {
        Some(server) => server.clone(),
        None => {
            let server = Arc::new(launch(&executable, model, kind, worker, cpu)?);
            *guard = Some(server.clone());
            server
        }
    };
    let lease = Lease::new(server.clone());
    let mut state = worker.locked();
    if state.cancelled {
        drop(state);
        server.stop_for("the recording was cancelled");
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
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot launch inference worker")?;
    let name = executable
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let tail = Arc::new(Mutex::new(Tail::default()));
    let killed = Arc::new(AtomicBool::new(false));
    let drained = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    if let Some(stderr) = child.stderr.take() {
        let (tail, killed, drained, started, name) = (
            tail.clone(),
            killed.clone(),
            drained.clone(),
            started.clone(),
            name.clone(),
        );
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stderr);
            let mut line = Vec::new();
            // Drain the pipe for as long as the worker runs, or it blocks on a full pipe.
            while reader.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
                let text = String::from_utf8_lossy(&line);
                if loggable(kind, &text) {
                    tail.locked().push(&text);
                }
                line.clear();
            }
            drained.store(true, Ordering::SeqCst);
            // Before the worker is ready, `launch` reports its failure with this output.
            if started.load(Ordering::SeqCst) && !killed.load(Ordering::SeqCst) {
                let detail = tail.locked().text();
                let source = if kind == Kind::Whisper {
                    "recognizer"
                } else {
                    "cleanup"
                };
                log::record(
                    log::Level::Warn,
                    source,
                    &format!("Worker {name} exited unexpectedly"),
                    detail.as_deref(),
                );
            }
        });
    } else {
        drained.store(true, Ordering::SeqCst);
    }
    let model_name = model
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let server = Server {
        kind,
        name,
        model: model_name,
        child: Mutex::new(child),
        tail,
        killed,
        drained,
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
        let cancelled = worker.locked().cancelled;
        if cancelled || !server.alive() {
            let reason = "Inference worker stopped or was cancelled";
            if cancelled {
                server.stop_for("the recording was cancelled");
            } else {
                launch_failed(&server, "it exited");
            }
            return Err(reason.into());
        }
        if client
            .get(format!("{}/health", server.endpoint))
            .bearer_auth(&server.token)
            .send()
            .is_ok_and(|r| r.status().is_success())
        {
            started.store(true, Ordering::SeqCst);
            log::info(
                server.source(),
                &format!(
                    "Worker started: {}, model {}, ready in {} ms",
                    server.name,
                    server.model,
                    start.elapsed().as_millis()
                ),
            );
            return Ok(server);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    launch_failed(&server, "it did not answer within 45 seconds");
    Err("Inference worker startup timed out".into())
}
/// Stops a worker that did not become ready and logs why, with the last lines
/// it wrote to stderr.
fn launch_failed(server: &Server, reason: &str) {
    server.kill();
    // The output ends once the process is gone; give the reader a moment to
    // take the last lines.
    let wait = Instant::now();
    while !server.drained.load(Ordering::SeqCst) && wait.elapsed() < Duration::from_millis(500) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let detail = server.tail.locked().text();
    log::record(
        log::Level::Error,
        server.source(),
        &format!("Worker {} did not start: {reason}", server.name),
        detail.as_deref(),
    );
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
        .map(|text| crate::core::strip_hint_echo(&flatten_segments(text), hint))
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
            .locked()
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
            self.locked.locked().get_or_insert(found);
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
    if request.snapshot.enhanced != "disabled" && !request.raw.contains("CONCHPROTECTED") {
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
    if snapshot.enhanced == "disabled" {
        return Err("Enhanced cleanup is disabled".into());
    }
    if snapshot.enhanced == "managed" && !managed_cleanup_supported(&snapshot.language) {
        return Err("Managed cleanup supports English only; using Basic".into());
    }
    if worker.locked().cancelled {
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
    let (endpoint, token, system, user) = if snapshot.enhanced == "managed" {
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
        let remote = snapshot.enhanced == "remote";
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
            if worker.locked().cancelled {return Err("Cancelled".to_string());}
            let mut builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(30));
            if snapshot.enhanced != "remote" {builder = builder.no_proxy();}
            let client = builder.build().map_err(|_| "Cannot initialize cleanup client")?;
            if snapshot.enhanced == "managed" {
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
            loop {if worker.locked().cancelled {return;} tokio::time::sleep(Duration::from_millis(20)).await;}
        };
        tokio::select! {biased; _ = cancelled => Err("Cancelled".into()), result = operation => result}
    })?;
    worker.locked().cleanup_model_time = model_time(&value);
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
        let mut snapshot = crate::core::Snapshot {
            enhanced: "managed".into(),
            ..Default::default()
        };
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
        snapshot.enhanced = "disabled".into();
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
token = "Bearer " + os.environ["CONCH_WORKER_TOKEN"]
class Handler(http.server.BaseHTTPRequestHandler):
    def parse_request(self):
        # Like the real servers, answer only the token Conch launched this
        # one with, so a test never takes another test's server on a reused
        # port for its own.
        if not super().parse_request():
            return False
        if self.headers.get("Authorization") != token:
            self.send_error(401)
            return False
        return True
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
token = "Bearer " + sys.argv[sys.argv.index("--api-key") + 1]
class Handler(http.server.BaseHTTPRequestHandler):
    def parse_request(self):
        # Like the real servers, answer only the token Conch launched this
        # one with, so a test never takes another test's server on a reused
        # port for its own.
        if not super().parse_request():
            return False
        if self.headers.get("Authorization") != token:
            self.send_error(401)
            return False
        return True
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
        /// A recognizer that writes to stderr and exits before it serves.
        pub fn install_dying(dir: &Path, message: &str) {
            let exe = dir.join("whisper-server");
            std::fs::write(&exe, format!("#!/bin/sh\necho '{message}' >&2\nexit 1\n")).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(dir.join("model.bin"), b"model").unwrap();
            std::fs::write(dir.join(super::VAD.name), b"vad").unwrap();
        }
        /// A recognizer that answers its health check, then writes to stderr
        /// and exits half a second later.
        pub fn install_crashing(dir: &Path, message: &str) {
            let exe = dir.join("whisper-server");
            let script = SCRIPT.replace(
                "http.server.ThreadingHTTPServer",
                &format!(
                    "import threading\nthreading.Timer(0.5, lambda: (sys.stderr.write('{message}\\n'), sys.stderr.flush(), os._exit(3))).start()\nhttp.server.ThreadingHTTPServer"
                ),
            );
            std::fs::write(&exe, script).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(dir.join("model.bin"), b"model").unwrap();
            std::fs::write(dir.join(super::VAD.name), b"vad").unwrap();
        }
        /// A model file next to the fake worker under a name no other test uses,
        /// so a test finds only its own entries in the shared log.
        pub fn model(dir: &Path, name: &str) -> std::path::PathBuf {
            std::fs::write(dir.join(name), b"model").unwrap();
            dir.join(name)
        }
        /// A recognizer that takes three seconds to answer its health check.
        pub fn install_slow(dir: &Path) {
            let exe = dir.join("whisper-server");
            let script = SCRIPT.replace(
                "http.server.ThreadingHTTPServer",
                "import time\ntime.sleep(3)\nhttp.server.ThreadingHTTPServer",
            );
            std::fs::write(&exe, script).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(dir.join(super::VAD.name), b"vad").unwrap();
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
        let snapshot = crate::core::Snapshot {
            enhanced: "managed".into(),
            ..Default::default()
        };
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
    fn the_recognizer_drops_its_echo_of_the_vocabulary_hint() {
        // Recorded in a dictation: whisper answered a pause with its prompt.
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        fake::reply(
            dir.path(),
            r#"{"text": "give it a score.\n Readme, Main, C2, Phonetica, Business.\n The way I would"}"#,
        );
        let text = transcribe(
            dir.path(),
            &dir.path().join("model.bin"),
            vec![0; 64],
            "Vocabulary: README, Main, C#, Phonetica, Business, podGC.",
            "en",
            &new_worker(),
            true,
        )
        .unwrap();
        assert_eq!(text, "give it a score. The way I would");
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
        snapshot.enhanced = enhanced.into();
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
    /// The process-wide log, started once in a directory that outlives the test run's tests.
    fn logged() -> &'static crate::log::Log {
        static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        crate::log::init(DIR.get_or_init(|| tempfile::tempdir().unwrap()).path())
    }
    fn entries(source: &str, query: &str) -> Vec<crate::log::Entry> {
        logged().page("all", source, query, 0, 50).unwrap().items
    }
    #[test]
    fn the_output_tail_keeps_the_last_lines_each_cut_short() {
        let mut tail = Tail::default();
        assert_eq!(tail.text(), None);
        tail.push("   \n");
        assert_eq!(tail.text(), None, "a blank line is not output");
        for n in 1..=30 {
            tail.push(&format!("line {n}\n"));
        }
        tail.push(&"x".repeat(1_000));
        let text = tail.text().unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 20);
        assert_eq!(lines[0], "line 12");
        assert_eq!(lines[18], "line 30");
        assert_eq!(lines[19].chars().count(), 300);
        assert!(lines[19].ends_with("..."));
    }
    #[cfg(unix)]
    #[test]
    fn a_worker_that_dies_while_starting_is_logged_with_its_last_output() {
        logged();
        let dir = tempfile::tempdir().unwrap();
        fake::install_dying(dir.path(), "ggml_init failed: out of memory 7c1d");
        let result = preload(
            dir.path(),
            &dir.path().join("model.bin"),
            Kind::Whisper,
            &new_worker(),
            true,
        );
        assert!(result.is_err());
        let found = entries("recognizer", "7c1d");
        assert_eq!(found.len(), 1, "logged once: {found:?}");
        assert_eq!(found[0].level, crate::log::Level::Error);
        assert!(
            found[0]
                .message
                .starts_with("Worker whisper-server did not start")
        );
        assert!(
            found[0]
                .detail
                .as_ref()
                .unwrap()
                .contains("out of memory 7c1d")
        );
    }
    #[cfg(unix)]
    #[test]
    fn a_started_worker_is_logged_with_its_model_and_start_time() {
        logged();
        let dir = tempfile::tempdir().unwrap();
        fake::install(dir.path());
        let model = fake::model(dir.path(), "started-1b7e.bin");
        preload(dir.path(), &model, Kind::Whisper, &new_worker(), true).unwrap();
        let started = entries("recognizer", "started-1b7e.bin");
        assert_eq!(started.len(), 1, "{started:?}");
        assert_eq!(started[0].level, crate::log::Level::Info);
        assert!(
            started[0]
                .message
                .starts_with("Worker started: whisper-server, model started-1b7e.bin, ready in ")
        );
        assert!(started[0].message.ends_with(" ms"));
        shutdown(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn a_worker_that_exits_after_starting_is_logged_but_one_we_stop_is_not() {
        logged();
        let dir = tempfile::tempdir().unwrap();
        fake::install_crashing(dir.path(), "ggml_abort: crash marker 9f2e");
        preload(
            dir.path(),
            &dir.path().join("model.bin"),
            Kind::Whisper,
            &new_worker(),
            true,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let found = loop {
            let found = entries("recognizer", "9f2e");
            if !found.is_empty() || Instant::now() > deadline {
                break found;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].level, crate::log::Level::Warn);
        assert!(
            found[0]
                .message
                .starts_with("Worker whisper-server exited unexpectedly")
        );
        assert!(found[0].detail.as_ref().unwrap().contains("9f2e"));
        shutdown(dir.path());
        // A worker that Conch stops writes nothing: its output ends and the
        // stop is the reason.
        let quiet = tempfile::tempdir().unwrap();
        fake::install(quiet.path());
        preload(
            quiet.path(),
            &quiet.path().join("model.bin"),
            Kind::Whisper,
            &new_worker(),
            true,
        )
        .unwrap();
        let before = entries("recognizer", "exited unexpectedly").len();
        shutdown(quiet.path());
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(entries("recognizer", "exited unexpectedly").len(), before);
    }
    #[test]
    fn only_library_diagnostics_from_the_recognizer_reach_the_log() {
        for line in [
            "ggml_metal_init: failed to allocate buffer",
            "whisper_init_from_file_with_params_no_state: loading model from 'm.bin'",
            "whisper_model_load: n_vocab = 51864",
            "GGML_ASSERT(ctx != NULL) failed",
            "error: failed to load model",
            "terminate called after throwing an instance of 'std::bad_alloc'",
        ] {
            assert!(loggable(Kind::Whisper, line), "{line}");
            assert!(!loggable(Kind::Llama, line), "the cleanup worker: {line}");
        }
        for line in [
            "whisper_full_with_state: prompt[0] = 'maxGroups podGC'",
            "whisper_full_with_state: decoder  0: token  1234 'helper'",
            "[00:00:00.000 --> 00:00:02.000]  rename the helper to fetchUser",
            "Parsed message: {\"role\":\"user\",\"content\":\"hello there\"}",
            "srv  log_server_r: request: POST /v1/chat/completions 127.0.0.1 200",
            "rename the helper",
        ] {
            assert!(!loggable(Kind::Whisper, line), "{line}");
        }
    }
    #[cfg(unix)]
    #[test]
    fn a_failed_worker_keeps_library_lines_and_drops_lines_that_may_be_text() {
        logged();
        let dir = tempfile::tempdir().unwrap();
        fake::install_dying(
            dir.path(),
            "ggml_init failed: bad model 4b7a\n[00:00 --> 00:02]  call me at 555 0100 4b7a",
        );
        assert!(
            preload(
                dir.path(),
                &dir.path().join("model.bin"),
                Kind::Whisper,
                &new_worker(),
                true,
            )
            .is_err()
        );
        let found = entries("recognizer", "bad model 4b7a");
        assert_eq!(found.len(), 1, "{found:?}");
        let detail = found[0].detail.as_ref().unwrap();
        assert!(detail.contains("ggml_init failed: bad model 4b7a"));
        assert!(!detail.contains("555 0100"), "{detail}");
    }
    #[cfg(unix)]
    #[test]
    fn stopping_a_worker_logs_its_model_and_why() {
        logged();
        let quit = tempfile::tempdir().unwrap();
        fake::install(quit.path());
        let model = fake::model(quit.path(), "quit-3c9d.bin");
        preload(quit.path(), &model, Kind::Whisper, &new_worker(), true).unwrap();
        shutdown(quit.path());
        let found = entries("recognizer", "quit-3c9d.bin");
        assert!(
            found.iter().any(|e| e.message
                == "Stopped whisper-server (model quit-3c9d.bin): Conch is exiting or installing an update"),
            "{found:?}"
        );
        let cancelled = tempfile::tempdir().unwrap();
        fake::install(cancelled.path());
        let model = fake::model(cancelled.path(), "cancel-8e2a.bin");
        let worker = new_worker();
        preload(cancelled.path(), &model, Kind::Whisper, &worker, true).unwrap();
        stop(&worker);
        stop(&worker);
        shutdown(cancelled.path());
        let found = entries(
            "recognizer",
            "Stopped whisper-server (model cancel-8e2a.bin)",
        );
        assert_eq!(
            found.len(),
            1,
            "logged once, for the first reason: {found:?}"
        );
        assert!(found[0].message.ends_with("the recording was cancelled"));
    }
    #[cfg(unix)]
    #[test]
    fn cancelling_while_a_worker_starts_logs_the_stop() {
        logged();
        let dir = tempfile::tempdir().unwrap();
        fake::install_slow(dir.path());
        let model = fake::model(dir.path(), "slow-6f40.bin");
        let worker = new_worker();
        let starting = {
            let (workers, model, worker) = (dir.path().to_owned(), model.clone(), worker.clone());
            std::thread::spawn(move || preload(&workers, &model, Kind::Whisper, &worker, true))
        };
        std::thread::sleep(Duration::from_millis(600));
        stop(&worker);
        assert!(starting.join().unwrap().is_err());
        let found = entries("recognizer", "slow-6f40.bin");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].message,
            "Stopped whisper-server (model slow-6f40.bin): the recording was cancelled"
        );
    }
}
