use super::tasks;
use crate::{client, validators};
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};

const STATES: &[&str] = &[
    "unspecified",
    "created",
    "queueing",
    "preparation",
    "scheduling",
    "processing",
    "success",
    "failed",
    "canceled",
];
const TYPES: &[&str] = &[
    "unspecified",
    "text2video",
    "img2video",
    "character2video",
    "upscale",
    "extend",
    "headtailimg2video",
    "scene",
    "scene2",
    "lora",
    "controlnet",
    "material2video",
    "text2audio",
    "reference2image",
    "auto2video",
    "remix2video",
    "video2extend",
    "video2extail",
    "video2audio",
    "multi_frame",
    "text2image",
    "auto_compose",
    "omni2video",
    "video_remix",
    "upscale_image",
    "one_click",
    "remake",
    "video_edit",
    "face_swap",
    "image_edit",
    "music_video",
    "lip_sync",
    "tts",
    "clip_compose",
    "short_story",
    "style_transfer",
    "motion_control",
    "offline_avatar",
];
const RERUN_TYPES: &[&str] = &[
    "text2image",
    "text2video",
    "img2video",
    "headtailimg2video",
    "reference2image",
    "character2video",
];

fn invalid(message: &str) -> ! {
    client::fail("client_error", message, None, None, None)
}
fn id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn check_id(id: &str) {
    if !id_valid(id) {
        invalid("Invalid task ID");
    }
}
fn url(path: &str) -> String {
    format!(
        "{}/vidu/v1/{}",
        client::base_url().trim_end_matches('/'),
        path
    )
}

fn list_url(
    base: &str,
    states: &[String],
    types: &[String],
    scenes: &[String],
    page_token: &str,
    pagesz: i64,
) -> Result<String, String> {
    if !(1..=100).contains(&pagesz) {
        return Err("pagesz must be between 1 and 100".into());
    }
    let mut url = reqwest::Url::parse(base).map_err(|e| e.to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("pager.pagesz", &pagesz.to_string());
        if !page_token.is_empty() {
            query.append_pair("pager.page_token", page_token);
        }
        for (key, values, allowed) in [
            ("states", states, Some(STATES)),
            ("types", types, Some(TYPES)),
            ("scenes", scenes, None),
        ] {
            for value in values {
                if allowed.is_some_and(|values| !values.contains(&value.as_str())) {
                    return Err(format!("Invalid {key}: {value}"));
                }
                query.append_pair(key, value);
            }
        }
    }
    Ok(url.into())
}

pub fn list(
    states: &[String],
    types: &[String],
    scenes: &[String],
    page_token: &str,
    pagesz: i64,
) -> Value {
    let endpoint = list_url(
        &url("tasks/history/me"),
        states,
        types,
        scenes,
        page_token,
        pagesz,
    )
    .unwrap_or_else(|e| invalid(&e));
    client::request_json("GET", &endpoint, Some("task_list"), None, None)
}

pub fn cancel(task_id: &str) -> Value {
    check_id(task_id);
    let mut result = client::request_json(
        "POST",
        &url(&format!("tasks/{task_id}/cancel")),
        Some("task_cancel"),
        Some(&json!({})),
        None,
    );
    result["task_id"] = json!(task_id);
    result["action"] = json!("canceled");
    result
}

pub fn delete(task_id: &str) -> Value {
    check_id(task_id);
    let mut result = client::request_json(
        "DELETE",
        &url(&format!("tasks/{task_id}")),
        Some("task_delete"),
        None,
        None,
    );
    result["task_id"] = json!(task_id);
    result["action"] = json!("deleted");
    result
}

fn submission(data: Value) -> Value {
    let id = data
        .get("id")
        .or_else(|| data.get("task_id"))
        .and_then(|id| match id {
            Value::String(id) => Some(id.clone()),
            Value::Number(id) => Some(id.to_string()),
            _ => None,
        })
        .unwrap_or_default();
    if id.is_empty() {
        client::fail(
            "parse_error",
            "No task id in submission response",
            None,
            None,
            None,
        );
    }
    json!({"task_id": id})
}

