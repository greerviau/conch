use conch_lib::{
    core::{self, Mapping, Snapshot},
    inference,
    storage::{self, Repository, Settings},
};
#[test]
fn snapshot_isolates_selection_and_survives_settings_changes_and_removal() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let repository = Repository {
        id: "r".into(),
        root: root.path().into(),
        selected: true,
        exclusions: vec![],
    };
    let mut settings = Settings {
        repositories: vec![repository],
        mappings: vec![Mapping {
            phrase: "foo".into(),
            replacement: "EXACT".into(),
            repository: Some("r".into()),
            enabled: true,
        }],
        ..Default::default()
    };
    storage::save(data.path(), &settings).unwrap();
    let snapshot = storage::snapshot(data.path(), &settings).unwrap();
    settings.repositories[0].selected = false;
    assert!(
        storage::snapshot(data.path(), &settings)
            .unwrap()
            .mappings
            .is_empty()
    );
    assert_eq!(core::prepare("foo", &snapshot).text, "EXACT");
    settings.repositories.clear();
    storage::save(data.path(), &settings).unwrap();
    assert!(storage::indexes(data.path()).unwrap().is_empty());
    assert!(
        storage::snapshot(data.path(), &settings)
            .unwrap()
            .mappings
            .is_empty()
    );
    let files: Vec<_> = std::fs::read_dir(data.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(files, vec!["conch.sqlite3"]);
}
#[test]
fn local_endpoint_receives_only_masked_transcript_and_fixed_instruction() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut bytes = vec![];
        let mut chunk = [0; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(header_end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let length: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_owned)
                    })
                    .unwrap()
                    .parse()
                    .unwrap();
                if bytes.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
        let request = String::from_utf8(bytes).unwrap();
        assert!(request.contains("CONCHPROTECTED0X"));
        assert!(!request.contains("SecretClass"));
        assert!(!request.contains("private/repo"));
        let body =
            r#"{"choices":[{"message":{"content":"Do not change CONCHPROTECTED0X before 12."}}]}"#;
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let mut snapshot = Snapshot::default();
    snapshot.cleanup.enhanced = "local".into();
    snapshot.cleanup.endpoint = endpoint;
    snapshot.mappings.push(Mapping {
        phrase: "secret class".into(),
        replacement: "SecretClass".into(),
        repository: Some("private/repo".into()),
        enabled: true,
    });
    let data = tempfile::tempdir().unwrap();
    let output = inference::enhanced(inference::CleanupRequest {
        workers: data.path(),
        data: data.path(),
        snapshot: &snapshot,
        raw: "do not change secret class before 12",
        worker: &inference::new_worker(),
        cpu: true,
    })
    .unwrap();
    assert_eq!(output, "Do not change SecretClass before 12.");
    thread.join().unwrap();
}
#[test]
fn disabled_remote_cleanup_makes_no_request_and_cancellation_prevents_launch() {
    let data = tempfile::tempdir().unwrap();
    let mut snapshot = Snapshot::default();
    snapshot.cleanup.enhanced = "remote".into();
    snapshot.cleanup.endpoint = "https://127.0.0.1:1".into();
    let worker = inference::new_worker();
    assert_eq!(
        inference::enhanced(inference::CleanupRequest {
            workers: data.path(),
            data: data.path(),
            snapshot: &snapshot,
            raw: "hello",
            worker: &worker,
            cpu: true
        })
        .unwrap_err(),
        "Remote cleanup is disabled"
    );
    inference::stop(&worker);
}
/// The real workers under a directory this test owns. The worker pool is keyed by executable
/// path and `shutdown` stops every server under a directory, so tests that share the real
/// `workers` directory would kill each other's servers. Dropping the guard stops this test's
/// servers, also when the test panics.
struct IsolatedWorkers(tempfile::TempDir);
impl IsolatedWorkers {
    fn new(root: &std::path::Path) -> Self {
        let source = std::env::var_os("CONCH_TEST_WORKERS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| root.join("src-tauri/workers"));
        // A relative symlink target would resolve under the temporary directory.
        let source = std::path::absolute(source).unwrap();
        let dir = tempfile::tempdir().unwrap();
        for name in ["whisper-server", "llama-server"] {
            let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
            let (from, to) = (source.join(&file), dir.path().join(&file));
            #[cfg(unix)]
            std::os::unix::fs::symlink(&from, &to).unwrap();
            #[cfg(not(unix))]
            std::fs::copy(&from, &to).map(drop).unwrap();
        }
        Self(dir)
    }
    fn path(&self) -> &std::path::Path {
        self.0.path()
    }
}
impl Drop for IsolatedWorkers {
    fn drop(&mut self) {
        inference::shutdown(self.0.path());
    }
}
#[test]
#[ignore = "Requires locally built workers and explicitly downloaded models"]
fn offline_workers_recognize_and_clean_without_a_model_server_install() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let data = root.join(".runtime");
    let guard = IsolatedWorkers::new(root);
    let workers = guard.path().to_path_buf();
    let wav = std::fs::read(data.join("synthetic.wav")).unwrap();
    let raw = inference::transcribe(
        &workers,
        &data.join("ggml-tiny.en.bin"),
        wav,
        "",
        "en",
        &inference::new_worker(),
        true,
    )
    .unwrap();
    assert!(raw.to_lowercase().contains("do not change"));
    assert!(raw.contains("12"));
    let hinted = inference::transcribe(
        &workers,
        &data.join("ggml-tiny.en.bin"),
        std::fs::read(data.join("synthetic.wav")).unwrap(),
        "Vocabulary: podGC, maxGroups.",
        "en",
        &inference::new_worker(),
        true,
    )
    .unwrap();
    assert!(hinted.to_lowercase().contains("do not change"));
    assert!(!hinted.contains("Vocabulary"));
    assert!(hinted.contains("podGC") && hinted.contains("maxGroups"));
    // Without voice detection whisper tiny.en answers silence or room
    // noise with "Thank you." when the initial prompt is present.
    let mut seed = 1u32;
    let noise: Vec<f32> = (0..3 * 16_000)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((seed >> 16) as f32 / 32_768.0 - 1.0) * 0.004
        })
        .collect();
    for samples in [noise, vec![0.0; 16_000]] {
        let text = inference::transcribe(
            &workers,
            &data.join("ggml-tiny.en.bin"),
            conch_lib::audio::wav(&samples).unwrap(),
            "Vocabulary: podGC, maxGroups.",
            "en",
            &inference::new_worker(),
            true,
        )
        .unwrap();
        assert_eq!(text, "", "non-speech audio produced text");
    }
    let mut snapshot = Snapshot::default();
    snapshot.cleanup.enhanced = "managed".into();
    let result = inference::enhanced(inference::CleanupRequest {
        workers: &workers,
        data: &data,
        snapshot: &snapshot,
        raw: "do not change 12 tests",
        worker: &inference::new_worker(),
        cpu: true,
    })
    .unwrap();
    assert!(result.to_lowercase().contains("do not change 12"));
}

