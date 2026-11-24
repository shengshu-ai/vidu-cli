//! Product contracts exercised through the real binary and a strict local HTTP server.
//! Each test owns its listener; no credentials, external service, or shared port is used.
use assert_cmd::Command;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Output;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

type Check = Box<dyn FnOnce(&Request) + Send>;
struct Step {
    method: &'static str,
    path: &'static str,
    query: Vec<(String, String)>,
    check: Check,
    status: u16,
    mime: &'static str,
    response: Vec<u8>,
}
impl Step {
    fn json(method: &'static str, path: &'static str, response: Value) -> Self {
        Self {
            method,
            path,
            query: vec![],
            check: Box::new(|r| assert!(r.body.is_empty(), "unexpected request body")),
            status: 200,
            mime: "application/json",
            response: response.to_string().into_bytes(),
        }
    }
    fn query(mut self, pairs: &[(&str, &str)]) -> Self {
        self.query = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self
    }
    fn body(mut self, value: Value) -> Self {
        self.check = Box::new(move |r| assert_eq!(r.json(), value));
        self
    }
    /// Assert the generation-specific HTTP binding for the same logical patch.
    /// V1 carries the request envelope; V2 carries element with mask in query.
    fn element_update_body(mut self, patch: Value) -> Self {
        if self.path.starts_with("/vidu/v2/material/elements/") {
            self.query = vec![("mask".into(), patch["mask"].as_str().unwrap().into())];
            self.body(patch["element"].clone())
        } else {
            self.body(patch)
        }
    }
    fn check(mut self, check: impl FnOnce(&Request) + Send + 'static) -> Self {
        self.check = Box::new(check);
        self
    }
    fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }
    fn raw(mut self, mime: &'static str, response: &[u8]) -> Self {
        self.mime = mime;
        self.response = response.to_vec();
        self
    }
}
struct Request {
    body: Vec<u8>,
    headers: BTreeMap<String, String>,
}
impl Request {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("request JSON")
    }
}
struct Mock {
    base: String,
    stop: mpsc::Sender<()>,
    join: thread::JoinHandle<()>,
}
impl Mock {
    fn new(build: impl FnOnce(&str) -> Vec<Step>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let steps = build(&base);
        let (stop, rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut steps = steps.into_iter();
            loop {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        // macOS inherits O_NONBLOCK from the listening socket.
                        // Read/write timeouts require a blocking accepted stream.
                        socket.set_nonblocking(false).unwrap();
                        socket
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        socket
                            .set_write_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        let (method, target, request) = read_request(&mut socket);
                        let step = steps
                            .next()
                            .unwrap_or_else(|| panic!("unexpected HTTP {method} {target}"));
                        assert_eq!(method, step.method);
                        let url =
                            reqwest::Url::parse(&format!("http://localhost{target}")).unwrap();
                        assert_eq!(url.path(), step.path);
                        let mut actual: Vec<_> = url
                            .query_pairs()
                            .map(|(k, v)| (k.into_owned(), v.into_owned()))
                            .collect();
                        let mut expected = step.query;
                        actual.sort();
                        expected.sort();
                        assert_eq!(actual, expected, "query for {target}");
                        if !step.path.starts_with("/blob") {
                            assert_eq!(
                                request.headers.get("authorization").map(String::as_str),
                                Some("Token e2e-test-token")
                            );
                        }
                        (step.check)(&request);
                        write!(socket,"HTTP/1.1 {} Test\r\nContent-Type: {}\r\nContent-Length: {}\r\nETag: test-etag\r\nConnection: close\r\n\r\n",step.status,step.mime,step.response.len()).unwrap();
                        socket.write_all(&step.response).unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if rx.try_recv().is_ok() {
                            assert!(steps.next().is_none(), "missing expected HTTP request");
                            break;
                        }
                        assert!(Instant::now() < deadline, "mock total deadline exceeded");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            }
        });
        Self { base, stop, join }
    }
    fn run(&self, args: &[&str]) -> Output {
        self.run_env(args, &[])
    }
    fn run_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        Command::cargo_bin("vidu-cli")
            .unwrap()
            .env("VIDU_BASE_URL", &self.base)
            .env("VIDU_TOKEN", "e2e-test-token")
            .env_remove("VIDU_DEBUG")
            .env_remove("VIDU_ELEMENT_PLATFORM_ASSETS")
            .env("VIDU_ELEMENT_PLATFORM_ASSETS", "true")
            .envs(env.iter().copied())
            .timeout(Duration::from_secs(12))
            .args(args)
            .output()
            .unwrap()
    }
    fn finish(self) {
        self.stop.send(()).ok();
        self.join.join().expect("mock HTTP contract failed");
    }
}
fn read_request(socket: &mut TcpStream) -> (String, String, Request) {
    let mut bytes = Vec::new();
    let mut b = [0; 4096];
    let end = loop {
        let n = socket.read(&mut b).expect("HTTP header read");
        assert!(n > 0, "closed before headers");
        bytes.extend_from_slice(&b[..n]);
        if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        assert!(bytes.len() < 65536);
    };
    let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
    let mut lines = head.lines();
    let first: Vec<_> = lines.next().unwrap().split_whitespace().collect();
    let headers: BTreeMap<_, _> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let size = headers
        .get("content-length")
        .map(|n| n.parse::<usize>().unwrap())
        .unwrap_or(0);
    assert!(size < 22 * 1024 * 1024);
    while bytes.len() < end + size {
        let n = socket.read(&mut b).unwrap();
        assert!(n > 0, "closed during body");
        bytes.extend_from_slice(&b[..n]);
    }
    (
        first[0].into(),
        first[1].into(),
        Request {
            body: bytes[end..end + size].to_vec(),
            headers,
        },
    )
}
fn envelope(output: &Output, success: bool) -> Value {
    assert_eq!(
        output.status.code(),
        Some(if success { 0 } else { 1 }),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value =
        serde_json::from_slice(&output.stdout).expect("stdout must contain exactly one JSON value");
    assert_eq!(value["ok"], success);
    value
}

#[test]
fn q4_fifteen_images_and_transition_match_submit_cost_and_rerun() {
    let empty_tool_path = tempfile::tempdir().unwrap();
    let mut prompts = vec![json!({"type":"text","content":"参考所有图片"})];
    prompts
        .extend((1..=15).map(|id| json!({"type":"image","content":format!("ssupload:?id={id}")})));
    let historical = json!({"type":"character2video","input":{"prompts":prompts},
        "settings":{"model_version":"3.4","duration":5,"resolution":"720p",
            "codec":"h265","sample_count":0,"schedule_mode":"normal","transition":"speed"}});
    for action in ["submit", "cost", "rerun"] {
        let server = Mock::new(|_| {
            let check_submission = || {
                let expected_prompts = prompts.clone();
                Step::json("POST", "/vidu/v1/tasks", json!({"id":"aligned-q4"})).check(
                    move |request| {
                        let body = request.json();
                        assert_eq!(body["input"]["prompts"], Value::Array(expected_prompts));
                        assert_eq!(body["settings"]["model_version"], "3.4");
                        assert_eq!(body["settings"]["sample_count"], 1);
                        assert_eq!(body["settings"]["codec"], "h265");
                        assert_eq!(body["settings"]["transition"], "speed");
                    },
                )
            };
            match action {
                "submit" => vec![check_submission()],
                "rerun" => vec![
                    get("/vidu/v1/tasks/alignment-source", historical.clone()),
                    check_submission(),
                ],
                _ => vec![
                    get("/vidu/v1/tasks/credits", json!({"cost_credits":30})).query(&[
                        ("type", "character2video"),
                        ("settings.model_version", "3.4"),
                        ("settings.duration", "5"),
                        ("settings.resolution", "720p"),
                        ("settings.sample_count", "1"),
                        ("settings.codec", "h265"),
                        ("settings.schedule_mode", "normal"),
                        ("settings.transition", "speed"),
                    ]),
                ],
            }
        });
        let mut args: Vec<String> = if action == "rerun" {
            vec!["task".into(), "rerun".into(), "alignment-source".into()]
        } else {
            [
                "task",
                action,
                "--type",
                "character2video",
                "--model-version",
                "3.4",
                "--duration",
                "5",
                "--resolution",
                "720p",
                "--codec",
                "h265",
                "--schedule-mode",
                "normal",
                "--transition",
                "speed",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        };
        if action == "submit" {
            args.extend(["--prompt".into(), "参考所有图片".into()]);
            for id in 1..=15 {
                args.extend(["--image".into(), format!("ssupload:?id={id}")]);
            }
        }
        let args: Vec<_> = args.iter().map(String::as_str).collect();
        envelope(
            &server.run_env(&args, &[("PATH", empty_tool_path.path().to_str().unwrap())]),
            true,
        );
        server.finish();
    }
}

#[test]
fn drama_and_wan_preserve_transition_for_backend_policy() {
    for model in ["viduq3.1-drama-std", "viduq2-drama-std"] {
        for action in ["submit", "cost", "rerun"] {
            let server = Mock::new(|_| {
                if action == "cost" {
                    vec![
                        get("/vidu/v1/tasks/credits", json!({"cost_credits":10})).query(&[
                            ("type", "character2video"),
                            ("settings.model_version", model),
                            ("settings.duration", "5"),
                            ("settings.resolution", "720p"),
                            ("settings.sample_count", "1"),
                            ("settings.codec", "h264"),
                            ("settings.aspect_ratio", "16:9"),
                            ("settings.schedule_mode", "normal"),
                            ("settings.transition", "speed"),
                        ]),
                    ]
                } else {
                    let mut steps = vec![];
                    if action == "rerun" {
                        steps.push(get(
                            "/vidu/v1/tasks/ignored-transition",
                            json!({
                                "type":"character2video", "input":{"prompts":[
                                    {"type":"text","content":"scene"},
                                    {"type":"image","content":"ssupload:?id=1"}]},
                                "settings":{"model_version":model,"duration":5,"resolution":"720p",
                                    "codec":"h264","transition":"speed","schedule_mode":"normal"}
                            }),
                        ));
                    }
                    steps.push(
                        Step::json("POST", "/vidu/v1/tasks", json!({"id":"new-task"})).check(
                            |request| assert_eq!(request.json()["settings"]["transition"], "speed"),
                        ),
                    );
                    steps
                }
            });
            let mut args = if action == "rerun" {
                vec!["task", "rerun", "ignored-transition"]
            } else {
                vec![
                    "task",
                    action,
                    "--type",
                    "character2video",
                    "--model-version",
                    model,
                    "--codec",
                    "h264",
                    "--transition",
                    "speed",
                ]
            };
            if action == "submit" {
                args.extend(["--prompt", "scene", "--image", "ssupload:?id=1"]);
            }
            envelope(&server.run(&args), true);
            server.finish();
        }
    }
}

#[test]
fn sd_auto_duration_and_upscale_ratios_match_all_generation_entrypoints() {
    for (resolution, ratio) in [("480p", "21:9"), ("2k", "adaptive"), ("4k", "16:9")] {
        for action in ["submit", "cost", "rerun"] {
            let server = Mock::new(|_| {
                if action == "cost" {
                    vec![
                        get("/vidu/v1/tasks/credits", json!({"cost_credits":10})).query(&[
                            ("type", "character2video"),
                            ("settings.model_version", "3.2_a"),
                            ("settings.duration", "-1"),
                            ("settings.resolution", resolution),
                            ("settings.sample_count", "1"),
                            ("settings.codec", "h264"),
                            ("settings.aspect_ratio", ratio),
                            ("settings.schedule_mode", "normal"),
                        ]),
                    ]
                } else {
                    let mut steps = vec![];
                    if action == "rerun" {
                        steps.push(get(
                            "/vidu/v1/tasks/sd-source",
                            json!({
                                "type":"character2video", "input":{"prompts":[
                                    {"type":"text","content":"scene"},
                                    {"type":"image","content":"ssupload:?id=1"}]},
                                "settings":{"model_version":"3.2_a","duration":-1,
                                    "resolution":resolution,"aspect_ratio":ratio,
                                    "codec":"h264","schedule_mode":"normal"}
                            }),
                        ));
                    }
                    let expected_resolution = resolution.to_owned();
                    let expected_ratio = ratio.to_owned();
                    steps.push(
                        Step::json("POST", "/vidu/v1/tasks", json!({"id":"sd-task"})).check(
                            move |request| {
                                let settings = &request.json()["settings"];
                                assert_eq!(settings["duration"], -1);
                                assert_eq!(settings["resolution"], expected_resolution);
                                assert_eq!(settings["aspect_ratio"], expected_ratio);
                            },
                        ),
                    );
                    steps
                }
            });
            let mut args = if action == "rerun" {
                vec!["task", "rerun", "sd-source"]
            } else {
                vec![
                    "task",
                    action,
                    "--type",
                    "character2video",
                    "--model-version",
                    "3.2_a",
                    "--duration",
                    "-1",
                    "--resolution",
                    resolution,
                    "--aspect-ratio",
                    ratio,
                    "--codec",
                    "h264",
                    "--schedule-mode",
                    "normal",
                ]
            };
            if action == "submit" {
                args.extend(["--prompt", "scene", "--image", "ssupload:?id=1"]);
            }
            envelope(&server.run(&args), true);
            server.finish();
        }
    }
}

#[test]
#[cfg(unix)]
fn reference_video_color_repair_failure_falls_back_to_original_upload() {
    use std::os::unix::fs::PermissionsExt;
    let tools = tempfile::tempdir().unwrap();
    for (name, script) in [
        (
            "ffprobe",
            "#!/bin/sh\nprintf '%s' '{\"streams\":[{\"codec_type\":\"video\"}]}'\n",
        ),
        ("ffmpeg", "#!/bin/sh\nexit 1\n"),
    ] {
        let path = tools.path().join(name);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/reference_864x486_5s.mp4"
    );
    let original = std::fs::read(fixture).unwrap();
    let server = Mock::new(|base| {
        vec![
            Step::json(
                "POST",
                "/tools/v1/files/uploads",
                json!({
                    "id":"123", "put_url":format!("{base}/blob/reference.mp4")
                }),
            )
            .body(json!({"scene":"vidu","metadata":{
                "image-width":"864","image-height":"486","duration":"5"
            }})),
            Step::json("PUT", "/blob/reference.mp4", json!({}))
                .check(move |request| assert_eq!(request.body, original)),
            Step::json("PUT", "/tools/v1/files/uploads/123/finish", json!({}))
                .body(json!({"etag":"test-etag","id":"123"})),
            Step::json("POST", "/vidu/v1/tasks", json!({"id":"reference-task"})).check(|request| {
                assert_eq!(
                    request.json()["input"]["prompts"][2]["content"],
                    "ssupload:?id=123"
                )
            }),
        ]
    });
    envelope(
        &server.run_env(
            &[
                "task",
                "submit",
                "--type",
                "character2video",
                "--model-version",
                "3.2_a",
                "--prompt",
                "scene",
                "--image",
                "ssupload:?id=1",
                "--video",
                fixture,
                "--duration",
                "5",
                "--resolution",
                "720p",
                "--codec",
                "h264",
                "--schedule-mode",
                "normal",
            ],
            &[("PATH", tools.path().to_str().unwrap())],
        ),
        true,
    );
    server.finish();
}

#[test]
fn invalid_generation_settings_match_submit_cost_and_rerun_without_uploads() {
    for (kind, model, duration, resolution, count, mode, error_field) in [
        ("img2video", "3.4", "2", "720p", "1", "normal", "duration"),
        ("img2video", "3.4", "3", "480p", "1", "normal", "resolution"),
        (
            "character2video",
            "viduq3.1-drama-std",
            "5",
            "720p",
            "5",
            "normal",
            "sample_count",
        ),
        (
            "character2video",
            "3.2_a",
            "5",
            "720p",
            "1",
            "nopeak",
            "nopeak",
        ),
        ("text2video", "3.4", "5", "720p", "1", "normal", "support"),
    ] {
        for action in ["submit", "cost", "rerun"] {
            let raw = json!({"type":kind,"input":{"prompts":[{"type":"text","content":"test"},
                {"type":"image","content":"ssupload:?id=1"}]},"settings":{
                "model_version":model,"duration":duration.parse::<i64>().unwrap(),
                "resolution":resolution,"sample_count":count.parse::<i64>().unwrap(),"schedule_mode":mode}});
            let server = Mock::new(|_| {
                if action == "rerun" {
                    vec![get("/vidu/v1/tasks/invalid-source", raw)]
                } else {
                    vec![]
                }
            });
            let mut args = if action == "rerun" {
                vec!["task", "rerun", "invalid-source"]
            } else {
                vec![
                    "task",
                    action,
                    "--type",
                    kind,
                    "--model-version",
                    model,
                    "--duration",
                    duration,
                    "--resolution",
                    resolution,
                    "--sample-count",
                    count,
                    "--schedule-mode",
                    mode,
                ]
            };
            if action == "submit" {
                args.extend(["--prompt", "test", "--image", "/unnecessary-upload.jpg"]);
            }
            let result = envelope(&server.run(&args), false);
            assert_eq!(result["error"]["type"], "client_error");
            assert!(
                result["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(error_field),
                "{result}"
            );
            server.finish();
        }
    }
}

#[test]
fn generation_prompt_length_is_decided_by_backend_for_submit_and_rerun() {
    for (model, kind, backend_limit) in [
        ("3.2", "text2video", 5_000),
        ("3.4", "character2video", 20_000),
        ("viduq3.1-drama-std", "text2video", 5_000),
        ("viduq2-drama-std", "text2video", 20_000),
    ] {
        for action in ["submit", "rerun"] {
            for accepted in [true, false] {
                let prompt = format!(
                    " \n{}\t ",
                    "猫".repeat(backend_limit + usize::from(!accepted))
                );
                let mut prompts = vec![json!({"type":"text","content":"old"})];
                if kind == "character2video" {
                    prompts.push(json!({"type":"image","content":"ssupload:?id=101"}));
                }
                let expected_prompt = prompt.clone();
                let server = Mock::new(|_| {
                    let mut steps = Vec::new();
                    if action == "rerun" {
                        steps.push(get(
                            "/vidu/v1/tasks/long-source",
                            json!({
                                "type":kind, "input":{"prompts":prompts},
                                "settings":{"model_version":model,"duration":5,
                                    "resolution":"1080p","sample_count":1,
                                    "codec":"h265","schedule_mode":"normal"}
                            }),
                        ));
                    }
                    let response = if accepted {
                        json!({"id":"long-prompt-task"})
                    } else {
                        json!({"reason":"PROMPT_TOO_LONG","message":"length is checked by backend"})
                    };
                    steps.push(
                        Step::json("POST", "/vidu/v1/tasks", response)
                            .status(if accepted { 200 } else { 400 })
                            .check(move |request| {
                                assert_eq!(
                                    request.json()["input"]["prompts"][0]["content"],
                                    expected_prompt
                                );
                            }),
                    );
                    steps
                });
                let mut args = if action == "rerun" {
                    vec!["task", "rerun", "long-source", "--prompt", &prompt]
                } else {
                    vec![
                        "task",
                        "submit",
                        "--type",
                        kind,
                        "--model-version",
                        model,
                        "--duration",
                        "5",
                        "--resolution",
                        "1080p",
                        "--schedule-mode",
                        "normal",
                        "--prompt",
                        &prompt,
                    ]
                };
                if action == "submit" && kind == "character2video" {
                    args.extend(["--image", "ssupload:?id=101"]);
                }
                let data = envelope(&server.run(&args), accepted);
                if accepted {
                    assert_eq!(data["task_id"], "long-prompt-task");
                } else {
                    assert_eq!(data["error"]["type"], "http_error");
                    assert_eq!(data["error"]["http_status"], 400);
                    assert_eq!(data["error"]["code"], "PROMPT_TOO_LONG");
                    assert_eq!(data["error"]["message"], "length is checked by backend");
                }
                server.finish();
            }
        }
    }
}

#[test]
fn invalid_prompt_and_excess_q4_images_fail_before_any_upload() {
    for (prompt, images, error_field) in [
        (" \t\n".to_owned(), 1, "text"),
        ("test".to_owned(), 16, "15"),
    ] {
        let server = Mock::new(|_| vec![]);
        let mut args: Vec<String> = [
            "task",
            "submit",
            "--type",
            "character2video",
            "--model-version",
            "3.4",
            "--duration",
            "5",
            "--resolution",
            "720p",
            "--schedule-mode",
            "normal",
            "--prompt",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        args.push(prompt);
        for _ in 0..images {
            args.extend(["--image".into(), "/unnecessary-upload.jpg".into()]);
        }
        let args: Vec<_> = args.iter().map(String::as_str).collect();
        let result = envelope(&server.run(&args), false);
        assert!(
            result["error"]["message"]
                .as_str()
                .unwrap()
                .contains(error_field),
            "{result}"
        );
        server.finish();
    }
}

#[test]
fn image25_additional_ratios_are_shared_by_submit_cost_and_rerun() {
    for ratio in ["3:2", "2:3", "21:9", "9:21"] {
        for action in ["submit", "cost", "rerun"] {
            let settings = json!({"model_version":"3.2_image_2_5_fast","duration":0,
                "resolution":"1080p","sample_count":1,"codec":"h264",
                "schedule_mode":"normal","aspect_ratio":ratio});
            let server = Mock::new(|_| {
                if action == "cost" {
                    vec![
                        get("/vidu/v1/tasks/credits", json!({"cost_credits":6})).query(&[
                            ("type", "text2image"),
                            ("settings.model_version", "3.2_image_2_5_fast"),
                            ("settings.duration", "0"),
                            ("settings.resolution", "1080p"),
                            ("settings.sample_count", "1"),
                            ("settings.codec", "h264"),
                            ("settings.schedule_mode", "normal"),
                            ("settings.aspect_ratio", ratio),
                        ]),
                    ]
                } else {
                    let mut steps = vec![];
                    if action == "rerun" {
                        steps.push(get("/vidu/v1/tasks/image-source", json!({"type":"text2image",
                            "settings":settings,"input":{"prompts":[{"type":"text","content":"cat"}]}})));
                    }
                    let ratio = ratio.to_owned();
                    steps.push(
                        Step::json("POST", "/vidu/v1/tasks", json!({"id":"image-aligned"})).check(
                            move |request| {
                                assert_eq!(request.json()["settings"]["aspect_ratio"], ratio)
                            },
                        ),
                    );
                    steps
                }
            });
            let args = if action == "rerun" {
                vec!["task", "rerun", "image-source"]
            } else {
                let mut args = vec![
                    "task",
                    action,
                    "--type",
                    "text2image",
                    "--model-version",
                    "3.2_image_2_5_fast",
                    "--resolution",
                    "1080p",
                    "--aspect-ratio",
                    ratio,
                    "--codec",
                    "h264",
                    "--schedule-mode",
                    "normal",
                ];
                if action == "submit" {
                    args.extend(["--prompt", "cat"]);
                }
                args
            };
            envelope(&server.run(&args), true);
            server.finish();
        }
    }
}

#[test]
fn image2_and_image25_accept_nineteen_direct_references_without_uploads() {
    for model in ["3.2_image_2", "3.2_image_2_5_fast", "3.2_image_2_5_pro"] {
        let server = Mock::new(|_| {
            vec![
                Step::json("POST", "/vidu/v1/tasks", json!({"id":"image-references"})).check(
                    |request| {
                        assert_eq!(
                            request.json()["input"]["prompts"].as_array().unwrap().len(),
                            20
                        )
                    },
                ),
            ]
        });
        let mut args: Vec<String> = [
            "task",
            "submit",
            "--type",
            "reference2image",
            "--model-version",
            model,
            "--prompt",
            "cat",
            "--resolution",
            "1080p",
            "--schedule-mode",
            "normal",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for id in 1..=19 {
            args.extend(["--image".into(), format!("ssupload:?id={id}")]);
        }
        let args: Vec<_> = args.iter().map(String::as_str).collect();
        envelope(&server.run(&args), true);
        server.finish();
    }
}

#[test]
fn damaged_second_image_is_detected_before_uploading_the_first() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.png");
    let second = directory.path().join("broken.png");
    let mut seed = 0x1234_5678u32;
    let image = image::RgbImage::from_fn(64, 64, |_, _| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let bytes = seed.to_le_bytes();
        image::Rgb([bytes[0], bytes[1], bytes[2]])
    });
    image.save(&first).unwrap();
    assert!(std::fs::metadata(&first).unwrap().len() > 512);
    std::fs::write(&second, b"not an image").unwrap();
    let server = Mock::new(|_| vec![]);
    let result = envelope(
        &server.run(&[
            "task",
            "submit",
            "--type",
            "character2video",
            "--model-version",
            "3.4",
            "--duration",
            "5",
            "--resolution",
            "720p",
            "--schedule-mode",
            "normal",
            "--prompt",
            "test",
            "--image",
            first.to_str().unwrap(),
            "--image",
            second.to_str().unwrap(),
        ]),
        false,
    );
    assert_eq!(result["error"]["type"], "client_error");
    assert!(result["error"]["message"]
        .as_str()
        .unwrap()
        .contains("image"));
    server.finish();
}

#[test]
fn output_audio_switch_reaches_submit_and_quote_without_changing_reference_inputs() {
    for (model, kinds) in [
        (
            "viduq2-drama-std",
            vec![
                "text2video",
                "img2video",
                "headtailimg2video",
                "character2video",
            ],
        ),
        ("viduq3.1-drama-std", vec!["text2video", "character2video"]),
        ("3.2_a", vec!["character2video"]),
    ] {
        for kind in kinds {
            for (option, with_audio) in [
                (None, None),
                (Some("--with-audio"), Some(true)),
                (Some("--with-audio=true"), Some(true)),
                (Some("--with-audio=false"), Some(false)),
            ] {
                let server = Mock::new(|_| {
                    let mut query = vec![
                        ("type", kind),
                        ("settings.model_version", model),
                        ("settings.duration", "5"),
                        ("settings.resolution", "720p"),
                        ("settings.sample_count", "1"),
                        ("settings.codec", "h264"),
                        ("settings.schedule_mode", "normal"),
                    ];
                    if model != "3.2_a" {
                        query.push(("settings.aspect_ratio", "16:9"));
                    }
                    if let Some(with_audio) = with_audio {
                        query.push((
                            "settings.with_audio",
                            if with_audio { "true" } else { "false" },
                        ));
                    }
                    vec![
                        Step::json("POST", "/vidu/v1/tasks", json!({"id":"audio-task"})).check(
                            move |request| {
                                let body = request.json();
                                for params in [&body["input"], &body["settings"]] {
                                    assert_eq!(
                                        params.get("with_audio").and_then(Value::as_bool),
                                        with_audio
                                    );
                                    if with_audio.is_none() {
                                        assert!(params.get("with_audio").is_none());
                                    }
                                }
                                let prompts = body["input"]["prompts"].as_array().unwrap();
                                assert_eq!(
                                    prompts.iter().filter(|p| p["type"] == "audio").count(),
                                    usize::from(kind == "character2video")
                                );
                                if kind == "character2video" {
                                    assert!(prompts.iter().any(|p| p["type"] == "audio"
                                        && p["content"] == "ssupload:?id=audio"));
                                }
                            },
                        ),
                        get(
                            "/vidu/v1/tasks/credits",
                            json!({"credits":12,"current_credits":100}),
                        )
                        .query(&query),
                    ]
                });
                let mut args = vec![
                    "task",
                    "submit",
                    "--type",
                    kind,
                    "--model-version",
                    model,
                    "--prompt",
                    "sound of waves",
                    "--duration",
                    "5",
                    "--resolution",
                    "720p",
                    "--schedule-mode",
                    "normal",
                    "--codec",
                    "h264",
                ];
                if let Some(option) = option {
                    args.push(option);
                }
                if kind != "text2video" {
                    args.extend(["--image", "ssupload:?id=first"]);
                }
                if kind == "headtailimg2video" {
                    args.extend(["--image", "ssupload:?id=last"]);
                }
                if kind == "character2video" {
                    args.extend(["--audio", "ssupload:?id=audio"]);
                }
                envelope(&server.run(&args), true);
                let mut cost_args = vec![
                    "task",
                    "cost",
                    "--type",
                    kind,
                    "--model-version",
                    model,
                    "--duration",
                    "5",
                    "--resolution",
                    "720p",
                    "--schedule-mode",
                    "normal",
                    "--codec",
                    "h264",
                ];
                if let Some(option) = option {
                    cost_args.push(option);
                }
                envelope(&server.run(&cost_args), true);
                server.finish();
            }
        }
    }
}

#[test]
fn rerun_audio_override_replaces_both_historical_fields_and_preserves_choice_when_omitted() {
    for model in ["viduq2-drama-std", "viduq3.1-drama-std", "3.2_a"] {
        for (option, with_audio) in [
            (None, None),
            (Some("--with-audio"), Some(true)),
            (Some("--with-audio=false"), Some(false)),
        ] {
            let raw = json!({"id":"old","state":"success","type":"character2video","input":{"with_audio":false,"lip_sync":true,"prompts":[{"type":"text","content":"test"},{"type":"image","content":"ssupload:?id=image"}]},"settings":{"with_audio":true,"lip_sync":true,"model_version":model,"duration":5,"resolution":"720p","sample_count":1,"schedule_mode":"normal","codec":"h264"}});
            let server = Mock::new(|_| {
                vec![
                    get("/vidu/v1/tasks/old", raw.clone()),
                    Step::json("POST", "/vidu/v1/tasks", json!({"id":"rerun-audio"})).check(
                        move |request| {
                            let body = request.json();
                            assert_eq!(body["input"]["prompts"], raw["input"]["prompts"]);
                            assert_eq!(body["input"]["with_audio"], with_audio.unwrap_or(false));
                            assert_eq!(body["settings"]["with_audio"], with_audio.unwrap_or(true));
                            for params in [&body["input"], &body["settings"]] {
                                assert_eq!(params["lip_sync"], with_audio != Some(false));
                            }
                        },
                    ),
                ]
            });
            let mut args = vec!["task", "rerun", "old"];
            if let Some(option) = option {
                args.push(option);
            }
            envelope(&server.run(&args), true);
            server.finish();
        }
    }
}

#[test]
fn output_audio_switch_rejects_image_tasks_before_upload_or_quote() {
    let server = Mock::new(|_| vec![]);
    for command in ["submit", "cost"] {
        for kind in ["text2image", "reference2image"] {
            for option in ["--with-audio", "--with-audio=false"] {
                let mut args = vec![
                    "task",
                    command,
                    "--type",
                    kind,
                    "--model-version",
                    "3.2_image_2",
                    "--resolution",
                    "1080p",
                    option,
                ];
                if command == "submit" {
                    args.extend(["--prompt", "test", "--image", "missing-image.png"]);
                }
                let result = envelope(&server.run(&args), false);
                assert_eq!(result["error"]["type"], "client_error");
                assert_eq!(
                    result["error"]["message"],
                    "--with-audio is only supported for video tasks"
                );
            }
        }
    }
    let output = server.run(&[
        "task",
        "cost",
        "--type",
        "text2video",
        "--model-version",
        "viduq2-drama-std",
        "--with-audio=invalid",
    ]);
    assert!(!output.status.success());
    server.finish();
}

#[test]
fn rerun_image_rejects_output_audio_before_creating_task() {
    let server = Mock::new(|_| {
        vec![get(
            "/vidu/v1/tasks/image-task",
            json!({"type":"text2image"}),
        )]
    });
    let result = envelope(
        &server.run(&["task", "rerun", "image-task", "--with-audio"]),
        false,
    );
    assert_eq!(
        result["error"]["message"],
        "--with-audio is only supported for video tasks"
    );
    server.finish();
}

#[test]
fn wan_submit_four_types_and_multimodal_references_use_main_site_task_api() {
    for (kind, media) in [
        ("text2video", vec![]),
        ("img2video", vec![("image", "first")]),
        (
            "headtailimg2video",
            vec![("image", "first"), ("image", "last")],
        ),
        (
            "character2video",
            vec![("image", "ref"), ("audio", "audio"), ("video", "video")],
        ),
        ("character2video", vec![("audio", "audio-only")]),
    ] {
        let mut prompts = vec![json!({"type":"text","content":"test wan"})];
        let mut args = vec![
            "task".to_string(),
            "submit".into(),
            "--type".into(),
            kind.into(),
            "--prompt".into(),
            "test wan".into(),
            "--model-version".into(),
            "viduq2-drama-std".into(),
            "--codec".into(),
            "h264".into(),
        ];
        for (kind, id) in media {
            let uri = format!("ssupload:?id={id}");
            args.extend([format!("--{kind}"), uri.clone()]);
            let mut prompt = json!({"type":kind,"content":uri});
            if kind != "image" {
                prompt["name"] = json!(uri);
            }
            prompts.push(prompt);
        }
        let body = json!({"type":kind,"input":{"prompts":prompts,"editor_mode":"normal","enhance":true},"settings":{"duration":5,"model_version":"viduq2-drama-std","sample_count":1,"codec":"h264","resolution":"720p","aspect_ratio":"16:9","schedule_mode":"normal"}});
        let server = Mock::new(|_| {
            vec![Step::json("POST", "/vidu/v1/tasks", json!({"id":"wan-task"})).body(body)]
        });
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        envelope(&server.run(&refs), true);
        server.finish();
    }
}

#[test]
fn local_reference_videos_send_dimensions_to_object_storage_before_submission() {
    // Native MP4 parsing must preserve dimensions without ffprobe/ffmpeg.
    let tool_path = tempfile::tempdir().unwrap();
    // Five seconds of H.264 video, 864x486, 30fps; valid for all three models.
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/reference_864x486_5s.mp4"
    );
    for (model, needs_image) in [
        ("viduq2-drama-std", false),
        ("viduq3.1-drama-std", false),
        ("3.2_a", true),
    ] {
        let server = Mock::new(|base| {
            vec![
                Step::json(
                    "POST",
                    "/tools/v1/files/uploads",
                    json!({
                        "id":"123", "put_url":format!("{base}/blob/reference.mp4"),
                        "headers":{
                            "x-amz-meta-image-width":"864",
                            "x-amz-meta-image-height":"486",
                            "x-amz-meta-duration":"5"
                        }
                    }),
                )
                .body(json!({"scene":"vidu","metadata":{
                    "image-width":"864", "image-height":"486", "duration":"5"
                }})),
                Step::json("PUT", "/blob/reference.mp4", json!({})).check(|request| {
                    // Color metadata may be injected without changing dimensions.
                    assert!(request.body.len() > 500);
                    assert!(request.body.windows(4).any(|bytes| bytes == b"ftyp"));
                    assert_eq!(request.headers["x-amz-meta-image-width"], "864");
                    assert_eq!(request.headers["x-amz-meta-image-height"], "486");
                    assert_eq!(request.headers["x-amz-meta-duration"], "5");
                    assert_eq!(request.headers["content-type"], "video/mp4");
                }),
                Step::json("PUT", "/tools/v1/files/uploads/123/finish", json!({}))
                    .body(json!({"etag":"test-etag","id":"123"})),
                Step::json("POST", "/vidu/v1/tasks", json!({"id":"reference-task"})).check(
                    move |request| {
                        let body = request.json();
                        assert_eq!(body["settings"]["model_version"], model);
                        let prompts = body["input"]["prompts"].as_array().unwrap();
                        let reference = prompts
                            .iter()
                            .find(|prompt| prompt["type"] == "video")
                            .unwrap();
                        assert_eq!(reference["content"], "ssupload:?id=123");
                    },
                ),
            ]
        });
        let mut args = vec![
            "task",
            "submit",
            "--type",
            "character2video",
            "--model-version",
            model,
            "--prompt",
            "edit video 1",
            "--video",
            fixture,
            "--duration",
            "5",
            "--resolution",
            "720p",
            "--aspect-ratio",
            "16:9",
            "--codec",
            "h264",
            "--schedule-mode",
            "normal",
        ];
        if needs_image {
            args.extend(["--image", "ssupload:?id=image"]);
        }
        envelope(
            &server.run_env(&args, &[("PATH", tool_path.path().to_str().unwrap())]),
            true,
        );
        server.finish();
    }
}

#[test]
fn wan_cost_defaults_and_resolution_quotes_are_returned_by_server() {
    for (resolution, rate) in [("480p", 6), ("720p", 10), ("1080p", 20)] {
        let server = Mock::new(|_| {
            vec![get(
                "/vidu/v1/tasks/credits",
                json!({"cost_credits":rate * 2,"can_submit":true}),
            )
            .query(&[
                ("type", "text2video"),
                ("settings.model_version", "viduq2-drama-std"),
                ("settings.duration", "2"),
                ("settings.resolution", resolution),
                ("settings.sample_count", "1"),
                ("settings.codec", "h264"),
                ("settings.schedule_mode", "normal"),
                ("settings.aspect_ratio", "adaptive"),
            ])]
        });
        let quote = envelope(
            &server.run(&[
                "task",
                "cost",
                "--type",
                "text2video",
                "--model-version",
                "viduq2-drama-std",
                "--duration",
                "2",
                "--resolution",
                resolution,
                "--aspect-ratio",
                "adaptive",
                "--codec",
                "h264",
            ]),
            true,
        );
        assert_eq!(quote["cost_credits"], rate * 2);
        assert_eq!(quote["can_submit"], true);
        server.finish();
    }
    let server = Mock::new(|_| {
        vec![
            get("/vidu/v1/tasks/credits", json!({"cost_credits":50})).query(&[
                ("type", "text2video"),
                ("settings.model_version", "viduq2-drama-std"),
                ("settings.duration", "5"),
                ("settings.resolution", "720p"),
                ("settings.sample_count", "1"),
                ("settings.codec", "h264"),
                ("settings.schedule_mode", "normal"),
                ("settings.aspect_ratio", "16:9"),
            ]),
        ]
    });
    envelope(
        &server.run(&[
            "task",
            "cost",
            "--type",
            "text2video",
            "--model-version",
            "viduq2-drama-std",
            "--codec",
            "h264",
        ]),
        true,
    );
    server.finish();
}

#[test]
fn wan_rerun_clears_legacy_transition_and_pass_mode() {
    let server = Mock::new(|_| {
        vec![
            get(
                "/vidu/v1/tasks/old",
                json!({"type":"img2video","input":{"enhance":true,"prompts":[{"type":"text","content":"test"},{"type":"image","content":"ssupload:?id=first"}]},"settings":{"model_version":"3.2","duration":5,"resolution":"1080p","transition":"speed","schedule_mode":"claw_pass"}}),
            ),
            Step::json("POST", "/vidu/v1/tasks", json!({"id":"wan-rerun"})).check(|request| {
                let body = request.json();
                assert_eq!(body["settings"]["model_version"], "viduq2-drama-std");
                assert_eq!(body["settings"]["schedule_mode"], "normal");
                assert!(body["settings"].get("transition").is_none());
                assert_eq!(body["input"]["prompts"][1]["content"], "ssupload:?id=first");
            }),
        ]
    });
    envelope(
        &server.run(&[
            "task",
            "rerun",
            "old",
            "--model-version",
            "viduq2-drama-std",
        ]),
        true,
    );
    server.finish();
}
fn get(path: &'static str, value: Value) -> Step {
    Step::json("GET", path, value)
}

fn upscale_body(input: Value, resolution: &str, duration: i32) -> Value {
    json!({"type":"upscale","input":input,"settings":{"resolution":resolution,"duration":duration,"sample_count":1,"schedule_mode":"normal"}})
}

fn upscale_quote(resolution: &str, duration: &str) -> Step {
    get("/vidu/v1/tasks/credits", json!({"cost_credits":48,"current_credits":100,"can_submit":true,"original_cost_credits":60}))
        .query(&[("type","upscale"),("settings.resolution",resolution),("settings.duration",duration),("settings.sample_count","1"),("settings.schedule_mode","normal")])
}

#[test]
fn upscale_creation_submit_and_cost_use_same_rounded_duration() {
    let detail = json!({"creation":{"type":"video","duration":5.5,"resolution":{"resolution":"720p"}},"task":{"state":"success"}});
    let server = Mock::new(|_| {
        vec![
            get("/vidu/v1/creations/77/detail", detail.clone()),
            upscale_quote("4k", "6"),
            get("/vidu/v1/creations/77/detail", detail),
            Step::json("POST", "/vidu/v1/tasks/tool", json!({"id":123})).body(upscale_body(
                json!({"creation_id":77}),
                "4k",
                6,
            )),
        ]
    });
    let quote = envelope(
        &server.run(&[
            "task",
            "upscale-cost",
            "--creation-id",
            "77",
            "--resolution",
            "4k",
        ]),
        true,
    );
    assert_eq!(quote["cost_credits"], 48);
    assert_eq!(quote["original_cost_credits"], 60);
    assert_eq!(quote["current_credits"], 100);
    assert_eq!(quote["can_submit"], true);
    assert_eq!(
        envelope(
            &server.run(&[
                "task",
                "upscale",
                "--creation-id",
                "77",
                "--resolution",
                "4k"
            ]),
            true
        )["task_id"],
        "123"
    );
    server.finish();
}

#[test]
fn upscale_remote_sources_preserve_uri_and_match_quote() {
    for (uri, resolution) in [
        ("https://example.com/video.mp4?sig=a%2Bb&token=x", "720p"),
        ("http://example.com/video", "1080p"),
        ("ssupload:?id=123", "2k"),
    ] {
        let server = Mock::new(|_| {
            vec![
                upscale_quote(resolution, "8"),
                Step::json(
                    "POST",
                    "/vidu/v1/tasks/tool",
                    json!({"task_id":"up_remote"}),
                )
                .body(upscale_body(
                    json!({"prompts":[{"type":"video","content":uri}]}),
                    resolution,
                    8,
                )),
            ]
        });
        envelope(
            &server.run(&[
                "task",
                "upscale-cost",
                "--video",
                uri,
                "--duration",
                "8",
                "--resolution",
                resolution,
            ]),
            true,
        );
        assert_eq!(
            envelope(
                &server.run(&[
                    "task",
                    "upscale",
                    "--video",
                    uri,
                    "--duration",
                    "8",
                    "--resolution",
                    resolution
                ]),
                true
            )["task_id"],
            "up_remote"
        );
        server.finish();
    }
}

#[test]
fn upscale_local_cost_does_not_upload_and_submit_sends_metadata() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/hevc_640x360_rotated.mp4"
    );
    let empty_path = tempfile::tempdir().unwrap();
    let server = Mock::new(|base| {
        vec![
            upscale_quote("2k", "1"),
            Step::json(
                "POST",
                "/tools/v1/files/uploads",
                json!({"id":"123","put_url":format!("{base}/blob/video")}),
            )
            .body(json!({"scene":"vidu","metadata":{"duration":"1"}})),
            Step::json("PUT", "/blob/video", json!({}))
                .check(|r| assert_eq!(r.body, include_bytes!("fixtures/hevc_640x360_rotated.mp4"))),
            Step::json("PUT", "/tools/v1/files/uploads/123/finish", json!({}))
                .body(json!({"etag":"test-etag","id":"123"})),
            Step::json("POST", "/vidu/v1/tasks/tool", json!({"id":"up_local"})).body(upscale_body(
                json!({"prompts":[{"type":"video","content":"ssupload:?id=123"}]}),
                "2k",
                1,
            )),
        ]
    });
    for action in ["upscale-cost", "upscale"] {
        envelope(
            &server.run_env(
                &["task", action, "--video", fixture, "--resolution", "2k"],
                &[("PATH", empty_path.path().to_str().unwrap())],
            ),
            true,
        );
    }
    server.finish();
}

