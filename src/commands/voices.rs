use crate::{client, commands::upload};
use lofty::{
    file::FileType,
    prelude::{AudioFile, TaggedFileExt},
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::Read,
    time::{Duration, Instant},
};

const MAX_AUDIO_BYTES: u64 = 20 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 1024 * 1024;

fn validate_id(id: &str) -> Result<(), String> {
    match id.strip_prefix("voice_") {
        Some(s)
            if !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') =>
        {
            Ok(())
        }
        _ => Err("VOICE_ID must be a personal voice_ ID".into()),
    }
}

fn check_id(id: &str) {
    if let Err(e) = validate_id(id) {
        client::fail("client_error", &e, None, None, None);
    }
}

fn validate_audio_properties(size: u64, duration: f64) -> Result<(), String> {
    if size == 0 || size > MAX_AUDIO_BYTES {
        return Err("Audio must be nonempty and at most 20 MiB".into());
    }
    if !duration.is_finite() || !(10.0..=300.0).contains(&duration) {
        return Err("Audio duration must be 10–300 seconds".into());
    }
    Ok(())
}

fn audio_duration(path: &str) -> Result<(f64, &'static str), String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("Cannot read audio: {e}"))?;
    if !metadata.is_file() {
        return Err("Audio must be a local regular file".into());
    }
    if metadata.len() == 0 || metadata.len() > MAX_AUDIO_BYTES {
        return Err("Audio must be nonempty and at most 20 MiB".into());
    }
    // Probe file contents, not the extension. Never trust supplied duration metadata.
    let audio =
        lofty::read_from_path(path).map_err(|e| format!("Cannot decode audio metadata: {e}"))?;
    let props = audio.properties();
    if props.sample_rate().unwrap_or(0) == 0 || props.channels().unwrap_or(0) == 0 {
        return Err("Audio has no valid sample rate/channels".into());
    }
    let duration = props.duration().as_secs_f64();
    validate_audio_properties(metadata.len(), duration)?;
    let mime = match audio.file_type() {
        FileType::Aac => "audio/aac",
        FileType::Aiff => "audio/aiff",
        FileType::Flac => "audio/flac",
        FileType::Mpeg => "audio/mpeg",
        FileType::Mp4 => "audio/mp4",
        FileType::Opus | FileType::Vorbis | FileType::Speex => "audio/ogg",
        FileType::Wav => "audio/wav",
        FileType::Ape => "audio/ape",
        FileType::Mpc => "audio/musepack",
        FileType::WavPack => "audio/wavpack",
        _ => return Err("Unsupported audio container".into()),
    };
    Ok((duration, mime))
}

pub fn create(
    audio: &str,
    name: Option<&str>,
    use_trial: bool,
    wait_requested: bool,
    timeout_secs: u64,
) -> Value {
    let (duration, mime) = audio_duration(audio)
        .unwrap_or_else(|e| client::fail("client_error", &e, None, None, None));
    if wait_requested && timeout_secs == 0 {
        client::fail("client_error", "timeout must be positive", None, None, None);
    }
    let uri = upload::upload_voice_audio(audio, duration, mime);
    let mut body = json!({"source_uri": uri, "use_trial": use_trial});
    if let Some(name) = name {
        body["name"] = json!(name);
    }
    // A single POST: never resubmit a possibly paid clone after a transport failure.
    let data = client::request_json_with_timeout_context(
        "POST",
        &format!("{}/vidu/v1/voices", client::base_url()),
        Some("create_voice"),
        Some(&body),
        None,
        Duration::from_secs(30),
        &json!({"upload_id":uri.strip_prefix("ssupload:?id=").unwrap_or("")}),
    );
    let id = data["voice_id"].as_str().unwrap_or_else(|| {
        client::fail(
            "parse_error",
            "Create voice response has no voice_id",
            None,
            None,
            Some("create_voice"),
        )
    });
    check_id(id);
    if wait_requested {
        wait(id, timeout_secs)
    } else {
        data
    }
}

pub fn list(page_token: &str, pagesz: i64) -> Value {
    if !(1..=100).contains(&pagesz) {
        client::fail("client_error", "pagesz must be 1–100", None, None, None);
    }
    let params = HashMap::from([
        ("pager.page_token".into(), page_token.into()),
        ("pager.pagesz".into(), pagesz.to_string()),
    ]);
    client::request_json(
        "GET",
        &format!("{}/vidu/v1/voices", client::base_url()),
        Some("list_voices"),
        None,
        Some(&params),
    )
}