#[test]
#[ignore = "Explicitly downloads the 77.7 MB recognition model"]
fn managed_download_installs_verified_model_without_persisting_audio_or_transcripts() {
    let dir = tempfile::tempdir().unwrap();
    let tiny = inference::recognizer("tiny.en").unwrap();
    let path = inference::download(dir.path(), tiny, |_, _| {}).unwrap();
    assert_eq!(path.metadata().unwrap().len(), tiny.size);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn pre_cancelled_external_cleanup_does_not_start_a_request() {
    let data = tempfile::tempdir().unwrap();
    let mut snapshot = Snapshot::default();
    snapshot.cleanup.enhanced = "local".into();
    snapshot.cleanup.endpoint = "http://127.0.0.1:1/v1/chat/completions".into();
    let worker = inference::new_worker();
    inference::stop(&worker);
    let error = inference::enhanced(inference::CleanupRequest {
        workers: data.path(),
        data: data.path(),
        snapshot: &snapshot,
        raw: "hello",
        worker: &worker,
        cpu: true,
    })
    .unwrap_err();
    assert_eq!(error, "Cancelled");
}

#[test]
fn cancellation_drops_an_in_flight_endpoint_request() {
    use std::io::Read;
    use std::sync::mpsc;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let (seen, received) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = [0; 4096];
        assert!(stream.read(&mut bytes).unwrap() > 0);
        seen.send(()).unwrap();
        let _ = wait.recv();
    });
    let worker = inference::new_worker();
    let running = worker.clone();
    let (send, result) = mpsc::channel();
    let job = std::thread::spawn(move || {
        let data = tempfile::tempdir().unwrap();
        let mut snapshot = Snapshot::default();
        snapshot.cleanup.enhanced = "local".into();
        snapshot.cleanup.endpoint = endpoint;
        let outcome = inference::enhanced(inference::CleanupRequest {
            workers: data.path(),
            data: data.path(),
            snapshot: &snapshot,
            raw: "hello",
            worker: &running,
            cpu: true,
        });
        send.send(outcome).unwrap();
    });
    received
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    inference::stop(&worker);
    let outcome = result.recv_timeout(std::time::Duration::from_secs(5));
    release.send(()).unwrap();
    server.join().unwrap();
    job.join().unwrap();
    assert_eq!(outcome.unwrap().unwrap_err(), "Cancelled");
}

