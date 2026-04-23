use crate::{client, validators};
use lofty::prelude::AudioFile;
use serde_json::{json, Value};
use std::path::Path;

fn resolve_schedule_mode(explicit: Option<&str>) -> String {
    if let Some(mode) = explicit {
        let err = validators::validate_schedule_mode(mode);
        if !err.is_empty() {
            client::fail("client_error", &err, None, None, None);
        }
        return mode.to_string();
    }
    let base = client::base_url();
    let data = client::request_json("GET", &format!("{}/credit/v1/claw-pass/status", base), None, None, None);
    let has_pass = data.get("has_pass").and_then(|v| v.as_bool()).unwrap_or(false);
    if has_pass { "claw_pass".to_string() } else { "normal".to_string() }
}

pub fn process_image_input(input: &str) -> String {
    // 1. If already ssupload URI, return directly
    if input.starts_with("ssupload:") {
        return input.to_string();
    }

    // 2. If HTTP/HTTPS URL, download and upload
    if input.starts_with("http://") || input.starts_with("https://") {
        return download_and_upload(input);
    }

    // 3. Otherwise treat as local file path, upload
    upload_local_file(input)
}

fn download_and_upload(url: &str) -> String {
    let resp = reqwest::blocking::get(url);
    match resp {
        Ok(mut r) => {
            let temp_dir = tempfile::tempdir().unwrap();
            let temp_path = temp_dir.path().join(format!("vidu_{}", uuid::Uuid::new_v4()));
            let mut file = std::fs::File::create(&temp_path).unwrap();
            std::io::copy(&mut r, &mut file).unwrap();

            let uri = crate::commands::upload::upload_and_get_uri(temp_path.to_str().unwrap());
            uri
        }
        Err(e) => {
            client::fail("client_error", &format!("Failed to download image: {}", e), None, None, None);
        }
    }
}

fn upload_local_file(path: &str) -> String {
    if !Path::new(path).exists() {
        client::fail("client_error", &format!("Image file not found: {}", path), None, None, None);
    }
    crate::commands::upload::upload_and_get_uri(path)
}

pub fn submit(
    task_type: &str, prompt: &str, images: &[String], materials: &[String],
    duration: i64, model_version: &str, aspect_ratio: Option<&str>,
    transition: Option<&str>, resolution: &str, sample_count: i64,
    codec: &str, movement_amplitude: &str, schedule_mode: Option<&str>,
) {
    let schedule_mode = resolve_schedule_mode(schedule_mode);
    let codec = if codec == "h265" && !crate::commands::upload::ffprobe_available() {
        "h264"
    } else {
        codec
    };
    let mut prompts = vec![json!({"type": "text", "content": prompt})];

    // Process images (auto-upload local files / URLs)
    for img_input in images {
        let ssupload_uri = process_image_input(img_input);
        prompts.push(json!({"type": "image", "content": ssupload_uri}));
    }

    for mat in materials {
        let parts: Vec<&str> = mat.split(':').collect();
        if parts.len() != 3 {
            client::fail("client_error", &format!("Invalid material format '{}'. Expected 'name:id:version'", mat), None, None, None);
        }
        prompts.push(json!({
            "type": "material",
            "name": parts[0],
            "material": {"id": parts[1], "version": parts[2]}
        }));
    }

    let mut body = json!({
        "type": task_type,
        "input": {
            "prompts": prompts,
            "editor_mode": "normal",
            "enhance": true
        },
        "settings": {
            "duration": duration,
            "model_version": model_version,
            "sample_count": sample_count,
            "schedule_mode": schedule_mode,
            "codec": codec,
            "resolution": resolution,
        }
    });

    if let Some(ar) = aspect_ratio {
        body["settings"]["aspect_ratio"] = json!(ar);
    }
    if let Some(tr) = transition {
        body["settings"]["transition"] = json!(tr);
    }
    if movement_amplitude != "auto" {
        body["settings"]["movement_amplitude"] = json!(movement_amplitude);
    }

    let err = validators::validate_task_body(&body);
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
    }

    let base = client::base_url();
    let data = client::request_json("POST", &format!("{}/vidu/v1/tasks", base), None, Some(&body), None);
    let task_id = data.get("id").map(|v| match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }).unwrap_or_default();
    if task_id.is_empty() {
        client::fail("parse_error", &format!("No task id in response: {}", data), None, None, None);
    }
    client::ok(json!({"task_id": task_id}));
}

