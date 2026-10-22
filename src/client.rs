use reqwest::blocking::{Client, Response};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{env, process, thread, time::Duration};

const MAX_RETRIES: usize = 3;
const RETRY_DELAYS: [u64; 3] = [1, 2, 4];

static TABLE_OUTPUT: AtomicBool = AtomicBool::new(false);

/// Select the process-wide presentation format; JSON remains the default.
pub fn set_output_format(table: bool) {
    TABLE_OUTPUT.store(table, Ordering::Relaxed);
}

fn cell(value: &Value) -> String {
    match value {
        Value::String(text) => text.chars().fold(String::new(), |mut out, ch| {
            match ch {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() => out.extend(c.escape_default()),
                c => out.push(c),
            }
            out
        }),
        other => other.to_string(),
    }
}

fn table(headers: &[String], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            rows.iter()
                .map(|row| row[i].chars().count())
                .max()
                .unwrap_or(0)
                .max(h.chars().count())
        })
        .collect();
    let line = |values: &[String]| {
        values
            .iter()
            .enumerate()
            .map(|(i, value)| format!("{}{}", value, " ".repeat(widths[i] - value.chars().count())))
            .collect::<Vec<_>>()
            .join(" | ")
            .trim_end()
            .to_string()
    };
    let mut lines = vec![
        line(headers),
        widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join("-+-"),
    ];
    lines.extend(rows.iter().map(|row| line(row)));
    lines.join("\n")
}

fn detail_rows(path: &str, value: &Value, rows: &mut Vec<Vec<String>>, lists: &mut Vec<String>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, value) in map {
                let key = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                detail_rows(&key, value, rows, lists);
            }
        }
        Value::Array(items) => {
            let mut section = format!("{path} ({} items)\n", items.len());
            if items.is_empty() {
                section.push_str("(empty)");
            } else if items.iter().all(Value::is_object) {
                let columns: Vec<String> = items
                    .iter()
                    .flat_map(|v| v.as_object().unwrap().keys().cloned())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                if columns.is_empty() {
                    section.push_str(&table(
                        &["value".into()],
                        &items.iter().map(|v| vec![cell(v)]).collect::<Vec<_>>(),
                    ));
                } else {
                    let values = items
                        .iter()
                        .map(|item| {
                            columns
                                .iter()
                                .map(|key| item.get(key).map(cell).unwrap_or_default())
                                .collect()
                        })
                        .collect::<Vec<_>>();
                    section.push_str(&table(&columns, &values));
                }
            } else {
                section.push_str(&table(
                    &["value".into()],
                    &items.iter().map(|v| vec![cell(v)]).collect::<Vec<_>>(),
                ));
            }
            lists.push(section);
        }
        _ => rows.push(vec![path.to_string(), cell(value)]),
    }
}

fn render_output(value: &Value, as_table: bool) -> String {
    if !as_table {
        return value.to_string();
    }
    let mut rows = Vec::new();
    let mut sections = Vec::new();
    detail_rows("", value, &mut rows, &mut sections);
    if !rows.is_empty() {
        sections.insert(0, table(&["field".into(), "value".into()], &rows));
    }
    sections.join("\n\n")
}

fn print_output(value: &Value) {
    println!(
        "{}",
        render_output(value, TABLE_OUTPUT.load(Ordering::Relaxed))
    );
}

pub fn base_url() -> String {
    env::var("VIDU_BASE_URL").unwrap_or_else(|_| "https://service.vidu.cn".to_string())
}

fn token() -> String {
    env::var("VIDU_TOKEN").unwrap_or_default()
}

fn render_message(message: &str, fields: Option<&Value>) -> String {
    match fields {
        Some(Value::String(s)) => message.replace("{{.fields}}", s),
        Some(v) => message.replace("{{.fields}}", &v.to_string()),
        None => message.to_string(),
    }
}

