use crate::{client, validators};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
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

pub fn get(task_id: &str) {
    let base = client::base_url();
    let data = client::request_json("GET", &format!("{}/vidu/v1/tasks/{}", base, task_id), None, None, None);
    let state = data.get("state").and_then(|v| v.as_str()).unwrap_or("");
    let mut result = json!({"task_id": task_id, "state": state});

    if state == "success" {
        let urls: Vec<&str> = data.get("creations")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|c| c.get("nomark_uri").and_then(|u| u.as_str())).collect())
            .unwrap_or_default();
        result["media_urls"] = json!(urls);
    } else if state == "failed" {
        result["err_code"] = json!(data.get("err_code").and_then(|v| v.as_str()).unwrap_or(""));
        result["err_msg"] = json!(data.get("err_msg").and_then(|v| v.as_str()).unwrap_or(""));
    }

    client::ok(result);
}

pub fn sse(task_id: &str) {
    let base = client::base_url();
    let url = format!("{}/vidu/v1/tasks/state?id={}", base, task_id);
    let mut extra = std::collections::HashMap::new();
    extra.insert("Accept".into(), "text/event-stream".into());
    let headers_map = client::get_headers(Some(&extra));

    let http_client = reqwest::blocking::Client::new();
    let mut builder = http_client.get(&url).timeout(std::time::Duration::from_secs(300));
    for (k, v) in &headers_map {
        builder = builder.header(k, v);
    }

    match builder.send() {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if status >= 400 {
                client::fail("http_error", &format!("SSE request failed with status {}", status), Some(status), None, None);
            }
            let reader = BufReader::new(resp);
            for line in reader.lines() {
                match line {
                    Ok(l) if !l.is_empty() => println!("{}", l),
                    Err(e) => {
                        client::fail("network_error", &e.to_string(), None, None, None);
                    }
                    _ => {}
                }
            }
        }
        Err(e) => {
            if e.is_timeout() {
                client::fail("network_error", "timeout", None, None, None);
            } else {
                client::fail("network_error", &e.to_string(), None, None, None);
            }
        }
    }
}