pub fn get(task_id: &str, output: Option<&str>) {
    let base = client::base_url();
    let data = client::request_json("GET", &format!("{}/vidu/v1/tasks/{}", base, task_id), None, None, None);
    let state = data.get("state").and_then(|v| v.as_str()).unwrap_or("");
    let task_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let model = data.get("input").and_then(|v| v.get("model_name")).and_then(|v| v.as_str()).unwrap_or("");

    let mut result = json!({
        "task_id": task_id,
        "state": state,
        "type": task_type,
        "model": model,
    });

    if state == "failed" {
        result["err_code"] = json!(data.get("err_code").and_then(|v| v.as_str()).unwrap_or(""));
        result["err_msg"] = json!(data.get("err_msg").and_then(|v| v.as_str()).unwrap_or(""));
    }

    if let Some(out_dir) = output {
        if state == "success" {
            let urls: Vec<&str> = data.get("creations")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|c| c.get("nomark_uri").and_then(|u| u.as_str())).collect())
                .unwrap_or_default();

            std::fs::create_dir_all(out_dir).unwrap_or_else(|e| {
                client::fail("client_error", &format!("Failed to create output directory: {}", e), None, None, None);
            });

            let http_client = reqwest::blocking::Client::new();
            let mut downloaded: Vec<String> = Vec::new();
            for (i, url) in urls.iter().enumerate() {
                match http_client.get(*url).timeout(std::time::Duration::from_secs(60)).send() {
                    Ok(resp) => {
                        let status = resp.status().as_u16();
                        if status >= 400 {
                            client::fail("http_error", &format!("Download failed for {}: HTTP {}", url, status), Some(status), None, None);
                        }
                        let bytes = resp.bytes().unwrap_or_else(|e| {
                            client::fail("client_error", &format!("Failed to read response bytes: {}", e), None, None, None);
                        });
                        let ext = ext_from_bytes(&bytes);
                        let filename = format!("{}_{}.{}", task_id, i, ext);
                        let filepath = Path::new(out_dir).join(&filename);
                        std::fs::write(&filepath, &bytes).unwrap_or_else(|e| {
                            client::fail("client_error", &format!("Failed to write file {}: {}", filepath.display(), e), None, None, None);
                        });
                        downloaded.push(filepath.to_string_lossy().to_string());
                    }
                    Err(e) => {
                        client::fail("network_error", &format!("Failed to download {}: {}", url, e), None, None, None);
                    }
                }
            }
            result["downloaded_files"] = json!(downloaded);
        } else {
            result["download_skipped"] = json!("task not ready");
        }
    }

    client::ok(result);
}