// Preserve the established diagnostic API used across command modules.
#[allow(clippy::too_many_arguments)]
pub fn fail_with_fields(
    error_type: &str,
    message: &str,
    http_status: Option<u16>,
    code: Option<&str>,
    step: Option<&str>,
    fields: Option<&Value>,
    trace_id: Option<&str>,
    cause: Option<&Value>,
    suggestion: Option<&str>,
) -> ! {
    let rendered_message = render_message(message, fields);
    let mut err = json!({"type": error_type, "message": rendered_message});
    if let Some(s) = http_status {
        err["http_status"] = json!(s);
    }
    if let Some(c) = code {
        err["code"] = json!(c);
    }
    if let Some(st) = step {
        err["step"] = json!(st);
    }
    if let Some(f) = fields {
        err["fields"] = f.clone();
    }
    if let Some(t) = trace_id {
        if !t.is_empty() {
            err["trace_id"] = json!(t);
        }
    }
    if let Some(c) = cause {
        err["cause"] = c.clone();
    }
    if let Some(s) = suggestion {
        err["suggestion"] = json!(s);
    }
    print_output(&json!({"ok": false, "error": err}));
    process::exit(1);
}

pub fn fail(
    error_type: &str,
    message: &str,
    http_status: Option<u16>,
    code: Option<&str>,
    step: Option<&str>,
) -> ! {
    fail_with_fields(
        error_type,
        message,
        http_status,
        code,
        step,
        None,
        None,
        None,
        None,
    );
}