fn validate_extend_options(
    creation_id: i64,
    duration: i64,
    transition: &str,
) -> Result<(), String> {
    if creation_id <= 0 {
        return Err("creation_id must be positive".into());
    }
    if !(1..=7).contains(&duration) {
        return Err("duration must be between 1 and 7".into());
    }
    if !["extendspeed", "extendpro"].contains(&transition) {
        return Err("transition must be extendspeed or extendpro".into());
    }
    Ok(())
}

fn source_creation_duration(creation: &Value) -> Result<i64, String> {
    if let Some(duration) = creation.get("duration").and_then(Value::as_f64) {
        let rounded = duration.round();
        // The service compares timing against int32(math.Round(creation.Duration)).
        if duration.is_finite()
            && duration > 0.0
            && rounded >= 1.0
            && rounded <= f64::from(i32::MAX)
        {
            return Ok(rounded as i64);
        }
    }
    Err("Source creation duration must be a finite positive number that rounds to a positive 32-bit integer".into())
}

fn extend_body(
    creation_id: i64,
    duration: i64,
    transition: &str,
    source_duration: i64,
) -> Result<Value, String> {
    validate_extend_options(creation_id, duration, transition)?;
    if !(1..=i64::from(i32::MAX)).contains(&source_duration) {
        return Err("Source duration must be a positive 32-bit integer".into());
    }
    Ok(
        json!({"type":"video2extend", "input":{"prompts":[{"type":"video", "content":format!("sscreation:?id={creation_id}"), "extend2video":{"from":0,"to":source_duration}}],"editor_mode":"normal","enhance":true},"settings":{"duration":duration,"model_version":"3.1","resolution":"1080p","sample_count":1,"transition":transition,"schedule_mode":"normal"}}),
    )
}

pub fn extend(creation_id: i64, duration: i64, transition: &str) -> Value {
    validate_extend_options(creation_id, duration, transition).unwrap_or_else(|e| invalid(&e));
    // Detail performs the authenticated creation/task scope checks; the submit
    // service repeats ownership, resolution and subscription duration checks.
    let detail = client::request_json(
        "GET",
        &url(&format!("creations/{creation_id}/detail")),
        Some("task_extend_source"),
        None,
        None,
    );
    let creation = &detail["creation"];
    if creation["type"].as_str() != Some("video")
        || creation["resolution"]["resolution"].as_str() != Some("1080p")
        || detail["task"]["state"].as_str() != Some("success")
        || creation["is_deleted"].as_bool() == Some(true)
        || creation["err_code"].as_str().is_some_and(|e| !e.is_empty())
    {
        invalid("Source must be an accessible completed 1080p video creation");
    }
    let source_duration = source_creation_duration(creation).unwrap_or_else(|error| {
        client::fail(
            "parse_error",
            &error,
            None,
            None,
            Some("task_extend_source"),
        )
    });
    let body = extend_body(creation_id, duration, transition, source_duration)
        .unwrap_or_else(|error| invalid(&error));
    submission(client::request_json(
        "POST",
        &url("tasks"),
        Some("task_extend"),
        Some(&body),
        None,
    ))
}

pub fn upscale_image(creation_id: i64, resolution: &str) -> Value {
    if creation_id <= 0 {
        invalid("creation_id must be positive");
    }
    if !["720p", "1080p", "2k", "4k"].contains(&resolution) {
        invalid("resolution must be 720p, 1080p, 2k or 4k");
    }
    let body = json!({"type":"upscale_image","input":{"creation_id":creation_id},"settings":{"resolution":resolution}});
    submission(client::request_json(
        "POST",
        &url("tasks/tool"),
        Some("task_upscale_image"),
        Some(&body),
        None,
    ))
}

fn subset(value: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .filter_map(|key| value.get(*key).map(|v| ((*key).to_string(), v.clone())))
            .collect::<Map<_, _>>(),
    )
}

