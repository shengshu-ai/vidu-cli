use serde_json::Value;
use std::process::Command;
use std::path::Path;

fn cli_with_env() -> Option<Command> {
    let token = std::env::var("VIDU_TOKEN").ok()?;
    if token.is_empty() || token == "your_token_here" {
        return None;
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vidu-cli"));
    cmd.env("VIDU_TOKEN", &token);
    if let Ok(base) = std::env::var("VIDU_BASE_URL") {
        cmd.env("VIDU_BASE_URL", &base);
    }
    Some(cmd)
}

fn run_cli(args: &[&str]) -> Value {
    let mut cmd = cli_with_env().expect("VIDU_TOKEN not set — skip E2E tests");
    let output = cmd.args(args).output().unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json_line = combined.lines().find(|l| l.starts_with('{')).unwrap_or_else(|| {
        panic!("No JSON in output: {}", combined)
    });
    serde_json::from_str(json_line).unwrap_or_else(|e| {
        panic!("Invalid JSON: {} — {}", e, json_line)
    })
}

fn assert_ok(val: &Value) {
    assert_eq!(val["ok"], Value::Bool(true), "Expected ok:true, got: {}", val);
}

fn extract_task_id(val: &Value) -> String {
    val["task_id"].as_str().unwrap_or("").to_string()
}

fn poll_task(task_id: &str, max_polls: u32) -> Value {
    for i in 0..max_polls {
        let result = run_cli(&["task", "get", task_id]);
        let state = result["state"].as_str().unwrap_or("");
        if state == "success" || state == "failed" {
            return result;
        }
        if i < max_polls - 1 {
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    }
    panic!("Task {} did not complete after {} polls", task_id, max_polls);
}

fn test_asset(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("test/assets").join(name);
    assert!(p.exists(), "Test asset not found: {}", p.display());
    p.to_string_lossy().to_string()
}

fn submit_and_get_task_id(args: &[&str]) -> Option<String> {
    let result = run_cli(args);
    if result["ok"] != Value::Bool(true) {
        let msg = result["error"]["message"].as_str().unwrap_or("");
        let code = result["error"]["code"].as_str().unwrap_or("");
        if code.contains("CRD") || msg.contains("Internal Server Error") || msg.contains("Cannot read") {
            eprintln!("  SKIP (server/account issue): {} {}", code, msg);
            return None;
        }
        panic!("Unexpected error: {}", result);
    }
    let task_id = extract_task_id(&result);
    assert!(!task_id.is_empty(), "Missing task_id in: {}", result);
    Some(task_id)
}

fn submit_and_poll(args: &[&str], max_polls: u32) {
    let task_id = match submit_and_get_task_id(args) {
        Some(id) => id,
        None => return,
    };
    let final_state = poll_task(&task_id, max_polls);
    assert_eq!(final_state["state"], "success", "Task failed: {}", final_state);
}

// ============================================================
// Submit + poll
// ============================================================

#[test]
#[ignore]
fn e2e_text2video_3_2_a() {
    submit_and_poll(&[
        "task", "submit",
        "--type", "text2video",
        "--prompt", "A cat walks in the snow at sunset",
        "--duration", "-1",
        "--model-version", "3.2_a",
        "--aspect-ratio", "16:9",
        "--resolution", "1080p",
        "--schedule-mode", "normal",
    ], 60);
}

#[test]
#[ignore]
fn e2e_img2video_3_2_a() {
    submit_and_poll(&[
        "task", "submit",
        "--type", "img2video",
        "--prompt", "The cat starts running",
        "--image", &test_asset("huahua.png"),
        "--duration", "-1",
        "--model-version", "3.2_a",
        "--resolution", "1080p",
        "--schedule-mode", "normal",
    ], 60);
}

#[test]
#[ignore]
fn e2e_character2video_3_2_a_with_audio() {
    submit_and_poll(&[
        "task", "submit",
        "--type", "character2video",
        "--prompt", "A person walks in the garden",
        "--image", &test_asset("image-1.jpeg"),
        "--audio", &test_asset("audio-1.wav"),
        "--duration", "-1",
        "--model-version", "3.2_a",
        "--aspect-ratio", "16:9",
        "--resolution", "1080p",
        "--schedule-mode", "normal",
    ], 60);
}

#[test]
#[ignore]
fn e2e_character2video_3_2_a_with_video() {
    submit_and_poll(&[
        "task", "submit",
        "--type", "character2video",
        "--prompt", "A person dances gracefully",
        "--image", &test_asset("image-1.jpeg"),
        "--video", &test_asset("video-1.mp4"),
        "--duration", "-1",
        "--model-version", "3.2_a",
        "--aspect-ratio", "16:9",
        "--resolution", "1080p",
        "--schedule-mode", "normal",
    ], 60);
}

// ============================================================
// TTS
// ============================================================

#[test]
#[ignore]
fn e2e_tts() {
    submit_and_poll(&[
        "task", "tts",
        "--prompt", "Hello, this is a test of text to speech.",
        "--voice-id", "English_Aussie_Bloke",
        "--schedule-mode", "normal",
    ], 30);
}

// ============================================================
// Lip-sync
// ============================================================

#[test]
#[ignore]
fn e2e_lip_sync_text() {
    submit_and_poll(&[
        "task", "lip-sync",
        "--video", &test_asset("video-1.mp4"),
        "--text", "Hello, this is a lip sync test with text to speech.",
        "--voice-id", "English_Aussie_Bloke",
        "--schedule-mode", "normal",
    ], 60);
}

// ============================================================
// Quota queries
// ============================================================

#[test]
#[ignore]
fn e2e_quota_pass() {
    let result = run_cli(&["quota", "pass"]);
    assert_ok(&result);
}

#[test]
#[ignore]
fn e2e_quota_credit() {
    let result = run_cli(&["quota", "credit"]);
    assert_ok(&result);
}

// ============================================================
// Cost queries
// ============================================================

#[test]
#[ignore]
fn e2e_task_cost() {
    let result = run_cli(&[
        "task", "cost",
        "--type", "text2video",
        "--model-version", "3.2",
        "--duration", "8",
        "--resolution", "1080p",
        "--schedule-mode", "normal",
    ]);
    assert_ok(&result);
}

#[test]
#[ignore]
fn e2e_tts_cost() {
    let result = run_cli(&[
        "task", "tts-cost",
        "--text", "Hello world test",
        "--voice-id", "English_Aussie_Bloke",
        "--schedule-mode", "normal",
    ]);
    assert_ok(&result);
}

#[test]
#[ignore]
fn e2e_lip_sync_cost() {
    let result = run_cli(&[
        "task", "lip-sync-cost",
        "--duration", "5",
        "--schedule-mode", "normal",
    ]);
    assert_ok(&result);
}

// ============================================================
// Voice listing
// ============================================================

#[test]
#[ignore]
fn e2e_lip_sync_voices() {
    let result = run_cli(&["task", "lip-sync-voices"]);
    assert_ok(&result);
    assert!(result["count"].as_u64().unwrap_or(0) > 0, "Expected voices, got: {}", result);
}

#[test]
#[ignore]
fn e2e_tts_voices() {
    let result = run_cli(&["task", "tts-voices"]);
    assert_ok(&result);
    let count = result["total"].as_u64().or(result["count"].as_u64()).unwrap_or(0);
    assert!(count > 0, "Expected voices, got: {}", result);
}

// ============================================================
// Element list
// ============================================================

#[test]
#[ignore]
fn e2e_element_list() {
    let result = run_cli(&["element", "list"]);
    assert_ok(&result);
}
