use crate::{client, validators};
use lofty::prelude::AudioFile;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
struct NormalizedTaskSettings {
    duration: i64,
    resolution: String,
    aspect_ratio: Option<String>,
}

fn normalize_task_settings(
    task_type: &str,
    model_version: &str,
    duration: Option<i64>,
    resolution: Option<&str>,
    aspect_ratio: Option<&str>,
    legacy_resolution_default: Option<&str>,
) -> Result<NormalizedTaskSettings, String> {
    let is_image_type = task_type == "text2image" || task_type == "reference2image";
    let duration = if is_image_type {
        0
    } else {
        duration
            .or_else(|| (model_version == validators::DRAMA_MODEL_VERSION).then_some(5))
            .ok_or_else(|| "duration is required for this task type".to_string())?
    };
    let resolution = resolution
        .or_else(|| {
            (model_version == validators::DRAMA_MODEL_VERSION)
                .then_some("720p")
                .or(legacy_resolution_default)
        })
        .ok_or_else(|| "resolution is required for this model".to_string())?
        .to_string();
    let aspect_ratio = aspect_ratio
        .or_else(|| (model_version == validators::DRAMA_MODEL_VERSION).then_some("16:9"))
        .map(str::to_string);

    Ok(NormalizedTaskSettings {
        duration,
        resolution,
        aspect_ratio,
    })
}

fn build_submit_body(
    task_type: &str,
    prompts: Vec<Value>,
    duration: i64,
    model_version: &str,
    aspect_ratio: Option<&str>,
    transition: Option<&str>,
    resolution: &str,
    sample_count: i64,
    codec: &str,
    movement_amplitude: &str,
    schedule_mode: Option<&str>,
) -> Value {
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
            "codec": codec,
            "resolution": resolution,
        }
    });

    if let Some(mode) = schedule_mode {
        body["settings"]["schedule_mode"] = json!(mode);
    }
    if let Some(ar) = aspect_ratio {
        body["settings"]["aspect_ratio"] = json!(ar);
    }
    if let Some(tr) = transition {
        body["settings"]["transition"] = json!(tr);
    }
    if movement_amplitude != "auto" {
        body["settings"]["movement_amplitude"] = json!(movement_amplitude);
    }
    body
}

fn build_credit_query_params(
    task_type: &str,
    model_version: &str,
    settings: &NormalizedTaskSettings,
    transition: Option<&str>,
    sample_count: i64,
    codec: &str,
    schedule_mode: &str,
) -> std::collections::HashMap<String, String> {
    let mut params = std::collections::HashMap::new();
    params.insert("type".to_string(), task_type.to_string());
    params.insert(
        "settings.model_version".to_string(),
        model_version.to_string(),
    );
    params.insert("settings.duration".to_string(), settings.duration.to_string());
    params.insert(
        "settings.resolution".to_string(),
        settings.resolution.clone(),
    );
    params.insert(
        "settings.sample_count".to_string(),
        sample_count.to_string(),
    );
    params.insert("settings.codec".to_string(), codec.to_string());
    params.insert("settings.schedule_mode".to_string(), schedule_mode.to_string());
    if let Some(ar) = settings.aspect_ratio.as_deref() {
        params.insert("settings.aspect_ratio".to_string(), ar.to_string());
    }
    if let Some(tr) = transition {
        params.insert("settings.transition".to_string(), tr.to_string());
    }
    params
}

fn resolve_schedule_mode(explicit: Option<&str>, model_version: Option<&str>) -> String {
    match validators::preferred_schedule_mode(model_version, explicit) {
        Ok(Some(mode)) => return mode,
        Ok(None) => {}
        Err(err) => client::fail("client_error", &err, None, None, None),
    }
    let base = client::base_url();
    let data = client::request_json(
        "GET",
        &format!("{}/credit/v1/claw-pass/status", base),
        None,
        None,
        None,
    );
    let has_pass = data
        .get("has_pass")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if has_pass {
        "claw_pass".to_string()
    } else {
        "normal".to_string()
    }
}
/// Pick the effective prompt text from `--prompt` and `--prompt-path`.
///
/// `--prompt` wins when both are provided. When only `--prompt-path` is set we
/// read the file via `validators::read_prompt_file`. Errors are reported
/// through the standard `client::fail` JSON contract.
fn resolve_prompt_inputs(prompt: Option<&str>, prompt_path: Option<&str>) -> Option<String> {
    if let Some(text) = prompt {
        return Some(text.to_string());
    }
    if let Some(path) = prompt_path {
        match validators::read_prompt_file(path) {
            Ok(s) => return Some(s),
            Err(e) => client::fail("client_error", &e, None, None, None),
        }
    }
    None
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
            let temp_path = temp_dir
                .path()
                .join(format!("vidu_{}", uuid::Uuid::new_v4()));
            let mut file = std::fs::File::create(&temp_path).unwrap();
            std::io::copy(&mut r, &mut file).unwrap();

            let uri = crate::commands::upload::upload_and_get_uri(temp_path.to_str().unwrap());
            uri
        }
        Err(e) => {
            client::fail(
                "client_error",
                &format!("Failed to download image: {}", e),
                None,
                None,
                None,
            );
        }
    }
}

fn upload_local_file(path: &str) -> String {
    if !Path::new(path).exists() {
        client::fail(
            "client_error",
            &format!("Image file not found: {}", path),
            None,
            None,
            None,
        );
    }
    crate::commands::upload::upload_and_get_uri(path)
}

fn reference_audio_durations(
    task_type: &str,
    model_version: &str,
    audios: &[String],
) -> Vec<Option<f64>> {
    let err = validators::validate_reference_audio_count(task_type, model_version, audios.len());
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
    }
    let (min, max) = if model_version == "3.4" {
        (1.0, 16.0)
    } else {
        (2.0, 15.0)
    };
    let mut total = 0.0;
    audios
        .iter()
        .map(|audio| {
            if audio.starts_with("ssupload:") {
                return None;
            }
            if audio.starts_with("http://") || audio.starts_with("https://") {
                client::fail("client_error", "HTTP/HTTPS URLs are not supported for audio input. Use a local file path or ssupload:?id=...", None, None, None);
            }
            let err = validators::validate_reference_audio_file(audio);
            if !err.is_empty() {
                client::fail("client_error", &err, None, None, None);
            }
            let duration = read_audio_duration_f64(audio);
            if !(min..=max).contains(&duration) {
                client::fail(
                    "client_error",
                    &format!("Audio duration {:.3}s out of range [{}, {}]s", duration, min, max),
                    None,
                    None,
                    None,
                );
            }
            total += duration;
            if model_version == "3.2_a" && total > 15.0 {
                client::fail(
                    "client_error",
                    &format!("Total audio duration {:.1}s exceeds 15s", total),
                    None,
                    None,
                    None,
                );
            }
            Some(duration)
        })
        .collect()
}

fn validate_local_reference_video_durations(
    model_version: &str,
    durations: &[f64],
) -> Result<(), String> {
    let is_drama = model_version == validators::DRAMA_MODEL_VERSION;
    if is_drama {
        if let Some(duration) = durations.iter().find(|d| !(4.0..=30.0).contains(*d)) {
            return Err(format!(
                "Drama local video duration {:.3}s out of range [4, 30]s",
                duration
            ));
        }
    }
    let total: f64 = durations.iter().sum();
    let max_total = if is_drama { 30.0 } else { 15.0 };
    if total > max_total {
        let message = if is_drama {
            format!("Total local video duration {:.1}s exceeds 30s", total)
        } else {
            format!("Total video duration {:.1}s exceeds 15s", total)
        };
        return Err(message);
    }
    Ok(())
}