pub fn update(voice_id: &str, name: Option<&str>, description: Option<&str>) -> Value {
    check_id(voice_id);
    let mut voice = json!({"voice_id":voice_id});
    let mut mask = Vec::new();
    if let Some(name) = name {
        voice["name"] = json!(name);
        mask.push("name");
    }
    if let Some(description) = description {
        voice["description"] = json!(description);
        mask.push("description");
    }
    if mask.is_empty() {
        client::fail(
            "client_error",
            "Provide --name or --description",
            None,
            None,
            None,
        );
    }
    client::request_json(
        "PUT",
        &format!("{}/vidu/v1/voices/{voice_id}", client::base_url()),
        Some("update_voice"),
        Some(&json!({"voice":voice,"mask":mask.join(",")})),
        None,
    )
}

// GetVoice itself exposes no task status, and may not exist until cloning succeeds.
// The status SSE always sends a snapshot first, so get reads exactly that snapshot.
pub fn get(voice_id: &str) -> Value {
    check_id(voice_id);
    read_status(voice_id, Instant::now() + Duration::from_secs(30), true)
}

pub fn wait(voice_id: &str, timeout_secs: u64) -> Value {
    check_id(voice_id);
    if timeout_secs == 0 {
        client::fail("client_error", "timeout must be positive", None, None, None);
    }
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(timeout_secs))
        .unwrap_or_else(|| client::fail("client_error", "timeout is too large", None, None, None));
    read_status(voice_id, deadline, false)
}

fn status_failure(kind: &str, message: &str, id: &str, state: &str, details: Option<&Value>) -> ! {
    let fields = json!({"voice_id":id,"state":state});
    client::fail_with_fields(
        kind,
        message,
        None,
        None,
        Some("voice_status"),
        Some(&fields),
        None,
        details,
        None,
    )
}

fn normalize_status(mut event: Value, id: &str) -> Result<Value, String> {
    if event["voice_id"].as_str() != Some(id) {
        return Err("Status event voice_id does not match requested voice".into());
    }
    let state = match &event["status"] {
        Value::String(s)
            if matches!(s.as_str(), "created" | "processing" | "success" | "failed") =>
        {
            s.clone()
        }
        Value::Number(n) => match n.as_u64() {
            Some(1) => "created",
            Some(2) => "processing",
            Some(3) => "success",
            Some(4) => "failed",
            _ => return Err("Unknown voice status".into()),
        }
        .into(),
        _ => return Err("Missing or unknown voice status".into()),
    };
    event["state"] = json!(state);
    Ok(event)
}

fn read_status(id: &str, deadline: Instant, snapshot: bool) -> Value {
    let mut state = "unknown".to_string();
    ensure_owned_task(id, deadline);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            status_failure(
                "timeout",
                "Voice wait timed out; server cloning continues",
                id,
                &state,
                None,
            );
        }
        let params = HashMap::from([("voice_id".into(), id.into())]);
        let step = format!("voice_status:{id}");
        let mut response = client::request_with_timeout_context(
            "GET",
            &format!("{}/vidu/v1/voices/status", client::base_url()),
            Some(&step),
            None,
            Some(&params),
            remaining,
            &json!({"voice_id":id,"state":state}),
        );
        let mut decoder = SseDecoder::default();
        let mut buffer = [0; 4096];
        loop {
            if Instant::now() >= deadline {
                status_failure(
                    "timeout",
                    "Voice wait timed out; server cloning continues",
                    id,
                    &state,
                    None,
                );
            }
            let count = match response.read(&mut buffer) {
                Ok(n) => n,
                Err(e) => {
                    if Instant::now() >= deadline {
                        status_failure(
                            "timeout",
                            "Voice wait timed out; server cloning continues",
                            id,
                            &state,
                            None,
                        );
                    }
                    status_failure(
                        "network_error",
                        &format!("Voice status stream interrupted: {e}"),
                        id,
                        &state,
                        None,
                    );
                }
            };
            let events = decoder
                .push(&buffer[..count], count == 0)
                .unwrap_or_else(|e| status_failure("parse_error", &e, id, &state, None));
            for event in events {
                let event = normalize_status(event, id)
                    .unwrap_or_else(|e| status_failure("parse_error", &e, id, &state, None));
                let next = event["state"].as_str().unwrap();
                if next != state {
                    eprintln!("音色 {id}: {next}");
                    state = next.to_string();
                }
                if state == "failed" {
                    status_failure(
                        "voice_failed",
                        "Voice cloning failed",
                        id,
                        &state,
                        Some(&event),
                    );
                }
                if snapshot || state == "success" {
                    return event;
                }
            }
            if count == 0 {
                break;
            }
        }
        // The backend closes nonterminal streams after 15 minutes. Reconnect to
        // obtain a fresh database snapshot, keeping the same overall deadline.
        let pause = Duration::from_secs(1).min(deadline.saturating_duration_since(Instant::now()));
        std::thread::sleep(pause);
    }
}