// Version overrides cannot expose transition/schedule/model overrides of their
// own, so translate legacy settings into the target version's accepted shape.
fn normalize_model_override(
    task_type: &str,
    settings: &mut Value,
    new_model: &str,
) -> Result<(), String> {
    settings.as_object_mut().unwrap().remove("model");
    let old_transition = settings["transition"].as_str().unwrap_or("");
    let accepts_transition = !["text2image", "reference2image"].contains(&task_type)
        && new_model != "3.4"
        && new_model != validators::DRAMA_MODEL_VERSION
        && (["img2video", "headtailimg2video"].contains(&task_type)
            || (["text2video", "character2video"].contains(&task_type) && new_model == "3.2"));
    if !accepts_transition {
        settings.as_object_mut().unwrap().remove("transition");
    } else if new_model == "3.0" {
        let transition = if ["creative", "stable"].contains(&old_transition) {
            old_transition
        } else {
            "stable"
        };
        settings["transition"] = json!(transition);
    } else if ["pro", "speed"].contains(&old_transition) {
        // Already valid in the target model; preserve the user's choice.
    } else if ["3.1", "3.2"].contains(&new_model) {
        // Legacy creative/stable are not equivalent to quality/speed modes.
        settings["transition"] = json!("speed");
    } else {
        settings.as_object_mut().unwrap().remove("transition");
    }
    if new_model == "3.0" && settings["style"].as_str().unwrap_or("").is_empty() {
        settings["style"] = json!("general");
    }
    if validators::preferred_schedule_mode(Some(new_model), settings["schedule_mode"].as_str())
        .is_err()
    {
        settings.as_object_mut().unwrap().remove("schedule_mode");
        if let Some(mode) = validators::preferred_schedule_mode(Some(new_model), None)? {
            settings["schedule_mode"] = json!(mode);
        }
    }
    Ok(())
}

fn rerun_body(
    raw: &Value,
    prompt: Option<&str>,
    model: Option<&str>,
    duration: Option<i64>,
    resolution: Option<&str>,
) -> Result<Value, String> {
    let task_type = raw["type"].as_str().unwrap_or("");
    if !RERUN_TYPES.contains(&task_type) {
        return Err(format!("Task type '{task_type}' does not support rerun"));
    }
    let source_prompts = raw["input"]["prompts"]
        .as_array()
        .ok_or("Historical task is missing input.prompts")?;
    let mut prompts = Vec::new();
    let mut replaced = false;
    for source in source_prompts {
        let kind = source["type"].as_str().unwrap_or("");
        if !["text", "image", "video", "audio", "material"].contains(&kind) {
            return Err(format!("Historical prompt type '{kind}' cannot be rerun"));
        }
        let mut p = subset(
            source,
            &[
                "type",
                "content",
                "negative",
                "name",
                "selected_region",
                "src_imgs",
                "material",
                "lip_sync",
            ],
        );
        // Material references are restricted to their user supplied identity.
        if kind == "material" {
            p["material"] = subset(&source["material"], &["id", "version"]);
        }
        if kind == "text" && source["negative"].as_bool() != Some(true) {
            if let Some(text) = prompt {
                if replaced {
                    continue;
                }
                p["content"] = json!(text);
                replaced = true;
            }
        }
        prompts.push(p);
    }
    if let Some(text) = prompt {
        if !replaced {
            prompts.insert(0, json!({"type":"text","content":text}));
        }
    }
    let mut input = subset(
        &raw["input"],
        &[
            "enhance",
            "editor_mode",
            "multi_image_boost",
            "with_audio",
            "lip_sync",
        ],
    );
    input["prompts"] = json!(prompts);
    if input.get("enhance").is_none() {
        input["enhance"] = json!(true);
    }
    if input.get("editor_mode").is_none() {
        input["editor_mode"] = json!("normal");
    }
    let mut settings = subset(
        &raw["settings"],
        &[
            "duration",
            "model",
            "style",
            "model_version",
            "resolution",
            "aspect_ratio",
            "sample_count",
            "codec",
            "movement_amplitude",
            "schedule_mode",
            "transition",
            "with_audio",
            "retain_original_audio",
            "lip_sync",
        ],
    );
    // Proto defaults and server derived values are not valid explicit overrides.
    if settings["transition"] == "unspecified" {
        settings.as_object_mut().unwrap().remove("transition");
    }
    if settings["schedule_mode"] == "unspecified" {
        settings.as_object_mut().unwrap().remove("schedule_mode");
    }
    if let Some(model) = model {
        if raw["settings"]["model_version"].as_str() != Some(model) {
            normalize_model_override(task_type, &mut settings, model)?;
        }
        settings["model_version"] = json!(model);
    }
    if let Some(duration) = duration {
        if ["text2image", "reference2image"].contains(&task_type) {
            return Err("duration override is only supported for video tasks".into());
        }
        settings["duration"] = json!(duration);
    }
    if let Some(resolution) = resolution {
        settings["resolution"] = json!(resolution);
    }
    let schedule = validators::preferred_schedule_mode(
        settings["model_version"].as_str(),
        settings["schedule_mode"].as_str(),
    )?;
    if let Some(schedule) = schedule {
        settings["schedule_mode"] = json!(schedule);
    }
    let body = json!({"type":task_type,"input":input,"settings":settings});
    let error = validators::validate_task_body(&body);
    if !error.is_empty() {
        return Err(error);
    }
    Ok(body)
}