#[cfg(unix)]
#[test]
fn upscale_ffprobe_fallback_and_missing_or_invalid_metadata() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("fallback.avi");
    std::fs::write(&video, b"fixture for mocked ffprobe").unwrap();
    let probe = dir.path().join("ffprobe");
    let cases = [
        (
            Some(r#"{"streams":[{"codec_type":"video"}],"format":{"duration":"7.5"}}"#),
            true,
        ),
        (
            Some(r#"{"streams":[{"codec_type":"audio"}],"format":{"duration":"8"}}"#),
            false,
        ),
        (
            Some(r#"{"streams":[{"codec_type":"video","duration":"NaN"}]}"#),
            false,
        ),
        (None, false),
    ];
    for (result, success) in cases {
        if let Some(result) = result {
            std::fs::write(&probe, format!("#!/bin/sh\nprintf '%s' '{result}'\n")).unwrap();
            std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).unwrap();
        } else {
            std::fs::remove_file(&probe).unwrap();
        }
        let server = Mock::new(|_| {
            if success {
                vec![upscale_quote("4k", "8")]
            } else {
                vec![]
            }
        });
        envelope(
            &server.run_env(
                &[
                    "task",
                    "upscale-cost",
                    "--video",
                    video.to_str().unwrap(),
                    "--resolution",
                    "4k",
                ],
                &[("PATH", dir.path().to_str().unwrap())],
            ),
            success,
        );
        server.finish();
    }
}

#[test]
fn upscale_invalid_arguments_and_files_never_reach_http() {
    let server = Mock::new(|_| vec![]);
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty.mp4");
    std::fs::write(&empty, []).unwrap();
    let oversized = dir.path().join("large.mp4");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(500 * 1024 * 1024 + 1)
        .unwrap();
    let invalid = dir.path().join("invalid.mp4");
    std::fs::write(&invalid, b"not a video").unwrap();
    for action in ["upscale", "upscale-cost"] {
        for tail in [
            vec![],
            vec!["--creation-id", "77", "--video", "ssupload:?id=123"],
            vec!["--creation-id", "0"],
            vec!["--creation-id", "77", "--duration", "8"],
            vec!["--video", "ssupload:?id=123"],
            vec!["--video", "https://example.com/v"],
            vec!["--video", "ssupload:?id=123", "--duration", "0"],
            vec!["--video", "ssupload:?id=123", "--duration", "2147483648"],
            vec!["--video", "ssupload:?id=123", "--duration", "1.5"],
            vec!["--video", "ssupload:?id=", "--duration", "8"],
            vec!["--video", "https:///v", "--duration", "8"],
            vec!["--video", "https://", "--duration", "8"],
            vec!["--video", "missing.mp4"],
            vec!["--video", ""],
            vec!["--video", empty.to_str().unwrap()],
            vec!["--video", oversized.to_str().unwrap()],
            vec!["--video", invalid.to_str().unwrap()],
            vec!["--video", invalid.to_str().unwrap(), "--duration", "8"],
        ] {
            let mut args = vec!["task", action, "--resolution", "2k"];
            args.extend(tail);
            assert!(
                !server
                    .run_env(&args, &[("PATH", dir.path().to_str().unwrap())])
                    .status
                    .success(),
                "{args:?}"
            );
        }
        for extra in [
            vec![],
            vec!["--resolution", "8k"],
            vec!["--resolution", "2k", "--output", "out"],
            vec!["--resolution", "2k", "--timeout", "1"],
        ] {
            let mut args = vec!["task", action, "--creation-id", "77"];
            args.extend(extra);
            assert!(!server.run(&args).status.success(), "{args:?}");
        }
    }
    assert!(!server
        .run(&[
            "task",
            "upscale-cost",
            "--creation-id",
            "77",
            "--resolution",
            "2k",
            "--wait"
        ])
        .status
        .success());
    server.finish();
}

#[test]
fn upscale_rejects_invalid_creations_and_permission_errors() {
    for detail in [
        json!({"creation":{"type":"image","duration":8},"task":{"state":"success"}}),
        json!({"creation":{"type":"video","duration":8},"task":{"state":"processing"}}),
        json!({"creation":{"type":"video","duration":8,"is_deleted":true},"task":{"state":"success"}}),
        json!({"creation":{"type":"video","duration":8,"err_code":"FAILED"},"task":{"state":"success"}}),
        json!({"creation":{"type":"video","duration":0},"task":{"state":"success"}}),
        json!({"creation":{"type":"video"},"task":{"state":"success"}}),
    ] {
        let server = Mock::new(|_| vec![get("/vidu/v1/creations/77/detail", detail)]);
        envelope(
            &server.run(&[
                "task",
                "upscale",
                "--creation-id",
                "77",
                "--resolution",
                "2k",
            ]),
            false,
        );
        server.finish();
    }
    let server = Mock::new(|_| {
        vec![get(
            "/vidu/v1/creations/77/detail",
            json!({"reason":"FORBIDDEN","message":"No access"}),
        )
        .status(403)]
    });
    envelope(
        &server.run(&[
            "task",
            "upscale-cost",
            "--creation-id",
            "77",
            "--resolution",
            "2k",
        ]),
        false,
    );
    server.finish();
}

#[test]
fn upscale_submit_failures_are_not_retried() {
    for status in [400, 500] {
        let server = Mock::new(|_| {
            vec![Step::json("POST","/vidu/v1/tasks/tool",json!({"reason":"REJECTED","message":"Insufficient credits or ambiguous server failure"})).status(status).body(upscale_body(json!({"prompts":[{"type":"video","content":"ssupload:?id=123"}]}),"2k",8))]
        });
        envelope(
            &server.run(&[
                "task",
                "upscale",
                "--video",
                "ssupload:?id=123",
                "--duration",
                "8",
                "--resolution",
                "2k",
            ]),
            false,
        );
        server.finish();
    }
}

#[test]
fn upscale_wait_download_and_terminal_errors() {
    for state in ["success", "failed", "canceled", "processing"] {
        let dir = tempfile::tempdir().unwrap();
        let server = Mock::new(|base| {
            let mut steps = vec![
                Step::json("POST", "/vidu/v1/tasks/tool", json!({"id":"up_wait"})).body(
                    upscale_body(
                        json!({"prompts":[{"type":"video","content":"ssupload:?id=123"}]}),
                        "2k",
                        8,
                    ),
                ),
                get(
                    "/vidu/v1/tasks/up_wait",
                    json!({"state":state,"type":"upscale","creations":[{"id":"77","uri":format!("{base}/blob/video.mp4")}]}),
                ),
            ];
            if state == "success" {
                steps.push(get("/blob/video.mp4", json!({})).raw("video/mp4", b"video result"));
            }
            steps
        });
        let data = envelope(
            &server.run(&[
                "task",
                "upscale",
                "--video",
                "ssupload:?id=123",
                "--duration",
                "8",
                "--resolution",
                "2k",
                "--wait",
                "--timeout",
                "1",
                "--output",
                dir.path().to_str().unwrap(),
            ]),
            state == "success",
        );
        if state == "success" {
            assert_eq!(data["task_id"], "up_wait");
            assert_eq!(
                std::fs::read(data["downloaded_files"][0].as_str().unwrap()).unwrap(),
                b"video result"
            );
        } else {
            assert_eq!(data["error"]["fields"]["task_id"], "up_wait");
            assert_eq!(data["error"]["fields"]["state"], state);
        }
        server.finish();
    }
}

#[test]
fn task_history_repeated_filters_and_compatible_details() {
    let raw = json!({"state":"success","type":"text2video","input":{"model_name":"Vidu","prompts":[{"type":"text","content":"sunset"}]},"settings":{"duration":5},"creations":[{"id":"77","uri":"media"},{"uri":"no-id"}]});
    let server = Mock::new(|_| {
        vec![
            get(
                "/vidu/v1/tasks/history/me",
                json!({"tasks":[],"next_page_token":"next"}),
            )
            .query(&[
                ("states", "success"),
                ("states", "failed"),
                ("types", "text2video"),
                ("types", "img2video"),
                ("scenes", "a & b"),
                ("scenes", "web"),
                ("pager.page_token", "c+1"),
                ("pager.pagesz", "20"),
            ]),
            get("/vidu/v1/tasks/task_1", raw.clone()),
            get("/vidu/v1/tasks/task_1", raw.clone()),
        ]
    });
    let history = envelope(
        &server.run(&[
            "task",
            "list",
            "--state",
            "success",
            "--state",
            "failed",
            "--type",
            "text2video",
            "--type",
            "img2video",
            "--scene",
            "a & b",
            "--scene",
            "web",
            "--page-token",
            "c+1",
        ]),
        true,
    );
    assert_eq!(history["next_page_token"], "next");
    assert_eq!(
        envelope(&server.run(&["task", "get", "task_1"]), true),
        json!({"ok":true,"task_id":"task_1","state":"success","type":"text2video","model":"Vidu"})
    );
    let detail = envelope(&server.run(&["task", "get", "task_1", "--details"]), true);
    assert_eq!(detail["input"], raw["input"]);
    assert_eq!(detail["settings"], raw["settings"]);
    assert_eq!(detail["creations"][0]["creation_id"], "77");
    assert!(detail["creations"][1].get("creation_id").is_none());
    server.finish();
}
#[test]
fn cancel_posts_and_processing_delete_keeps_server_error() {
    let server = Mock::new(|_| {
        vec![Step::json("POST","/vidu/v1/tasks/task_1/cancel",json!({})).body(json!({})),Step::json("DELETE","/vidu/v1/tasks/task_1",json!({"reason":"TASK_PROCESSING","message":"Cannot delete processing task","metadata":{"fields":{"state":"processing"},"trace_id":"trace-delete"}})).status(400)]
    });
    assert_eq!(
        envelope(&server.run(&["task", "cancel", "task_1"]), true)["task_id"],
        "task_1"
    );
    let error = envelope(&server.run(&["task", "delete", "task_1"]), false);
    assert_eq!(error["error"]["http_status"], 400);
    assert_eq!(error["error"]["code"], "TASK_PROCESSING");
    assert_eq!(error["error"]["fields"]["state"], "processing");
    assert_eq!(error["error"]["trace_id"], "trace-delete");
    server.finish();
}
fn personal(token: &str, response: Value) -> Step {
    get("/vidu/v1/material/elements/personal", response).query(&[
        ("pager.page_token", token),
        ("pager.pagesz", "20"),
        ("modalities", "image"),
        ("without_preset", "true"),
    ])
}
#[test]
fn personal_element_cursor_numeric_page_and_past_end() {
    let first =
        json!({"elements":[{"id":"11","version":"1","name":"first"}],"next_page_token":"second"});
    let second =
        json!({"elements":[{"id":"22","version":"1","name":"second"}],"next_page_token":""});
    let server = Mock::new(|_| {
        vec![
            personal("second", second.clone()),
            personal("", first.clone()),
            personal("second", second.clone()),
            personal("", first),
            personal("second", second),
        ]
    });
    for args in [
        vec!["element", "list", "--page-token", "second"],
        vec!["element", "list", "--page", "1"],
    ] {
        let data = envelope(&server.run(&args), true);
        assert_eq!(data["elements"][0]["id"], "22");
        assert_eq!(data["next_page_token"], "");
    }
    assert_eq!(
        envelope(&server.run(&["element", "list", "--page", "8"]), true)["elements"],
        json!([])
    );
    server.finish();
}
#[test]
fn element_update_labels_recaption_and_delete() {
    let server = Mock::new(|_| {
        vec![
            get("/vidu/v1/material/elements/detail/42", json!({"element":{"id":"42","edition":1,"name":"old name","modality":"image","recaption":{"style":"realistic","description":"old"}}})),
            Step::json("PUT","/vidu/v1/material/elements/42",json!({"version":"2"})).body(json!({"mask":"name,labels,recaption","element":{"id":"42","name":"new","labels":[{"id":"7"},{"id":"8"}],"modality":"image","recaption":{"style":"realistic","description":"fresh"}}})),
            get("/vidu/v1/material/elements/detail/42", json!({"element":{"id":"42","edition":1,"name":"new","modality":"image","recaption":{"style":"realistic","description":"fresh"}}})),
            Step::json("PUT","/vidu/v1/material/elements/42",json!({})).body(json!({"mask":"labels","element":{"id":"42","name":"new","modality":"image","recaption":{"style":"realistic","description":"fresh"},"labels":[]}})),
            Step::json("DELETE","/vidu/v1/material/elements/42",json!({})),
        ]
    });
    assert_eq!(
        envelope(
            &server.run(&[
                "element",
                "update",
                "42",
                "--name",
                "new",
                "--label",
                "7",
                "--label",
                "8",
                "--recaption",
                "fresh"
            ]),
            true
        )["version"],
        "2"
    );
    envelope(
        &server.run(&["element", "update", "42", "--clear-labels"]),
        true,
    );
    envelope(&server.run(&["element", "delete", "42"]), true);
    server.finish();
}

#[test]
fn element_create_uses_one_step_material_contract_and_preserves_image_order() {
    let server = Mock::new(|_| {
        vec![
        Step::json("POST", "/vidu/v1/material/elements/create", json!({"id":42,"version":"9007199254740993"}))
            .body(json!({"name":"角色", "modality":"image", "tag_ids":["9007199254740993"],
                "components":[
                    {"content":"ssupload:?id=11","src_img":"ssupload:?id=11","content_type":"image","type":"main"},
                    {"content":"ssupload:?id=22","src_img":"ssupload:?id=22","content_type":"image","type":"auxiliary"},
                    {"content":"ssupload:?id=33","src_img":"ssupload:?id=33","content_type":"image","type":"auxiliary"}],
                "recaption":{"description":"角色描述"}}))
    ]
    });
    let result = envelope(
        &server.run(&[
            "element",
            "create",
            "--name",
            "角色",
            "--tag",
            "9007199254740993",
            "--image",
            "ssupload:?id=11",
            "--image",
            "ssupload:?id=22",
            "--image",
            "ssupload:?id=33",
            "--description",
            "角色描述",
        ]),
        true,
    );
    assert_eq!(result["id"], "42");
    assert_eq!(result["version"], "9007199254740993");
    server.finish();
}

/// Exercise real decoding/upload bytes without external files or credentials.
fn element_image_fixture() -> (tempfile::TempDir, std::path::PathBuf, Vec<u8>) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("character.png");
    let mut seed = 0x1234_5678u32;
    image::RgbImage::from_fn(64, 64, |_, _| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let bytes = seed.to_le_bytes();
        image::Rgb([bytes[0], bytes[1], bytes[2]])
    })
    .save(&path)
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 512);
    (directory, path, bytes)
}