#[test]
fn vocabulary_snapshots_exclude_unselected_and_missing_repositories_and_keep_prior_versions() {
    use conch_lib::index;
    let data = tempfile::tempdir().unwrap();
    let alpha = tempfile::tempdir().unwrap();
    let beta = tempfile::tempdir().unwrap();
    std::fs::write(alpha.path().join("alpha.py"), "class AlphaOnly: pass").unwrap();
    std::fs::write(beta.path().join("beta.ts"), "class BetaOnly {}").unwrap();
    let mut settings = Settings {
        repositories: vec![
            Repository {
                id: "a".into(),
                root: alpha.path().into(),
                selected: true,
                exclusions: vec![],
            },
            Repository {
                id: "b".into(),
                root: beta.path().into(),
                selected: false,
                exclusions: vec![],
            },
        ],
        ..Default::default()
    };
    storage::put_index(
        data.path(),
        "a",
        &index::scan(alpha.path(), "a", &[]).unwrap(),
    )
    .unwrap();
    storage::put_index(
        data.path(),
        "b",
        &index::scan(beta.path(), "b", &[]).unwrap(),
    )
    .unwrap();
    let captured = storage::snapshot(data.path(), &settings).unwrap();
    assert!(captured.vocabulary.iter().any(|v| v.text == "AlphaOnly"));
    assert!(!captured.vocabulary.iter().any(|v| v.repository == "b"));
    settings.repositories[0].root = alpha.path().join("missing");
    settings.repositories[1].selected = true;
    let next = storage::snapshot(data.path(), &settings).unwrap();
    assert!(!next.vocabulary.iter().any(|v| v.repository == "a"));
    assert!(next.vocabulary.iter().any(|v| v.text == "BetaOnly"));
    assert!(captured.vocabulary.iter().any(|v| v.text == "AlphaOnly"));
}