pub fn submit_lip_sync(
    video: &str,
    text: Option<&str>,
    audio: Option<&str>,
    voice_id: &str,
    speed: f64,
    volume: f64,
    enhance: bool,
    codec: &str,
    schedule_mode: Option<&str>,
) {
    match (text, audio) {
        (Some(_), Some(_)) => client::fail("client_error", "--text and --audio are mutually exclusive", None, None, None),
        (None, None) => client::fail("client_error", "Either --text or --audio is required", None, None, None),
        _ => {}
    }

    let schedule_mode = resolve_schedule_mode(schedule_mode);
    let codec = if codec == "h265" && !crate::commands::upload::ffprobe_available() {
        "h264"
    } else {
        codec
    };

    let err = validators::validate_video_file(video);
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
    }

    let err = validators::validate_lip_sync_speed(speed);
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
    }

    if volume != 0.0 {
        let err = validators::validate_lip_sync_volume(volume);
        if !err.is_empty() {
            client::fail("client_error", &err, None, None, None);
        }
    }

    let video_uri = upload_media_file(video);
    let video_name = Path::new(video).file_name().and_then(|n| n.to_str()).unwrap_or("video1");
    let video_prompt = json!({"type": "video", "content": video_uri, "name": video_name});

    let (prompts, settings) = if let Some(txt) = text {
        let err = validators::validate_lip_sync_text(txt);
        if !err.is_empty() {
            client::fail("client_error", &err, None, None, None);
        }
        let err = validators::validate_voice_id(voice_id);
        if !err.is_empty() {
            client::fail("client_error", &err, None, None, None);
        }
        let duration = calculate_text_duration(txt);
        if duration < 2 {
            client::fail("client_error", "Text is too short, duration must be at least 2 seconds", None, None, None);
        }
        let prompts = vec![json!({"type": "text", "content": txt}), video_prompt];
        let mut settings = json!({"speed": speed, "voice_id": voice_id, "duration": duration, "codec": codec, "schedule_mode": schedule_mode});
        if volume != 0.0 {
            settings["volume"] = json!(volume);
        }
        (prompts, settings)
    } else {
        let audio_path = audio.unwrap();
        let err = validators::validate_audio_file(audio_path);
        if !err.is_empty() {
            client::fail("client_error", &err, None, None, None);
        }
        let duration_f64 = read_audio_duration_f64(audio_path);
        let duration = duration_f64.ceil() as i64;

        let mut metadata = serde_json::Map::new();
        metadata.insert("duration".to_string(), json!(duration.to_string()));
        let audio_uri = crate::commands::upload::upload_media_and_get_uri_with_metadata(audio_path, Some(metadata));

        let audio_name = Path::new(audio_path).file_name().and_then(|n| n.to_str()).unwrap_or("audio1");
        let prompts = vec![video_prompt, json!({"type": "audio", "content": audio_uri, "name": audio_name})];
        let settings = json!({"codec": codec, "duration": duration, "schedule_mode": schedule_mode});
        (prompts, settings)
    };

    let body = json!({
        "type": "lip_sync",
        "input": {"prompts": prompts, "enhance": enhance},
        "settings": settings,
    });

    let base = client::base_url();
    let data = client::request_json("POST", &format!("{}/vidu/v1/tasks/tool", base), None, Some(&body), None);
    let task_id = data.get("id").map(|v| match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }).unwrap_or_default();
    if task_id.is_empty() {
        client::fail("parse_error", &format!("No task id in response: {}", data), None, None, None);
    }
    client::ok(json!({"task_id": task_id}));
}

fn calculate_text_duration(text: &str) -> i64 {
    let has_cjk = text.chars().any(|c| {
        ('\u{4E00}'..='\u{9FFF}').contains(&c)
            || ('\u{3400}'..='\u{4DBF}').contains(&c)
            || ('\u{F900}'..='\u{FAFF}').contains(&c)
    });
    let char_count = text.chars().count() as i64;
    if has_cjk {
        (char_count + 4) / 5
    } else {
        (char_count + 9) / 10
    }
}

fn read_audio_duration_f64(path: &str) -> f64 {
    match lofty::read_from_path(path) {
        Ok(tagged_file) => {
            let secs = tagged_file.properties().duration().as_secs_f64();
            secs.max(0.1)
        }
        Err(e) => client::fail("client_error", &format!("Cannot read audio duration: {}", e), None, None, None),
    }
}

fn upload_media_file(path: &str) -> String {
    if !Path::new(path).exists() {
        client::fail("client_error", &format!("File not found: {}", path), None, None, None);
    }
    crate::commands::upload::upload_media_and_get_uri(path)
}

pub fn list_voices() {
    let voices = validators::all_voice_ids();
    let mut result = serde_json::Map::new();
    result.insert("count".into(), json!(voices.len()));
    result.insert("voice_ids".into(), json!(voices));
    client::ok(serde_json::Value::Object(result));
}