// The SSE task lookup currently lacks an owner filter. Voice records only exist
// after success, so GET /voices/{id} cannot authorize pending/failed tasks.
// ListVoiceTasks does filter by creator; enumerate its ordinary page pager with
// no statuses filter before subscribing, including all task states.
fn ensure_owned_task(id: &str, deadline: Instant) {
    let mut page = 0_u64;
    let mut seen_pages = std::collections::HashSet::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            status_failure(
                "timeout",
                "Voice ownership check timed out",
                id,
                "unknown",
                None,
            );
        }
        let params = HashMap::from([
            ("pager.page".into(), page.to_string()),
            ("pager.pagesz".into(), "100".into()),
        ]);
        let data = client::request_json_with_timeout_context(
            "GET",
            &format!("{}/vidu/v1/voices/tasks", client::base_url()),
            Some("voice_owner"),
            None,
            Some(&params),
            remaining,
            &json!({"voice_id":id,"state":"unknown"}),
        );
        let tasks = data["tasks"].as_array().unwrap_or_else(|| {
            status_failure(
                "parse_error",
                "Voice task list has no tasks array",
                id,
                "unknown",
                None,
            )
        });
        if tasks
            .iter()
            .any(|task| task["voice_id"].as_str() == Some(id))
        {
            return;
        }
        if tasks.len() < 100 {
            status_failure(
                "forbidden",
                "Voice is not in the current user's clone tasks",
                id,
                "unknown",
                None,
            );
        }
        let mut page_ids: Vec<&str> = tasks
            .iter()
            .map(|task| task["voice_id"].as_str().unwrap_or(""))
            .collect();
        page_ids.sort_unstable();
        if !seen_pages.insert(page_ids.join("\n")) {
            status_failure(
                "parse_error",
                "Voice task pagination repeated a page",
                id,
                "unknown",
                None,
            );
        }
        page += 1;
    }
}

