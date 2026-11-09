use crate::core::{self, Snapshot};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[derive(Default)]
pub struct WorkerState {
    child: Option<Child>,
    cancelled: bool,
}
pub type Worker = Arc<Mutex<WorkerState>>;
pub fn new_worker() -> Worker {
    Arc::new(Mutex::new(WorkerState::default()))
}
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
struct Running {
    worker: Worker,
}
impl Drop for Running {
    fn drop(&mut self) {
        let mut state = self.worker.lock().unwrap();
        if let Some(mut child) = state.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
pub fn stop(worker: &Worker) {
    let mut state = worker.lock().unwrap();
    state.cancelled = true;
    if let Some(mut child) = state.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}
fn launch(
    workers: &Path,
    model: &Path,
    whisper: bool,
    worker: &Worker,
    cpu: bool,
) -> Result<(Running, String, String), String> {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "Cannot allocate inference port")?
        .local_addr()
        .map_err(|_| "Cannot allocate inference port")?
        .port();
    let token = uuid::Uuid::new_v4().to_string();
    let name = if whisper {
        "whisper-server"
    } else {
        "llama-server"
    };
    let executable = workers.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if !executable.is_file() {
        return Err(
            "Bundled inference worker is missing; run the runtime build before packaging".into(),
        );
    }
    if !model.is_file() {
        return Err("Download the required model in Settings first".into());
    }
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
    if whisper {
        command.env("CONCH_WORKER_TOKEN", &token);
        command.args(["--language", "en", "--suppress-nst"]);
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
    {
        let mut state = worker.lock().unwrap();
        if state.cancelled {
            return Err("Cancelled".into());
        }
        state.child = Some(
            command
                .spawn()
                .map_err(|_| "Cannot launch inference worker")?,
        );
    }
    let running = Running {
        worker: worker.clone(),
    };
    let endpoint = format!("http://127.0.0.1:{port}");
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(1))
        .build()
        .map_err(|_| "Cannot initialize local IPC")?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(45) {
        {
            let mut guard = worker.lock().unwrap();
            if guard
                .child
                .as_mut()
                .is_none_or(|c| c.try_wait().ok().flatten().is_some())
            {
                return Err("Inference worker stopped or was cancelled".into());
            }
        }
        if client
            .get(format!("{endpoint}/health"))
            .bearer_auth(&token)
            .send()
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok((running, endpoint, token));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("Inference worker startup timed out".into())
}
pub fn transcribe(
    workers: &Path,
    model: &Path,
    wav: Vec<u8>,
    worker: &Worker,
    cpu: bool,
) -> Result<String, String> {
    let (_running, url, token) = launch(workers, model, true, worker, cpu)?;
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|_| "Cannot initialize recognizer IPC")?;
    let form = reqwest::blocking::multipart::Form::new()
        .part(
            "file",
            reqwest::blocking::multipart::Part::bytes(wav).file_name("recording.wav"),
        )
        .text("response_format", "json")
        .text("temperature", "0");
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
        .map(|s| s.trim().to_owned())
        .ok_or_else(|| "Recognizer returned invalid output".into())
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
    if cleanup.mode != "enhanced" {
        return Err("Enhanced cleanup is disabled".into());
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
    let mut running = None;
    let (endpoint, token, system, user) = if cleanup.provider == "managed" {
        let (guard, url, key) = launch(
            workers,
            &data.join("s1-mini-q4_k_m.gguf"),
            false,
            worker,
            cpu,
        )?;
        running = Some(guard);
        (
            format!("{url}/v1/chat/completions"),
            key,
            S1_SYSTEM,
            format!("[Styling: semi-formal] [Structure: prose] [Context: general]\n{basic}"),
        )
    } else {
        let remote = cleanup.provider == "remote";
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
    let mut builder = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30));
    if cleanup.provider != "remote" {
        builder = builder.no_proxy();
    }
    let client = builder
        .build()
        .map_err(|_| "Cannot initialize cleanup client")?;
    let mut request = client.post(endpoint).json(&json!({"model":cleanup.model,"messages":[{"role":"system","content":system},{"role":"user","content":user}],"temperature":0,"max_tokens":1024,"chat_template_kwargs":{"enable_thinking":false}}));
    if !token.is_empty() {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|_| "Enhanced cleanup failed or timed out; using Basic")?;
    use std::io::Read;
    let mut bytes = vec![];
    response
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|_| "Invalid cleanup response")?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid cleanup response")?;
    let after = value["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("Invalid cleanup response")?;
    let result = core::validate_enhanced(&basic, after.trim(), &masked)
        .ok_or("Enhanced rewrite failed preservation checks; using Basic")?;
    drop(running);
    Ok(result)
}
#[derive(Clone, Copy)]
pub struct Model {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}
pub const MODELS: [Model; 2] = [
    Model {
        name: "ggml-tiny.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-tiny.en.bin",
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        size: 77704715,
    },
    Model {
        name: "s1-mini-q4_k_m.gguf",
        url: "https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf",
        sha256: "3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634",
        size: 484219808,
    },
];
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