pub fn submit_tts(
    prompt: Option<&str>,
    texts: &[String],
    emotions: &[String],
    voice_id: &str,
    speed: f64,
    volume: i32,
    language_boost: Option<&str>,
    schedule_mode: Option<&str>,
) {
    let schedule_mode = resolve_schedule_mode(schedule_mode);
    // 1. 构建 (content, Option<emotion>) 列表
    let segments: Vec<(&str, Option<&str>)> = if let Some(p) = prompt {
        if p.trim().is_empty() {
            client::fail("client_error", "Prompt cannot be empty", None, None, None);
        }
        let emo = emotions.first().map(|s| s.as_str()).filter(|s| !s.trim().is_empty());
        vec![(p, emo)]
    } else if !texts.is_empty() {
        if emotions.len() > texts.len() {
            client::fail("client_error",
                &format!("Too many --emotion values ({}): must not exceed number of --text segments ({})", emotions.len(), texts.len()),
                None, None, None);
        }
        texts.iter().enumerate().map(|(i, t)| {
            let emo = emotions.get(i).map(|s| s.as_str()).filter(|s| !s.trim().is_empty());
            (t.as_str(), emo)
        }).collect()
    } else {
        client::fail("client_error", "Either --prompt or at least one --text is required", None, None, None);
    };

    // 2. 校验段数
    if segments.len() > 20 {
        client::fail("client_error", &format!("Too many segments ({}). Maximum: 20", segments.len()), None, None, None);
    }

    // 3. 校验每段内容和 emotion
    for (i, (content, emo)) in segments.iter().enumerate() {
        if content.trim().is_empty() {
            client::fail("client_error", &format!("Segment {} text cannot be empty", i + 1), None, None, None);
        }
        let char_count = content.chars().count();
        if char_count > 2000 {
            client::fail("client_error",
                &format!("Segment {} text too long ({} characters). Maximum: 2000", i + 1, char_count),
                None, None, None);
        }
        if let Some(e) = emo {
            let err = validators::validate_tts_emotion(e);
            if !err.is_empty() {
                client::fail("client_error", &format!("Segment {}: {}", i + 1, err), None, None, None);
            }
        }
    }

    // 4. 校验公共参数
    let err = validators::validate_tts_voice_id(voice_id);
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
    }

    let err = validators::validate_tts_speed(speed);
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
    }

    let err = validators::validate_tts_volume(volume);
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
    }

    // 5. 构建 prompts 数组
    let prompts: Vec<serde_json::Value> = segments.iter().map(|(content, emo)| {
        let mut p = json!({"type": "text", "content": content});
        if let Some(e) = emo {
            p["audio"] = json!({"emotion": e});
        }
        p
    }).collect();

    // 6. 构建 settings
    let mut settings = json!({
        "voice_id": voice_id,
        "speed": speed,
        "vol": volume,
        "schedule_mode": schedule_mode,
    });

    if let Some(lb) = language_boost {
        let lb_trimmed = lb.trim();
        if !lb_trimmed.is_empty() {
            let err = validators::validate_tts_language_boost(lb_trimmed);
            if !err.is_empty() {
                client::fail("client_error", &err, None, None, None);
            }
            settings["language_boost"] = json!(lb_trimmed);
        }
    }

    let body = json!({
        "type": "tts",
        "input": {
            "prompts": prompts,
            "enhance": true
        },
        "settings": settings,
    });

    // 7. 发送请求
    let base = client::base_url();
    let data = client::request_json("POST", &format!("{}/vidu/v1/tasks", base), None, Some(&body), None);

    let task_id = data.get("id").map(|v| match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }).unwrap_or_default();

    if task_id.is_empty() {
        client::fail("parse_error", &format!("No task id in response: {}", data), None, None, None);
    }

    client::ok(json!({"task_id": task_id}));
}

pub fn list_tts_voices() {
    let grouped = validators::tts_voices_grouped();
    let total: usize = grouped.iter().map(|(_, v)| v.len()).sum();
    let languages: Vec<serde_json::Value> = grouped.into_iter().map(|(lang, voices)| {
        json!({
            "language": lang,
            "count": voices.len(),
            "voice_ids": voices,
        })
    }).collect();
    client::ok(json!({
        "total": total,
        "languages": languages,
    }));
}

// --- Compose (视频合成) ---

