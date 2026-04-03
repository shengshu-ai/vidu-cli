use reqwest::blocking::{Client, Response};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::{env, process, thread, time::Duration};

const MAX_RETRIES: usize = 3;
const RETRY_DELAYS: [u64; 3] = [1, 2, 4];

pub fn base_url() -> String {
    env::var("VIDU_BASE_URL").unwrap_or_else(|_| "https://service.vidu.cn".to_string())
}

fn token() -> String {
    env::var("VIDU_TOKEN").unwrap_or_default()
}

pub fn fail(error_type: &str, message: &str, http_status: Option<u16>, code: Option<&str>, step: Option<&str>) -> ! {
    let mut err = json!({"type": error_type, "message": message});
    if let Some(s) = http_status {
        err["http_status"] = json!(s);
    }
    if let Some(c) = code {
        err["code"] = json!(c);
    }
    if let Some(st) = step {
        err["step"] = json!(st);
    }
    println!("{}", json!({"ok": false, "error": err}));
    process::exit(1);
}

pub fn ok(extra: Value) {
    let mut obj = json!({"ok": true});
    if let Value::Object(map) = extra {
        for (k, v) in map {
            obj[k] = v;
        }
    }
    println!("{}", obj);
    process::exit(0);
}

pub fn get_headers(extra: Option<&HashMap<String, String>>) -> HashMap<String, String> {
    let tok = token();
    if tok.is_empty() {
        fail("client_error", "VIDU_TOKEN not set", None, None, None);
    }
    let mut headers = HashMap::new();
    headers.insert("Authorization".into(), format!("Token {tok}"));
    headers.insert("Content-Type".into(), "application/json".into());
    headers.insert("User-Agent".into(), format!("viduclawbot/1.0 (+{})", base_url()));
    if let Some(e) = extra {
        for (k, v) in e {
            headers.insert(k.clone(), v.clone());
        }
    }
    headers
}

fn build_reqwest_headers(map: &HashMap<String, String>) -> reqwest::header::HeaderMap {
    let mut hm = reqwest::header::HeaderMap::new();
    for (k, v) in map {
        if let (Ok(name), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            reqwest::header::HeaderValue::from_str(v),
        ) {
            hm.insert(name, val);
        }
    }
    hm
}

fn parse_error_body(resp: Response) -> (String, String) {
    let text = resp.text().unwrap_or_default();
    if let Ok(body) = serde_json::from_str::<Value>(&text) {
        let code = body.get("code")
            .or_else(|| body.get("err_code"))
            .and_then(|v| v.as_str().or_else(|| v.as_i64().map(|_| "")))
            .unwrap_or("")
            .to_string();
        let code = if code.is_empty() {
            body.get("code").or_else(|| body.get("err_code"))
                .map(|v| v.to_string()).unwrap_or_default()
        } else { code };
        let msg = body.get("message")
            .or_else(|| body.get("msg"))
            .or_else(|| body.get("err_msg"))
            .and_then(|v| v.as_str())
            .unwrap_or(&text)
            .to_string();
        (code, msg)
    } else {
        let truncated: String = text.chars().take(200).collect();
        (String::new(), truncated)
    }
}

pub fn request(method: &str, url: &str, step: Option<&str>, retries: bool, body: Option<&Value>, params: Option<&HashMap<String, String>>) -> Response {
    let client = Client::new();
    let attempts = if retries { MAX_RETRIES } else { 1 };
    let headers_map = get_headers(None);
    let headers = build_reqwest_headers(&headers_map);
    let mut last_exc: Option<String> = None;
    let mut last_resp: Option<Response> = None;

    for i in 0..attempts {
        let mut builder = match method {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            _ => client.get(url),
        };
        builder = builder.headers(headers.clone()).timeout(Duration::from_secs(30));
        if let Some(b) = body {
            builder = builder.json(b);
        }
        if let Some(p) = params {
            builder = builder.query(&p.iter().collect::<Vec<_>>());
        }

        match builder.send() {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status >= 500 && i < attempts - 1 {
                    last_resp = Some(resp);
                    thread::sleep(Duration::from_secs(RETRY_DELAYS[i]));
                    continue;
                }
                if status >= 400 {
                    let (code, msg) = parse_error_body(resp);
                    let code_opt = if code.is_empty() { None } else { Some(code.as_str()) };
                    fail("http_error", &msg, Some(status), code_opt, step);
                }
                return resp;
            }
            Err(e) => {
                if e.is_timeout() {
                    last_exc = Some("timeout".into());
                } else {
                    last_exc = Some(e.to_string());
                }
                if i < attempts - 1 {
                    thread::sleep(Duration::from_secs(RETRY_DELAYS[i]));
                }
            }
        }
    }

    if let Some(exc) = last_exc {
        fail("network_error", &exc, None, None, step);
    }
    // 5xx retries exhausted
    if let Some(resp) = last_resp {
        let status = resp.status().as_u16();
        let (code, msg) = parse_error_body(resp);
        let code_opt = if code.is_empty() { None } else { Some(code.as_str()) };
        fail("http_error", &msg, Some(status), code_opt, step);
    }
    fail("network_error", "unknown error", None, None, step);
}