fn element_upload_steps(base: &str, id: &str, bytes: Vec<u8>) -> Vec<Step> {
    vec![
        Step::json("POST", "/tools/v1/assets/uploads", json!({"upload_token":id,"put_url":format!("{base}/blob/image"),"headers":{"x-upload-test":"required"}}))
            .body(json!({"file_name":"image.png","content_type":"image/png","scene":"material"})),
        Step::json("PUT", "/blob/image", json!({})).check(move |request| {
            assert_eq!(request.body, bytes);
            assert_eq!(request.headers["content-type"], "image/png");
            assert_eq!(request.headers["x-upload-test"], "required");
            assert!(!request.headers.contains_key("authorization"));
        }),
        Step::json("PUT", "/tools/v1/assets/uploads/complete", json!({"upload_uri":format!("ssupload:?id={id}")}))
            .body(json!({"upload_token":id,"etag":"test-etag"})),
    ]
}

#[test]
fn element_create_uploads_local_and_http_images_before_material_write() {
    let (_directory, path, bytes) = element_image_fixture();
    let server = Mock::new(|base| {
        let mut steps = element_upload_steps(base, "11", bytes.clone());
        steps.push(Step::json("GET", "/blob/source", json!({})).raw("image/png", &bytes));
        steps.extend(element_upload_steps(base, "22", bytes));
        steps.push(Step::json("POST", "/vidu/v1/material/elements/create", json!({"id":"42","version":"2"}))
            .body(json!({"name":"uploaded", "modality":"image", "tag_ids":["1"], "components":[
                {"content":"ssupload:?id=11","src_img":"ssupload:?id=11","content_type":"image","type":"main"},
                {"content":"ssupload:?id=22","src_img":"ssupload:?id=22","content_type":"image","type":"auxiliary"}]})));
        steps
    });
    let url = format!("{}/blob/source", server.base);
    envelope(
        &server.run(&[
            "element",
            "create",
            "--name",
            "uploaded",
            "--tag",
            "1",
            "--image",
            path.to_str().unwrap(),
            "--image",
            &url,
        ]),
        true,
    );
    server.finish();
}

