use std::process::Command;

fn cli() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vidu-cli"));
    cmd.env_remove("VIDU_TOKEN");
    cmd.env_remove("VIDU_BASE_URL");
    cmd
}

fn run_submit(args: &[&str]) -> (String, bool) {
    let mut all_args = vec!["task", "submit"];
    all_args.extend_from_slice(args);
    if !args.contains(&"--schedule-mode") {
        all_args.push("--schedule-mode");
        all_args.push("normal");
    }
    let output = cli().args(&all_args).output().unwrap();
    let combined = format!("{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));
    (combined, output.status.success())
}

fn assert_client_error(output: &str, expected_msg: &str) {
    assert!(output.contains("\"ok\":false") || output.contains("\"ok\": false"),
        "Expected ok:false, got: {}", output);
    assert!(output.contains(expected_msg),
        "Expected '{}' in output: {}", expected_msg, output);
}

// --- Missing VIDU_TOKEN ---

#[test]
fn submit_without_token_fails() {
    let output = cli()
        .args(["task", "submit",
            "--type", "text2video", "--prompt", "test",
            "--duration", "5", "--model-version", "3.2", "--resolution", "1080p"])
        .output().unwrap();
    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(!output.status.success());
    assert_client_error(&combined, "VIDU_TOKEN");
}

// --- Client-side validation errors (no network needed) ---

#[test]
fn submit_invalid_type() {
    let (stdout, success) = run_submit(&[
        "--type", "bad_type", "--prompt", "test",
        "--duration", "5", "--model-version", "3.2", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "Invalid type");
}

#[test]
fn submit_invalid_model_version() {
    let (stdout, success) = run_submit(&[
        "--type", "text2video", "--prompt", "test",
        "--duration", "5", "--model-version", "9.9", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "Invalid model_version");
}

#[test]
fn submit_3_2_a_invalid_duration() {
    let (stdout, success) = run_submit(&[
        "--type", "text2video", "--prompt", "test",
        "--duration", "3", "--model-version", "3.2_a", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "invalid for 3.2_a");
}

#[test]
fn submit_duration_out_of_range() {
    let (stdout, success) = run_submit(&[
        "--type", "text2video", "--prompt", "test",
        "--duration", "99", "--model-version", "3.1", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "out of range");
}

#[test]
fn submit_img2video_missing_image() {
    let (stdout, success) = run_submit(&[
        "--type", "img2video", "--prompt", "test",
        "--duration", "5", "--model-version", "3.1", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "requires exactly 1 image");
}

#[test]
fn submit_character2video_3_2_missing_transition() {
    let (stdout, success) = run_submit(&[
        "--type", "character2video", "--prompt", "test",
        "--image", "ssupload:?id=fake",
        "--duration", "8", "--model-version", "3.2", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "requires transition");
}

// --- Audio input validation ---

#[test]
fn submit_audio_wrong_task_type() {
    let (stdout, success) = run_submit(&[
        "--type", "text2video", "--prompt", "test",
        "--audio", "ssupload:?id=fake",
        "--duration", "5", "--model-version", "3.2", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "Audio input is only supported for character2video");
}

#[test]
fn submit_audio_too_many() {
    let (stdout, success) = run_submit(&[
        "--type", "character2video", "--prompt", "test",
        "--image", "ssupload:?id=fake",
        "--audio", "ssupload:?id=a1",
        "--audio", "ssupload:?id=a2",
        "--audio", "ssupload:?id=a3",
        "--audio", "ssupload:?id=a4",
        "--duration", "-1", "--model-version", "3.2_a", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "Too many audio inputs");
}

// --- Video input validation ---

#[test]
fn submit_video_wrong_task_type() {
    let (stdout, success) = run_submit(&[
        "--type", "text2video", "--prompt", "test",
        "--video", "ssupload:?id=fake",
        "--duration", "5", "--model-version", "3.2", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "Video input is only supported for character2video");
}

#[test]
fn submit_video_wrong_model() {
    let (stdout, success) = run_submit(&[
        "--type", "character2video", "--prompt", "test",
        "--image", "ssupload:?id=fake",
        "--video", "ssupload:?id=fake",
        "--duration", "8", "--model-version", "3.2", "--resolution", "1080p",
        "--transition", "pro",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "Video input is only supported for character2video with model_version 3.2_a");
}

#[test]
fn submit_video_exceeds_3() {
    let (stdout, success) = run_submit(&[
        "--type", "character2video", "--prompt", "test",
        "--image", "ssupload:?id=fake",
        "--video", "ssupload:?id=v1",
        "--video", "ssupload:?id=v2",
        "--video", "ssupload:?id=v3",
        "--video", "ssupload:?id=v4",
        "--duration", "-1", "--model-version", "3.2_a", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "Too many video inputs");
}

#[test]
fn submit_audio_url_rejected() {
    let (stdout, success) = run_submit(&[
        "--type", "character2video", "--prompt", "test",
        "--image", "ssupload:?id=fake",
        "--audio", "https://example.com/audio.wav",
        "--duration", "-1", "--model-version", "3.2_a", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "HTTP/HTTPS URLs are not supported for audio");
}

#[test]
fn submit_video_url_rejected() {
    let (stdout, success) = run_submit(&[
        "--type", "character2video", "--prompt", "test",
        "--image", "ssupload:?id=fake",
        "--video", "https://example.com/video.mp4",
        "--duration", "-1", "--model-version", "3.2_a", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "HTTP/HTTPS URLs are not supported for video");
}

#[test]
fn submit_video_nonexistent_file() {
    let (stdout, success) = run_submit(&[
        "--type", "character2video", "--prompt", "test",
        "--image", "ssupload:?id=fake",
        "--video", "/tmp/nonexistent_video_12345.mp4",
        "--duration", "-1", "--model-version", "3.2_a", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&stdout, "Video file not found");
}

#[test]
fn submit_video_wrong_format() {
    let tmp = std::env::temp_dir().join("vidu_test_bad.avi");
    std::fs::write(&tmp, b"fake").unwrap();
    let (stdout, success) = run_submit(&[
        "--type", "character2video", "--prompt", "test",
        "--image", "ssupload:?id=fake",
        "--video", tmp.to_str().unwrap(),
        "--duration", "-1", "--model-version", "3.2_a", "--resolution", "1080p",
    ]);
    std::fs::remove_file(&tmp).ok();
    assert!(!success);
    assert_client_error(&stdout, "Invalid video format");
}

// --- Audio file validation ---

#[test]
fn submit_audio_wrong_format() {
    let tmp = std::env::temp_dir().join("vidu_test_bad.ogg");
    std::fs::write(&tmp, b"fake").unwrap();
    let (stdout, success) = run_submit(&[
        "--type", "character2video", "--prompt", "test",
        "--image", "ssupload:?id=fake",
        "--audio", tmp.to_str().unwrap(),
        "--duration", "-1", "--model-version", "3.2_a", "--resolution", "1080p",
    ]);
    std::fs::remove_file(&tmp).ok();
    assert!(!success);
    assert_client_error(&stdout, "Invalid audio format");
}

// --- Compose timeline validation ---

#[test]
fn compose_timeline_missing_fields() {
    let timeline = r#"{"video_tracks":[{"video_track_clips":[{"media_url":"x"}]}]}"#;
    let output = cli()
        .args(["task", "compose", "--timeline", timeline, "--schedule-mode", "normal"])
        .output().unwrap();
    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(!output.status.success());
    assert_client_error(&combined, "missing required field 'timeline_in'");
}

#[test]
fn compose_timeline_null_fields() {
    let timeline = r#"{"video_tracks":[{"video_track_clips":[{"timeline_in":null,"timeline_out":5,"media_url":"x"}]}]}"#;
    let output = cli()
        .args(["task", "compose", "--timeline", timeline, "--schedule-mode", "normal"])
        .output().unwrap();
    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(!output.status.success());
    assert_client_error(&combined, "missing required field 'timeline_in'");
}

// --- JSON output format ---

#[test]
fn error_output_is_json() {
    let (output, _) = run_submit(&[
        "--type", "bad", "--prompt", "test",
        "--duration", "5", "--model-version", "3.2", "--resolution", "1080p",
    ]);
    let json_line = output.lines().find(|l| l.starts_with('{')).expect("No JSON line found");
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(json_line);
    assert!(parsed.is_ok(), "Output should be valid JSON: {}", json_line);
    let val = parsed.unwrap();
    assert_eq!(val["ok"], serde_json::Value::Bool(false));
    assert!(val["error"]["type"].is_string());
    assert!(val["error"]["message"].is_string());
}

// --- Lip-sync validation ---

#[test]
fn lip_sync_missing_text_and_audio() {
    let output = cli()
        .args(["task", "lip-sync", "--video", "/tmp/nonexistent.mp4"])
        .output().unwrap();
    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(!output.status.success());
    assert!(combined.contains("required") || combined.contains("error"), "Got: {}", combined);
}

// --- Compose: valid timeline passes validation ---

#[test]
fn compose_valid_timeline_reaches_token_check() {
    let tmp = std::env::temp_dir().join("vidu_test_valid_timeline.json");
    std::fs::write(&tmp, r#"{"video_tracks":[{"video_track_clips":[{"timeline_in":0,"timeline_out":5,"media_url":"http://example.com/v.mp4"}]}]}"#).unwrap();
    let output = cli()
        .args(["task", "compose", "--timeline", tmp.to_str().unwrap(), "--schedule-mode", "normal"])
        .output().unwrap();
    std::fs::remove_file(&tmp).ok();
    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(!output.status.success());
    assert_client_error(&combined, "VIDU_TOKEN");
}

// --- Compose: invalid JSON ---

#[test]
fn compose_invalid_json() {
    let output = cli()
        .args(["task", "compose", "--timeline", "not json", "--schedule-mode", "normal"])
        .output().unwrap();
    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(!output.status.success());
    assert_client_error(&combined, "Invalid timeline JSON");
}

// --- Submit: valid request reaches token check ---

#[test]
fn submit_valid_text2video_reaches_token_check() {
    let (output, success) = run_submit(&[
        "--type", "text2video", "--prompt", "a cat",
        "--duration", "5", "--model-version", "3.2", "--resolution", "1080p",
    ]);
    assert!(!success);
    assert_client_error(&output, "VIDU_TOKEN");
}

// --- Submit: aspect ratio and resolution edge cases ---

#[test]
fn submit_invalid_resolution() {
    let (output, success) = run_submit(&[
        "--type", "text2video", "--prompt", "test",
        "--duration", "5", "--model-version", "3.2", "--resolution", "8k",
    ]);
    assert!(!success);
    assert_client_error(&output, "Invalid resolution");
}

#[test]
fn submit_text2image_high_res() {
    let (output, success) = run_submit(&[
        "--type", "text2image", "--prompt", "test",
        "--duration", "0", "--model-version", "3.1", "--resolution", "4k",
    ]);
    assert!(!success);
    assert_client_error(&output, "VIDU_TOKEN");
}

// --- Help output ---

#[test]
fn help_output() {
    let output = cli().arg("--help").output().unwrap();
    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(combined.contains("vidu-cli") || combined.contains("Vidu"));
}

#[test]
fn version_output() {
    let output = cli().arg("--version").output().unwrap();
    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(combined.contains("vidu-cli"));
}

// --- No subcommand shows help ---

#[test]
fn no_subcommand_shows_help() {
    let output = cli().output().unwrap();
    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(combined.contains("Usage") || combined.contains("vidu-cli"));
}