pub fn request_json(method: &str, url: &str, step: Option<&str>, body: Option<&Value>, params: Option<&HashMap<String, String>>) -> Value {
    let resp = request(method, url, step, true, body, params);
    let trace_id = resp.headers()
        .get("x-md-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let text = resp.text().unwrap_or_default();

    // Debug mode: print response body to stderr
    if env::var("VIDU_DEBUG").is_ok() {
        eprintln!("[DEBUG] Response body: {}", text);
    }

    match serde_json::from_str::<Value>(&text) {
        Ok(mut v) => {
            if !trace_id.is_empty() {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("trace_id".to_string(), Value::String(trace_id));
                }
            }
            v
        }
        Err(_) => {
            let truncated: String = text.chars().take(200).collect();
            fail("parse_error", &format!("Response is not valid JSON: {truncated}"), None, None, step);
        }
    }
}

pub fn put_raw(url: &str, data: Vec<u8>, headers_map: &HashMap<String, String>, step: Option<&str>) -> (String,) {
    let client = Client::new();
    let headers = build_reqwest_headers(headers_map);
    let mut last_exc: Option<String> = None;

    for i in 0..MAX_RETRIES {
        match client.put(url).headers(headers.clone()).body(data.clone()).timeout(Duration::from_secs(60)).send() {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status >= 500 && i < MAX_RETRIES - 1 {
                    thread::sleep(Duration::from_secs(RETRY_DELAYS[i]));
                    continue;
                }
                if status >= 400 {
                    let text: String = resp.text().unwrap_or_default().chars().take(200).collect();
                    fail("http_error", &format!("PUT failed: {text}"), Some(status), None, step);
                }
                let etag = resp.headers().get("ETag")
                    .map(|v| v.to_str().unwrap_or("").to_string())
                    .unwrap_or_default();
                return (etag,);
            }
            Err(e) => {
                last_exc = Some(if e.is_timeout() { "timeout".into() } else { e.to_string() });
                if i < MAX_RETRIES - 1 {
                    thread::sleep(Duration::from_secs(RETRY_DELAYS[i]));
                }
            }
        }
    }
    fail("network_error", &last_exc.unwrap_or_default(), None, None, step);
}

pub fn put_raw_large(url: &str, data: Vec<u8>, headers_map: &HashMap<String, String>, step: Option<&str>) -> (String,) {
    let client = Client::new();
    let headers = build_reqwest_headers(headers_map);
    let mut last_exc: Option<String> = None;

    for i in 0..MAX_RETRIES {
        match client.put(url).headers(headers.clone()).body(data.clone()).timeout(Duration::from_secs(600)).send() {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status >= 500 && i < MAX_RETRIES - 1 {
                    thread::sleep(Duration::from_secs(RETRY_DELAYS[i]));
                    continue;
                }
                if status >= 400 {
                    let text: String = resp.text().unwrap_or_default().chars().take(200).collect();
                    fail("http_error", &format!("PUT failed: {text}"), Some(status), None, step);
                }
                let etag = resp.headers().get("ETag")
                    .map(|v| v.to_str().unwrap_or("").to_string())
                    .unwrap_or_default();
                return (etag,);
            }
            Err(e) => {
                last_exc = Some(if e.is_timeout() { "timeout".into() } else { e.to_string() });
                if i < MAX_RETRIES - 1 {
                    thread::sleep(Duration::from_secs(RETRY_DELAYS[i]));
                }
            }
        }
    }
    fail("network_error", &last_exc.unwrap_or_default(), None, None, step);
}