/// Share of the expected words the recognizer returned, ignoring case and punctuation.
fn word_overlap(expected: &str, heard: &str) -> f32 {
    let words = |text: &str| -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect()
    };
    let heard = words(heard);
    let expected = words(expected);
    expected.iter().filter(|w| heard.contains(w)).count() as f32 / expected.len() as f32
}
/// Speech generated with macOS `say` voices tests the plumbing and a rough error
/// rate. It does not test accents, code-switching, or other speakers.
#[test]
#[ignore = "Requires built workers, multilingual Tiny, Silero, and say-generated WAVs in CONCH_TEST_MODELS"]
fn the_chosen_language_reaches_the_multilingual_recognizer_on_each_request() {
    let models = std::path::PathBuf::from(
        std::env::var_os("CONCH_TEST_MODELS").expect("CONCH_TEST_MODELS names the model directory"),
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let guard = IsolatedWorkers::new(root);
    let workers = guard.path().to_path_buf();
    let model = models.join("ggml-tiny.bin");
    let worker = inference::new_worker();
    let samples = [
        ("en", "Hello, this is a test of speech recognition."),
        ("fr", "Bonjour, ceci est un test de reconnaissance vocale."),
        ("de", "Hallo, das ist ein Test der Spracherkennung."),
        ("es", "Hola, esto es una prueba de reconocimiento de voz."),
    ];
    // One pooled worker serves every language: the language travels with each request.
    for language in ["en", "fr", "de", "es", "auto"] {
        for (code, expected) in samples {
            if language != code && language != "auto" {
                continue;
            }
            let wav = std::fs::read(models.join(format!("{code}.wav"))).unwrap();
            let heard =
                inference::transcribe(&workers, &model, wav, "", language, &worker, true).unwrap();
            let overlap = word_overlap(expected, &heard);
            eprintln!("{language} on {code}: {overlap:.2} {heard:?}");
            assert!(overlap >= 0.5, "{language} on {code}: {heard:?}");
        }
    }
}

/// Needs the clips written by `scripts/measure-auto-detect.py generate` in `CONCH_TEST_CLIPS`
/// and multilingual Small in `CONCH_TEST_MODELS`. Each recording is a sentence followed by a
/// short phrase of the same language, the way a recording ends with a short tail.
#[test]
#[ignore = "Requires built workers, multilingual Small, Silero, and clips from scripts/measure-auto-detect.py"]
fn the_first_chunks_language_keeps_a_short_tail_in_that_language() {
    let models = std::path::PathBuf::from(
        std::env::var_os("CONCH_TEST_MODELS").expect("CONCH_TEST_MODELS names the model directory"),
    );
    let clips = std::path::PathBuf::from(
        std::env::var_os("CONCH_TEST_CLIPS").expect("CONCH_TEST_CLIPS names the clip directory"),
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let guard = IsolatedWorkers::new(root);
    let workers = guard.path().to_path_buf();
    let samples: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("scripts/auto-detect-samples.json")).unwrap(),
    )
    .unwrap();
    let model = models.join("ggml-small.bin");
    let worker = inference::new_worker();
    let (mut locked_total, mut free_total, mut recordings) = (0.0, 0.0, 0.0);
    for (language, phrases) in samples["phrases"].as_object().unwrap() {
        for (index, phrase) in phrases.as_array().unwrap().iter().enumerate() {
            let sentence =
                std::fs::read(clips.join(format!("sentence-{language}-{index}.wav"))).unwrap();
            let tail = std::fs::read(clips.join(format!("phrase-{language}-{index}.wav"))).unwrap();
            let new =
                || inference::ChunkRecognizer::new("auto", &workers, &model, "", &worker, false);
            let recording = new();
            recording.transcribe(sentence).unwrap();
            let locked = recording.transcribe(tail.clone()).unwrap();
            let free = new().transcribe(tail).unwrap();
            let expected = phrase.as_str().unwrap();
            let (a, b) = (
                word_overlap(expected, &locked),
                word_overlap(expected, &free),
            );
            eprintln!("{language} #{index}: locked {a:.2} {locked:?}, detecting {b:.2} {free:?}");
            locked_total += a;
            free_total += b;
            recordings += 1.0;
        }
    }
    eprintln!(
        "mean word overlap of the tail: locked {:.3}, detecting each chunk {:.3}",
        locked_total / recordings,
        free_total / recordings
    );
    assert!(locked_total > free_total, "the lock did not help the tails");
}

/// Without voice detection, multilingual Whisper answers silence and room noise with words such
/// as "Thank you." The app starts the worker with voice detection, so audio without speech must
/// come back empty in every language setting.
#[test]
#[ignore = "Requires built workers and multilingual Tiny, Base, Small, and Silero in CONCH_TEST_MODELS"]
fn multilingual_models_return_no_text_for_silence_and_noise() {
    let models = std::path::PathBuf::from(
        std::env::var_os("CONCH_TEST_MODELS").expect("CONCH_TEST_MODELS names the model directory"),
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let guard = IsolatedWorkers::new(root);
    let workers = guard.path().to_path_buf();
    let mut seed = 1u32;
    let noise: Vec<f32> = (0..3 * 16_000)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((seed >> 16) as f32 / 32_768.0 - 1.0) * 0.004
        })
        .collect();
    let worker = inference::new_worker();
    for name in ["tiny", "base", "small"] {
        let model = models.join(format!("ggml-{name}.bin"));
        for samples in [vec![0.0; 16_000], noise.clone()] {
            for language in ["auto", "fr", "en"] {
                for hint in ["", "Vocabulary: podGC, maxGroups."] {
                    let text = inference::transcribe(
                        &workers,
                        &model,
                        conch_lib::audio::wav(&samples).unwrap(),
                        hint,
                        language,
                        &worker,
                        false,
                    )
                    .unwrap();
                    assert_eq!(text, "", "{name} {language} hint {hint:?}");
                }
            }
        }
    }
}