#[derive(Default)]
struct SseDecoder {
    pending: Vec<u8>,
    data: Vec<String>,
    event_bytes: usize,
}
impl SseDecoder {
    fn dispatch(&mut self, events: &mut Vec<Value>) -> Result<(), String> {
        if !self.data.is_empty() {
            let text = self.data.join("\n");
            self.data.clear();
            self.event_bytes = 0;
            events.push(
                serde_json::from_str(&text)
                    .map_err(|e| format!("Malformed voice SSE data: {e}"))?,
            );
        }
        Ok(())
    }
    fn push(&mut self, chunk: &[u8], eof: bool) -> Result<Vec<Value>, String> {
        let mut events = Vec::new();
        for &byte in chunk {
            if byte == b'\n' {
                let mut line = std::mem::take(&mut self.pending);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                self.line(&line, &mut events)?;
            } else {
                self.pending.push(byte);
                if self.pending.len() + self.event_bytes > MAX_EVENT_BYTES {
                    return Err("Voice SSE event exceeds 1 MiB".into());
                }
            }
        }
        if eof {
            if !self.pending.is_empty() {
                let line = std::mem::take(&mut self.pending);
                self.line(&line, &mut events)?;
            }
            self.dispatch(&mut events)?;
        }
        Ok(events)
    }
    fn line(&mut self, line: &[u8], events: &mut Vec<Value>) -> Result<(), String> {
        if line.is_empty() {
            return self.dispatch(events);
        }
        let line = std::str::from_utf8(line).map_err(|_| "Voice SSE is not valid UTF-8")?;
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.strip_prefix(' ').unwrap_or(data);
            self.event_bytes += data.len() + 1;
            self.data.push(data.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_boundaries() {
        for duration in [10.0, 300.0] {
            assert!(validate_audio_properties(MAX_AUDIO_BYTES, duration).is_ok());
        }
        for duration in [9.999, 300.001, f64::NAN, f64::INFINITY] {
            assert!(validate_audio_properties(1, duration).is_err());
        }
        assert!(validate_audio_properties(MAX_AUDIO_BYTES + 1, 10.0).is_err());
        assert!(validate_audio_properties(0, 10.0).is_err());
    }
    #[test]
    fn personal_ids_cannot_inject_paths_or_queries() {
        assert!(validate_id("voice_abc-123_def").is_ok());
        for id in [
            "voice_",
            "official",
            "voice_../x",
            "voice_x?y=1",
            "voice_x%2fy",
        ] {
            assert!(validate_id(id).is_err());
        }
    }
    #[test]
    fn sse_incremental_multiline_and_heartbeat() {
        let text = ": heartbeat\r\nevent: voice.status\r\ndata: {\"voice_id\":\"voice_x\",\r\ndata: \"status\":\"success\"}\r\n\r\n";
        let mut decoder = SseDecoder::default();
        let mut events = Vec::new();
        for byte in text.as_bytes() {
            events.extend(decoder.push(&[*byte], false).unwrap());
        }
        assert_eq!(events.len(), 1);
        assert_eq!(
            normalize_status(events.remove(0), "voice_x").unwrap()["state"],
            "success"
        );
    }
    #[test]
    fn sse_eof_malformed_unknown_and_wrong_voice() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.push(b": ping\n\n", false).unwrap().is_empty());
        assert_eq!(
            decoder
                .push(b"data: {\"voice_id\":\"voice_x\",\"status\":4}", true)
                .unwrap()
                .len(),
            1
        );
        assert!(SseDecoder::default()
            .push(b"data: broken\n\n", false)
            .is_err());
        assert!(
            normalize_status(json!({"voice_id":"voice_x","status":"other"}), "voice_x").is_err()
        );
        assert!(
            normalize_status(json!({"voice_id":"voice_y","status":"success"}), "voice_x").is_err()
        );
    }
    #[test]
    fn rejects_fake_audio_contents() {
        let file = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
        std::fs::write(file.path(), b"not real audio").unwrap();
        assert!(audio_duration(file.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn reads_actual_pcm_duration_at_boundaries() {
        // 8 kHz mono, 8-bit PCM; duration comes from actual sample data.
        for seconds in [9_u32, 10, 300, 301] {
            let samples = seconds * 8000;
            let mut wav = Vec::new();
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&(36 + samples).to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16_u32.to_le_bytes());
            wav.extend_from_slice(&1_u16.to_le_bytes());
            wav.extend_from_slice(&1_u16.to_le_bytes());
            wav.extend_from_slice(&8000_u32.to_le_bytes());
            wav.extend_from_slice(&8000_u32.to_le_bytes());
            wav.extend_from_slice(&1_u16.to_le_bytes());
            wav.extend_from_slice(&8_u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&samples.to_le_bytes());
            wav.resize(44 + samples as usize, 128);
            let file = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
            std::fs::write(file.path(), wav).unwrap();
            let result = audio_duration(file.path().to_str().unwrap());
            assert_eq!(result.is_ok(), (10..=300).contains(&seconds));
            if let Ok((duration, mime)) = result {
                assert_eq!(mime, "audio/wav");
                assert_eq!(duration, seconds as f64);
            }
        }
    }

    #[test]
    fn caps_sse_line_and_multiline_event_memory() {
        assert!(SseDecoder::default()
            .push(&vec![b'x'; MAX_EVENT_BYTES + 1], false)
            .is_err());
        let mut decoder = SseDecoder::default();
        let line = format!("data: {}\n", "x".repeat(MAX_EVENT_BYTES / 2));
        assert!(decoder.push(line.as_bytes(), false).is_ok());
        assert!(decoder.push(line.as_bytes(), false).is_err());
    }
}
