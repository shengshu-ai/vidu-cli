use crate::{client, validators};
use lofty::prelude::AudioFile;
use serde_json::{json, Value};
use std::path::Path;

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
    codec: &str, movement_amplitude: &str, schedule_mode: &str,
) {
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
                let filename = format!("{}_{}.mp4", task_id, i);
                let filepath = Path::new(out_dir).join(&filename);
                match http_client.get(*url).timeout(std::time::Duration::from_secs(60)).send() {
                    Ok(mut resp) => {
                        let status = resp.status().as_u16();
                        if status >= 400 {
                            client::fail("http_error", &format!("Download failed for {}: HTTP {}", url, status), Some(status), None, None);
                        }
                        let mut file = std::fs::File::create(&filepath).unwrap_or_else(|e| {
                            client::fail("client_error", &format!("Failed to create file {}: {}", filepath.display(), e), None, None, None);
                        });
                        std::io::copy(&mut resp, &mut file).unwrap_or_else(|e| {
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
) {
    match (text, audio) {
        (Some(_), Some(_)) => client::fail("client_error", "--text and --audio are mutually exclusive", None, None, None),
        (None, None) => client::fail("client_error", "Either --text or --audio is required", None, None, None),
        _ => {}
    }

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
        let mut settings = json!({"speed": speed, "voice_id": voice_id, "duration": duration, "codec": codec});
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
        let settings = json!({"codec": codec, "duration": duration});
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
) {
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