pub fn rerun(
    task_id: &str,
    prompt: Option<&str>,
    prompt_path: Option<&str>,
    model: Option<&str>,
    duration: Option<i64>,
    resolution: Option<&str>,
) -> Value {
    check_id(task_id);
    if prompt.is_some() && prompt_path.is_some() {
        invalid("prompt and prompt-file are mutually exclusive");
    }
    let file_prompt =
        prompt_path.map(|path| validators::read_prompt_file(path).unwrap_or_else(|e| invalid(&e)));
    let raw = client::request_json(
        "GET",
        &url(&format!("tasks/{task_id}")),
        Some("task_rerun_source"),
        None,
        None,
    );
    let body = rerun_body(
        &raw,
        prompt.or(file_prompt.as_deref()),
        model,
        duration,
        resolution,
    )
    .unwrap_or_else(|e| invalid(&e));
    submission(client::request_json(
        "POST",
        &url("tasks"),
        Some("task_rerun"),
        Some(&body),
        None,
    ))
}

fn terminal(state: &str) -> bool {
    ["success", "failed", "canceled"].contains(&state)
}
fn wait_fail(task_id: &str, kind: &str, message: &str, state: &str) -> ! {
    client::fail_with_fields(
        kind,
        message,
        None,
        None,
        Some("wait_task"),
        Some(&json!({"task_id":task_id,"state":state})),
        None,
        None,
        None,
    )
}