fn inspect_reference_videos(
    task_type: &str,
    model_version: &str,
    videos: &[String],
) -> Vec<Option<f64>> {
    let video_error = validators::validate_reference_video_count(task_type, model_version, videos.len());
    if !video_error.is_empty() {
        client::fail("client_error", &video_error, None, None, None);
    }

    let durations: Vec<Option<f64>> = videos
        .iter()
        .map(|video_input| {
            if video_input.starts_with("ssupload:") {
                // The backend validates the complete metadata set for existing uploads.
                return None;
            }
            if video_input.starts_with("http://") || video_input.starts_with("https://") {
                client::fail("client_error", "HTTP/HTTPS URLs are not supported for video input. Use a local file path or ssupload:?id=...", None, None, None);
            }

            let err = validators::validate_reference_video_file(video_input);
            if !err.is_empty() {
                client::fail("client_error", &err, None, None, None);
            }
            let metadata = match read_local_video_metadata(video_input) {
                Ok(metadata) => metadata,
                Err(error) => client::fail(
                    "client_error",
                    &format_local_video_metadata_error(video_input, error),
                    None,
                    None,
                    None,
                ),
            };
            let (w, h) = match metadata.dimensions {
                Some(dimensions) => dimensions,
                None => client::fail(
                    "client_error",
                    &format_local_video_dimensions_error(video_input),
                    None,
                    None,
                    None,
                ),
            };
            if let Err(error) = validate_local_reference_video_geometry(video_input, w, h) {
                client::fail("client_error", &error, None, None, None);
            }

            let duration = match metadata.duration {
                Some(value) => value,
                None => client::fail(
                    "client_error",
                    &format_local_video_duration_error(video_input),
                    None,
                    None,
                    None,
                ),
            };
            Some(duration)
        })
        .collect();
    let local_durations: Vec<f64> = durations.iter().filter_map(|duration| *duration).collect();
    if let Err(err) = validate_local_reference_video_durations(model_version, &local_durations) {
        client::fail("client_error", &err, None, None, None);
    }
    durations
}

pub fn submit(
    task_type: &str,
    prompt: Option<&str>,
    prompt_path: Option<&str>,
    images: &[String],
    materials: &[String],
    audios: &[String],
    videos: &[String],
    duration: Option<i64>,
    model_version: &str,
    aspect_ratio: Option<&str>,
    transition: Option<&str>,
    resolution: Option<&str>,
    sample_count: i64,
    codec: &str,
    movement_amplitude: &str,
    schedule_mode: Option<&str>,
) {
    let prompt = resolve_prompt_inputs(prompt, prompt_path).unwrap_or_else(|| {
        client::fail(
            "client_error",
            "Either --prompt or --prompt-path is required",
            None,
            None,
            None,
        )
    });
    let settings = normalize_task_settings(
        task_type,
        model_version,
        duration,
        resolution,
        aspect_ratio,
        None,
    )
    .unwrap_or_else(|err| client::fail("client_error", &err, None, None, None));
    let codec = if codec == "h265" && !crate::commands::upload::ffprobe_available() {
        "h264"
    } else {
        codec
    };
    let schedule_mode = resolve_schedule_mode(schedule_mode, Some(model_version));

    // Validate all model-specific options and reference counts before performing uploads.
    let mut preflight_prompts = vec![json!({"type": "text", "content": "preflight"})];
    preflight_prompts.extend((0..images.len()).map(|_| json!({"type": "image", "content": "ssupload:?id=preflight"})));
    preflight_prompts.extend((0..materials.len()).map(|_| json!({"type": "material", "name": "preflight", "material": {"id": "preflight", "version": "preflight"}})));
    preflight_prompts.extend((0..audios.len()).map(|_| json!({"type": "audio", "content": "ssupload:?id=preflight"})));
    preflight_prompts.extend((0..videos.len()).map(|_| json!({"type": "video", "content": "ssupload:?id=preflight"})));
    let preflight_body = build_submit_body(
        task_type,
        preflight_prompts,
        settings.duration,
        model_version,
        settings.aspect_ratio.as_deref(),
        transition,
        &settings.resolution,
        sample_count,
        codec,
        movement_amplitude,
        Some(&schedule_mode),
    );
    let preflight_error = validators::validate_task_body(&preflight_body);
    if !preflight_error.is_empty() {
        client::fail("client_error", &preflight_error, None, None, None);
    }

    for material in materials {
        if material.split(':').count() != 3 {
            client::fail(
                "client_error",
                &format!(
                    "Invalid material format '{}'. Expected 'name:id:version'",
                    material
                ),
                None,
                None,
                None,
            );
        }
    }
    for image in images {
        if !image.starts_with("ssupload:")
            && !image.starts_with("http://")
            && !image.starts_with("https://")
            && !Path::new(image).is_file()
        {
            client::fail(
                "client_error",
                &format!("Image file not found: {}", image),
                None,
                None,
                None,
            );
        }
    }

    let video_durations = inspect_reference_videos(task_type, model_version, videos);
    let audio_durations = reference_audio_durations(task_type, model_version, audios);
    let mut prompts = vec![json!({"type": "text", "content": prompt})];

    // Process images (auto-upload local files / URLs)
    for img_input in images {
        let ssupload_uri = process_image_input(img_input);
        prompts.push(json!({"type": "image", "content": ssupload_uri}));
    }

    for mat in materials {
        let parts: Vec<&str> = mat.split(':').collect();
        if parts.len() != 3 {
            client::fail(
                "client_error",
                &format!(
                    "Invalid material format '{}'. Expected 'name:id:version'",
                    mat
                ),
                None,
                None,
                None,
            );
        }
        prompts.push(json!({
            "type": "material",
            "name": parts[0],
            "material": {"id": parts[1], "version": parts[2]}
        }));
    }

    for (audio_input, duration) in audios.iter().zip(audio_durations) {
        let uri = if let Some(duration) = duration {
            let mut metadata = serde_json::Map::new();
            metadata.insert("duration".to_string(), json!(duration.to_string()));
            crate::commands::upload::upload_media_and_get_uri_with_metadata(
                audio_input,
                Some(metadata),
            )
        } else {
            audio_input.clone()
        };
        let name = Path::new(audio_input)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio");
        prompts.push(json!({"type": "audio", "content": uri, "name": name}));
    }

    // Process video inputs after validating every local clip. Existing ssupload
    // references remain untouched so the backend can validate their full set.
    for (video_input, duration) in videos.iter().zip(video_durations) {
        let uri = if video_input.starts_with("ssupload:") {
            video_input.clone()
        } else {
            let mut metadata = serde_json::Map::new();
            if let Some(duration) = duration {
                metadata.insert("duration".to_string(), json!(duration.to_string()));
            }
            crate::commands::upload::upload_media_and_get_uri_with_metadata(
                video_input,
                Some(metadata),
            )
        };
        let name = Path::new(video_input)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("video");
        prompts.push(json!({"type": "video", "content": uri, "name": name}));
    }

    let body = build_submit_body(
        task_type,
        prompts,
        settings.duration,
        model_version,
        settings.aspect_ratio.as_deref(),
        transition,
        &settings.resolution,
        sample_count,
        codec,
        movement_amplitude,
        Some(&schedule_mode),
    );

    let err = validators::validate_task_body(&body);
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
    }

    let base = client::base_url();
    let data = client::request_json(
        "POST",
        &format!("{}/vidu/v1/tasks", base),
        None,
        Some(&body),
        None,
    );
    let task_id = data
        .get("id")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();
    if task_id.is_empty() {
        client::fail(
            "parse_error",
            &format!("No task id in response: {}", data),
            None,
            None,
            None,
        );
    }
    client::ok(json!({"task_id": task_id}));
}