pub fn compose(
    timeline_input: &str,
    width: Option<i32>,
    height: Option<i32>,
    schedule_mode: Option<&str>,
) {
    let schedule_mode = resolve_schedule_mode(schedule_mode);
    let mut timeline = parse_timeline(timeline_input);
    normalize_timeline_urls(&mut timeline);
    validate_track_limits(&timeline);

    let mut output_media_config = serde_json::Map::new();
    if let Some(w) = width {
        output_media_config.insert("width".into(), json!(w));
    }
    if let Some(h) = height {
        output_media_config.insert("height".into(), json!(h));
    }

    let mut body = json!({ "timeline": timeline, "schedule_mode": schedule_mode });
    if !output_media_config.is_empty() {
        body["output_media_config"] = Value::Object(output_media_config);
    }

    let base = client::base_url();
    let data = client::request_json(
        "POST",
        &format!("{}/vidu/v1/clip/compose", base),
        None,
        Some(&body),
        None,
    );

    let task_id = data
        .get("job")
        .and_then(|j| j.get("task_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if task_id.is_empty() {
        client::fail(
            "parse_error",
            &format!("No task_id in response: {}", data),
            None,
            None,
            None,
        );
    }
    client::ok(json!({"task_id": task_id}));
}

fn parse_timeline(input: &str) -> Value {
    let looks_like_path = input.ends_with(".json")
        || input.contains('/')
        || input.contains('\\')
        || (Path::new(input).extension().is_some() && !input.starts_with('{'));

    if looks_like_path {
        if !Path::new(input).exists() {
            client::fail(
                "client_error",
                &format!("Timeline file not found: {}", input),
                None, None, None,
            );
        }
        let content = match std::fs::read_to_string(input) {
            Ok(c) => c,
            Err(e) => client::fail(
                "client_error",
                &format!("Failed to read timeline file: {}", e),
                None, None, None,
            ),
        };
        match serde_json::from_str(&content) {
            Ok(v) => return v,
            Err(e) => client::fail(
                "client_error",
                &format!("Invalid JSON in timeline file: {}", e),
                None, None, None,
            ),
        }
    }
    match serde_json::from_str(input) {
        Ok(v) => v,
        Err(e) => client::fail(
            "client_error",
            &format!("Invalid timeline JSON: {}", e),
            None, None, None,
        ),
    }
}
fn normalize_timeline_urls(timeline: &mut Value) {
    let track_clip_pairs = [
        ("video_tracks", "video_track_clips", "media_url"),
        ("audio_tracks", "audio_track_clips", "media_url"),
        ("subtitle_tracks", "subtitle_track_clips", "file_url"),
    ];

    for (track_key, clip_key, url_key) in &track_clip_pairs {
        if let Some(tracks) = timeline.get_mut(*track_key).and_then(|v| v.as_array_mut()) {
            for track in tracks.iter_mut() {
                if let Some(clips) = track.get_mut(*clip_key).and_then(|v| v.as_array_mut()) {
                    for clip in clips.iter_mut() {
                        if *url_key == "media_url" {
                            if let Some(map) = clip.as_object_mut() {
                                if let Some(Value::String(url)) = map.get_mut("media_url") {
                                    *url = normalize_media_url(url);
                                }
                                let is_ssupload = map.get("media_url")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.starts_with("ssupload:"))
                                    .unwrap_or(false);
                                if is_ssupload {
                                    if let Some(val) = map.remove("media_url") {
                                        map.insert("media_id".to_string(), val);
                                    }
                                }
                            }
                        } else if let Some(Value::String(url)) = clip.get_mut(*url_key) {
                            *url = normalize_file_url(url);
                        }
                    }
                }
            }
        }
    }
}


const MAX_TRACKS_PER_TYPE: usize = 100;

fn validate_track_limits(timeline: &Value) {
    let checks = [
        ("video_tracks", "Video"),
        ("audio_tracks", "Audio"),
        ("subtitle_tracks", "Subtitle"),
        ("effect_tracks", "Effect"),
    ];
    for (key, label) in &checks {
        let count = timeline.get(key).and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        if count > MAX_TRACKS_PER_TYPE {
            client::fail(
                "client_error",
                &format!("{} track count ({}) exceeds maximum of {}.", label, count, MAX_TRACKS_PER_TYPE),
                None, None, None,
            );
        }
    }
}