pub fn ok(extra: Value) {
    let mut obj = json!({"ok": true});
    if let Value::Object(map) = extra {
        for (k, v) in map {
            obj[k] = v;
        }
    }
    print_output(&obj);
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
    headers.insert(
        "User-Agent".into(),
        format!("viduclawbot/1.0 (+{})", base_url()),
    );
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

fn merge_error_fields(fields: Option<&Value>, context: Option<&Value>) -> Option<Value> {
    let Some(context) = context else {
        return fields.cloned();
    };
    let mut merged = match context {
        Value::Object(map) => map.clone(),
        value => serde_json::Map::from_iter([("context".into(), value.clone())]),
    };
    match fields {
        Some(Value::Object(map)) => merged.extend(map.clone()),
        Some(value) => {
            merged.insert("response_fields".into(), value.clone());
        }
        None => {}
    }
    Some(Value::Object(merged))
}

fn fail_context(error_type: &str, message: &str, step: Option<&str>, context: Option<&Value>) -> ! {
    let fields = merge_error_fields(None, context);
    fail_with_fields(
        error_type,
        message,
        None,
        None,
        step,
        fields.as_ref(),
        None,
        None,
        None,
    );
}

fn parse_error_body(
    resp: Response,
    step: Option<&str>,
    explicit_timeout: bool,
    context: Option<&Value>,
) -> (
    String,
    String,
    Option<Value>,
    String,
    Option<Value>,
    Option<String>,
) {
    let header_trace_id = resp
        .headers()
        .get("x-md-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let text = response_text(resp, step, explicit_timeout, context);
    if env::var("VIDU_DEBUG").is_ok() {
        eprintln!("[DEBUG] Error response body: {}", text);
    }
    if let Ok(body) = serde_json::from_str::<Value>(&text) {
        let error_body = body.get("error").unwrap_or(&body);
        let code = error_body
            .get("reason")
            .or_else(|| body.get("reason"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                error_body
                    .get("code")
                    .or_else(|| body.get("code"))
                    .or_else(|| error_body.get("err_code"))
                    .or_else(|| body.get("err_code"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .or_else(|| {
                error_body
                    .get("code")
                    .or_else(|| body.get("code"))
                    .or_else(|| error_body.get("err_code"))
                    .or_else(|| body.get("err_code"))
                    .and_then(|v| v.as_i64())
                    .map(|n| n.to_string())
            })
            .unwrap_or_default();

        let msg = error_body
            .get("message")
            .or_else(|| error_body.get("msg"))
            .or_else(|| error_body.get("err_msg"))
            .and_then(|v| v.as_str())
            .unwrap_or(&text)
            .to_string();

        let fields = error_body
            .get("metadata")
            .and_then(|m| m.get("fields"))
            .or_else(|| error_body.get("fields"))
            .or_else(|| body.get("metadata").and_then(|m| m.get("fields")))
            .or_else(|| body.get("fields"))
            .cloned();
        let trace_id = if !header_trace_id.is_empty() {
            header_trace_id
        } else {
            error_body
                .get("metadata")
                .and_then(|m| m.get("trace_id"))
                .or_else(|| error_body.get("trace_id"))
                .or_else(|| body.get("metadata").and_then(|m| m.get("trace_id")))
                .or_else(|| body.get("trace_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let cause = error_body
            .get("cause")
            .or_else(|| error_body.get("metadata").and_then(|m| m.get("cause")))
            .or_else(|| body.get("cause"))
            .or_else(|| body.get("metadata").and_then(|m| m.get("cause")))
            .cloned();
        let suggestion = error_body
            .get("suggestion")
            .or_else(|| error_body.get("metadata").and_then(|m| m.get("suggestion")))
            .or_else(|| body.get("suggestion"))
            .or_else(|| body.get("metadata").and_then(|m| m.get("suggestion")))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        (code, msg, fields, trace_id, cause, suggestion)
    } else {
        let truncated: String = text.chars().take(200).collect();
        (String::new(), truncated, None, header_trace_id, None, None)
    }
}

fn fail_response(
    resp: Response,
    step: Option<&str>,
    explicit_timeout: bool,
    context: Option<&Value>,
) -> ! {
    let status = resp.status().as_u16();
    let (code, msg, fields, trace_id, cause, suggestion) =
        parse_error_body(resp, step, explicit_timeout, context);
    let fields = merge_error_fields(fields.as_ref(), context);
    fail_with_fields(
        "http_error",
        &msg,
        Some(status),
        if code.is_empty() {
            None
        } else {
            Some(code.as_str())
        },
        step,
        fields.as_ref(),
        if trace_id.is_empty() {
            None
        } else {
            Some(trace_id.as_str())
        },
        cause.as_ref(),
        suggestion.as_deref(),
    );
}

pub fn request(
    method: &str,
    url: &str,
    step: Option<&str>,
    retries: bool,
    body: Option<&Value>,
    params: Option<&HashMap<String, String>>,
) -> Response {
    request_inner(
        method,
        url,
        step,
        body,
        params,
        RequestOptions {
            retries,
            timeout: Duration::from_secs(30),
            explicit_timeout: false,
            context: None,
        },
    )
}

/// A single bounded request that retains resource identifiers in error.fields.
pub fn request_with_timeout_context(
    method: &str,
    url: &str,
    step: Option<&str>,
    body: Option<&Value>,
    params: Option<&HashMap<String, String>>,
    timeout: Duration,
    context: &Value,
) -> Response {
    request_inner(
        method,
        url,
        step,
        body,
        params,
        RequestOptions {
            retries: false,
            timeout,
            explicit_timeout: true,
            context: Some(context),
        },
    )
}

struct RequestOptions<'a> {
    retries: bool,
    timeout: Duration,
    explicit_timeout: bool,
    context: Option<&'a Value>,
}

// Decode URL components without interpreting them as request data. Invalid UTF-8 is
// kept in its original encoded form; both forms are checked before displaying causes.
fn decoded_url_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                decoded.push((high * 16 + low) as u8);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| value.to_owned())
}

fn network_url_secrets(url: &str) -> Vec<String> {
    let mut values = vec![url.to_owned()];
    let (path, query, fragment) = if let Ok(parsed) = reqwest::Url::parse(url) {
        values.push(parsed.to_string());
        values.push(parsed.username().to_owned());
        values.extend(parsed.password().map(str::to_owned));
        values.extend(parsed.query_pairs().map(|(_, value)| value.into_owned()));
        (
            parsed.path().to_owned(),
            parsed.query().map(str::to_owned),
            parsed.fragment().map(str::to_owned),
        )
    } else {
        // An invalid port can prevent URL parsing. Extract only private portions
        // using URL boundaries, leaving the scheme, host and port diagnostic intact.
        let remainder = if let Some((_, after_scheme)) = url.split_once("://") {
            let authority_end = after_scheme
                .find(['/', '?', '#'])
                .unwrap_or(after_scheme.len());
            let authority = &after_scheme[..authority_end];
            if let Some((userinfo, _)) = authority.rsplit_once('@') {
                if let Some((username, password)) = userinfo.split_once(':') {
                    values.push(username.to_owned());
                    values.push(password.to_owned());
                } else {
                    values.push(userinfo.to_owned());
                }
            }
            &after_scheme[authority_end..]
        } else {
            url
        };
        let (before_fragment, fragment) = remainder
            .split_once('#')
            .map_or((remainder, None), |(head, tail)| {
                (head, Some(tail.to_owned()))
            });
        let (path, query) = before_fragment
            .split_once('?')
            .map_or((before_fragment, None), |(head, tail)| {
                (head, Some(tail.to_owned()))
            });
        (path.to_owned(), query, fragment)
    };
    // Protect the complete path and its slash-free representation, rather than
    // treating each short segment as a secret that hides unrelated diagnostics.
    if path != "/" {
        values.push(path.clone());
        values.push(path.trim_matches('/').to_owned());
    }
    if let Some(query) = query {
        for pair in query.split('&') {
            if let Some((_, value)) = pair.split_once('=') {
                values.push(value.to_owned());
                values.push(decoded_url_component(&value.replace('+', " ")));
            }
        }
    }
    values.extend(fragment);
    let decoded: Vec<_> = values
        .iter()
        .map(|value| decoded_url_component(value))
        .collect();
    values.extend(decoded);
    values.retain(|value| !value.is_empty());
    values.sort();
    values.dedup();
    values
}

fn safe_network_error_chain(error: &dyn std::error::Error, secrets: &[String]) -> String {
    let mut messages = Vec::new();
    let mut current = Some(error);
    // Bound traversal in case an external error implementation has a cyclic source.
    for _ in 0..32 {
        let Some(cause) = current else { break };
        let message = cause.to_string();
        // Drop an unsafe cause as a whole. Substring replacement could leave a
        // differently formatted credential or signature fragment behind.
        if !message.is_empty()
            && !message.contains("://")
            && !secrets.iter().any(|secret| message.contains(secret))
            && !messages.contains(&message)
        {
            messages.push(message);
        }
        current = cause.source();
    }
    if messages.is_empty() {
        "network request failed".to_owned()
    } else {
        messages.join(": ")
    }
}

pub(crate) fn network_error_message(error: reqwest::Error, request_url: &str) -> String {
    let mut secrets = network_url_secrets(request_url);
    if let Some(url) = error.url() {
        secrets.extend(network_url_secrets(url.as_str()));
    }
    let error = error.without_url();
    safe_network_error_chain(&error, &secrets)
}

fn request_inner(
    method: &str,
    url: &str,
    step: Option<&str>,
    body: Option<&Value>,
    params: Option<&HashMap<String, String>>,
    options: RequestOptions<'_>,
) -> Response {
    let RequestOptions {
        retries,
        timeout,
        explicit_timeout,
        context,
    } = options;
    if explicit_timeout && timeout.is_zero() {
        fail_context("timeout", "Wait timeout exceeded", step, context);
    }
    if context.is_some() && token().is_empty() {
        fail_context("client_error", "VIDU_TOKEN not set", step, context);
    }
    let client = Client::new();
    let attempts = if retries { MAX_RETRIES } else { 1 };
    let headers_map = get_headers(None);
    let headers = build_reqwest_headers(&headers_map);
    let mut last_exc: Option<String> = None;
    let mut last_resp: Option<Response> = None;

    for (i, &delay) in RETRY_DELAYS.iter().enumerate().take(attempts) {
        let mut builder = match method {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            _ => client.get(url),
        };
        builder = builder.headers(headers.clone()).timeout(timeout);
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
                    thread::sleep(Duration::from_secs(delay));
                    continue;
                }
                if status >= 400 {
                    fail_response(resp, step, explicit_timeout, context);
                }
                return resp;
            }
            Err(e) => {
                if e.is_timeout() {
                    if explicit_timeout {
                        fail_context("timeout", "Wait request timed out", step, context);
                    }
                    last_exc = Some("timeout".into());
                } else {
                    last_exc = Some(network_error_message(e, url));
                }
                if i < attempts - 1 {
                    thread::sleep(Duration::from_secs(delay));
                }
            }
        }
    }

    if let Some(exc) = last_exc {
        fail_context("network_error", &exc, step, context);
    }
    // 5xx retries exhausted
    if let Some(resp) = last_resp {
        fail_response(resp, step, explicit_timeout, context);
    }
    fail_context("network_error", "unknown error", step, context);
}

pub fn request_json(
    method: &str,
    url: &str,
    step: Option<&str>,
    body: Option<&Value>,
    params: Option<&HashMap<String, String>>,
) -> Value {
    // A write may have reached the server even when its response is lost.
    // Retry only reads so task creation and material updates cannot be replayed.
    let resp = request(method, url, step, method == "GET", body, params);
    response_json(resp, step, false, None)
}

/// Bounded JSON request with explicit resource context for every failure path.
pub fn request_json_with_timeout_context(
    method: &str,
    url: &str,
    step: Option<&str>,
    body: Option<&Value>,
    params: Option<&HashMap<String, String>>,
    timeout: Duration,
    context: &Value,
) -> Value {
    response_json(
        request_with_timeout_context(method, url, step, body, params, timeout, context),
        step,
        true,
        Some(context),
    )
}

fn response_text(
    resp: Response,
    step: Option<&str>,
    explicit_timeout: bool,
    context: Option<&Value>,
) -> String {
    match resp.text() {
        Ok(text) => text,
        Err(error) if explicit_timeout => {
            fail_context(
                if error.is_timeout() {
                    "timeout"
                } else {
                    "network_error"
                },
                &error.to_string(),
                step,
                context,
            );
        }
        Err(_) => String::new(),
    }
}

fn response_json(
    resp: Response,
    step: Option<&str>,
    explicit_timeout: bool,
    context: Option<&Value>,
) -> Value {
    let trace_id = resp
        .headers()
        .get("x-md-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let text = response_text(resp, step, explicit_timeout, context);

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
            fail_context(
                "parse_error",
                &format!("Response is not valid JSON: {truncated}"),
                step,
                context,
            );
        }
    }
}

pub fn put_raw(
    url: &str,
    data: Vec<u8>,
    headers_map: &HashMap<String, String>,
    step: Option<&str>,
) -> (String,) {
    let client = Client::new();
    let headers = build_reqwest_headers(headers_map);
    let mut last_exc: Option<String> = None;

    for (i, &delay) in RETRY_DELAYS.iter().enumerate() {
        match client
            .put(url)
            .headers(headers.clone())
            .body(data.clone())
            .timeout(Duration::from_secs(60))
            .send()
        {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status >= 500 && i < MAX_RETRIES - 1 {
                    thread::sleep(Duration::from_secs(delay));
                    continue;
                }
                if status >= 400 {
                    let text: String = resp.text().unwrap_or_default().chars().take(200).collect();
                    fail(
                        "http_error",
                        &format!("PUT failed: {text}"),
                        Some(status),
                        None,
                        step,
                    );
                }
                let etag = resp
                    .headers()
                    .get("ETag")
                    .map(|v| v.to_str().unwrap_or("").to_string())
                    .unwrap_or_default();
                return (etag,);
            }
            Err(e) => {
                last_exc = Some(if e.is_timeout() {
                    "timeout".into()
                } else {
                    network_error_message(e, url)
                });
                if i < MAX_RETRIES - 1 {
                    thread::sleep(Duration::from_secs(delay));
                }
            }
        }
    }
    fail(
        "network_error",
        &last_exc.unwrap_or_default(),
        None,
        None,
        step,
    );
}

pub fn put_raw_large(
    url: &str,
    data: Vec<u8>,
    headers_map: &HashMap<String, String>,
    step: Option<&str>,
) -> (String,) {
    let client = Client::new();
    let headers = build_reqwest_headers(headers_map);
    let mut last_exc: Option<String> = None;

    for (i, &delay) in RETRY_DELAYS.iter().enumerate() {
        match client
            .put(url)
            .headers(headers.clone())
            .body(data.clone())
            .timeout(Duration::from_secs(600))
            .send()
        {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status >= 500 && i < MAX_RETRIES - 1 {
                    thread::sleep(Duration::from_secs(delay));
                    continue;
                }
                if status >= 400 {
                    let text: String = resp.text().unwrap_or_default().chars().take(200).collect();
                    fail(
                        "http_error",
                        &format!("PUT failed: {text}"),
                        Some(status),
                        None,
                        step,
                    );
                }
                let etag = resp
                    .headers()
                    .get("ETag")
                    .map(|v| v.to_str().unwrap_or("").to_string())
                    .unwrap_or_default();
                return (etag,);
            }
            Err(e) => {
                last_exc = Some(if e.is_timeout() {
                    "timeout".into()
                } else {
                    network_error_message(e, url)
                });
                if i < MAX_RETRIES - 1 {
                    thread::sleep(Duration::from_secs(delay));
                }
            }
        }
    }
    fail(
        "network_error",
        &last_exc.unwrap_or_default(),
        None,
        None,
        step,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_context_merges_without_losing_server_fields() {
        let fields = json!({"reason": "denied", "nested": {"code": 42}});
        let context = json!({"task_id": "task_123"});
        assert_eq!(
            merge_error_fields(Some(&fields), Some(&context)),
            Some(json!({
                "task_id": "task_123", "reason": "denied", "nested": {"code": 42}
            }))
        );
        assert_eq!(merge_error_fields(None, Some(&context)), Some(context));
    }

    #[test]
    fn error_context_preserves_scalar_server_fields_and_no_context_behavior() {
        let fields = json!("server message");
        let context = json!({"voice_id": "voice_123"});
        assert_eq!(
            merge_error_fields(Some(&fields), Some(&context)),
            Some(json!({
                "voice_id": "voice_123", "response_fields": "server message"
            }))
        );
        assert_eq!(merge_error_fields(Some(&fields), None), Some(fields));
        assert_eq!(merge_error_fields(None, None), None);
    }

    #[test]
    fn json_output_preserves_success_and_error_envelopes() {
        for value in [
            json!({"ok": true, "tasks": [], "nested": {"value": [1, "two"]}}),
            json!({"ok": false, "error": {"type": "http_error", "http_status": 403,
                "fields": {"name": "invalid"}, "message": "rejected"}}),
        ] {
            assert_eq!(render_output(&value, false), value.to_string());
            assert_eq!(
                serde_json::from_str::<Value>(&render_output(&value, false)).unwrap(),
                value
            );
        }
    }

    #[test]
    fn empty_list_shows_resource_and_pagination() {
        let output = render_output(
            &json!({"ok": true, "tasks": [], "next_page_token": "cursor_2"}),
            true,
        );
        assert!(output.contains("tasks (0 items)\n(empty)"));
        assert!(output.contains("next_page_token"));
        assert!(output.contains("cursor_2"));
        assert!(output.contains("true"));
    }

    #[test]
    fn nested_details_and_list_fields_are_not_lost() {
        let output = render_output(
            &json!({"ok": true, "task": {
                "settings": {"duration": 5, "input": {"prompt": "sunset"}},
                "creations": [{"creation_id": "c1", "metadata": {"width": 100}, "urls": ["https://example.test/1"]},
                    {"creation_id": "c2", "state": "ready"}]
            }}),
            true,
        );
        for expected in [
            "task.settings.duration",
            "task.settings.input.prompt",
            "sunset",
            "task.creations (2 items)",
            "creation_id",
            "c1",
            "c2",
            "state",
            "ready",
            "{\"width\":100}",
            "[\"https://example.test/1\"]",
        ] {
            assert!(output.contains(expected), "Missing {expected} in {output}");
        }
    }

    #[test]
    fn table_errors_include_all_diagnostics() {
        let output = render_output(
            &json!({"ok": false, "error": {
                "type": "http_error", "message": "denied", "http_status": 403,
                "code": "permission_denied", "step": "task.get", "trace_id": "trace_1",
                "fields": {"task_id": "bad"}, "cause": {"message": "owner mismatch"},
                "suggestion": "check account"
            }}),
            true,
        );
        for expected in [
            "error.type",
            "http_error",
            "denied",
            "403",
            "permission_denied",
            "task.get",
            "trace_1",
            "error.fields.task_id",
            "bad",
            "error.cause.message",
            "owner mismatch",
            "check account",
            "false",
        ] {
            assert!(output.contains(expected), "Missing {expected} in {output}");
        }
    }

    #[test]
    fn multiline_and_control_values_stay_on_one_table_row() {
        let output = render_output(
            &json!({"ok": true, "message": "a\nb\rc\td\u{1b}[31m\\tail"}),
            true,
        );
        assert_eq!(output.lines().count(), 4);
        assert!(output.contains("a\\nb\\rc\\td\\u{1b}[31m\\\\tail"));
        assert!(!output.contains('\u{1b}'));
    }

    #[test]
    fn primitive_and_empty_object_lists_are_rendered() {
        let output = render_output(
            &json!({"values": [1, true, null, "hi"], "objects": [{}, {}]}),
            true,
        );
        for expected in [
            "values (4 items)",
            "true",
            "null",
            "hi",
            "objects (2 items)",
            "{}",
        ] {
            assert!(output.contains(expected));
        }
    }

    #[derive(Debug)]
    struct TestNetworkCause {
        message: String,
        source: Option<Box<TestNetworkCause>>,
    }

    impl std::fmt::Display for TestNetworkCause {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(&self.message)
        }
    }

    impl std::error::Error for TestNetworkCause {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.source
                .as_deref()
                .map(|source| source as &dyn std::error::Error)
        }
    }

    fn network_cause_chain(messages: &[&str]) -> TestNetworkCause {
        TestNetworkCause {
            message: messages[0].to_owned(),
            source: (messages.len() > 1).then(|| Box::new(network_cause_chain(&messages[1..]))),
        }
    }

    #[test]
    fn network_diagnostics_hide_signed_url_and_repeated_sensitive_components() {
        let url = "https://private-user:p%40ssword@upload.example.test/private-object?signature=private-signature&token=private%20token";
        let embedded_url = format!("failed request for {url}");
        let chain = network_cause_chain(&[
            "error sending request",
            &embedded_url,
            "credential private-user rejected",
            "credential p@ssword rejected",
            "signature private-signature rejected",
            "token private token rejected",
            "path private-object unavailable",
            "tcp connect error",
            "connection refused",
            "connection refused",
        ]);
        let message = safe_network_error_chain(&chain, &network_url_secrets(url));
        assert_eq!(
            message,
            "error sending request: tcp connect error: connection refused"
        );
        for secret in [
            url,
            "private-user",
            "p%40ssword",
            "p@ssword",
            "private-object",
            "private-signature",
            "private%20token",
            "private token",
        ] {
            assert!(
                !message.contains(secret),
                "sensitive URL component survived"
            );
        }
    }

    #[test]
    fn network_diagnostics_keep_local_reqwest_parse_cause_without_network() {
        let url = "https://private-user:private-password@upload.example.test:invalid/private-object?signature=private-signature";
        let error = Client::new().put(url).build().unwrap_err();
        let message = network_error_message(error, url);
        assert!(
            message.contains("invalid port"),
            "missing parse diagnostic: {message}"
        );
        for secret in [
            url,
            "private-user",
            "private-password",
            "private-object",
            "private-signature",
        ] {
            assert!(
                !message.contains(secret),
                "sensitive URL component survived"
            );
        }
    }

    #[test]
    fn malformed_url_secrets_exclude_scheme_port_and_query_keys() {
        let secrets = network_url_secrets(
            "https://private-user:private-password@upload.example.test:invalid/private-object?invalid=private-signature",
        );
        for diagnostic in ["https", "invalid", "upload.example.test"] {
            assert!(!secrets.iter().any(|secret| secret == diagnostic));
        }
        for secret in [
            "private-user",
            "private-password",
            "private-object",
            "private-signature",
        ] {
            assert!(secrets.iter().any(|value| value == secret));
        }
    }

    #[test]
    fn network_diagnostics_skip_unknown_urls_and_fall_back_safely() {
        let chain = network_cause_chain(&[
            "request failed at https://other.example.test/secret?token=unknown",
            "TLS handshake failed",
            "invalid peer certificate",
        ]);
        assert_eq!(
            safe_network_error_chain(&chain, &[]),
            "TLS handshake failed: invalid peer certificate"
        );
        let unsafe_chain = network_cause_chain(&["https://other.example.test/secret"]);
        assert_eq!(
            safe_network_error_chain(&unsafe_chain, &[]),
            "network request failed"
        );
    }
}