pub fn wait(
    task_id: &str,
    timeout_secs: u64,
    poll_interval_secs: u64,
    output: Option<&str>,
) -> Value {
    check_id(task_id);
    if timeout_secs == 0 || poll_interval_secs == 0 {
        invalid("timeout and poll-interval must be positive");
    }
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(timeout_secs))
        .unwrap_or_else(|| invalid("timeout is too large"));
    let mut previous = String::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            wait_fail(
                task_id,
                "timeout_error",
                "Task wait timed out; the background task continues",
                &previous,
            );
        }
        let raw = client::request_json_with_timeout_context(
            "GET",
            &url(&format!("tasks/{task_id}")),
            Some("wait_task"),
            None,
            None,
            remaining.min(Duration::from_secs(30)),
            &json!({"task_id": task_id}),
        );
        if Instant::now() >= deadline {
            wait_fail(
                task_id,
                "timeout_error",
                "Task wait timed out; the background task continues",
                &previous,
            );
        }
        let state = raw["state"].as_str().unwrap_or("");
        if state.is_empty() {
            wait_fail(
                task_id,
                "parse_error",
                "Task response is missing state",
                state,
            );
        }
        if previous != state {
            eprintln!("Task {task_id}: {state}");
            previous = state.to_string();
        }
        if terminal(state) {
            if state == "success" {
                return tasks::task_result(task_id, &raw, output, false);
            }
            wait_fail(
                task_id,
                "task_error",
                raw["err_msg"].as_str().filter(|s| !s.is_empty()).unwrap_or(
                    if state == "canceled" {
                        "Task was canceled"
                    } else {
                        "Task failed"
                    },
                ),
                state,
            );
        }
        std::thread::sleep(
            Duration::from_secs(poll_interval_secs)
                .min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

pub fn finish_submission(
    data: Value,
    wait_requested: bool,
    timeout_secs: u64,
    output: Option<&str>,
) -> Value {
    if !wait_requested {
        if output.is_some() {
            invalid("--output requires --wait for submissions");
        }
        return data;
    }
    let id = data["task_id"].as_str().unwrap_or_else(|| {
        client::fail(
            "parse_error",
            "Submission response is missing task_id",
            None,
            None,
            None,
        )
    });
    wait(id, timeout_secs, 5, output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_filters_survive_url_encoding() {
        let url = list_url(
            "http://localhost/tasks",
            &["success".into(), "failed".into()],
            &["text2video".into()],
            &["hello & world".into(), "other".into()],
            "abc+def",
            20,
        )
        .unwrap();
        let url = reqwest::Url::parse(&url).unwrap();
        let pairs: Vec<_> = url.query_pairs().collect();
        assert_eq!(pairs.iter().filter(|(k, _)| k == "states").count(), 2);
        assert!(pairs
            .iter()
            .any(|(k, v)| k == "scenes" && v == "hello & world"));
        assert!(list_url("http://localhost/tasks", &["bad".into()], &[], &[], "", 20).is_err());
    }
    #[test]
    fn extension_uses_video_prompt_and_no_clipping() {
        let body = extend_body(123, 7, "extendpro", 5).unwrap();
        assert_eq!(body["input"]["prompts"][0]["content"], "sscreation:?id=123");
        assert_eq!(
            body["input"]["prompts"][0]["extend2video"],
            json!({"from":0,"to":5})
        );
        assert_eq!(body["settings"]["model_version"], "3.1");
        assert!(extend_body(0, 1, "extendspeed", 5).is_err());
        assert!(extend_body(1, 8, "extendspeed", 5).is_err());
        assert!(extend_body(1, 1, "speed", 5).is_err());
    }
    #[test]
    fn source_duration_rounds_like_service_and_uses_full_creation() {
        for (duration, expected) in [(5.056, 5), (5.5, 6), (0.5, 1), (2147483647.0, 2147483647)] {
            let source_duration = source_creation_duration(&json!({"duration": duration})).unwrap();
            assert_eq!(source_duration, expected);
            let body = extend_body(123, 1, "extendspeed", source_duration).unwrap();
            assert_eq!(
                body["input"]["prompts"][0]["extend2video"],
                json!({"from": 0, "to": expected})
            );
            assert_eq!(body["settings"]["duration"], 1);
            assert_eq!(body["settings"]["resolution"], "1080p");
        }
    }

    #[test]
    fn source_duration_rejects_missing_invalid_and_unsafe_values() {
        assert!(source_creation_duration(&json!({"settings":{"duration":5}})).is_err());
        for duration in [
            Value::Null,
            json!("5"),
            json!(true),
            json!(0),
            json!(-5),
            json!(0.4),
            json!(2147483647.5),
            json!(1e300),
        ] {
            assert!(source_creation_duration(&json!({"duration": duration})).is_err());
        }
        assert!(extend_body(123, 1, "extendspeed", 0).is_err());
        assert!(extend_body(123, 1, "extendspeed", i64::MAX).is_err());
    }

    #[test]
    fn rerun_preserves_media_and_only_permitted_fields() {
        let raw = json!({"id":"old","type":"img2video","input":{"seed":42,"skip_moderation":true,"enhance":false,"prompts":[{"type":"text","content":"old","recaption":"server"},{"type":"image","content":"ssupload:original","audit":{"state":"success"}}]},"settings":{"model_version":"3.2","duration":5,"resolution":"1080p","sample_count":1,"use_trial":true,"is_locked":true}});
        let body = rerun_body(&raw, Some("new"), Some("3.2_a"), Some(6), Some("720p")).unwrap();
        assert_eq!(body["input"]["prompts"][1]["content"], "ssupload:original");
        assert_eq!(body["input"]["prompts"][0]["content"], "new");
        assert_eq!(body["settings"]["duration"], 6);
        for key in ["seed", "skip_moderation"] {
            assert!(body["input"].get(key).is_none());
        }
        for key in ["use_trial", "is_locked"] {
            assert!(body["settings"].get(key).is_none());
        }
        assert!(body["input"]["prompts"][1].get("audit").is_none());
        assert!(body["input"]["prompts"][0].get("recaption").is_none());
        let mut unsupported = raw;
        unsupported["type"] = json!("tts");
        assert!(rerun_body(&unsupported, None, None, None, None).is_err());
    }
    #[test]
    fn rerun_preserves_character_lip_sync_settings() {
        let raw = json!({"type":"character2video", "input":{"enhance":true,"lip_sync":true,"prompts":[{"type":"text","content":"hello"},{"type":"image","content":"ssupload:original"}]},"settings":{"model_version":"3.1","duration":5,"resolution":"1080p","model":"vidu","style":"historical_style","lip_sync":true}});
        let body = rerun_body(&raw, None, None, None, None).unwrap();
        assert_eq!(body["input"]["lip_sync"], true);
        assert_eq!(body["settings"]["lip_sync"], true);
        assert_eq!(body["settings"]["model"], "vidu");
        assert_eq!(body["settings"]["style"], "historical_style");
    }
    #[test]
    fn rerun_preserves_legacy_anime_without_model_override() {
        let raw = json!({"type":"text2video","input":{"enhance":true,"prompts":[{"type":"text","content":"anime scene"}]},"settings":{"model":"vidu","model_version":"3.0","duration":5,"resolution":"1080p","style":"anime"}});
        let body = rerun_body(&raw, None, None, None, None).unwrap();
        assert_eq!(body["settings"]["style"], "anime");
        assert_eq!(body["settings"]["model"], "vidu");
    }
    #[test]
    fn version_override_normalizes_inherited_transition_schedule_and_vendor() {
        let raw = json!({"type":"character2video","input":{"enhance":true,"lip_sync":true,"prompts":[{"type":"text","content":"hello"},{"type":"image","content":"ssupload:original"}]},"settings":{"model":"old-vendor","model_version":"3.1","duration":5,"resolution":"1080p","transition":"pro","schedule_mode":"claw_pass","style":"anime"}});
        for model in ["3.4", validators::DRAMA_MODEL_VERSION] {
            let body = rerun_body(&raw, None, Some(model), None, None).unwrap();
            assert!(body["settings"].get("transition").is_none());
            assert!(body["settings"].get("model").is_none());
            assert_eq!(body["settings"]["schedule_mode"], "normal");
            assert_eq!(body["settings"]["style"], "anime");
            assert_eq!(body["settings"]["duration"], 5);
            assert_eq!(body["settings"]["resolution"], "1080p");
        }
        let mut image = raw;
        image["type"] = json!("img2video");
        let body = rerun_body(&image, None, Some("3.0"), None, None).unwrap();
        assert_eq!(body["settings"]["transition"], "stable");
        let body = rerun_body(&body, None, Some("3.2"), None, None).unwrap();
        assert_eq!(body["settings"]["transition"], "speed");
        let mut missing_style = body;
        missing_style["settings"]
            .as_object_mut()
            .unwrap()
            .remove("style");
        let legacy = rerun_body(&missing_style, None, Some("3.0"), None, None).unwrap();
        assert_eq!(legacy["settings"]["style"], "general");
        assert_eq!(legacy["settings"]["transition"], "stable");
    }
    #[test]
    fn identifiers_and_terminal_states() {
        assert!(id_valid("task_123"));
        assert!(id_valid("123"));
        for id in ["", "../123", "x?secret=1", "a/b"] {
            assert!(!id_valid(id));
        }
        for state in ["success", "failed", "canceled"] {
            assert!(terminal(state));
        }
        for state in [
            "processing",
            "queueing",
            "created",
            "scheduling",
            "preparation",
            "",
        ] {
            assert!(!terminal(state));
        }
    }
    #[test]
    fn asynchronous_submission_is_returned_unchanged() {
        let data = json!({"task_id":"123","trace_id":"keep","extra":true});
        assert_eq!(finish_submission(data.clone(), false, 1800, None), data);
    }
}