fn normalize_media_url(url: &str) -> String {
    if url.starts_with("ssupload:") {
        return url.to_string();
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    if Path::new(url).exists() {
        return upload_local_media(url);
    }
    client::fail(
        "client_error",
        &format!("Cannot resolve media_url: '{}'. Expected ssupload:?id=, URL, or local file path.", url),
        None, None, None,
    );
}

fn normalize_file_url(url: &str) -> String {
    if url.starts_with("ssupload:") {
        return url.to_string();
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    if Path::new(url).exists() {
        return upload_local_subtitle(url);
    }
    client::fail(
        "client_error",
        &format!("Cannot resolve file_url: '{}'. Expected ssupload:?id=, URL, or local file path.", url),
        None, None, None,
    );
}

fn upload_local_subtitle(path: &str) -> String {
    if !Path::new(path).exists() {
        client::fail("client_error", &format!("Subtitle file not found: {}", path), None, None, None);
    }
    crate::commands::upload::upload_media_and_get_uri(path)
}

const COMPOSE_MAX_IMAGE_SIZE: u64 = 50 * 1024 * 1024;
const COMPOSE_MAX_VIDEO_SIZE: u64 = 500 * 1024 * 1024;

fn validate_compose_media(path: &str) -> Option<(u32, u32)> {
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    let ext = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let is_image = matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "bmp" | "webp");
    let is_video = matches!(ext.as_str(), "mp4" | "mov");

    if is_image && file_size > COMPOSE_MAX_IMAGE_SIZE {
        client::fail(
            "client_error",
            &format!("Image too large: {} ({:.1}MB). Maximum: 50MB.", path, file_size as f64 / 1024.0 / 1024.0),
            None, None, None,
        );
    }
    if is_video && file_size > COMPOSE_MAX_VIDEO_SIZE {
        client::fail(
            "client_error",
            &format!("Video too large: {} ({:.1}MB). Maximum: 500MB.", path, file_size as f64 / 1024.0 / 1024.0),
            None, None, None,
        );
    }

    let dims = if is_image || is_video {
        get_media_dimensions(path, &ext)
    } else {
        None
    };

    if let Some((w, h)) = dims {
        let short = w.min(h);
        if w < 128 || h < 128 {
            client::fail("client_error",
                &format!("Media dimensions too small: {}x{}. Minimum: 128x128.", w, h),
                None, None, None);
        }
        if w > 4096 || h > 4096 {
            client::fail("client_error",
                &format!("Media dimensions too large: {}x{}. Maximum: 4096x4096.", w, h),
                None, None, None);
        }
        if short > 2160 {
            client::fail("client_error",
                &format!("Short side too large: {}x{} (short side {}). Maximum short side: 2160.", w, h, short),
                None, None, None);
        }
    }

    dims
}

fn get_media_dimensions(path: &str, ext: &str) -> Option<(u32, u32)> {
    match ext {
        "jpg" | "jpeg" | "png" | "bmp" | "webp" => {
            image::image_dimensions(path).ok()
        }
        "mp4" | "mov" => {
            read_mp4_dimensions(path)
        }
        _ => None,
    }
}

fn read_mp4_dimensions(path: &str) -> Option<(u32, u32)> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let ctx = mp4parse::read_mp4(&mut reader).ok()?;
    for track in &ctx.tracks {
        if track.track_type == mp4parse::TrackType::Video {
            if let Some(ref stsd) = track.stsd {
                if let Some(desc) = stsd.descriptions.first() {
                    if let mp4parse::SampleEntry::Video(ref video) = desc {
                        return Some((video.width as u32, video.height as u32));
                    }
                }
            }
        }
    }
    None
}

fn upload_local_media(path: &str) -> String {
    if !Path::new(path).exists() {
        client::fail(
            "client_error",
            &format!("Media file not found: {}", path),
            None, None, None,
        );
    }
    let dims = validate_compose_media(path);
    upload_compose_media(path, dims)
}

fn upload_compose_media(path: &str, dims: Option<(u32, u32)>) -> String {
    let metadata = dims.map(|(w, h)| {
        let mut m = serde_json::Map::new();
        m.insert("image-width".into(), json!(w.to_string()));
        m.insert("image-height".into(), json!(h.to_string()));
        m
    });
    crate::commands::upload::upload_media_and_get_uri_with_metadata(path, metadata)
}