#[test]
fn element_update_replaces_images_and_caption_without_changing_taxonomy() {
    let (_directory, path, bytes) = element_image_fixture();
    let server = Mock::new(|base| {
        let mut steps = vec![get(
            "/vidu/v1/material/elements/detail/42",
            json!({"element":{
            "id":"42","edition":2,"name":"old","modality":"image","recaption":{"description":"old description","style":"old style","extra":"keep"},
            "components":[{"content":"https://private.invalid/old-image"}],"voice":{"id":"voice-1"},"tag_ids":["99"]}}),
        )];
        steps.extend(element_upload_steps(base, "11", bytes));
        steps.push(Step::json("PUT", "/vidu/v2/material/elements/42", json!({"id":"42","version":"2"}))
            .element_update_body(json!({"mask":"name,recaption,components","element":{
                "id":"42","name":"new","modality":"image",
                "recaption":{"description":"fresh","style":"old style","extra":"keep"},
                "components":[{"content":"ssupload:?id=11","src_img":"ssupload:?id=11","content_type":"image","type":"main"}]}})));
        steps
    });
    let result = envelope(
        &server.run(&[
            "element",
            "update",
            "42",
            "--name",
            "new",
            "--recaption",
            "fresh",
            "--image",
            path.to_str().unwrap(),
        ]),
        true,
    );
    assert_eq!(result["version"], "2");
    server.finish();
}

#[test]
fn element_metadata_edits_work_with_asset_mode_without_media_registration() {
    for (args, expected) in [
        (
            vec!["--recaption", "updated"],
            json!({"mask":"recaption","element":{"id":"42","name":"old","modality":"image","recaption":{"style":"anime","description":"updated"}}}),
        ),
        (
            vec!["--name", "updated"],
            json!({"mask":"name","element":{"id":"42","name":"updated","modality":"image","recaption":{"style":"anime","description":"keep"}}}),
        ),
    ] {
        let server = Mock::new(|_| {
            vec![
                get(
                    "/vidu/v1/material/elements/detail/42",
                    json!({"element":{"id":"42","edition":2,"name":"old","modality":"image","recaption":{"style":"anime","description":"keep"},"components":[{"content":"untouched"}]}}),
                ),
                Step::json(
                    "PUT",
                    "/vidu/v2/material/elements/42",
                    json!({"version":"2"}),
                )
                .element_update_body(expected),
            ]
        });
        let mut command = vec!["element", "update", "42"];
        command.extend(args);
        envelope(
            &server.run_env(&command, &[("VIDU_ELEMENT_PLATFORM_ASSETS", "true")]),
            true,
        );
        server.finish();
    }
}

/// Enabling platform assets allows direct asset references without uploads.
#[test]
fn element_asset_ids_create_and_replace_without_uploads() {
    let server = Mock::new(|_| {
        vec![
        Step::json("POST", "/vidu/v1/material/elements/create", json!({"id":"42","version":"1"}))
            .body(json!({"name":"test","modality":"image","tag_ids":["1"],"components":[
                {"asset_id":"9007199254740993","type":"main","content_type":"image"},
                {"asset_id":"8","type":"auxiliary","content_type":"image"}]})),
        get("/vidu/v1/material/elements/detail/42", json!({"element":{"id":"42","edition":2,"name":"test","modality":"image","recaption":{"style":"realistic","description":"keep"}}})),
        Step::json("PUT", "/vidu/v2/material/elements/42", json!({"id":"42","version":"2","components":[{"asset_id":"9","content":"https://preview.invalid/image"}]}))
            .element_update_body(json!({"mask":"components","element":{"id":"42","name":"test","modality":"image","recaption":{"style":"realistic","description":"keep"},"components":[{"asset_id":"9","type":"main","content_type":"image"}]}})),
    ]
    });
    envelope(
        &server.run_env(
            &[
                "element",
                "create",
                "--name",
                "test",
                "--tag",
                "1",
                "--asset_id",
                "9007199254740993",
                "--asset-id",
                "8",
            ],
            &[("VIDU_ELEMENT_PLATFORM_ASSETS", "true")],
        ),
        true,
    );
    let result = envelope(
        &server.run(&["element", "update", "42", "--asset_id", "9"]),
        true,
    );
    assert_eq!(result["components"][0]["asset_id"], "9");
    server.finish();
}

#[test]
fn element_removed_switch_and_conflicting_inputs_fail_at_parse() {
    let server = Mock::new(|_| vec![]);
    for args in [
        vec![
            "element",
            "create",
            "--name",
            "test",
            "--tag",
            "1",
            "--asset-id",
            "7",
            "--platform-assets",
        ],
        vec![
            "element",
            "create",
            "--name",
            "test",
            "--tag",
            "1",
            "--asset-id",
            "7",
            "--image",
            "ssupload:?id=1",
        ],
        vec![
            "element",
            "update",
            "42",
            "--asset-id",
            "7",
            "--clear-labels",
        ],
    ] {
        assert_eq!(server.run(&args).status.code(), Some(2));
    }
    server.finish();
}