pub fn get(task_id: &str, output: Option<&str>) {
    let base = client::base_url();
    let data = client::request_json(
        "GET",
        &format!("{}/vidu/v1/tasks/{}", base, task_id),
        None,
        None,
        None,
    );
    let state = data.get("state").and_then(|v| v.as_str()).unwrap_or("");
    let task_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let model = data
        .get("input")
        .and_then(|v| v.get("model_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

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

    if let Some(output_path) = output {
        if state == "success" {
            let media_urls: Vec<&str> = data
                .get("creations")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(download_url_for_creation).collect())
                .unwrap_or_default();
            let subtitle_urls: Vec<&str> = data
                .get("creations")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(subtitle_url_for_creation).collect())
                .unwrap_or_default();

            let artifact_count = media_urls.len() + subtitle_urls.len();
            let output_target = resolve_download_output_target(output_path, artifact_count);

            let http_client = reqwest::blocking::Client::new();
            let mut downloaded: Vec<String> = Vec::new();
            let urls = media_urls
                .iter()
                .enumerate()
                .map(|(i, url)| (*url, None, i, false))
                .chain(
                    subtitle_urls
                        .iter()
                        .map(|url| (*url, Some(subtitle_download_filename(task_id)), 0, true)),
                );
            for (url, filename, file_index, is_subtitle) in urls {
                match http_client
                    .get(url)
                    .timeout(std::time::Duration::from_secs(60))
                    .send()
                {
                    Ok(resp) => {
                        let status = resp.status().as_u16();
                        if status >= 400 {
                            client::fail(
                                "http_error",
                                &format!("Download failed for {}: HTTP {}", url, status),
                                Some(status),
                                None,
                                None,
                            );
                        }
                        let bytes = resp.bytes().unwrap_or_else(|e| {
                            client::fail(
                                "client_error",
                                &format!("Failed to read response bytes: {}", e),
                                None,
                                None,
                                None,
                            );
                        });
                        let bytes = if is_subtitle {
                            preprocess_subtitle_json(&bytes)
                        } else {
                            bytes.to_vec()
                        };
                        let filepath = match filename {
                            Some(filename) => output_target.path_for_name(&filename),
                            None => {
                                let ext = ext_from_bytes(&bytes);
                                output_target.path_for(task_id, file_index, ext)
                            }
                        };
                        std::fs::write(&filepath, &bytes).unwrap_or_else(|e| {
                            client::fail(
                                "client_error",
                                &format!("Failed to write file {}: {}", filepath.display(), e),
                                None,
                                None,
                                None,
                            );
                        });
                        downloaded.push(filepath.to_string_lossy().to_string());
                    }
                    Err(e) => {
                        client::fail(
                            "network_error",
                            &format!("Failed to download {}: {}", url, e),
                            None,
                            None,
                            None,
                        );
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

enum DownloadOutputTarget {
    Directory(PathBuf),
    File(PathBuf),
}

impl DownloadOutputTarget {
    fn path_for(&self, task_id: &str, index: usize, ext: &str) -> PathBuf {
        match self {
            DownloadOutputTarget::Directory(dir) => {
                dir.join(format!("{}_{}.{}", task_id, index, ext))
            }
            DownloadOutputTarget::File(path) => path.clone(),
        }
    }

    fn path_for_name(&self, filename: &str) -> PathBuf {
        match self {
            DownloadOutputTarget::Directory(dir) => dir.join(filename),
            DownloadOutputTarget::File(path) => path.clone(),
        }
    }
}

fn resolve_download_output_target(output: &str, artifact_count: usize) -> DownloadOutputTarget {
    let path = Path::new(output);
    if path.exists() && path.is_dir() {
        std::fs::create_dir_all(path).unwrap_or_else(|e| {
            client::fail(
                "client_error",
                &format!("Failed to create output directory: {}", e),
                None,
                None,
                None,
            );
        });
        return DownloadOutputTarget::Directory(path.to_path_buf());
    }

    let looks_like_file = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| Path::new(name).extension().is_some())
        .unwrap_or(false);
    if looks_like_file {
        if artifact_count > 1 {
            client::fail(
                "client_error",
                "-o/--output as a file path is only supported for single-artifact tasks",
                None,
                None,
                None,
            );
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                    client::fail(
                        "client_error",
                        &format!("Failed to create output parent directory: {}", e),
                        None,
                        None,
                        None,
                    );
                });
            }
        }
        return DownloadOutputTarget::File(path.to_path_buf());
    }

    std::fs::create_dir_all(path).unwrap_or_else(|e| {
        client::fail(
            "client_error",
            &format!("Failed to create output directory: {}", e),
            None,
            None,
            None,
        );
    });
    DownloadOutputTarget::Directory(path.to_path_buf())
}

fn download_url_for_creation(creation: &Value) -> Option<&str> {
    ["nomark_uri", "download_uri", "uri"]
        .iter()
        .filter_map(|field| creation.get(*field).and_then(|u| u.as_str()))
        .find(|uri| !uri.is_empty())
}

fn subtitle_url_for_creation(creation: &Value) -> Option<&str> {
    creation
        .get("subtitle_uri")
        .and_then(|u| u.as_str())
        .filter(|uri| !uri.is_empty())
}

fn download_filename(task_id: &str, creation_index: usize, bytes: &[u8]) -> String {
    format!("{}_{}.{}", task_id, creation_index, ext_from_bytes(bytes))
}

fn subtitle_download_filename(task_id: &str) -> String {
    format!("{}_subtitle.json", task_id)
}

fn preprocess_subtitle_json(bytes: &[u8]) -> Vec<u8> {
    let value: Value = serde_json::from_slice(bytes).unwrap_or_else(|e| {
        client::fail(
            "client_error",
            &format!("Failed to parse subtitle JSON: {}", e),
            None,
            None,
            None,
        );
    });
    let items = value.as_array().unwrap_or_else(|| {
        client::fail(
            "client_error",
            "Subtitle JSON must be an array",
            None,
            None,
            None,
        );
    });
    let simplified: Vec<Value> = items
        .iter()
        .map(|item| {
            let obj = item.as_object().unwrap_or_else(|| {
                client::fail(
                    "client_error",
                    "Subtitle JSON item must be an object",
                    None,
                    None,
                    None,
                );
            });
            let mut out = Map::new();
            for field in ["text", "time_begin", "time_end"] {
                let value = obj.get(field).unwrap_or_else(|| {
                    client::fail(
                        "client_error",
                        &format!("Subtitle JSON item missing field: {}", field),
                        None,
                        None,
                        None,
                    );
                });
                out.insert(field.to_string(), value.clone());
            }
            Value::Object(out)
        })
        .collect();
    serde_json::to_vec_pretty(&simplified).unwrap_or_else(|e| {
        client::fail(
            "client_error",
            &format!("Failed to serialize subtitle JSON: {}", e),
            None,
            None,
            None,
        );
    })
}

fn normalize_tts_text_arg(text: &str) -> String {
    text.replace("\\n", "\n")
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
        (Some(_), Some(_)) => client::fail(
            "client_error",
            "--text and --audio are mutually exclusive",
            None,
            None,
            None,
        ),
        (None, None) => client::fail(
            "client_error",
            "Either --text or --audio is required",
            None,
            None,
            None,
        ),
        _ => {}
    }

    let schedule_mode = resolve_schedule_mode(schedule_mode, None);
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
    let video_name = Path::new(video)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("video1");
    let video_prompt = json!({"type": "video", "content": video_uri, "name": video_name});

    let (prompts, settings) = if let Some(txt) = text {
        let err = validators::validate_lip_sync_text(txt);
        if !err.is_empty() {
            client::fail("client_error", &err, None, None, None);
        }
        let err = validators::validate_tts_voice_id(voice_id);
        if !err.is_empty() {
            client::fail("client_error", &err, None, None, None);
        }
        let duration = calculate_text_duration(txt);
        if duration < 2 {
            client::fail(
                "client_error",
                "Text is too short, duration must be at least 2 seconds",
                None,
                None,
                None,
            );
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
        let duration = round_media_duration(duration_f64);

        let mut metadata = serde_json::Map::new();
        metadata.insert("duration".to_string(), json!(duration_f64.to_string()));
        let audio_uri = crate::commands::upload::upload_media_and_get_uri_with_metadata(
            audio_path,
            Some(metadata),
        );

        let audio_name = Path::new(audio_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio1");
        let prompts = vec![
            video_prompt,
            json!({"type": "audio", "content": audio_uri, "name": audio_name}),
        ];
        let settings =
            json!({"codec": codec, "duration": duration, "schedule_mode": schedule_mode});
        (prompts, settings)
    };

    let body = json!({
        "type": "lip_sync",
        "input": {"prompts": prompts, "enhance": enhance},
        "settings": settings,
    });

    let base = client::base_url();
    let data = client::request_json(
        "POST",
        &format!("{}/vidu/v1/tasks/tool", base),
        None,
        Some(&body),
        None,
    );
    let task_id = data
        .get("id")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();
    if task_id.is_empty() {
        client::fail(
            "parse_error",
            &format!("No task id in response: {}", data),
            None,
            None,
            None,
        );
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

fn round_media_duration(duration: f64) -> i64 {
    duration.round() as i64
}

fn read_audio_duration_f64(path: &str) -> f64 {
    match lofty::read_from_path(path) {
        Ok(tagged_file) => {
            let secs = tagged_file.properties().duration().as_secs_f64();
            secs.max(0.1)
        }
        Err(e) => client::fail(
            "client_error",
            &format!("Cannot read audio duration: {}", e),
            None,
            None,
            None,
        ),
    }
}

fn upload_media_file(path: &str) -> String {
    if !Path::new(path).exists() {
        client::fail(
            "client_error",
            &format!("File not found: {}", path),
            None,
            None,
            None,
        );
    }
    let metadata = read_video_duration_f64(path).map(|dur| {
        let mut m = serde_json::Map::new();
        m.insert("duration".to_string(), json!(dur.to_string()));
        m
    });
    crate::commands::upload::upload_media_and_get_uri_with_metadata(path, metadata)
}

pub fn list_voices() {
    list_tts_voices();
}

pub fn submit_tts(
    prompt: Option<&str>,
    prompt_path: Option<&str>,
    texts: &[String],
    emotions: &[String],
    voice_id: &str,
    speed: f64,
    volume: i32,
    language_boost: Option<&str>,
    subtitle_enable: bool,
    schedule_mode: Option<&str>,
) {
    // subtitle only supports single segment (--prompt or single --text)
    if subtitle_enable && texts.len() > 1 {
        client::fail(
            "client_error",
            "subtitle-enable currently supports only single --prompt or single --text",
            None,
            None,
            None,
        );
    }

    let schedule_mode = resolve_schedule_mode(schedule_mode, None);
    // 1. 构建 (content, Option<emotion>) 列表
    // --prompt wins over --prompt-path; the resolver yields an owned String so
    // the borrows in `segments` stay valid.
    let resolved_prompt: Option<String> = resolve_prompt_inputs(prompt, prompt_path);
    let raw_segments: Vec<(&str, Option<&str>)> = if let Some(p) = resolved_prompt.as_deref() {
        if p.trim().is_empty() {
            client::fail("client_error", "Prompt cannot be empty", None, None, None);
        }
        let emo = emotions
            .first()
            .map(|s| s.as_str())
            .filter(|s| !s.trim().is_empty());
        vec![(p, emo)]
    } else if !texts.is_empty() {
        if emotions.len() > texts.len() {
            client::fail("client_error",
                &format!("Too many --emotion values ({}): must not exceed number of --text segments ({})", emotions.len(), texts.len()),
                None, None, None);
        }
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let emo = emotions
                    .get(i)
                    .map(|s| s.as_str())
                    .filter(|s| !s.trim().is_empty());
                (t.as_str(), emo)
            })
            .collect()
    } else {
        client::fail(
            "client_error",
            "Either --prompt, --prompt-path, or at least one --text is required",
            None,
            None,
            None,
        );
    };
    let segments: Vec<(String, Option<&str>)> = raw_segments
        .into_iter()
        .map(|(content, emo)| (normalize_tts_text_arg(content), emo))
        .collect();

    // 2. 校验段数
    if segments.len() > 20 {
        client::fail(
            "client_error",
            &format!("Too many segments ({}). Maximum: 20", segments.len()),
            None,
            None,
            None,
        );
    }

    // 3. 校验每段内容和 emotion
    for (i, (content, emo)) in segments.iter().enumerate() {
        if content.trim().is_empty() {
            client::fail(
                "client_error",
                &format!("Segment {} text cannot be empty", i + 1),
                None,
                None,
                None,
            );
        }
        let char_count = content.chars().count();
        if char_count > 2000 {
            client::fail(
                "client_error",
                &format!(
                    "Segment {} text too long ({} characters). Maximum: 2000",
                    i + 1,
                    char_count
                ),
                None,
                None,
                None,
            );
        }
        if let Some(e) = emo {
            let err = validators::validate_tts_emotion(e);
            if !err.is_empty() {
                client::fail(
                    "client_error",
                    &format!("Segment {}: {}", i + 1, err),
                    None,
                    None,
                    None,
                );
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
    let prompts: Vec<serde_json::Value> = segments
        .iter()
        .map(|(content, emo)| {
            let mut p = json!({"type": "text", "content": content});
            if let Some(e) = emo {
                p["audio"] = json!({"emotion": e});
            }
            p
        })
        .collect();

    // 6. 构建 settings
    let mut settings = json!({
        "voice_id": voice_id,
        "speed": speed,
        "vol": volume,
        "subtitle_enable": subtitle_enable,
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
    let data = client::request_json(
        "POST",
        &format!("{}/vidu/v1/tasks", base),
        None,
        Some(&body),
        None,
    );

    let task_id = data
        .get("id")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();

    if task_id.is_empty() {
        client::fail(
            "parse_error",
            &format!("No task id in response: {}", data),
            None,
            None,
            None,
        );
    }

    client::ok(json!({"task_id": task_id}));
}

pub fn list_tts_voices() {
    let grouped = validators::tts_voices_grouped();
    let total: usize = grouped.iter().map(|(_, v)| v.len()).sum();
    let languages: Vec<serde_json::Value> = grouped
        .into_iter()
        .map(|(lang, voices)| {
            json!({
                "language": lang,
                "count": voices.len(),
                "voice_ids": voices,
            })
        })
        .collect();
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
    let schedule_mode = resolve_schedule_mode(schedule_mode, None);
    let mut timeline = parse_timeline(timeline_input);
    validate_timeline_clips(&timeline);
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
                None,
                None,
                None,
            );
        }
        let content = match std::fs::read_to_string(input) {
            Ok(c) => c,
            Err(e) => client::fail(
                "client_error",
                &format!("Failed to read timeline file: {}", e),
                None,
                None,
                None,
            ),
        };
        match serde_json::from_str(&content) {
            Ok(v) => return v,
            Err(e) => client::fail(
                "client_error",
                &format!("Invalid JSON in timeline file: {}", e),
                None,
                None,
                None,
            ),
        }
    }
    match serde_json::from_str(input) {
        Ok(v) => v,
        Err(e) => client::fail(
            "client_error",
            &format!("Invalid timeline JSON: {}", e),
            None,
            None,
            None,
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
                                let is_ssupload = map
                                    .get("media_url")
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
        let count = timeline
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if count > MAX_TRACKS_PER_TYPE {
            client::fail(
                "client_error",
                &format!(
                    "{} track count ({}) exceeds maximum of {}.",
                    label, count, MAX_TRACKS_PER_TYPE
                ),
                None,
                None,
                None,
            );
        }
    }
}

fn validate_timeline_clips(timeline: &Value) {
    let err = validators::validate_timeline_clips(timeline);
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
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
        &format!(
            "Cannot resolve media_url: '{}'. Expected ssupload:?id=, URL, or local file path.",
            url
        ),
        None,
        None,
        None,
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
        &format!(
            "Cannot resolve file_url: '{}'. Expected ssupload:?id=, URL, or local file path.",
            url
        ),
        None,
        None,
        None,
    );
}

fn upload_local_subtitle(path: &str) -> String {
    if !Path::new(path).exists() {
        client::fail(
            "client_error",
            &format!("Subtitle file not found: {}", path),
            None,
            None,
            None,
        );
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
            &format!(
                "Image too large: {} ({:.1}MB). Maximum: 50MB.",
                path,
                file_size as f64 / 1024.0 / 1024.0
            ),
            None,
            None,
            None,
        );
    }
    if is_video && file_size > COMPOSE_MAX_VIDEO_SIZE {
        client::fail(
            "client_error",
            &format!(
                "Video too large: {} ({:.1}MB). Maximum: 500MB.",
                path,
                file_size as f64 / 1024.0 / 1024.0
            ),
            None,
            None,
            None,
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
            client::fail(
                "client_error",
                &format!("Media dimensions too small: {}x{}. Minimum: 128x128.", w, h),
                None,
                None,
                None,
            );
        }
        if w > 4096 || h > 4096 {
            client::fail(
                "client_error",
                &format!(
                    "Media dimensions too large: {}x{}. Maximum: 4096x4096.",
                    w, h
                ),
                None,
                None,
                None,
            );
        }
        if short > 2160 {
            client::fail(
                "client_error",
                &format!(
                    "Short side too large: {}x{} (short side {}). Maximum short side: 2160.",
                    w, h, short
                ),
                None,
                None,
                None,
            );
        }
    }

    dims
}

fn get_media_dimensions(path: &str, ext: &str) -> Option<(u32, u32)> {
    match ext {
        "jpg" | "jpeg" | "png" | "bmp" | "webp" => image::image_dimensions(path).ok(),
        "mp4" | "mov" => read_mp4_dimensions(path),
        _ => None,
    }
}

fn video_track_dimensions(track: &mp4parse::Track) -> Option<(u32, u32)> {
    if track.track_type != mp4parse::TrackType::Video {
        return None;
    }

    let sample_entry_dimensions = track
        .stsd
        .as_ref()
        .and_then(|stsd| stsd.descriptions.first())
        .and_then(|desc| match desc {
            mp4parse::SampleEntry::Video(video) => {
                Some((u32::from(video.width), u32::from(video.height)))
            }
            _ => None,
        })
        .filter(|(width, height)| *width > 0 && *height > 0);
    if sample_entry_dimensions.is_some() {
        return sample_entry_dimensions;
    }

    // The sample entry has no dimensions for codecs such as HEVC. tkhd stores
    // the track dimensions as 16.16 fixed-point values; keep its declared
    // width/height, matching the coded dimensions returned above.
    let header = track.tkhd.as_ref()?;
    let dimensions = (header.width >> 16, header.height >> 16);
    (dimensions.0 > 0 && dimensions.1 > 0).then_some(dimensions)
}

#[derive(Debug, Default)]
struct LocalVideoMetadata {
    dimensions: Option<(u32, u32)>,
    duration: Option<f64>,
}

#[derive(Debug)]
enum LocalVideoMetadataReadError {
    FileOpen(std::io::Error),
    Parse(mp4parse::Error),
    NoVideoTrack,
}

fn local_video_metadata_from_context(
    context: &mp4parse::MediaContext,
) -> Result<LocalVideoMetadata, LocalVideoMetadataReadError> {
    let first_video_track = context
        .tracks
        .iter()
        .find(|track| track.track_type == mp4parse::TrackType::Video)
        .ok_or(LocalVideoMetadataReadError::NoVideoTrack)?;

    Ok(LocalVideoMetadata {
        // Preserve the existing semantics: dimensions use the first video
        // track that has readable dimensions; duration uses the first video track.
        dimensions: context.tracks.iter().find_map(video_track_dimensions),
        duration: video_track_duration_f64(first_video_track),
    })
}

fn read_local_video_metadata(
    path: &str,
) -> Result<LocalVideoMetadata, LocalVideoMetadataReadError> {
    let file = std::fs::File::open(path).map_err(LocalVideoMetadataReadError::FileOpen)?;
    let mut reader = std::io::BufReader::new(file);
    let context = mp4parse::read_mp4(&mut reader).map_err(LocalVideoMetadataReadError::Parse)?;
    local_video_metadata_from_context(&context)
}

fn format_local_video_metadata_error(path: &str, error: LocalVideoMetadataReadError) -> String {
    match error {
        LocalVideoMetadataReadError::FileOpen(error) => format!(
            "Cannot open reference video '{}': {}. Check the file path and permissions.",
            path, error
        ),
        LocalVideoMetadataReadError::Parse(error) => format!(
            "Cannot parse MP4/MOV metadata for '{}': {}. Re-export as MP4/MOV and retry.",
            path, error
        ),
        LocalVideoMetadataReadError::NoVideoTrack => {
            format!("No video track found in reference video '{}'.", path)
        }
    }
}

fn format_local_video_dimensions_error(path: &str) -> String {
    format!(
        "Video track in reference video '{}' has no readable dimensions; re-export as MP4/MOV and retry.",
        path
    )
}

fn format_local_video_duration_error(path: &str) -> String {
    format!(
        "Video track in reference video '{}' has no readable duration metadata; re-export as MP4/MOV and retry.",
        path
    )
}

fn video_track_duration_f64(track: &mp4parse::Track) -> Option<f64> {
    if track.track_type != mp4parse::TrackType::Video {
        return None;
    }
    let duration = track.duration?;
    let timescale = track.timescale?;
    (timescale.0 > 0).then_some(duration.0 as f64 / timescale.0 as f64)
}

fn read_mp4_dimensions(path: &str) -> Option<(u32, u32)> {
    read_local_video_metadata(path).ok()?.dimensions
}

fn read_video_duration_f64(path: &str) -> Option<f64> {
    read_local_video_metadata(path).ok()?.duration
}

fn validate_local_reference_video_geometry(
    path: &str,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let pixels = u64::from(width) * u64::from(height);
    let aspect = (height > 0).then_some(width as f64 / height as f64);
    let aspect_text = aspect
        .map(|ratio| format!("{:.2}", ratio))
        .unwrap_or_else(|| "undefined".to_string());

    let violation = if aspect.map_or(true, |ratio| !(0.4..=2.5).contains(&ratio)) {
        Some("aspect ratio must be in [0.4, 2.5]")
    } else if !(300..=60000).contains(&width) {
        Some("width must be in [300, 60000]")
    } else if !(300..=60000).contains(&height) {
        Some("height must be in [300, 60000]")
    } else if !(409600..=2086876).contains(&pixels) {
        Some("total pixels must be in [409600, 2086876]")
    } else {
        None
    };

    if let Some(violation) = violation {
        let suggestion = if width < 300 || height < 300 || pixels < 409600 {
            "Upscale/rescale the video while preserving its aspect ratio"
        } else {
            "Resize or re-export the video to meet the geometry constraints"
        };
        Err(format!(
            "Reference video '{}': dimensions {}x{} ({} pixels, aspect ratio {}); {}. {}.",
            path, width, height, pixels, aspect_text, violation, suggestion
        ))
    } else {
        Ok(())
    }
}

fn upload_local_media(path: &str) -> String {
    if !Path::new(path).exists() {
        client::fail(
            "client_error",
            &format!("Media file not found: {}", path),
            None,
            None,
            None,
        );
    }
    let dims = validate_compose_media(path);
    upload_compose_media(path, dims)
}

fn upload_compose_media(path: &str, dims: Option<(u32, u32)>) -> String {
    let mut metadata = dims.map(|(w, h)| {
        let mut m = serde_json::Map::new();
        m.insert("image-width".into(), json!(w.to_string()));
        m.insert("image-height".into(), json!(h.to_string()));
        m
    });
    if let Some(dur) = read_video_duration_f64(path) {
        let m = metadata.get_or_insert_with(serde_json::Map::new);
        m.insert("duration".to_string(), json!(dur.to_string()));
    }
    crate::commands::upload::upload_media_and_get_uri_with_metadata(path, metadata)
}

pub fn query_credits(
    task_type: &str,
    model_version: &str,
    duration: Option<i64>,
    resolution: Option<&str>,
    aspect_ratio: Option<&str>,
    transition: Option<&str>,
    sample_count: i64,
    codec: &str,
    schedule_mode: Option<&str>,
) {
    let settings = normalize_task_settings(
        task_type,
        model_version,
        duration,
        resolution,
        aspect_ratio,
        Some("1080p"),
    )
    .unwrap_or_else(|err| client::fail("client_error", &err, None, None, None));
    let schedule_mode = resolve_schedule_mode(schedule_mode, Some(model_version));

    if model_version == "3.4" {
        let err = validators::validate_q4_cost(
            task_type,
            settings.duration,
            &settings.resolution,
            transition,
        );
        if !err.is_empty() {
            client::fail("client_error", &err, None, None, None);
        }
    } else if model_version == validators::DRAMA_MODEL_VERSION {
        let err = validators::validate_drama_cost(
            task_type,
            settings.duration,
            &settings.resolution,
            transition,
        );
        if !err.is_empty() {
            client::fail("client_error", &err, None, None, None);
        }
    }
    let params = build_credit_query_params(
        task_type,
        model_version,
        &settings,
        transition,
        sample_count,
        codec,
        &schedule_mode,
    );

    let base = client::base_url();
    let data = client::request_json(
        "GET",
        &format!("{}/vidu/v1/tasks/credits", base),
        None,
        None,
        Some(&params),
    );
    output_credits_result(&data);
}

pub fn query_tts_credits(
    text: &str,
    voice_id: &str,
    speed: f64,
    pitch: i32,
    volume: i32,
    schedule_mode: Option<&str>,
) {
    let schedule_mode = resolve_schedule_mode(schedule_mode, None);

    let mut params = std::collections::HashMap::new();
    params.insert("type".to_string(), "tts".to_string());
    params.insert("settings.voice_id".to_string(), voice_id.to_string());
    params.insert("settings.speed".to_string(), speed.to_string());
    params.insert("settings.pitch".to_string(), pitch.to_string());
    params.insert("settings.volume".to_string(), volume.to_string());
    params.insert(
        "settings.schedule_mode".to_string(),
        schedule_mode.to_string(),
    );
    params.insert("text".to_string(), text.to_string());

    let base = client::base_url();
    let data = client::request_json(
        "GET",
        &format!("{}/vidu/v1/tasks/credits", base),
        None,
        None,
        Some(&params),
    );
    output_credits_result(&data);
}

pub fn query_lip_sync_credits(
    duration: i64,
    voice_id: &str,
    speed: f64,
    volume: f64,
    codec: &str,
    schedule_mode: Option<&str>,
) {
    let schedule_mode = resolve_schedule_mode(schedule_mode, None);

    let mut params = std::collections::HashMap::new();
    params.insert("type".to_string(), "lip_sync".to_string());
    params.insert("settings.duration".to_string(), duration.to_string());
    params.insert("settings.voice_id".to_string(), voice_id.to_string());
    params.insert("settings.speed".to_string(), speed.to_string());
    params.insert("settings.codec".to_string(), codec.to_string());
    params.insert(
        "settings.schedule_mode".to_string(),
        schedule_mode.to_string(),
    );
    if volume != 0.0 {
        params.insert("settings.volume".to_string(), volume.to_string());
    }

    let base = client::base_url();
    let data = client::request_json(
        "GET",
        &format!("{}/vidu/v1/tasks/credits", base),
        None,
        None,
        Some(&params),
    );
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
    if let Some(quote) = data.get("claw_pass_quote") {
        result["claw_pass_quote"] = quote.clone();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_video_metadata_reader_distinguishes_open_and_parse_errors() {
        let temp_dir = tempfile::tempdir().unwrap();
        let missing = temp_dir.path().join("missing.mp4");
        let open_error = read_local_video_metadata(missing.to_str().unwrap()).unwrap_err();
        let open_message = format_local_video_metadata_error(missing.to_str().unwrap(), open_error);
        assert!(open_message.contains("Cannot open reference video"));
        assert!(open_message.contains(missing.to_str().unwrap()));
        assert!(open_message.contains("permissions"));

        let damaged = temp_dir.path().join("damaged.mp4");
        std::fs::write(&damaged, b"not a valid MP4").unwrap();
        let parse_error = read_local_video_metadata(damaged.to_str().unwrap()).unwrap_err();
        let parse_message = format_local_video_metadata_error(damaged.to_str().unwrap(), parse_error);
        assert!(parse_message.contains("Cannot parse MP4/MOV metadata"));
        assert!(parse_message.contains(damaged.to_str().unwrap()));
        assert!(parse_message.contains("Re-export as MP4/MOV and retry"));
    }

    #[test]
    fn local_video_metadata_reader_rejects_audio_only_tracks_and_keeps_fields_independent() {
        let mut audio_only = mp4parse::MediaContext::default();
        audio_only
            .tracks
            .push(mp4parse::Track {
                track_type: mp4parse::TrackType::Audio,
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            local_video_metadata_from_context(&audio_only),
            Err(LocalVideoMetadataReadError::NoVideoTrack)
        ));

        let mut context = mp4parse::MediaContext::default();
        let mut video_track = mp4parse::Track::default();
        video_track.track_type = mp4parse::TrackType::Video;
        video_track.duration = Some(mp4parse::TrackScaledTime(4, 1));
        video_track.timescale = Some(mp4parse::TrackTimeScale(1, 1));
        context.tracks.push(video_track).unwrap();
        let metadata = local_video_metadata_from_context(&context).unwrap();
        assert_eq!(metadata.dimensions, None);
        assert_eq!(metadata.duration, Some(4.0));
        assert!(format_local_video_dimensions_error("audio-only.mp4")
            .contains("has no readable dimensions"));
    }

    #[test]
    fn local_reference_video_geometry_error_includes_path_details_and_resize_guidance() {
        let error = validate_local_reference_video_geometry("sample.mp4", 704, 396).unwrap_err();
        assert_eq!(
            error,
            "Reference video 'sample.mp4': dimensions 704x396 (278784 pixels, aspect ratio 1.78); total pixels must be in [409600, 2086876]. Upscale/rescale the video while preserving its aspect ratio."
        );
        assert_eq!(
            validate_local_reference_video_geometry("sample.mp4", 864, 486),
            Ok(())
        );
    }

    #[test]
    fn read_mp4_dimensions_uses_tkhd_for_hevc_and_keeps_coded_size_for_rotated_track() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("hevc-rotated.mp4");
        std::fs::write(
            &path,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/hevc_640x360_rotated.mp4"
            )),
        )
        .unwrap();

        assert_eq!(
            read_mp4_dimensions(path.to_str().unwrap()),
            Some((640, 360))
        );

        let file = std::fs::File::open(&path).unwrap();
        let mut reader = std::io::BufReader::new(file);
        let context = mp4parse::read_mp4(&mut reader).unwrap();
        let mut track = context.tracks.into_iter().next().unwrap();
        assert_eq!(track.track_type, mp4parse::TrackType::Video);
        assert!(matches!(
            track
                .stsd
                .as_ref()
                .and_then(|stsd| stsd.descriptions.first()),
            Some(mp4parse::SampleEntry::Unknown)
        ));
        let header = track.tkhd.as_ref().unwrap();
        assert_eq!(
            (
                header.matrix.a,
                header.matrix.b,
                header.matrix.c,
                header.matrix.d
            ),
            (0, -65536, 65536, 0)
        );
        assert_eq!(video_track_dimensions(&track), Some((640, 360)));

        track.track_type = mp4parse::TrackType::Audio;
        assert_eq!(video_track_dimensions(&track), None);
        track.track_type = mp4parse::TrackType::Video;
        let header = track.tkhd.as_mut().unwrap();
        header.width = 0;
        header.height = 0;
        assert_eq!(video_track_dimensions(&track), None);
    }

    #[test]
    fn drama_submit_and_cost_build_the_same_default_settings() {
        let submit = normalize_task_settings(
            "text2video",
            validators::DRAMA_MODEL_VERSION,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let cost = normalize_task_settings(
            "text2video",
            validators::DRAMA_MODEL_VERSION,
            None,
            None,
            None,
            Some("1080p"),
        )
        .unwrap();
        assert_eq!(submit, cost);
        assert_eq!(submit.duration, 5);
        assert_eq!(submit.resolution, "720p");
        assert_eq!(submit.aspect_ratio.as_deref(), Some("16:9"));
        assert_eq!(
            validators::preferred_schedule_mode(Some(validators::DRAMA_MODEL_VERSION), None),
            Ok(Some("normal".into()))
        );

        let body = build_submit_body(
            "text2video",
            vec![json!({"type": "text", "content": "story"})],
            submit.duration,
            validators::DRAMA_MODEL_VERSION,
            submit.aspect_ratio.as_deref(),
            None,
            &submit.resolution,
            1,
            "h264",
            "auto",
            Some("normal"),
        );
        assert_eq!(body["settings"]["duration"], 5);
        assert_eq!(body["settings"]["resolution"], "720p");
        assert_eq!(body["settings"]["aspect_ratio"], "16:9");

        let params = build_credit_query_params(
            "text2video",
            validators::DRAMA_MODEL_VERSION,
            &cost,
            None,
            1,
            "h264",
            "normal",
        );
        assert_eq!(params["settings.duration"], "5");
        assert_eq!(params["settings.resolution"], "720p");
        assert_eq!(params["settings.aspect_ratio"], "16:9");
    }

    #[test]
    fn legacy_resolution_and_duration_rules_remain_unchanged() {
        assert!(normalize_task_settings("text2video", "3.2", None, None, None, None)
            .unwrap_err()
            .contains("duration is required"));
        assert!(normalize_task_settings("text2video", "3.2", Some(5), None, None, None)
            .unwrap_err()
            .contains("resolution is required"));

        let cost = normalize_task_settings(
            "text2video",
            "3.2",
            Some(5),
            None,
            None,
            Some("1080p"),
        )
        .unwrap();
        assert_eq!(cost.duration, 5);
        assert_eq!(cost.resolution, "1080p");
        assert_eq!(cost.aspect_ratio, None);
    }

    #[test]
    fn drama_local_reference_video_duration_rules_are_checked_before_upload() {
        assert_eq!(
            validate_local_reference_video_durations(
                validators::DRAMA_MODEL_VERSION,
                &[4.0, 26.0],
            ),
            Ok(())
        );
        assert!(validate_local_reference_video_durations(
            validators::DRAMA_MODEL_VERSION,
            &[3.99],
        )
        .unwrap_err()
        .contains("out of range [4, 30]"));
        assert!(validate_local_reference_video_durations(
            validators::DRAMA_MODEL_VERSION,
            &[15.1, 15.1],
        )
        .unwrap_err()
        .contains("exceeds 30s"));
        assert_eq!(validate_local_reference_video_durations("3.2_a", &[8.0, 7.0]), Ok(()));
        assert!(validate_local_reference_video_durations("3.2_a", &[8.0, 7.1])
            .unwrap_err()
            .contains("exceeds 15s"));
    }

    #[test]
    fn ext_from_bytes_jpeg() {
        assert_eq!(ext_from_bytes(&[0xFF, 0xD8, 0xFF, 0xE0]), "jpg");
    }

    #[test]
    fn ext_from_bytes_png() {
        assert_eq!(ext_from_bytes(&[0x89, 0x50, 0x4E, 0x47]), "png");
    }

    #[test]
    fn ext_from_bytes_gif() {
        assert_eq!(ext_from_bytes(b"GIF89a"), "gif");
    }

    #[test]
    fn ext_from_bytes_webp() {
        assert_eq!(ext_from_bytes(b"RIFF\x00\x00\x00\x00WEBP"), "webp");
    }

    #[test]
    fn ext_from_bytes_wav() {
        assert_eq!(ext_from_bytes(b"RIFF\x00\x00\x00\x00WAVE"), "wav");
    }

    #[test]
    fn ext_from_bytes_mp3_id3() {
        assert_eq!(ext_from_bytes(b"ID3\x04"), "mp3");
    }

    #[test]
    fn ext_from_bytes_mp3_sync() {
        assert_eq!(ext_from_bytes(&[0xFF, 0xFB, 0x90, 0x00]), "mp3");
    }

    #[test]
    fn ext_from_bytes_aac_adts() {
        // Note: 0xFF 0xF1 matches MP3 sync check first (0xF1 & 0xE0 == 0xE0)
        // AAC ADTS detection is shadowed by MP3 detection for these bytes
        assert_eq!(ext_from_bytes(&[0xFF, 0xF1, 0x00, 0x00]), "mp3");
    }

    #[test]
    fn ext_from_bytes_mp4_ftyp() {
        assert_eq!(ext_from_bytes(b"\x00\x00\x00\x1cftypisom"), "mp4");
    }

    #[test]
    fn ext_from_bytes_short_input() {
        assert_eq!(ext_from_bytes(&[0x00, 0x01]), "mp4");
    }

    #[test]
    fn ext_from_bytes_unknown() {
        assert_eq!(
            ext_from_bytes(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
            "mp4"
        );
    }

    #[test]
    fn download_url_prefers_nomark_uri() {
        let creation = json!({
            "nomark_uri": "https://example.com/no-watermark.mp4",
            "download_uri": "https://example.com/watermarked-download.mp4",
            "uri": "https://example.com/watermarked.mp4",
        });

        assert_eq!(
            download_url_for_creation(&creation),
            Some("https://example.com/no-watermark.mp4")
        );
    }

    #[test]
    fn download_url_falls_back_when_nomark_uri_empty() {
        let creation = json!({
            "nomark_uri": "",
            "download_uri": "https://example.com/watermarked-download.mp4",
            "uri": "https://example.com/watermarked.mp4",
        });

        assert_eq!(
            download_url_for_creation(&creation),
            Some("https://example.com/watermarked-download.mp4")
        );
    }

    #[test]
    fn download_url_ignores_subtitle_uri_for_primary_media() {
        let creation = json!({
            "subtitle_uri": "https://example.com/subtitle.json",
            "uri": "https://example.com/watermarked.mp4",
        });

        assert_eq!(
            download_url_for_creation(&creation),
            Some("https://example.com/watermarked.mp4")
        );
    }

    #[test]
    fn subtitle_url_returns_subtitle_uri() {
        let creation = json!({
            "subtitle_uri": "https://example.com/subtitle.json",
        });

        assert_eq!(
            subtitle_url_for_creation(&creation),
            Some("https://example.com/subtitle.json")
        );
    }

    #[test]
    fn subtitle_url_ignores_empty_subtitle_uri() {
        let creation = json!({
            "subtitle_uri": "",
        });

        assert_eq!(subtitle_url_for_creation(&creation), None);
    }

    #[test]
    fn subtitle_download_filename_uses_json_extension() {
        assert_eq!(
            subtitle_download_filename("task_123"),
            "task_123_subtitle.json".to_string()
        );
    }

    #[test]
    fn preprocess_subtitle_json_keeps_only_public_fields() {
        let input = r#"[
            {
                "text": "Vidu是生数科技推出的旗舰AI视频生成模型，",
                "pronounce_text": "Vidu是生数科技推出的旗舰AI视频生成模型，",
                "time_begin": 0,
                "time_end": 3777.732426303855,
                "text_begin": 0,
                "text_end": 23
            }
        ]"#;

        let output = preprocess_subtitle_json(input.as_bytes());
        let value: Value = serde_json::from_slice(&output).expect("valid json");
        assert_eq!(
            value,
            json!([
                {
                    "text": "Vidu是生数科技推出的旗舰AI视频生成模型，",
                    "time_begin": 0,
                    "time_end": 3777.732426303855
                }
            ])
        );
    }

    #[test]
    fn normalize_tts_text_arg_converts_escaped_newline() {
        assert_eq!(
            normalize_tts_text_arg("第一行\\n第二行"),
            "第一行\n第二行".to_string()
        );
    }

    #[test]
    fn normalize_tts_text_arg_keeps_real_newline() {
        assert_eq!(
            normalize_tts_text_arg("第一行\n第二行"),
            "第一行\n第二行".to_string()
        );
    }

    #[test]
    fn calculate_text_duration_english() {
        assert_eq!(calculate_text_duration("hello"), 1);
        assert_eq!(calculate_text_duration("hello world test"), 2);
    }

    #[test]
    fn calculate_text_duration_chinese() {
        // (6 + 4) / 5 = 2, (2 + 4) / 5 = 1
        assert_eq!(calculate_text_duration("你好世界测试"), 2);
        assert_eq!(calculate_text_duration("你好"), 1);
    }

    #[test]
    fn calculate_text_duration_empty() {
        // (0 + 9) / 10 = 0
        assert_eq!(calculate_text_duration(""), 0);
    }

    #[test]
    fn round_media_duration_matches_upload_metadata() {
        assert_eq!(round_media_duration(4.1), 4);
        assert_eq!(round_media_duration(4.5), 5);
        assert_eq!(round_media_duration(4.6), 5);
    }

    // --- resolve_prompt_inputs ---

    fn write_tmp(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let dir = tempfile::tempdir().unwrap().keep();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn resolve_prompt_inputs_uses_prompt_when_only_prompt_given() {
        let out = resolve_prompt_inputs(Some("inline text"), None);
        assert_eq!(out.as_deref(), Some("inline text"));
    }

    #[test]
    fn resolve_prompt_inputs_reads_file_when_only_path_given() {
        let path = write_tmp("only_path.txt", b"from file\n");
        let out = resolve_prompt_inputs(None, Some(path.to_str().unwrap()));
        assert_eq!(out.as_deref(), Some("from file"));
    }

    #[test]
    fn resolve_prompt_inputs_prompt_wins_over_path() {
        // Even an unreadable path must be ignored when --prompt is provided,
        // proving the precedence is short-circuit and the file is never read.
        let out =
            resolve_prompt_inputs(Some("inline wins"), Some("/definitely/does/not/exist.txt"));
        assert_eq!(out.as_deref(), Some("inline wins"));
    }

    #[test]
    fn resolve_prompt_inputs_returns_none_when_neither_given() {
        assert!(resolve_prompt_inputs(None, None).is_none());
    }
}