pub fn query_credits(
    task_type: &str, model_version: &str, duration: i64, resolution: &str,
    aspect_ratio: Option<&str>, transition: Option<&str>,
    sample_count: i64, codec: &str, schedule_mode: Option<&str>,
) {
    let schedule_mode = resolve_schedule_mode(schedule_mode);

    let mut params = std::collections::HashMap::new();
    params.insert("type".to_string(), task_type.to_string());
    params.insert("settings.model_version".to_string(), model_version.to_string());
    params.insert("settings.duration".to_string(), duration.to_string());
    params.insert("settings.resolution".to_string(), resolution.to_string());
    params.insert("settings.sample_count".to_string(), sample_count.to_string());
    params.insert("settings.codec".to_string(), codec.to_string());
    params.insert("settings.schedule_mode".to_string(), schedule_mode.to_string());

    if let Some(ar) = aspect_ratio {
        params.insert("settings.aspect_ratio".to_string(), ar.to_string());
    }
    if let Some(tr) = transition {
        params.insert("settings.transition".to_string(), tr.to_string());
    }

    let base = client::base_url();
    let data = client::request_json("GET", &format!("{}/vidu/v1/tasks/credits", base), None, None, Some(&params));
    output_credits_result(&data);
}

pub fn query_tts_credits(
    text: &str, voice_id: &str, speed: f64, pitch: i32, volume: i32,
    schedule_mode: Option<&str>,
) {
    let schedule_mode = resolve_schedule_mode(schedule_mode);

    let mut params = std::collections::HashMap::new();
    params.insert("type".to_string(), "tts".to_string());
    params.insert("settings.voice_id".to_string(), voice_id.to_string());
    params.insert("settings.speed".to_string(), speed.to_string());
    params.insert("settings.pitch".to_string(), pitch.to_string());
    params.insert("settings.volume".to_string(), volume.to_string());
    params.insert("settings.schedule_mode".to_string(), schedule_mode.to_string());
    params.insert("text".to_string(), text.to_string());

    let base = client::base_url();
    let data = client::request_json("GET", &format!("{}/vidu/v1/tasks/credits", base), None, None, Some(&params));
    output_credits_result(&data);
}

pub fn query_lip_sync_credits(
    duration: i64, voice_id: &str, speed: f64, volume: f64, codec: &str,
    schedule_mode: Option<&str>,
) {
    let schedule_mode = resolve_schedule_mode(schedule_mode);

    let mut params = std::collections::HashMap::new();
    params.insert("type".to_string(), "lip_sync".to_string());
    params.insert("settings.duration".to_string(), duration.to_string());
    params.insert("settings.voice_id".to_string(), voice_id.to_string());
    params.insert("settings.speed".to_string(), speed.to_string());
    params.insert("settings.codec".to_string(), codec.to_string());
    params.insert("settings.schedule_mode".to_string(), schedule_mode.to_string());
    if volume != 0.0 {
        params.insert("settings.volume".to_string(), volume.to_string());
    }

    let base = client::base_url();
    let data = client::request_json("GET", &format!("{}/vidu/v1/tasks/credits", base), None, None, Some(&params));
    output_credits_result(&data);
}

fn output_credits_result(data: &serde_json::Value) {
    let mut result = json!({
        "cost_credits": data.get("cost_credits").and_then(|v| v.as_i64()).unwrap_or(0),
        "can_submit": data.get("can_submit").and_then(|v| v.as_bool()).unwrap_or(false),
        "current_credits": data.get("current_credits").and_then(|v| v.as_i64()).unwrap_or(0),
        "original_cost_credits": data.get("original_cost_credits").and_then(|v| v.as_i64()).unwrap_or(0),
    });

    if let Some(claw) = data.get("claw_pass_quota") {
        result["claw_pass_quota"] = crate::commands::quota::format_claw_pass_json(claw);
    }

    client::ok(result);
}

fn ext_from_bytes(bytes: &[u8]) -> &'static str {
    if bytes.len() < 4 {
        return "mp4";
    }
    // JPEG
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return "jpg";
    }
    // PNG
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return "png";
    }
    // GIF
    if bytes.starts_with(b"GIF8") {
        return "gif";
    }
    // RIFF-based: WebP or WAV
    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 {
        if &bytes[8..12] == b"WEBP" {
            return "webp";
        }
        if &bytes[8..12] == b"WAVE" {
            return "wav";
        }
    }
    // MP3 (ID3 tag or sync bytes)
    if bytes.starts_with(b"ID3") || bytes[0] == 0xFF && (bytes[1] & 0xE0 == 0xE0) {
        return "mp3";
    }
    // AAC (ADTS)
    if bytes[0] == 0xFF && bytes[1] == 0xF1 {
        return "aac";
    }
    // MP4 / MOV (ftyp box at offset 4)
    if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
        return "mp4";
    }
    "mp4"
}