#[test]
fn element_invalid_media_metadata_and_modes_never_reach_http() {
    let server = Mock::new(|_| vec![]);
    let long_name = "名".repeat(31);
    for options in [
        vec![],
        vec!["--image", ""],
        vec!["--image", "ssupload:?id=1", "--tag", "0"],
        vec!["--image", "ssupload:?id=1", "--tag", "9223372036854775808"],
        vec!["--image", "ssupload:?id=1", "--description", ""],
        vec!["--image", "ssupload:?id=1", "--name", &long_name],
        vec!["--asset-id", "not-an-id"],
        vec![
            "--image", "1", "--image", "2", "--image", "3", "--image", "4",
        ],
    ] {
        // Keep one name/tag occurrence: clap rejects repeats before command code.
        let mut command = vec!["element", "create"];
        if !options.contains(&"--name") {
            command.extend(["--name", "test"]);
        }
        if !options.contains(&"--tag") {
            command.extend(["--tag", "1"]);
        }
        command.extend(options);
        let result = envelope(&server.run(&command), false);
        assert_eq!(result["error"]["type"], "client_error");
    }
    server.finish();
}

#[test]
fn element_media_replacement_rejects_other_modalities_before_upload() {
    for modality in ["video", "text"] {
        let server = Mock::new(|_| {
            vec![get(
                "/vidu/v1/material/elements/detail/42",
                json!({"element":{
            "id":"42","edition":2,"name":"old","modality":modality,"recaption":{"style":"","description":"keep"}}}),
            )]
        });
        let result = envelope(
            &server.run(&["element", "update", "42", "--image", "missing.png"]),
            false,
        );
        assert_eq!(result["error"]["type"], "client_error");
        server.finish();
    }
}

#[test]
fn element_library_errors_are_not_retried_or_replayed_to_legacy() {
    let server = Mock::new(|_| {
        vec![
            Step::json(
                "POST",
                "/vidu/v1/material/elements/create",
                json!({"code":"NotImplemented","message":"not deployed"}),
            )
            .status(501)
            .check(|request| assert!(request.json().get("components").is_some())),
            Step::json(
                "POST",
                "/vidu/v1/material/elements/create",
                json!({"version":"2"}),
            )
            .check(|request| assert!(request.json().get("components").is_some())),
        ]
    });
    let args = [
        "element",
        "create",
        "--name",
        "test",
        "--tag",
        "1",
        "--image",
        "ssupload:?id=11",
    ];
    envelope(&server.run(&args), false);
    let result = envelope(&server.run(&args), false);
    assert_eq!(result["error"]["type"], "parse_error");
    assert!(result["error"]["message"]
        .as_str()
        .unwrap()
        .contains("may have succeeded"));
    server.finish();
}

#[test]
fn element_tags_requests_first_level_from_server_and_projects_fields() {
    let response = json!({"tags":[
        {"id":1,"display_name_zh":"角色","display_name_en":"Character","rank":5,"extra":{"element":{"selectable":true}}},
        {"id":"3","parent_id":"0","display_name_zh":"场景","extra":{"element":{"selectable":false}}},
        {"id":"4","display_name_zh":"道具"}
    ]});
    let server = Mock::new(|_| {
        vec![get("/vidu/v1/material/elements/tags", response).query(&[("parent_id", "0")])]
    });
    assert_eq!(
        envelope(&server.run(&["element", "tags"]), true),
        json!({"ok":true,"tags":[
            {"id":"1","name":"角色","name_en":"Character"},
            {"id":"3","name":"场景","name_en":""},
            {"id":"4","name":"道具","name_en":""}
        ]})
    );
    server.finish();
}

#[test]
fn each_element_update_keeps_required_context_without_expanding_mask() {
    for (args, mask, name, description, labels) in [
        (
            vec!["--name", "renamed"],
            "name",
            "renamed",
            "original description",
            None,
        ),
        (
            vec!["--label", "7", "--label", "8"],
            "labels",
            "original name",
            "original description",
            Some(json!([{"id":"7"},{"id":"8"}])),
        ),
        (
            vec!["--clear-labels"],
            "labels",
            "original name",
            "original description",
            Some(json!([])),
        ),
        (
            vec!["--recaption", "new description"],
            "recaption",
            "original name",
            "new description",
            None,
        ),
    ] {
        let mut expected = json!({"mask":mask,"element":{"id":"42","name":name,"modality":"image","recaption":{"style":"realistic","description":description}}});
        let legacy_labels = labels.is_some();
        if let Some(labels) = labels {
            expected["element"]["labels"] = labels;
        }
        let server = Mock::new(|_| {
            vec![
                get(
                    "/vidu/v1/material/elements/detail/42",
                    json!({"element":{"id":"42","edition":if legacy_labels {1} else {2},"name":"original name","modality":"image","recaption":{"style":"realistic","description":"original description"}}}),
                ),
                Step::json(
                    "PUT",
                    if legacy_labels {
                        "/vidu/v1/material/elements/42"
                    } else {
                        "/vidu/v2/material/elements/42"
                    },
                    json!({"version":"2"}),
                )
                .element_update_body(expected),
            ]
        });
        let mut command = vec!["element", "update", "42"];
        command.extend(args);
        assert_eq!(envelope(&server.run(&command), true)["version"], "2");
        server.finish();
    }
}

#[test]
fn element_update_without_safe_recaption_context_never_puts() {
    for element in [
        json!({"id":"42","edition":2,"name":"original name","modality":"image"}),
        json!({"id":"42","edition":2,"name":"original name","modality":"image","recaption":null}),
        json!({"id":"42","edition":2,"name":"original name","modality":"image","recaption":"malformed"}),
    ] {
        let server = Mock::new(|_| {
            vec![get(
                "/vidu/v1/material/elements/detail/42",
                json!({"element":element}),
            )]
        });
        let data = envelope(
            &server.run(&["element", "update", "42", "--name", "new"]),
            false,
        );
        assert_eq!(data["error"]["type"], "api_error");
        assert!(data["error"]["message"]
            .as_str()
            .unwrap()
            .contains("recaption"));
        server.finish();
    }
}

#[test]
fn shared_search_favorites_and_collect_use_shared_id() {
    let shared = json!({"element":{"id":"42"},"share":{"id":"99"}});
    let server = Mock::new(|_| {
        vec![
            get(
                "/vidu/v1/material/share_elements/feed",
                json!({"share_elements":[],"next_page_token":"s2"}),
            )
            .query(&[
                ("keyword", "cat"),
                ("category", "character"),
                ("topic", "summer"),
                ("sort_by", "time"),
                ("modalities", "image"),
                ("is_like", "false"),
                ("is_collect", "false"),
                ("pager.pagesz", "20"),
                ("pager.page_token", ""),
            ]),
            get(
                "/vidu/v1/material/share_elements/feed",
                json!({"share_elements":[shared.clone()],"next_page_token":"f2"}),
            )
            .query(&[
                ("is_collect", "true"),
                ("pager.pagesz", "20"),
                ("pager.page_token", "f1"),
            ]),
            get("/vidu/v1/material/elements/detail/42", shared.clone()),
            Step::json("PUT", "/tools/v1/votes/share_element/99", json!({}))
                .body(json!({"obj_type":"share_element","obj_id":"99","vote":"collect"})),
            get("/vidu/v1/material/elements/detail/42", shared),
            Step::json("DELETE", "/tools/v1/votes/share_element/99", json!({}))
                .query(&[("vote", "collect")]),
        ]
    });
    envelope(
        &server.run(&[
            "element",
            "search",
            "--keyword",
            "cat",
            "--category",
            "character",
            "--topic",
            "summer",
            "--sort-by",
            "time",
        ]),
        true,
    );
    assert_eq!(
        envelope(
            &server.run(&["element", "favorites", "--page-token", "f1"]),
            true
        )["next_page_token"],
        "f2"
    );
    envelope(&server.run(&["element", "favorite", "42"]), true);
    envelope(&server.run(&["element", "unfavorite", "42"]), true);
    server.finish();
}
#[test]
fn ledger_and_creation_offset_paging_and_creation_favorites() {
    let server = Mock::new(|_| {
        vec![
            get(
                "/credit/v1/credits/records",
                json!({"total":1,"records":[{"amount":-12,"remark":"generate"}]}),
            )
            .query(&[("pager.page", "2"), ("pager.pagesz", "10")]),
            get(
                "/vidu/v1/my-creations",
                json!({"total":1,"creations":[{"id":"77","task_id":"task_1"}]}),
            )
            .query(&[
                ("pager.page", "0"),
                ("pager.pagesz", "20"),
                ("is_favor", "false"),
            ]),
            get("/vidu/v1/my-creations", json!({"total":0,"creations":[]})).query(&[
                ("pager.page", "3"),
                ("pager.pagesz", "10"),
                ("is_favor", "true"),
            ]),
            Step::json("PUT", "/tools/v1/favors/creation/77", json!({}))
                .body(json!({"obj_id":77,"obj_type":"creation"})),
            Step::json("DELETE", "/tools/v1/favors/creation/77", json!({})),
        ]
    });
    let ledger = envelope(
        &server.run(&["quota", "records", "--page", "2", "--pagesz", "10"]),
        true,
    );
    assert_eq!(ledger["records"][0]["amount"], -12);
    assert!(ledger.get("next_page_token").is_none());
    assert_eq!(
        envelope(&server.run(&["creation", "list"]), true)["creations"][0]["creation_id"],
        "77"
    );
    envelope(
        &server.run(&[
            "creation",
            "list",
            "--favorites-only",
            "--page",
            "3",
            "--pagesz",
            "10",
        ]),
        true,
    );
    envelope(&server.run(&["creation", "favorite", "77"]), true);
    envelope(&server.run(&["creation", "unfavorite", "77"]), true);
    server.finish();
}
#[test]
fn extend_preflights_creation_and_upscale_uses_tool_endpoint() {
    let server = Mock::new(|_| {
        vec![
        get("/vidu/v1/creations/77/detail",json!({"creation":{"id":"77","type":"video","duration":5.056,"resolution":{"resolution":"1080p"}},"task":{"state":"success"}})),
        Step::json("POST","/vidu/v1/tasks",json!({"id":"task_extend"})).body(json!({"type":"video2extend","input":{"prompts":[{"type":"video","content":"sscreation:?id=77","extend2video":{"from":0,"to":5}}],"editor_mode":"normal","enhance":true},"settings":{"duration":4,"model_version":"3.1","resolution":"1080p","sample_count":1,"transition":"extendspeed","schedule_mode":"normal"}})),
        Step::json("POST","/vidu/v1/tasks/tool",json!({"id":"task_upscale"})).body(json!({"type":"upscale_image","input":{"creation_id":77},"settings":{"resolution":"2k"}}))]
    });
    assert_eq!(
        envelope(
            &server.run(&["task", "extend", "--creation-id", "77", "--duration", "4"]),
            true
        )["task_id"],
        "task_extend"
    );
    assert_eq!(
        envelope(
            &server.run(&["task", "upscale-image", "--creation-id", "77"]),
            true
        )["task_id"],
        "task_upscale"
    );
    server.finish();
}
#[test]
fn rerun_six_supported_types_normalizes_legacy_codec_and_preserves_original_assets() {
    for kind in [
        "text2image",
        "text2video",
        "img2video",
        "headtailimg2video",
        "reference2image",
        "character2video",
    ] {
        let image = json!({"type":"image","content":"ssupload:?id=101"});
        let material =
            json!({"type":"material","name":"hero","material":{"id":"42","version":"3"}});
        let mut prompts = vec![json!({"type":"text","content":"old"})];
        match kind {
            "img2video" => prompts.push(image.clone()),
            "headtailimg2video" => {
                prompts.push(image.clone());
                prompts.push(json!({"type":"image","content":"ssupload:?id=102"}));
            }
            "reference2image" | "character2video" => {
                prompts.push(image);
                prompts.push(material);
            }
            _ => {}
        }
        let image_task = kind.ends_with("image");
        let mut settings = json!({"model_version":"3.1","resolution":"1080p","duration":if image_task {0}else{5},"sample_count":1,"schedule_mode":"normal","codec":"unspecified"});
        if ["img2video", "headtailimg2video"].contains(&kind) {
            settings["transition"] = json!("speed");
        }
        let original = prompts.clone();
        let mut expected_settings = settings.clone();
        expected_settings.as_object_mut().unwrap().remove("codec");
        let server = Mock::new(|_| {
            vec![
                get(
                    "/vidu/v1/tasks/task_old",
                    json!({"type":kind,"input":{"enhance":false,"editor_mode":"normal","prompts":prompts},"settings":settings}),
                ),
                Step::json("POST", "/vidu/v1/tasks", json!({"id":"task_new"})).check(move |r| {
                    let body = r.json();
                    assert_eq!(body["type"], kind);
                    assert_eq!(body["input"]["enhance"], false);
                    let mut expected = original;
                    expected[0]["content"] = json!("new prompt");
                    assert_eq!(body["input"]["prompts"], json!(expected));
                    assert_eq!(body["settings"], expected_settings);
                }),
            ]
        });
        assert_eq!(
            envelope(
                &server.run(&["task", "rerun", "task_old", "--prompt", "new prompt"]),
                true
            )["task_id"],
            "task_new"
        );
        server.finish();
    }
}
#[test]
fn rerun_rejects_tts_without_submitting() {
    let server = Mock::new(|_| {
        vec![get(
            "/vidu/v1/tasks/task_old",
            json!({"type":"tts","input":{"prompts":[{"type":"text","content":"old"}]}}),
        )]
    });
    let output = envelope(&server.run(&["task", "rerun", "task_old"]), false);
    assert!(output["error"]["message"]
        .as_str()
        .unwrap()
        .contains("does not support rerun"));
    server.finish();
}
#[test]
fn task_wait_polls_downloads_and_keeps_progress_on_stderr() {
    let png = b"\x89PNG\r\n\x1a\nfixture";
    let server = Mock::new(|base| {
        vec![
            get(
                "/vidu/v1/tasks/task_wait",
                json!({"state":"processing","type":"text2image"}),
            ),
            get(
                "/vidu/v1/tasks/task_wait",
                json!({"state":"success","type":"text2image","creations":[{"id":"77","uri":format!("{base}/blob/image")}]}),
            ),
            get("/blob/image", json!({})).raw("image/png", png),
        ]
    });
    let dir = tempfile::tempdir().unwrap();
    let output = server.run(&[
        "task",
        "wait",
        "task_wait",
        "--poll-interval",
        "1",
        "--timeout",
        "5",
        "--output",
        dir.path().to_str().unwrap(),
    ]);
    let data = envelope(&output, true);
    assert_eq!(data["state"], "success");
    let file = data["downloaded_files"][0].as_str().unwrap();
    assert_eq!(std::fs::read(file).unwrap(), png);
    assert!(std::path::Path::new(file).starts_with(dir.path()));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("processing"));
    assert!(stderr.contains("success"));
    server.finish();
}
#[test]
fn failed_canceled_and_timed_out_wait_preserve_task_id() {
    for state in ["failed", "canceled", "processing"] {
        let server = Mock::new(|_| {
            vec![get(
                "/vidu/v1/tasks/task_wait",
                json!({"state":state,"err_msg":"service result"}),
            )]
        });
        let start = Instant::now();
        let data = envelope(
            &server.run(&[
                "task",
                "wait",
                "task_wait",
                "--timeout",
                "1",
                "--poll-interval",
                "2",
            ]),
            false,
        );
        assert_eq!(data["error"]["fields"]["task_id"], "task_wait");
        assert_eq!(data["error"]["fields"]["state"], state);
        if state == "processing" {
            assert!(data["error"]["type"].as_str().unwrap().contains("timeout"));
            assert!(start.elapsed() >= Duration::from_millis(800));
            assert!(start.elapsed() < Duration::from_secs(4));
        }
        server.finish();
    }
}
#[test]
fn submit_wait_emits_exactly_one_json_envelope() {
    let server = Mock::new(|_| {
        vec![
            get("/credit/v1/claw-pass/status", json!({"has_pass": false})),
            Step::json("POST", "/vidu/v1/tasks", json!({"id":"task_new"})).check(|r| {
                let body = r.json();
                assert_eq!(body["type"], "text2video");
                assert_eq!(
                    body["input"]["prompts"],
                    json!([{"type":"text","content":"sunset"}])
                );
                assert_eq!(body["settings"]["model_version"], "3.1");
                assert_eq!(body["settings"]["duration"], 5);
                assert_eq!(body["settings"]["resolution"], "1080p");
                assert_eq!(body["settings"]["schedule_mode"], "normal");
            }),
            get(
                "/vidu/v1/tasks/task_new",
                json!({"state":"success","type":"text2video"}),
            ),
        ]
    });
    let output = server.run(&[
        "task",
        "submit",
        "--type",
        "text2video",
        "--prompt",
        "sunset",
        "--model",
        "3.1",
        "--duration",
        "5",
        "--resolution",
        "1080p",
        "--wait",
        "--timeout",
        "3",
    ]);
    assert_eq!(envelope(&output, true)["task_id"], "task_new");
    assert!(String::from_utf8_lossy(&output.stderr).contains("success"));
    server.finish();
}
#[test]
fn table_mode_and_default_json_have_same_list_content() {
    let server = Mock::new(|_| {
        vec![
            get(
                "/credit/v1/credits/records",
                json!({"total":1,"records":[{"amount":-12,"remark":"generate"}]}),
            )
            .query(&[("pager.page", "0"), ("pager.pagesz", "20")]),
            get(
                "/credit/v1/credits/records",
                json!({"total":1,"records":[{"amount":-12,"remark":"generate"}]}),
            )
            .query(&[("pager.page", "0"), ("pager.pagesz", "20")]),
        ]
    });
    assert_eq!(
        envelope(&server.run(&["quota", "records"]), true)["records"][0]["remark"],
        "generate"
    );
    let output = server.run(&["--format", "table", "quota", "records"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    for field in ["amount", "remark", "generate", "-12", "total"] {
        assert!(text.contains(field));
    }
    assert!(serde_json::from_slice::<Value>(&output.stdout).is_err());
    server.finish();
}
#[test]
fn invalid_arguments_never_reach_http() {
    let server = Mock::new(|_| vec![]);
    for args in [
        vec!["element", "update", "42"],
        vec!["element", "update", "42", "--label", "invalid-label"],
        vec!["element", "update", "42", "--recaption", ""],
        vec!["element", "list", "--page", "0", "--page-token", "x"],
        vec!["element", "update", "42", "--label", "7", "--clear-labels"],
        vec![
            "task",
            "rerun",
            "task_x",
            "--prompt",
            "x",
            "--prompt-file",
            "missing",
        ],
        vec!["task", "cancel", "../unsafe"],
        vec!["element", "delete", "not-an-id"],
        vec!["creation", "favorite", "0"],
        vec!["voice", "get", "official"],
        vec!["quota", "records", "--page", "-1"],
        vec!["element", "list", "--pagesz", "9"],
        vec!["task", "wait", "task_x", "--timeout", "0"],
        vec![
            "task",
            "upscale-image",
            "--creation-id",
            "77",
            "--output",
            "out",
        ],
    ] {
        let output = server.run(&args);
        assert!(!output.status.success(), "accepted invalid args: {args:?}");
    }
    server.finish();
}
fn wav_fixture() -> tempfile::NamedTempFile {
    let samples = 80000_u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&samples.to_le_bytes());
    bytes.resize(44 + samples as usize, 128);
    let file = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
    std::fs::write(file.path(), bytes).unwrap();
    file
}
fn voice_upload_steps(base: &str, finish_status: u16) -> Vec<Step> {
    vec![
        Step::json("POST","/tools/v1/files/uploads",json!({"id":"123","put_url":format!("{base}/blob/audio"),"headers":{"x-upload-test":"retained"}})).body(json!({"scene":"vidu","metadata":{"duration":"10"}})),
        Step::json("PUT","/blob/audio",json!({})).check(|r|{assert_eq!(r.body.len(),80044);assert_eq!(&r.body[..4],b"RIFF");assert_eq!(r.headers.get("x-upload-test").map(String::as_str),Some("retained"));}),
        Step::json("PUT","/tools/v1/files/uploads/123/finish",if finish_status==200 {json!({"uri":"ssupload:?id=123"})}else{json!({"reason":"AUDIT_REJECTED","message":"Recording rejected"})}).body(json!({"id":"123","etag":"test-etag"})).status(finish_status)]
}
fn voice_owner() -> Step {
    get(
        "/vidu/v1/voices/tasks",
        json!({"tasks": [{"voice_id": "voice_test", "status": "processing"}]}),
    )
    .query(&[("pager.page", "0"), ("pager.pagesz", "100")])
}
fn voice_stream(text: &[u8]) -> Step {
    get("/vidu/v1/voices/status", json!({}))
        .query(&[("voice_id", "voice_test")])
        .raw("text/event-stream", text)
}
#[test]
fn cloned_voice_creation_wait_list_update_and_personal_tts() {
    let server = Mock::new(|base| {
        let mut steps = voice_upload_steps(base, 200);
        steps.extend([
        Step::json("POST","/vidu/v1/voices",json!({"voice_id":"voice_test"})).body(json!({"name":"Narrator","source_uri":"ssupload:?id=123","use_trial":true})),
        voice_owner(),
        voice_stream(b": heartbeat\r\nevent: status\r\ndata: {\"voice_id\":\"voice_test\",\r\ndata: \"status\":\"processing\"}\r\n\r\ndata: {\"voice_id\":\"voice_test\",\"status\":\"success\"}\n\n"),
        get("/vidu/v1/voices",json!({"voices":[{"voice_id":"voice_test"}],"next_page_token":"next"})).query(&[("pager.page_token","prev"),("pager.pagesz","10")]),
        Step::json("PUT","/vidu/v1/voices/voice_test",json!({})).body(json!({"voice":{"voice_id":"voice_test","name":"Renamed","description":"Personal"},"mask":"name,description"})),
        get("/credit/v1/claw-pass/status", json!({"has_pass": false})),
        Step::json("POST","/vidu/v1/tasks",json!({"id":"task_tts"})).check(|r|{let b=r.json();assert_eq!(b["type"],"tts");assert_eq!(b["input"]["prompts"],json!([{"type":"text","content":"hello"}]));assert_eq!(b["settings"]["voice_id"],"voice_test");assert_eq!(b["settings"]["schedule_mode"],"normal");})]);
        steps
    });
    let audio = wav_fixture();
    let output = server.run(&[
        "voice",
        "create",
        "--audio",
        audio.path().to_str().unwrap(),
        "--name",
        "Narrator",
        "--use-trial",
        "--wait",
        "--timeout",
        "3",
    ]);
    let voice = envelope(&output, true);
    assert_eq!(voice["voice_id"], "voice_test");
    assert_eq!(voice["state"], "success");
    assert!(String::from_utf8_lossy(&output.stderr).contains("processing"));
    assert_eq!(
        envelope(
            &server.run(&["voice", "list", "--page-token", "prev", "--pagesz", "10"]),
            true
        )["next_page_token"],
        "next"
    );
    envelope(
        &server.run(&[
            "voice",
            "update",
            "voice_test",
            "--name",
            "Renamed",
            "--description",
            "Personal",
        ]),
        true,
    );
    assert_eq!(
        envelope(
            &server.run(&[
                "task",
                "tts",
                "--prompt",
                "hello",
                "--voice-id",
                "voice_test"
            ]),
            true
        )["task_id"],
        "task_tts"
    );
    server.finish();
}
#[test]
fn voice_upload_rejection_never_creates_clone() {
    let server = Mock::new(|base| voice_upload_steps(base, 400));
    let audio = wav_fixture();
    let output = envelope(
        &server.run(&["voice", "create", "--audio", audio.path().to_str().unwrap()]),
        false,
    );
    assert_eq!(output["error"]["code"], "AUDIT_REJECTED");
    server.finish();
}
#[test]
fn voice_get_snapshot_failure_and_closed_stream_timeout() {
    let server = Mock::new(|_| {
        vec![
        voice_owner(),
        voice_stream(b": ping\n\ndata: {\"voice_id\":\"voice_test\",\"status\":2}\n\n"),
        voice_owner(),
        voice_stream(b"data: {\"voice_id\":\"voice_test\",\n data:ignored\ndata: \"status\":\"failed\",\"err_code\":\"CLONE_REJECTED\",\"err_msg\":\"rejected\"}\n\n"),
        voice_owner(),
        voice_stream(b"data: {\"voice_id\":\"voice_test\",\"status\":\"processing\"}\n\n")]
    });
    assert_eq!(
        envelope(&server.run(&["voice", "get", "voice_test"]), true)["state"],
        "processing"
    );
    let failed = envelope(
        &server.run(&["voice", "wait", "voice_test", "--timeout", "2"]),
        false,
    );
    assert_eq!(failed["error"]["fields"]["voice_id"], "voice_test");
    assert_eq!(failed["error"]["fields"]["state"], "failed");
    let start = Instant::now();
    let timeout = envelope(
        &server.run(&["voice", "wait", "voice_test", "--timeout", "1"]),
        false,
    );
    assert!(start.elapsed() < Duration::from_secs(4));
    assert_eq!(timeout["error"]["fields"]["voice_id"], "voice_test");
    assert!(timeout["error"]["type"]
        .as_str()
        .unwrap()
        .contains("timeout"));
    server.finish();
}

#[test]
fn another_users_voice_does_not_subscribe_to_unscoped_status_stream() {
    for action in ["get", "wait"] {
        let server = Mock::new(|_| {
            vec![get("/vidu/v1/voices/tasks", json!({"tasks": []}))
                .query(&[("pager.page", "0"), ("pager.pagesz", "100")])]
        });
        let data = envelope(&server.run(&["voice", action, "voice_test"]), false);
        assert!(data["error"]["message"]
            .as_str()
            .is_some_and(|s| !s.is_empty()));
        server.finish();
    }
}

#[test]
fn submission_server_error_does_not_retry_a_potentially_paid_task() {
    let server = Mock::new(|_| {
        vec![Step::json("POST","/vidu/v1/tasks/tool",json!({"reason":"INTERNAL_ERROR","message":"Task may already have been submitted"})).body(json!({"type":"upscale_image","input":{"creation_id":77},"settings":{"resolution":"2k"}})).status(500)]
    });
    let data = envelope(
        &server.run(&["task", "upscale-image", "--creation-id", "77"]),
        false,
    );
    assert_eq!(data["error"]["http_status"], 500);
    assert_eq!(data["error"]["code"], "INTERNAL_ERROR");
    server.finish();
}

#[test]
fn rerun_applies_model_duration_resolution_without_changing_asset_identity() {
    let server = Mock::new(|_| {
        vec![
        get("/vidu/v1/tasks/task_old",json!({"type":"img2video","input":{"enhance":true,"editor_mode":"normal","prompts":[{"type":"text","content":"original"},{"type":"image","content":"ssupload:?id=123"}]},"settings":{"model_version":"3.1","duration":5,"resolution":"1080p","transition":"speed","sample_count":1,"schedule_mode":"normal"}})),
        Step::json("POST","/vidu/v1/tasks",json!({"id":"task_override"})).check(|r|{let b=r.json();assert_eq!(b["input"]["prompts"],json!([{"type":"text","content":"original"},{"type":"image","content":"ssupload:?id=123"}]));assert_eq!(b["settings"]["model_version"],"3.2_a");assert_eq!(b["settings"]["duration"],6);assert_eq!(b["settings"]["resolution"],"720p");})]
    });
    assert_eq!(
        envelope(
            &server.run(&[
                "task",
                "rerun",
                "task_old",
                "--model",
                "3.2_a",
                "--duration",
                "6",
                "--resolution",
                "720p"
            ]),
            true
        )["task_id"],
        "task_override"
    );
    server.finish();
}

#[test]
fn extend_rejects_ineligible_source_without_creating_task() {
    let server = Mock::new(|_| {
        vec![get(
            "/vidu/v1/creations/77/detail",
            json!({"creation":{"id":"77","type":"video","duration":5.056,"resolution":{"resolution":"720p"}},"task":{"state":"success"}}),
        )]
    });
    let data = envelope(
        &server.run(&["task", "extend", "--creation-id", "77", "--duration", "4"]),
        false,
    );
    assert!(data["error"]["message"].as_str().unwrap().contains("1080p"));
    server.finish();
}

#[test]
fn download_errors_keep_causes_and_status_without_exposing_signed_urls() {
    let invalid_url = "http://download_user:download_password@127.0.0.1:invalid-port/private-render-path?signature=private-signature-value";
    let server = Mock::new(|_| {
        vec![get(
            "/vidu/v1/tasks/task_download",
            json!({"state":"success","type":"text2video","creations":[{"id":"77","uri":invalid_url}]}),
        )]
    });
    let dir = tempfile::tempdir().unwrap();
    let output = server.run(&[
        "task",
        "get",
        "task_download",
        "--output",
        dir.path().to_str().unwrap(),
    ]);
    let data = envelope(&output, false);
    assert_eq!(data["error"]["type"], "network_error");
    let message = data["error"]["message"].as_str().unwrap();
    assert!(message.contains("invalid port"), "missing cause: {message}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for secret in [
        "download_user",
        "download_password",
        "private-render-path",
        "signature",
        "private-signature-value",
        invalid_url,
    ] {
        assert!(
            !stdout.contains(secret),
            "download diagnostic leaked {secret}"
        );
    }
    server.finish();

    let server = Mock::new(|base| {
        vec![
            get(
                "/vidu/v1/tasks/task_download",
                json!({"state":"success","type":"text2video","creations":[{"id":"77","uri":format!("{base}/blob/private-render-path?signature=private-signature-value")}]}),
            ),
            get("/blob/private-render-path", json!({"message":"denied"}))
                .query(&[("signature", "private-signature-value")])
                .status(403),
        ]
    });
    let output = server.run(&[
        "task",
        "get",
        "task_download",
        "--output",
        dir.path().to_str().unwrap(),
    ]);
    let data = envelope(&output, false);
    assert_eq!(data["error"]["type"], "http_error");
    assert_eq!(data["error"]["http_status"], 403);
    assert!(data["error"]["message"].as_str().unwrap().contains("403"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for secret in [
        server.base.as_str(),
        "private-render-path",
        "signature",
        "private-signature-value",
    ] {
        assert!(!stdout.contains(secret), "HTTP diagnostic leaked {secret}");
    }
    server.finish();
}

#[test]
fn extend_rejects_unknown_or_zero_length_source_without_submitting() {
    // Source metadata must identify an actual positive-length clip. In
    // particular, a small positive float must not become a zero-second trim.
    for duration in [
        None,
        Some(Value::Null),
        Some(json!("5.056")),
        Some(json!(0)),
        Some(json!(-2)),
        Some(json!(0.4)),
    ] {
        let mut creation = json!({"id":"77","type":"video","resolution":{"resolution":"1080p"}});
        if let Some(duration) = duration {
            creation["duration"] = duration;
        }
        let server = Mock::new(|_| {
            vec![get(
                "/vidu/v1/creations/77/detail",
                json!({"creation":creation,"task":{"state":"success","settings":{"duration":5}}}),
            )]
        });
        let data = envelope(
            &server.run(&["task", "extend", "--creation-id", "77", "--duration", "4"]),
            false,
        );
        assert_eq!(data["error"]["type"], "parse_error");
        assert_eq!(data["error"]["step"], "task_extend_source");
        assert!(data["error"]["message"]
            .as_str()
            .unwrap()
            .contains("duration"));
        server.finish();
    }
}

#[test]
fn element_single_tag_and_service_owned_style_reject_invalid_cli_inputs() {
    let server = Mock::new(|_| vec![]);
    for action in ["create", "update"] {
        for extra in [
            vec!["--tag", "1", "--tag", "2"],
            vec!["--tag", "1", "--style", "realistic"],
        ] {
            let mut args = vec!["element", action];
            if action == "create" {
                args.extend(["--name", "test", "--image", "missing.png"]);
            } else {
                args.push("42");
            }
            let extra = if action == "update" && extra.contains(&"--style") {
                vec!["--style", "realistic"]
            } else {
                extra
            };
            args.extend(extra);
            let output = server.run(&args);
            assert_eq!(output.status.code(), Some(2));
            assert!(String::from_utf8_lossy(&output.stderr).contains(
                if args.contains(&"--style") {
                    "--style"
                } else {
                    "--tag"
                }
            ));
        }
    }
    server.finish();
}

/// The old protocol keeps its original schema and rejects the new category flag.
#[test]
fn legacy_element_update_rejects_taxonomy_change_before_http() {
    let server = Mock::new(|_| vec![]);
    let output = server.run_env(
        &["element", "update", "42", "--name", "new", "--tag", "1"],
        &[("VIDU_ELEMENT_PLATFORM_ASSETS", "false")],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--tag"));
    server.finish();
}

/// Use the proto spelling tag_ids in the V2 query FieldMask; the element field
/// stays tag_ids. Standalone and combined edits use a single V2 write.
#[test]
fn element_update_category_uses_camel_case_mask_and_single_id() {
    for combined in [false, true] {
        let server = Mock::new(|_| {
            vec![
            get("/vidu/v1/material/elements/detail/42", json!({"element":{"id":"42","name":"original","modality":"image","edition":2,"tag_ids":["1"],"recaption":{"description":"keep","style":"realistic"}}})),
            Step::json("PUT","/vidu/v2/material/elements/42",json!({"id":"42","edition":2,"tag_ids":["9007199254740993"]})).element_update_body(if combined {
                json!({"mask":"name,recaption,tag_ids,components","element":{"id":"42","name":"renamed","modality":"image","recaption":{"description":"updated","style":"realistic"},"tag_ids":["9007199254740993"],"components":[{"asset_id":"9","content_type":"image","type":"main"}]}})
            } else {
                json!({"mask":"tag_ids","element":{"id":"42","name":"original","modality":"image","recaption":{"description":"keep","style":"realistic"},"tag_ids":["9007199254740993"]}})
            }),
        ]
        });
        let mut args = vec!["element", "update", "42", "--tag", "9007199254740993"];
        if combined {
            args.extend([
                "--name",
                "renamed",
                "--recaption",
                "updated",
                "--asset-id",
                "9",
            ]);
        }
        let result = envelope(&server.run(&args), true);
        assert_eq!(result["tag_ids"], json!(["9007199254740993"]));
        server.finish();
    }
}

#[test]
fn element_update_category_invalid_and_conflicting_args_never_reach_http() {
    for options in [
        vec!["--tag", "0"],
        vec!["--tag", "bad"],
        vec!["--tag", "9223372036854775808"],
        vec!["--tag", "1", "--tag", "2"],
        vec!["--tag", "1", "--label", "7"],
        vec!["--tag", "1", "--clear-labels"],
    ] {
        let server = Mock::new(|_| vec![]);
        let mut args = vec!["element", "update", "42"];
        args.extend(options);
        let output = server.run(&args);
        assert!(!output.status.success());
        server.finish();
    }
}

#[test]
fn element_update_category_server_rejection_does_not_fallback_or_retry() {
    let server = Mock::new(|_| {
        vec![
        get("/vidu/v1/material/elements/detail/42",json!({"element":{"id":"42","edition":2,"name":"original","recaption":{"description":"keep","style":""}}})),
        Step::json("PUT","/vidu/v2/material/elements/42",json!({"message":"invalid category"})).status(400).element_update_body(json!({"mask":"tag_ids","element":{"id":"42","name":"original","recaption":{"description":"keep","style":""},"tag_ids":["1"]}})),
    ]
    });
    let result = envelope(
        &server.run(&["element", "update", "42", "--tag", "1"]),
        false,
    );
    assert_eq!(result["error"]["type"], "http_error");
    server.finish();
}

/// Discovery and collections must use server-side filtering with opaque cursors.
#[test]
fn official_queries_preserve_filters_identity_and_cursor() {
    for command in ["list", "search", "favorites"] {
        let collected = if command == "favorites" {
            "true"
        } else {
            "false"
        };
        let server = Mock::new(|_| {
            vec![get(
                "/vidu/v1/material/elements/official",
                json!({
                    "elements":[{"id":42,"version":"9007199254740993","tag_ids":[1],
                        "source":"preset","is_favorited":true}],"next_page_token":"next/+="
                }),
            )
            .query(&[
                ("query.pager.pagesz", "20"),
                ("query.pager.page_token", "opaque/+=token"),
                ("query.keyword", "女孩"),
                ("query.tag_ids", "1"),
                ("collected_only", collected),
            ])]
        });
        let result = envelope(
            &server.run(&[
                "element",
                command,
                "--source",
                "official",
                "--keyword",
                "女孩",
                "--tag",
                "1",
                "--page-token",
                "opaque/+=token",
            ]),
            true,
        );
        assert_eq!(
            result["elements"],
            json!([{"id":"42","version":"9007199254740993","name":"","description":""}])
        );
        assert!(result.get("library_source").is_none());
        assert_eq!(result["next_page_token"], "next/+=");
        server.finish();
    }
}

/// Official collection uses element identity directly, including offline removal.
#[test]
fn official_collection_never_resolves_share_or_detail() {
    let server = Mock::new(|_| {
        vec![
            Step::json("PUT", "/tools/v1/votes/official_element/42", json!({}))
                .body(json!({"obj_type":"official_element","obj_id":"42","vote":"collect"})),
            Step::json("DELETE", "/tools/v1/votes/official_element/42", json!({}))
                .query(&[("vote", "collect")]),
        ]
    });
    for command in ["favorite", "unfavorite"] {
        envelope(
            &server.run(&["element", command, "42", "--source", "official"]),
            true,
        );
    }
    server.finish();
}

#[test]
fn official_errors_do_not_fallback_or_retry() {
    for status in [403, 404, 501] {
        let server = Mock::new(|_| {
            vec![Step::json(
                "PUT",
                "/tools/v1/votes/official_element/42",
                json!({"message":"unavailable"}),
            )
            .body(json!({"obj_type":"official_element","obj_id":"42","vote":"collect"}))
            .status(status)]
        });
        envelope(
            &server.run(&["element", "favorite", "42", "--source", "official"]),
            false,
        );
        server.finish();
    }
}

/// Unsupported combinations fail before touching the network.
#[test]
fn official_filters_are_not_silently_ignored() {
    let server = Mock::new(|_| vec![]);
    for args in [
        vec!["element", "favorites", "--keyword", "女孩"],
        vec!["element", "search", "--keyword", "女孩", "--tag", "1"],
        vec![
            "element",
            "search",
            "--source",
            "official",
            "--keyword",
            "女孩",
            "--sort-by",
            "recommend",
        ],
        vec![
            "element",
            "search",
            "--source",
            "official",
            "--keyword",
            "女孩",
            "--category",
            "character",
        ],
        vec![
            "element",
            "search",
            "--source",
            "official",
            "--keyword",
            "女孩",
            "--topic",
            "summer",
        ],
        vec!["element", "list", "--source", "official", "--tag", "0"],
    ] {
        envelope(&server.run(&args), false);
    }
    for args in [
        vec!["element", "list", "--source", "community"],
        vec![
            "element", "list", "--source", "official", "--tag", "1", "--tag", "2",
        ],
    ] {
        assert_eq!(server.run(&args).status.code(), Some(2));
    }
    server.finish();
}

#[test]
fn official_numeric_pages_walk_server_cursors() {
    let server = Mock::new(|_| {
        vec![
            get(
                "/vidu/v1/material/elements/official",
                json!({"elements":[],"next_page_token":"cursor-1"}),
            )
            .query(&[
                ("query.pager.pagesz", "20"),
                ("query.pager.page_token", ""),
                ("collected_only", "false"),
            ]),
            get(
                "/vidu/v1/material/elements/official",
                json!({"elements":[{"id":"42","version":"1"}]}),
            )
            .query(&[
                ("query.pager.pagesz", "20"),
                ("query.pager.page_token", "cursor-1"),
                ("collected_only", "false"),
            ]),
        ]
    });
    let result = envelope(
        &server.run(&["element", "list", "--source", "official", "--page", "1"]),
        true,
    );
    assert_eq!(result["elements"][0]["id"], "42");
    assert_eq!(result["next_page_token"], "");
    server.finish();
}

/// The 20261119 API declares flat Element arrays and opaque string cursors.
#[test]
fn official_list_contract_rejects_malformed_responses() {
    for response in [
        json!([]),
        json!({"elements":{}}),
        json!({"elements":[null]}),
        json!({"next_page_token":42}),
    ] {
        let server = Mock::new(|_| {
            vec![
                get("/vidu/v1/material/elements/official", response).query(&[
                    ("query.pager.pagesz", "20"),
                    ("query.pager.page_token", ""),
                    ("collected_only", "false"),
                ]),
            ]
        });
        envelope(
            &server.run(&["element", "list", "--source", "official"]),
            false,
        );
        server.finish();
    }
}

#[test]
fn search_rejects_unknown_sort_and_blank_keyword_before_network() {
    let server = Mock::new(|_| vec![]);
    assert_eq!(
        server
            .run(&["element", "search", "--keyword", "女孩", "--sort-by", "hot"])
            .status
            .code(),
        Some(2)
    );
    envelope(
        &server.run(&["element", "search", "--keyword", "   "]),
        false,
    );
    server.finish();
}

/// A failed or malformed asset completion must never create a subject or fall
/// back to the old Files service. Each mock permits only the specified stages.
#[test]
fn element_asset_upload_failures_stop_before_subject_write() {
    let (_directory, path, bytes) = element_image_fixture();
    for stage in ["ticket", "complete", "missing_uri"] {
        let server = Mock::new(|base| {
            if stage == "ticket" {
                return vec![Step::json(
                    "POST",
                    "/tools/v1/assets/uploads",
                    json!({"message":"denied"}),
                )
                .body(
                    json!({"file_name":"image.png","content_type":"image/png","scene":"material"}),
                )
                .status(403)];
            }
            let mut steps = element_upload_steps(base, "11", bytes.clone());
            steps.pop();
            steps.push(
                Step::json(
                    "PUT",
                    "/tools/v1/assets/uploads/complete",
                    if stage == "missing_uri" {
                        json!({"asset_id":"99"})
                    } else {
                        json!({"message":"audit rejected"})
                    },
                )
                .body(json!({"upload_token":"11","etag":"test-etag"}))
                .status(if stage == "complete" { 403 } else { 200 }),
            );
            steps
        });
        envelope(
            &server.run(&[
                "element",
                "create",
                "--name",
                "test",
                "--tag",
                "1",
                "--image",
                path.to_str().unwrap(),
            ]),
            false,
        );
        server.finish();
    }
}

#[test]
fn element_asset_binding_error_is_not_retried_or_downgraded() {
    let server = Mock::new(|_| {
        vec![
        get("/vidu/v1/material/elements/detail/42", json!({"element":{"id":"42","edition":2,"name":"test","modality":"image","recaption":{"style":"","description":"keep"}}})),
        Step::json("PUT", "/vidu/v2/material/elements/42", json!({"message":"asset forbidden"}))
            .element_update_body(json!({"mask":"components","element":{"id":"42","name":"test","modality":"image","recaption":{"style":"","description":"keep"},"components":[{"asset_id":"9","content_type":"image","type":"main"}]}})).status(403),
    ]
    });
    envelope(
        &server.run(&["element", "update", "42", "--asset-id", "9"]),
        false,
    );
    server.finish();
}

#[test]
fn element_get_keeps_asset_references() {
    let server = Mock::new(|_| {
        vec![get(
            "/vidu/v1/material/elements/detail/42",
            json!({
                "element":{"id":"42","edition":1,
                "components":[{"asset_id":"9007199254740993","content":"https://example.com/image.png"}]},
                "trace_id":"trace-detail"
            }),
        )]
    });
    let result = envelope(&server.run(&["element", "get", "42"]), true);
    assert_eq!(
        result["element"]["components"][0]["asset_id"],
        "9007199254740993"
    );
    assert_eq!(result["element"]["edition"], 1);
    assert_eq!(result["trace_id"], "trace-detail");
    server.finish();
}

#[test]
fn element_update_keeps_asset_references_for_both_write_endpoints() {
    for labels in [false, true] {
        let server = Mock::new(|_| {
            vec![
            get("/vidu/v1/material/elements/detail/42", json!({"element":{"id":"42","edition":if labels {1} else {2},"name":"test","modality":"image","recaption":{"description":"keep","style":""}}})),
            Step::json("PUT", if labels {"/vidu/v1/material/elements/42"} else {"/vidu/v2/material/elements/42"},
                json!({"id":"42","edition":2,"components":[{"asset_id":"9","content":"https://example.com/image.png"}]}))
                .element_update_body(if labels {json!({"mask":"labels","element":{"id":"42","name":"test","modality":"image","recaption":{"description":"keep","style":""},"labels":[]}})}
                    else {json!({"mask":"name","element":{"id":"42","name":"new","modality":"image","recaption":{"description":"keep","style":""}}})}),
        ]
        });
        let args = if labels {
            vec!["element", "update", "42", "--clear-labels"]
        } else {
            vec!["element", "update", "42", "--name", "new"]
        };
        let result = envelope(&server.run(&args), true);
        assert_eq!(result["components"][0]["asset_id"], "9");
        assert_eq!(result["edition"], 2);
        server.finish();
    }
}

/// Conditional search shares server filtering with browse, including cursor
/// scope and preset exclusion, without imposing list's 10-item minimum.
#[test]
fn personal_search_and_list_support_single_taxonomy_tag() {
    for command in ["search", "list"] {
        let pagesz = if command == "search" { "1" } else { "20" };
        let server = Mock::new(|_| {
            vec![get(
                "/vidu/v1/material/elements/personal",
                json!({"elements":[{"id":"42","version":"1","name":"男生","recaption":{"description":"角色描述"}}]}),
            )
            .query(&[
                ("pager.pagesz", pagesz),
                ("pager.page_token", "cursor"),
                ("without_preset", "true"),
                ("modalities", "image"),
                ("tag_ids", "1"),
                ("keyword", "男生"),
            ])]
        });
        let result = envelope(
            &server.run(&[
                "element",
                command,
                "--source",
                "personal",
                "--tag",
                "1",
                "--keyword",
                "男生",
                "--pagesz",
                pagesz,
                "--page-token",
                "cursor",
            ]),
            true,
        );
        assert_eq!(
            result["elements"],
            json!([{"id":"42","version":"1","name":"男生","description":"角色描述"}])
        );
        server.finish();
    }
}

#[test]
fn official_search_accepts_tag_without_keyword() {
    let server = Mock::new(|_| {
        vec![get(
            "/vidu/v1/material/elements/official",
            json!({"elements":[]}),
        )
        .query(&[
            ("query.pager.pagesz", "20"),
            ("query.pager.page_token", ""),
            ("query.tag_ids", "1"),
            ("collected_only", "false"),
        ])]
    });
    envelope(
        &server.run(&["element", "search", "--source", "official", "--tag", "1"]),
        true,
    );
    server.finish();
}

#[test]
fn community_search_accepts_category_without_keyword_and_stays_default() {
    let server = Mock::new(|_| {
        vec![get(
            "/vidu/v1/material/share_elements/feed",
            json!({"share_elements":[]}),
        )
        .query(&[
            ("pager.pagesz", "20"),
            ("pager.page_token", ""),
            ("category", "character"),
            ("keyword", ""),
            ("sort_by", "recommend"),
            ("is_collect", "false"),
            ("is_like", "false"),
            ("modalities", "image"),
        ])]
    });
    envelope(
        &server.run(&["element", "search", "--category", "character"]),
        true,
    );
    server.finish();
}

#[test]
fn search_validates_conditions_and_source_before_http() {
    let server = Mock::new(|_| vec![]);
    for args in [
        vec!["element", "search"],
        vec!["element", "search", "--source", "personal"],
        vec!["element", "search", "--source", "official"],
        vec!["element", "search", "--tag", "1"],
        vec!["element", "search", "--source", "personal", "--tag", "0"],
        vec![
            "element",
            "search",
            "--source",
            "personal",
            "--category",
            "character",
        ],
        vec![
            "element",
            "search",
            "--source",
            "official",
            "--keyword",
            " ",
        ],
        vec!["element", "search", "--topic", " "],
    ] {
        envelope(&server.run(&args), false);
    }
    assert_eq!(
        server
            .run(&["element", "search", "--source", "personal", "--tag", "1", "--tag", "2"])
            .status
            .code(),
        Some(2)
    );
    server.finish();
}

/// The default/explicit V1 flow must remain preprocess then legacy create,
/// with no platform asset registration or taxonomy requirement.
#[test]
fn legacy_element_create_keeps_original_write_protocol() {
    let components = json!([{"content":"ssupload:?id=old","src_img":"ssupload:?id=old","content_type":"image","type":"main"}]);
    let server = Mock::new(|_| {
        vec![
        get("/vidu/v1/material/elements/create/check", json!({})).query(&[("name", "old character")]),
        Step::json("POST", "/vidu/v1/material/elements/pre-process", json!({"id":"42","creator_id":"7","recaption":{"style":"realistic","description":"generated"}}))
            .body(json!({"components":components,"name":"old character","type":"user"})),
        Step::json("POST", "/vidu/v1/material/elements", json!({"id":"42","version":"1"}))
            .body(json!({"id":"42","name":"old character","modality":"image","type":"user","components":components,"creator_id":"7","recaption":{"style":"realistic","description":"generated"}})),
    ]
    });
    assert_eq!(
        envelope(
            &server.run_env(
                &[
                    "element",
                    "create",
                    "--name",
                    "old character",
                    "--image",
                    "ssupload:?id=old"
                ],
                &[("VIDU_ELEMENT_PLATFORM_ASSETS", "false")]
            ),
            true
        ),
        json!({"ok":true,"id":"42","version":"1"})
    );
    server.finish();
}

#[test]
fn legacy_element_name_update_stays_on_v1_and_preserves_raw_detail() {
    let current = json!({"id":"42","name":"old","modality":"image","recaption":{"style":"realistic","description":"old"},});
    let server = Mock::new(|_| {
        vec![
        get("/vidu/v1/material/elements/detail/42", json!({"element":current})),
        Step::json("PUT", "/vidu/v1/material/elements/42", json!({"version":"2"}))
            .body(json!({"mask":"name","element":{"id":"42","name":"renamed","modality":"image","recaption":{"style":"realistic","description":"old"}}})),
        get("/vidu/v1/material/elements/detail/42", json!({"element":current})),
    ]
    });
    envelope(
        &server.run_env(
            &["element", "update", "42", "--name", "renamed"],
            &[("VIDU_ELEMENT_PLATFORM_ASSETS", "false")],
        ),
        true,
    );
    let detail = envelope(
        &server.run_env(
            &["element", "get", "42"],
            &[("VIDU_ELEMENT_PLATFORM_ASSETS", "false")],
        ),
        true,
    );
    server.finish();
}

/// Only the API display fields determine names; output-shaped fields are ignored.
#[test]
fn element_tags_reads_only_api_display_names() {
    let server = Mock::new(|_| {
        vec![
        get("/vidu/v1/material/elements/tags", json!({"tags":[
            {"id":"1","display_name_zh":"角色","display_name_en":"Character","name":"忽略","name_en":"Ignored"},
            {"id":2,"name":"忽略","name_en":"Ignored","display_name_zh":"场景","display_name_en":"Scene"}
        ]})).query(&[("parent_id", "0")])
    ]
    });
    assert_eq!(
        envelope(&server.run(&["element", "tags"]), true),
        json!({"ok":true,"tags":[
            {"id":"1","name":"角色","name_en":"Character"},
            {"id":"2","name":"场景","name_en":"Scene"}
        ]})
    );
    server.finish();
}

/// Routing decisions happen after one GET and before every upload or PUT.
#[test]
fn element_update_rejects_unknown_generation_and_second_generation_labels() {
    for edition in [
        None,
        Some(json!(0)),
        Some(json!(3)),
        Some(json!("unknown")),
        Some(json!(2)),
        Some(json!("2")),
    ] {
        for options in [
            vec!["--label", "7"],
            vec!["--clear-labels"],
            vec!["--name", "blocked", "--clear-labels"],
        ] {
            let mut current = json!({"id":"42","name":"original","modality":"image","recaption":{"style":"realistic","description":"keep"}});
            if let Some(value) = &edition {
                current["edition"] = value.clone();
            }
            let server = Mock::new(|_| {
                vec![get(
                    "/vidu/v1/material/elements/detail/42",
                    json!({"element":current}),
                )]
            });
            let mut args = vec!["element", "update", "42"];
            args.extend(options);
            let result = envelope(&server.run(&args), false);
            assert_eq!(
                result["error"]["type"],
                if edition == Some(json!(2)) || edition == Some(json!("2")) {
                    "client_error"
                } else {
                    "api_error"
                }
            );
            server.finish();
        }
    }
}

#[test]
fn element_update_uses_generation_for_metadata_routing() {
    for edition in [json!(1), json!("1"), json!(2), json!("2")] {
        let first = edition == json!(1) || edition == json!("1");
        let server = Mock::new(|_| {
            vec![
            get("/vidu/v1/material/elements/detail/42",json!({"element":{"id":"42","name":"original","edition":edition,"recaption":{"description":"keep","style":"realistic"}}})),
            Step::json("PUT",if first {"/vidu/v1/material/elements/42"} else {"/vidu/v2/material/elements/42"},json!({"id":"42"})).element_update_body(json!({"mask":"name,recaption","element":{"id":"42","name":"updated","recaption":{"description":"changed","style":"realistic"}}}))
        ]
        });
        envelope(
            &server.run(&[
                "element",
                "update",
                "42",
                "--name",
                "updated",
                "--recaption",
                "changed",
            ]),
            true,
        );
        server.finish();
    }
}

#[test]
fn element_generation_guards_precede_media_uploads_and_new_fields() {
    for edition in [None, Some(json!(1)), Some(json!("1")), Some(json!(3))] {
        for options in [
            vec!["--name", "new"],
            vec!["--tag", "1"],
            vec!["--asset-id", "9"],
            vec!["--image", "/missing.png"],
        ] {
            if options[0] == "--name" && (edition == Some(json!(1)) || edition == Some(json!("1")))
            {
                continue;
            }
            let mut current = json!({"id":"42","name":"original","modality":"image","recaption":{"description":"keep","style":""}});
            if let Some(value) = &edition {
                current["edition"] = value.clone();
            }
            let server = Mock::new(|_| {
                vec![get(
                    "/vidu/v1/material/elements/detail/42",
                    json!({"element":current}),
                )]
            });
            let mut args = vec!["element", "update", "42"];
            args.extend(options);
            envelope(&server.run(&args), false);
            server.finish();
        }
    }
}

/// Reject the complete first-generation edit before uploads or any write,
/// even when image replacement is combined with otherwise valid metadata.
#[test]
fn element_first_generation_image_updates_never_upload_or_write() {
    for edition in [json!(1), json!("1")] {
        for extra in [
            vec![],
            vec!["--name", "updated"],
            vec!["--recaption", "custom"],
            vec!["--clear-labels"],
            vec!["--label", "91"],
        ] {
            let server = Mock::new(|_| {
                vec![get(
                    "/vidu/v1/material/elements/detail/42",
                    json!({"element":{"id":"42","edition":edition,"name":"old","modality":"image","recaption":{"description":"keep","style":"realistic"}}}),
                )]
            });
            let mut args = vec!["element", "update", "42", "--image", "/missing.png"];
            args.extend(extra);
            let result = envelope(&server.run(&args), false);
            assert!(result["error"]["message"]
                .as_str()
                .unwrap()
                .contains("second-generation"));
            server.finish();
        }
    }
}

/// Second-generation labels are rejected after GET, before any image upload.
#[test]
fn element_second_generation_labels_with_images_never_upload() {
    for label_args in [vec!["--clear-labels"], vec!["--label", "91"]] {
        let server = Mock::new(|_| {
            vec![get(
                "/vidu/v1/material/elements/detail/42",
                json!({"element":{"id":"42","edition":2}}),
            )]
        });
        let mut args = vec!["element", "update", "42", "--image", "/missing.png"];
        args.extend(label_args);
        let result = envelope(&server.run(&args), false);
        assert!(result["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Second-generation"));
        server.finish();
    }
}

/// Both switch values issue the same V1 metadata request for first-generation elements.
/// Update names reach the service unchanged; creation keeps its own validation.
#[test]
fn element_second_generation_update_leaves_name_validation_to_service() {
    for name in ["".to_string(), " ".to_string(), "名".repeat(31)] {
        for asset in [false, true] {
            let mut element = json!({"id":"42","name":name,"modality":"image","recaption":{"description":"keep","style":""}});
            if asset {
                element["components"] =
                    json!([{"asset_id":"9","type":"main","content_type":"image"}]);
            }
            let server = Mock::new(|_| {
                vec![
                get("/vidu/v1/material/elements/detail/42", json!({"element":{"id":"42","edition":2,"name":"old","modality":"image","recaption":{"description":"keep","style":""}}})),
                Step::json("PUT", "/vidu/v2/material/elements/42", json!({"message":"invalid name"})).status(400)
                    .element_update_body(json!({"mask":if asset {"name,components"} else {"name"},"element":element})),
            ]
            });
            let mut args = vec!["element", "update", "42", "--name", name.as_str()];
            if asset {
                args.extend(["--asset-id", "9"]);
            }
            let result = envelope(&server.run(&args), false);
            assert_eq!(result["error"]["http_status"], 400);
            server.finish();
        }
    }
}

#[test]
fn element_first_generation_update_matches_false_mode_request() {
    for options in [
        vec!["--name", "updated"],
        vec![
            "--name",
            "a-name-longer-than-thirty-characters-is-left-to-the-service",
        ],
        vec!["--recaption", "updated"],
        vec!["--label", "91", "--label", "92"],
        vec!["--clear-labels"],
    ] {
        let mut element = json!({"id":"42","name":"old","modality":"image","recaption":{"description":"keep","style":"realistic"}});
        let mask = match options[0] {
            "--name" => {
                element["name"] = json!(options[1]);
                "name"
            }
            "--recaption" => {
                element["recaption"]["description"] = json!(options[1]);
                "recaption"
            }
            "--label" => {
                element["labels"] = json!([{"id":"91"},{"id":"92"}]);
                "labels"
            }
            _ => {
                element["labels"] = json!([]);
                "labels"
            }
        };
        for switch in ["false", "true"] {
            let server = Mock::new(|_| {
                vec![
                    get(
                        "/vidu/v1/material/elements/detail/42",
                        json!({"element":{"id":"42","edition":1,"name":"old","modality":"image","recaption":{"description":"keep","style":"realistic"}}}),
                    ),
                    Step::json(
                        "PUT",
                        "/vidu/v1/material/elements/42",
                        json!({"id":"42","version":"2"}),
                    )
                    .body(json!({"mask":mask,"element":element})),
                ]
            });
            let mut args = vec!["element", "update", "42"];
            args.extend(options.iter().copied());
            envelope(
                &server.run_env(&args, &[("VIDU_ELEMENT_PLATFORM_ASSETS", switch)]),
                true,
            );
            server.finish();
        }
    }
}

#[test]
fn element_legacy_label_string_edition_uses_one_get_then_v1_update() {
    let server = Mock::new(|_| {
        vec![
        get("/vidu/v1/material/elements/detail/42", json!({"element":{"id":"42","edition":"1","name":"old","recaption":{"description":"keep","style":"realistic"}}})),
        Step::json("PUT","/vidu/v1/material/elements/42",json!({"id":"42","edition":1,"version":"2"}))
            .body(json!({"mask":"labels","element":{"id":"42","name":"old","labels":[],"recaption":{"description":"keep","style":"realistic"}}}))
    ]
    });
    let result = envelope(
        &server.run(&["element", "update", "42", "--clear-labels"]),
        true,
    );
    assert_eq!(result["version"], "2");
    assert_eq!(result["edition"], 1);
    server.finish();
}

/// Exercise all supported source/command combinations against exact HTTP query
/// contracts. Filters reach the server, and returned edition is not inferred.
#[test]
fn element_edition_filters_compose_with_source_and_pagination() {
    for source in ["personal", "official"] {
        for command in ["list", "search", "favorites"] {
            if source == "personal" && command == "favorites" {
                continue;
            }
            for edition in ["1", "2"] {
                let official = source == "official";
                let query = if official {
                    vec![
                        ("query.pager.pagesz", "20"),
                        ("query.pager.page_token", "cursor"),
                        ("query.keyword", "test"),
                        ("query.edition", edition),
                        (
                            "collected_only",
                            if command == "favorites" {
                                "true"
                            } else {
                                "false"
                            },
                        ),
                    ]
                } else {
                    vec![
                        ("pager.pagesz", "20"),
                        ("pager.page_token", "cursor"),
                        ("keyword", "test"),
                        ("edition", edition),
                        ("modalities", "image"),
                        ("without_preset", "true"),
                    ]
                };
                let server = Mock::new(|_| {
                    vec![get(
                    if official {"/vidu/v1/material/elements/official"} else {"/vidu/v1/material/elements/personal"},
                    json!({"elements":[{"id":"42","version":"9","name":"test","edition":edition.parse::<u8>().unwrap()}],"next_page_token":"next"})
                ).query(&query)]
                });
                let result = envelope(
                    &server.run(&[
                        "element",
                        command,
                        "--source",
                        source,
                        "--edition",
                        edition,
                        "--keyword",
                        "test",
                        "--page-token",
                        "cursor",
                    ]),
                    true,
                );
                assert_eq!(
                    result["elements"][0]["edition"],
                    edition.parse::<u8>().unwrap()
                );
                assert_eq!(result["next_page_token"], "next");
                server.finish();
            }
        }
    }
}

/// The same filter must be present on every page walked for numeric pagination.
#[test]
fn element_edition_filter_is_preserved_on_each_page() {
    let server = Mock::new(|_| {
        vec![
            get(
                "/vidu/v1/material/elements/personal",
                json!({"elements":[],"next_page_token":"next"}),
            )
            .query(&[
                ("pager.pagesz", "20"),
                ("pager.page_token", ""),
                ("edition", "2"),
                ("modalities", "image"),
                ("without_preset", "true"),
            ]),
            get(
                "/vidu/v1/material/elements/personal",
                json!({"elements":[{"id":"42","version":"9","name":"second","edition":2}]}),
            )
            .query(&[
                ("pager.pagesz", "20"),
                ("pager.page_token", "next"),
                ("edition", "2"),
                ("modalities", "image"),
                ("without_preset", "true"),
            ]),
        ]
    });
    let result = envelope(
        &server.run(&["element", "list", "--edition", "2", "--page", "1"]),
        true,
    );
    assert_eq!(result["elements"][0]["edition"], 2);
    server.finish();
}

#[test]
fn element_search_accepts_edition_only_for_personal_and_official() {
    for source in ["personal", "official"] {
        let server = Mock::new(|_| {
            vec![get(
                if source == "personal" {
                    "/vidu/v1/material/elements/personal"
                } else {
                    "/vidu/v1/material/elements/official"
                },
                json!({"elements":[]}),
            )
            .query(if source == "personal" {
                &[
                    ("pager.pagesz", "20"),
                    ("pager.page_token", ""),
                    ("edition", "2"),
                    ("modalities", "image"),
                    ("without_preset", "true"),
                ]
            } else {
                &[
                    ("query.pager.pagesz", "20"),
                    ("query.pager.page_token", ""),
                    ("query.edition", "2"),
                    ("collected_only", "false"),
                ]
            })]
        });
        envelope(
            &server.run(&["element", "search", "--source", source, "--edition", "2"]),
            true,
        );
        server.finish();
    }
}

#[test]
fn element_edition_rejects_unsupported_community_and_invalid_values_before_http() {
    for command in ["search", "favorites"] {
        for source_args in [vec![], vec!["--source", "community"]] {
            let server = Mock::new(|_| vec![]);
            let mut args = vec!["element", command, "--edition", "2"];
            args.extend(source_args);
            let result = envelope(&server.run(&args), false);
            assert!(result["error"]["message"]
                .as_str()
                .unwrap()
                .contains("--source personal or official"));
            server.finish();
        }
    }
    for edition in ["0", "3", "v2", "-1"] {
        let server = Mock::new(|_| vec![]);
        let output = server.run(&["element", "list", "--edition", edition]);
        assert_eq!(output.status.code(), Some(2));
        server.finish();
    }
}

#[test]
fn element_edition_summary_does_not_guess_missing_metadata_or_filter_locally() {
    let server = Mock::new(|_| {
        vec![get("/vidu/v1/material/elements/personal",json!({"elements":[{"id":"1","version":"9","name":"first","edition":1},{"id":"2","version":"9","name":"second","edition":2},{"id":"3","version":"9","name":"unknown"}]})).query(&[("pager.pagesz","20"),("pager.page_token",""),("modalities","image"),("without_preset","true")])]
    });
    let result = envelope(&server.run(&["element", "list"]), true);
    assert_eq!(result["elements"].as_array().unwrap().len(), 3);
    assert_eq!(result["elements"][0]["edition"], 1);
    assert_eq!(result["elements"][1]["edition"], 2);
    assert!(result["elements"][2].get("edition").is_none());
    server.finish();
}

#[test]
fn element_table_displays_generation_names_without_changing_json() {
    let server = Mock::new(|_| {
        vec![get(
            "/vidu/v1/material/elements/detail/42",
            json!({"element":{"id":"42","edition":2}}),
        )]
    });
    let output = server.run(&["--format", "table", "element", "get", "42"]);
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout).unwrap().contains("第二代"));
    server.finish();
}
