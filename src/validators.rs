use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

fn set(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Shared enum sets — kept as `pub const` so `main.rs` (clap PossibleValuesParser)
// and the validators below use exactly the same source of truth. Adding a new
// model / resolution / etc. should only require touching this file.
// ---------------------------------------------------------------------------

/// All accepted `--type` values for `task submit` and `task cost`.
pub const TASK_TYPES: &[&str] = &[
    "text2image",
    "text2video",
    "img2video",
    "headtailimg2video",
    "reference2image",
    "character2video",
];

/// Type aliases: user-friendly shorthand -> canonical type
pub const TYPE_ALIASES: &[(&str, &str)] = &[("img2image", "reference2image")];

/// Vidu Q3.1 Drama model, exposed as an additional model version rather than a type alias.
pub const DRAMA_MODEL_VERSION: &str = "viduq3.1-drama-std";
/// Wan 3 standard, routed through the main-site task API.
pub const WAN_MODEL_VERSION: &str = "viduq2-drama-std";
const WAN_TASK_TYPES: &[&str] = &[
    "text2video",
    "img2video",
    "headtailimg2video",
    "character2video",
];
const WAN_ASPECT_RATIOS: &[&str] = &["adaptive", "21:9", "16:9", "4:3", "1:1", "3:4", "9:16"];

pub fn validate_output_audio(task_type: &str, with_audio: Option<bool>) -> Result<(), String> {
    if with_audio.is_some()
        && ![
            "text2video",
            "img2video",
            "headtailimg2video",
            "character2video",
        ]
        .contains(&task_type)
    {
        return Err("--with-audio is only supported for video tasks".into());
    }
    Ok(())
}

/// Parse and normalize task type (handles aliases)
pub fn parse_task_type(input: &str) -> Result<String, String> {
    // Check aliases first
    for (alias, canonical) in TYPE_ALIASES {
        if input == *alias {
            return Ok(canonical.to_string());
        }
    }
    // Check canonical types
    if TASK_TYPES.contains(&input) {
        return Ok(input.to_string());
    }
    // Build error message
    let mut all_types: Vec<&str> = TASK_TYPES.to_vec();
    all_types.sort();
    Err(format!(
        "Invalid type '{}'. Valid: {}",
        input,
        all_types.join(", ")
    ))
}

/// All accepted `--model-version` values across every task type
/// (per-type compatibility is then enforced by `validate_task_body`).
pub const MODEL_VERSIONS: &[&str] = &[
    "3.0",
    "3.1",
    "3.1_pro",
    "3.2",
    "3.2_a",
    "3.4",
    "3.2_fast_m",
    "3.2_pro_m",
    "3.2_image_2",
    "3.2_image_2_5_fast",
    "3.2_image_2_5_pro",
    DRAMA_MODEL_VERSION,
    WAN_MODEL_VERSION,
];

pub const RESOLUTIONS: &[&str] = &["480p", "540p", "720p", "1080p", "2k", "4k"];
const Q4_RESOLUTIONS: &[&str] = &["540p", "720p", "1080p", "2k", "4k"];
// SD can generate these natively except 2k, which the configured 720p +
// Tencent upscale path supports. Deployment-dependent availability stays server-side.
const SD_RESOLUTIONS: &[&str] = &["480p", "720p", "1080p", "2k", "4k"];
const SD_ASPECT_RATIOS: &[&str] = &["16:9", "9:16", "1:1", "4:3", "3:4", "21:9", "adaptive"];

/// All accepted `--aspect-ratio` values.
pub const ASPECT_RATIOS: &[&str] = &[
    "16:9", "9:16", "1:1", "4:3", "3:4", "3:2", "2:3", "21:9", "9:21", "adaptive", "auto",
];

/// Parse the syntax shared by model-specific aspect ratio rules. Empty/auto is
/// the API's intelligent image ratio; membership and model capabilities are
/// checked separately by `validate_generation_settings`.
pub fn parse_aspect_ratio(input: &str) -> Result<String, String> {
    let ratio = input.trim();
    if ratio.is_empty() || ratio == "auto" {
        return Ok(String::new());
    }
    if ASPECT_RATIOS.contains(&ratio) || numeric_aspect_ratio(ratio).is_some() {
        return Ok(ratio.to_string());
    }
    Err("aspect_ratio must be auto, adaptive, or a positive width:height ratio".into())
}

fn numeric_aspect_ratio(ratio: &str) -> Option<f64> {
    let (width, height) = ratio.split_once(':')?;
    let width: f64 = width.trim().parse().ok()?;
    let height: f64 = height.trim().parse().ok()?;
    let value = width / height;
    (width.is_finite()
        && height.is_finite()
        && width > 0.0
        && height > 0.0
        && value.is_finite()
        && value > 0.0)
        .then_some(value)
}

/// All accepted `--transition` values across task types.
pub const TRANSITIONS: &[&str] = &["creative", "stable", "pro", "speed", "pro_ad"];

/// All accepted `--schedule-mode` values.
pub const SCHEDULE_MODES: &[&str] = &["claw_pass", "nopeak", "normal"];

/// All accepted `--codec` values.
pub const CODECS: &[&str] = &["h264", "h265", "h264_pro", "h265_pro"];

/// All accepted `--language-boost` values for `task tts`.
/// Sorted alphabetically to keep `--help` output stable.
pub const LANGUAGE_BOOSTS: &[&str] = &[
    "Afrikaans",
    "Arabic",
    "Bulgarian",
    "Catalan",
    "Chinese",
    "Chinese,Yue",
    "Croatian",
    "Czech",
    "Danish",
    "Dutch",
    "English",
    "Filipino",
    "Finnish",
    "French",
    "German",
    "Greek",
    "Hebrew",
    "Hindi",
    "Hungarian",
    "Indonesian",
    "Italian",
    "Japanese",
    "Korean",
    "Malay",
    "Norwegian",
    "Nynorsk",
    "Persian",
    "Polish",
    "Portuguese",
    "Romanian",
    "Russian",
    "Slovak",
    "Slovenian",
    "Spanish",
    "Swedish",
    "Tamil",
    "Thai",
    "Turkish",
    "Ukrainian",
    "Vietnamese",
    "auto",
];

fn valid_task_types() -> HashSet<String> {
    set(TASK_TYPES)
}

fn valid_model_versions() -> HashSet<String> {
    set(MODEL_VERSIONS)
}

fn resolution_support() -> HashMap<String, HashSet<String>> {
    let mut m = HashMap::new();
    m.insert("text2image".into(), set(&["1080p", "2k", "4k"]));
    m.insert("reference2image".into(), set(&["1080p", "2k", "4k"]));
    m.insert("text2video".into(), set(&["1080p"]));
    m.insert("img2video".into(), set(&["1080p"]));
    m.insert("headtailimg2video".into(), set(&["1080p"]));
    m.insert("character2video".into(), set(&["1080p"]));
    m
}

fn valid_aspect_ratios() -> HashSet<String> {
    set(&["16:9", "9:16", "1:1", "4:3", "3:4"])
}

fn duration_ranges() -> HashMap<String, HashMap<String, (i64, i64)>> {
    let mut m = HashMap::new();
    let mut tv = HashMap::new();
    tv.insert("3.0".into(), (5, 5));
    tv.insert("3.1".into(), (2, 8));
    tv.insert("3.2".into(), (1, 16));
    let mut drama_tv = tv.clone();
    drama_tv.insert(DRAMA_MODEL_VERSION.into(), (4, 30));
    m.insert("text2video".into(), drama_tv);
    let mut iv = tv.clone();
    iv.insert("3.4".into(), (3, 16));
    m.insert("img2video".into(), iv);
    m.insert("headtailimg2video".into(), tv);
    let mut cv = HashMap::new();
    cv.insert("3.0".into(), (5, 5));
    cv.insert("3.1".into(), (2, 8));
    cv.insert("3.1_pro".into(), (-1, 8));
    cv.insert("3.2".into(), (1, 16));
    cv.insert("3.4".into(), (1, 16));
    cv.insert(DRAMA_MODEL_VERSION.into(), (4, 30));
    m.insert("character2video".into(), cv);
    let mut ri = HashMap::new();
    ri.insert("3.1".into(), (0, 0));
    ri.insert("3.2_fast_m".into(), (0, 0));
    ri.insert("3.2_pro_m".into(), (0, 0));
    ri.insert("3.2_image_2".into(), (0, 0));
    ri.insert("3.2_image_2_5_fast".into(), (0, 0));
    ri.insert("3.2_image_2_5_pro".into(), (0, 0));
    m.insert("reference2image".into(), ri.clone());
    m.insert("text2image".into(), ri);
    m
}

fn model_support() -> HashMap<String, HashSet<String>> {
    let mut m = HashMap::new();
    m.insert(
        "text2image".into(),
        set(&[
            "3.1",
            "3.2_fast_m",
            "3.2_pro_m",
            "3.2_image_2",
            "3.2_image_2_5_fast",
            "3.2_image_2_5_pro",
        ]),
    );
    m.insert(
        "text2video".into(),
        set(&["3.0", "3.1", "3.2", "3.2_a", DRAMA_MODEL_VERSION]),
    );
    m.insert(
        "img2video".into(),
        set(&["3.0", "3.1", "3.2", "3.2_a", "3.4"]),
    );
    m.insert(
        "headtailimg2video".into(),
        set(&["3.0", "3.1", "3.2", "3.2_a"]),
    );
    m.insert(
        "reference2image".into(),
        set(&[
            "3.1",
            "3.2_fast_m",
            "3.2_pro_m",
            "3.2_image_2",
            "3.2_image_2_5_fast",
            "3.2_image_2_5_pro",
        ]),
    );
    m.insert(
        "character2video".into(),
        set(&[
            "3.0",
            "3.1",
            "3.1_pro",
            "3.2",
            "3.2_a",
            "3.4",
            DRAMA_MODEL_VERSION,
        ]),
    );
    m
}

fn sorted_join(s: &HashSet<String>) -> String {
    let mut v: Vec<&str> = s.iter().map(|x| x.as_str()).collect();
    v.sort();
    v.join(", ")
}

/// Validate model settings without depending on prompts or uploaded media.
/// Submit, cost and rerun share this check; account entitlements stay server-side.
pub fn validate_generation_settings(task_type: &str, settings: &Value) -> String {
    if !valid_task_types().contains(task_type) {
        return format!(
            "Invalid type '{}'. Valid: {}",
            task_type,
            sorted_join(&valid_task_types())
        );
    }
    if !settings.is_object() {
        return "settings must be an object".into();
    }
    let model = settings["model_version"].as_str().unwrap_or("");
    if !valid_model_versions().contains(model) {
        return format!(
            "Invalid model_version '{}'. Valid: {}",
            model,
            sorted_join(&valid_model_versions())
        );
    }
    let supports_type = if model == WAN_MODEL_VERSION {
        WAN_TASK_TYPES.contains(&task_type)
    } else {
        model_support()
            .get(task_type)
            .is_some_and(|models| models.contains(model))
    };
    if !supports_type {
        return format!("model_version {} does not support {}", model, task_type);
    }
    let sample_count = match settings.get("sample_count") {
        None => 1,
        Some(value) => match value.as_i64() {
            Some(count) if (1..=i64::from(i32::MAX)).contains(&count) => count,
            _ => return "sample_count must be a positive 32-bit integer".into(),
        },
    };
    if model == DRAMA_MODEL_VERSION && sample_count > 4 {
        return "Drama sample_count must be 1-4".into();
    }
    if let Some(codec) = settings.get("codec") {
        if !codec.as_str().is_some_and(|codec| CODECS.contains(&codec)) {
            return format!("Invalid codec. Valid: {}", CODECS.join(", "));
        }
    }
    let duration = match settings.get("duration") {
        None => 0,
        Some(value) => match value.as_i64() {
            Some(duration) if i32::try_from(duration).is_ok() => duration,
            _ => return "duration must be a 32-bit integer".into(),
        },
    };
    let resolution = settings["resolution"].as_str().unwrap_or("");
    if resolution.is_empty() {
        return format!("resolution is required for {}", task_type);
    }
    let transition = match settings.get("transition") {
        None => None,
        Some(value) => match value.as_str() {
            Some("") => None,
            Some(value) if TRANSITIONS.contains(&value) => Some(value),
            Some(value) => {
                return format!(
                    "Invalid transition '{}'. Valid: {}",
                    value,
                    TRANSITIONS.join(", ")
                )
            }
            None => return "transition must be a string".into(),
        },
    };
    let aspect_ratio = match settings.get("aspect_ratio") {
        None => None,
        Some(value) => match value.as_str() {
            Some(ratio) => Some(ratio),
            None => return "aspect_ratio must be a string".into(),
        },
    };
    if let Err(error) = preferred_schedule_mode(Some(model), settings["schedule_mode"].as_str()) {
        return error;
    }
    if model == WAN_MODEL_VERSION {
        return validate_wan_cost(
            task_type,
            duration,
            resolution,
            aspect_ratio.filter(|ratio| !ratio.is_empty()),
            transition,
            sample_count,
        );
    }
    if model == "3.2_a" {
        if duration != -1 && !(4..=15).contains(&duration) {
            return format!(
                "duration {} is invalid for 3.2_a: must be -1 or 4-15",
                duration
            );
        }
    } else if model == "3.2" && task_type == "character2video" && transition == Some("pro_ad") {
        if !(3..=16).contains(&duration) {
            return format!("duration {} out of range [3, 16] for 3.2 pro_ad", duration);
        }
    } else if let Some(&(min, max)) = duration_ranges()
        .get(task_type)
        .and_then(|ranges| ranges.get(model))
    {
        let valid = if min < 0 {
            duration == -1 || (2..=max).contains(&duration)
        } else if min == 0 {
            true
        }
        // The backend normalizes image duration to zero.
        else {
            (min..=max).contains(&duration)
        };
        if !valid {
            return format!(
                "duration {} out of range [{}, {}] for {} with {}",
                duration, min, max, task_type, model
            );
        }
    }
    let valid_resolutions = if model == DRAMA_MODEL_VERSION {
        set(&["480p", "720p", "1080p"])
    } else if model == "3.4" {
        set(Q4_RESOLUTIONS)
    } else if model == "3.2_a" {
        set(SD_RESOLUTIONS)
    } else {
        resolution_support().remove(task_type).unwrap_or_default()
    };
    if !valid_resolutions.contains(resolution) {
        return format!(
            "Invalid resolution '{}' for {}. Valid: {}",
            resolution,
            task_type,
            sorted_join(&valid_resolutions)
        );
    }
    if let Some(ratio) = aspect_ratio.filter(|ratio| !ratio.is_empty()) {
        let image_task = ["text2image", "reference2image"].contains(&task_type);
        let valid_ratio = if image_task && model == "3.1" {
            numeric_aspect_ratio(ratio).is_some_and(|ratio| (0.1..=10.0).contains(&ratio))
        } else if image_task {
            numeric_aspect_ratio(ratio).is_some()
        } else if model == "3.2_a" {
            SD_ASPECT_RATIOS.contains(&ratio)
        } else if model == DRAMA_MODEL_VERSION {
            valid_aspect_ratios().contains(ratio)
        } else {
            numeric_aspect_ratio(ratio).is_some()
        };
        if !valid_ratio {
            return format!(
                "Invalid aspect_ratio '{}' for {} with {}",
                ratio, task_type, model
            );
        }
    }
    // Transition affects trial matching, account gates and billing before some
    // generation builders ignore it. Preserve every recognized protocol value;
    // do not manufacture model-specific enum rejection or silently rewrite it.
    if task_type == "img2video" && ["3.0", "3.1", "3.2"].contains(&model) && transition.is_none() {
        return format!("{} with {} requires transition parameter", task_type, model);
    }
    if task_type == "character2video" && model == "3.2" && transition.is_none() {
        return "character2video with 3.2 requires transition parameter".into();
    }
    String::new()
}

pub fn validate_task_body(body: &Value) -> String {
    let task_type = body["type"].as_str().unwrap_or("");
    if task_type.is_empty() {
        return "Missing required field: type".into();
    }
    if !valid_task_types().contains(task_type) {
        return format!(
            "Invalid type '{}'. Valid: {}",
            task_type,
            sorted_join(&valid_task_types())
        );
    }
    let input = match body.get("input") {
        Some(value) if value.is_object() => value,
        _ => return "input must be an object".into(),
    };
    let prompts = match input["prompts"].as_array() {
        Some(prompts) if !prompts.is_empty() => prompts,
        _ => return "input.prompts is required and must not be empty".into(),
    };
    let settings = match body.get("settings") {
        Some(value) if value.is_object() => value,
        _ => return "settings must be an object".into(),
    };
    let error = validate_generation_settings(task_type, settings);
    if !error.is_empty() {
        return error;
    }
    let model = settings["model_version"].as_str().unwrap();
    let editor_mode = match input.get("editor_mode") {
        None | Some(Value::Null) => 0,
        Some(Value::String(mode)) => match mode.as_str() {
            "unspecified" => 0,
            "normal" => 1,
            "professional" => 2,
            _ => return format!("Invalid editor_mode '{}'", mode),
        },
        Some(mode) => match mode.as_i64().filter(|mode| i32::try_from(*mode).is_ok()) {
            Some(mode) => mode,
            None => return "editor_mode must be a protocol enum name or a 32-bit integer".into(),
        },
    };
    if editor_mode == 2 && model != "3.0" {
        return "professional editor_mode is only supported for model_version 3.0".into();
    }
    if model != DRAMA_MODEL_VERSION && model != WAN_MODEL_VERSION && prompts.len() > 20 {
        return "input.prompts allows at most 20 items".into();
    }
    let mut texts = 0usize;
    let mut images = 0usize;
    let mut materials = 0usize;
    let mut possible_image_material = false;
    let mut videos = 0usize;
    let mut audios = 0usize;
    for prompt in prompts {
        let Some(kind) = prompt.get("type").and_then(Value::as_str) else {
            return "Each prompt must be an object with a type".into();
        };
        match kind {
            "text" | "image" | "video" | "audio" => {
                let Some(content) = prompt["content"].as_str() else {
                    return format!("{} prompt content must be a string", kind);
                };
                if content.trim().is_empty() {
                    return format!("{} prompt content must not be empty", kind);
                }
                match kind {
                    "text" => texts += 1,
                    "image" => images += 1,
                    "video" => videos += 1,
                    "audio" => audios += 1,
                    _ => unreachable!(),
                }
            }
            "material" => {
                materials += 1;
                if [DRAMA_MODEL_VERSION, WAN_MODEL_VERSION].contains(&model) {
                    if model == WAN_MODEL_VERSION {
                        return "Wan 3 does not support 'material' prompt references".into();
                    }
                    return format!("{} does not support material references", model);
                }
                let Some(material) = prompt.get("material").filter(|value| value.is_object())
                else {
                    return "material prompt requires a material object".into();
                };
                for field in ["id", "version"] {
                    let value = &material[field];
                    let valid = value
                        .as_i64()
                        .or_else(|| value.as_str()?.parse::<i64>().ok())
                        .is_some_and(|value| value > 0);
                    if !valid {
                        return format!("material.{} must be a positive 64-bit integer", field);
                    }
                }
                if model == "3.4" {
                    match material["modality"].as_str() {
                        Some("text") => {}
                        Some("image" | "unspecified" | "") | None => possible_image_material = true,
                        _ => return "Q4 supports only image or text materials".into(),
                    }
                }
                // The server resolves material IDs and fills/trims their image
                // components. Neither one material nor historical contents is a
                // reliable local image count.
            }
            _ => return format!("{} does not support '{}' prompt references", model, kind),
        }
    }
    if ([DRAMA_MODEL_VERSION, WAN_MODEL_VERSION].contains(&model)
        || (model == "3.4" && task_type == "character2video"))
        && texts != 1
    {
        return format!("{} requires exactly one text prompt", model);
    }
    if task_type == "text2video" && (texts == 0 || images + videos + audios + materials > 0) {
        return "text2video accepts text prompts only and requires at least one text prompt; use character2video for references".into();
    }
    if input
        .get("enhance")
        .is_some_and(|value| !value.is_boolean())
    {
        return "input.enhance must be true or false".into();
    }
    if model == WAN_MODEL_VERSION {
        return validate_wan_task_body(body);
    }
    match task_type {
        "img2video" if images != 1 => {
            return format!("img2video requires exactly 1 image, got {}", images)
        }
        "headtailimg2video" if images != 2 => {
            return format!(
                "headtailimg2video requires exactly 2 images, got {}",
                images
            )
        }
        "character2video" => {
            if texts == 0 {
                return "character2video requires at least one text prompt".into();
            }
            let max = match model {
                "3.4" => Some(15),
                "3.2_a" | DRAMA_MODEL_VERSION => Some(9),
                "3.1_pro" => None, // The server trims reference media to 48 slots.
                _ if materials > 0 => Some(7),
                _ => None,
            };
            if let Some(max) = max {
                if images > max {
                    return format!("character2video allows max {} images, got {}", max, images);
                }
            }
            let has_reference = if model == "3.4" {
                images + audios > 0 || possible_image_material
            } else if model == DRAMA_MODEL_VERSION {
                images + videos + audios > 0
            } else {
                images + videos + materials > 0
            };
            if !has_reference {
                return format!("{} character2video requires at least one image, video or audio reference (image materials are resolved by the server)", model);
            }
        }
        "reference2image" if materials > 0 && images > 7 => {
            return format!("reference2image allows max 7 images, got {}", images);
        }
        _ => {}
    }
    let error = validate_reference_video_count(task_type, model, videos);
    if !error.is_empty() {
        return error;
    }
    let error = validate_reference_audio_count(task_type, model, audios);
    if !error.is_empty() {
        return error;
    }
    String::new()
}

pub fn preferred_schedule_mode(
    model_version: Option<&str>,
    explicit: Option<&str>,
) -> Result<Option<String>, String> {
    let mode = explicit.or_else(|| {
        [
            Some("3.4"),
            Some(DRAMA_MODEL_VERSION),
            Some(WAN_MODEL_VERSION),
        ]
        .contains(&model_version)
        .then_some("normal")
    });
    if let Some(mode) = mode {
        let err = validate_schedule_mode(mode);
        if !err.is_empty() {
            return Err(err);
        }
        if model_version == Some("3.4") && mode == "claw_pass" {
            return Err(
                "Q4 does not support claw_pass; use --schedule-mode normal or nopeak".into(),
            );
        }
        if model_version == Some("3.2_a") && mode == "nopeak" {
            return Err("3.2_a does not support nopeak schedule mode".into());
        }
        if model_version == Some(DRAMA_MODEL_VERSION) && mode != "normal" {
            return Err("viduq3.1-drama-std supports only --schedule-mode normal (credits)".into());
        }
        if model_version == Some(WAN_MODEL_VERSION) && mode != "normal" {
            return Err("Wan 3 supports only --schedule-mode normal (credits)".into());
        }
    }
    Ok(mode.map(str::to_string))
}

#[cfg(test)]
pub fn validate_q4_cost(
    task_type: &str,
    duration: i64,
    resolution: &str,
    transition: Option<&str>,
) -> String {
    let mut settings = serde_json::json!({
        "model_version": "3.4", "duration": duration, "resolution": resolution,
    });
    if let Some(transition) = transition {
        settings["transition"] = transition.into();
    }
    validate_generation_settings(task_type, &settings)
}

#[cfg(test)]
pub fn validate_drama_cost(
    task_type: &str,
    duration: i64,
    resolution: &str,
    transition: Option<&str>,
) -> String {
    let mut settings = serde_json::json!({
        "model_version": DRAMA_MODEL_VERSION, "duration": duration, "resolution": resolution,
    });
    if let Some(transition) = transition {
        settings["transition"] = transition.into();
    }
    validate_generation_settings(task_type, &settings)
}

pub fn validate_wan_cost(
    task_type: &str,
    duration: i64,
    resolution: &str,
    aspect_ratio: Option<&str>,
    transition: Option<&str>,
    sample_count: i64,
) -> String {
    if !WAN_TASK_TYPES.contains(&task_type) {
        return format!(
            "model_version {} does not support {}",
            WAN_MODEL_VERSION, task_type
        );
    }
    if !(2..=30).contains(&duration) {
        return format!(
            "Wan 3 duration {} out of range [2, 30]; automatic duration is not supported",
            duration
        );
    }
    if !["480p", "720p", "1080p"].contains(&resolution) {
        return format!("Invalid resolution '{}' for Wan 3", resolution);
    }
    if aspect_ratio.is_some_and(|ratio| !WAN_ASPECT_RATIOS.contains(&ratio)) {
        return "Invalid aspect_ratio for Wan 3".into();
    }
    if transition
        .is_some_and(|transition| !transition.is_empty() && !TRANSITIONS.contains(&transition))
    {
        return "Invalid transition protocol value".into();
    }
    if !(1..=4).contains(&sample_count) {
        return "Wan 3 sample_count must be 1-4".into();
    }
    String::new()
}

fn validate_wan_task_body(body: &Value) -> String {
    let settings = &body["settings"];
    let task_type = body["type"].as_str().unwrap_or("");
    let error = validate_wan_cost(
        task_type,
        settings["duration"].as_i64().unwrap_or(0),
        settings["resolution"].as_str().unwrap_or(""),
        settings["aspect_ratio"].as_str(),
        settings["transition"].as_str(),
        settings["sample_count"].as_i64().unwrap_or(1),
    );
    if !error.is_empty() {
        return error;
    }
    if let Err(error) =
        preferred_schedule_mode(Some(WAN_MODEL_VERSION), settings["schedule_mode"].as_str())
    {
        return error;
    }
    let input = &body["input"];
    if input
        .get("enhance")
        .is_some_and(|value| !value.is_boolean())
    {
        return "input.enhance must be true or false".into();
    }
    let mut counts = [0usize; 4];
    for prompt in input["prompts"].as_array().unwrap() {
        let kind = prompt["type"].as_str().unwrap_or("");
        let Some(index) = ["text", "image", "video", "audio"]
            .iter()
            .position(|&value| value == kind)
        else {
            return format!("Wan 3 does not support '{}' prompt references", kind);
        };
        if prompt["content"].as_str().unwrap_or("").trim().is_empty() {
            return "Wan 3 prompt content must not be empty".into();
        }
        counts[index] += 1;
    }
    let [text, images, videos, audios] = counts;
    if text != 1 {
        return "Wan 3 requires exactly one text prompt".into();
    }
    if images > 10 || videos > 5 || audios > 5 || images + videos + audios > 20 {
        return "Wan 3 allows max 10 images, 5 videos, 5 audios and 20 media inputs total".into();
    }
    let media_count = images + videos + audios;
    match task_type {
        "text2video" if media_count > 0 => {
            "Wan 3 text2video accepts text prompts only; use character2video for references".into()
        }
        "img2video" if images != 1 || videos + audios > 0 => {
            "Wan 3 img2video requires exactly 1 image and no video/audio references".into()
        }
        "headtailimg2video" if images != 2 || videos + audios > 0 => {
            "Wan 3 headtailimg2video requires exactly 2 images and no video/audio references".into()
        }
        "character2video" if media_count == 0 => {
            "Wan 3 character2video requires at least one image, video or audio reference".into()
        }
        _ => String::new(),
    }
}

pub fn validate_schedule_mode(mode: &str) -> String {
    if !SCHEDULE_MODES.contains(&mode) {
        return format!(
            "Invalid schedule_mode '{}'. Valid: {}",
            mode,
            SCHEDULE_MODES.join(", ")
        );
    }
    String::new()
}

pub fn validate_element_preprocess(body: &Value) -> String {
    if !body.is_object() {
        return "body must be an object".into();
    }
    if body.get("components").is_none() {
        return "Missing required field: components".into();
    }
    if body.get("name").is_none() {
        return "Missing required field: name".into();
    }
    if body.get("type").is_none() {
        return "Missing required field: type".into();
    }
    let components = match body.get("components") {
        Some(Value::Array(arr)) if !arr.is_empty() => arr,
        _ => return "components must be a non-empty array".into(),
    };
    if components.len() > 3 {
        return "components must have at most 3 items".into();
    }
    let main_count = components
        .iter()
        .filter(|c| c.get("type").and_then(|v| v.as_str()) == Some("main"))
        .count();
    if main_count != 1 {
        return "components must have exactly one item with type='main'".into();
    }
    String::new()
}

pub fn validate_image_file(path: &str) -> String {
    let p = Path::new(path);
    if !p.is_file() {
        return format!("Image file not found: {}", path);
    }
    String::new()
}

pub fn validate_video_file(path: &str) -> String {
    let p = Path::new(path);
    if !p.is_file() {
        return format!("Video file not found: {}", path);
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["mp4", "mov", "avi"].contains(&ext.as_str()) {
        return format!("Invalid video format '{}'. Supported: mp4, mov, avi", ext);
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > 500 * 1024 * 1024 {
        return format!(
            "Video file too large ({:.1}MB). Max: 500MB",
            size as f64 / 1024.0 / 1024.0
        );
    }
    String::new()
}

pub fn validate_audio_file(path: &str) -> String {
    let p = Path::new(path);
    if !p.is_file() {
        return format!("Audio file not found: {}", path);
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["mp3", "wav", "aac", "m4a"].contains(&ext.as_str()) {
        return format!(
            "Invalid audio format '{}'. Supported: mp3, wav, aac, m4a",
            ext
        );
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > 100 * 1024 * 1024 {
        return format!(
            "Audio file too large ({:.1}MB). Max: 100MB",
            size as f64 / 1024.0 / 1024.0
        );
    }
    String::new()
}

pub fn validate_reference_audio_count(
    task_type: &str,
    model_version: &str,
    count: usize,
) -> String {
    if count == 0 {
        return String::new();
    }
    if task_type != "character2video"
        || !["3.2_a", "3.4", DRAMA_MODEL_VERSION, WAN_MODEL_VERSION].contains(&model_version)
    {
        return "Audio input is only supported for character2video with model_version 3.2_a, 3.4, viduq3.1-drama-std, or viduq2-drama-std".into();
    }
    let max_count = if model_version == DRAMA_MODEL_VERSION {
        10
    } else if model_version == WAN_MODEL_VERSION {
        5
    } else {
        3
    };
    if count > max_count {
        return format!("Too many audio inputs: {}. Max: {}", count, max_count);
    }
    String::new()
}

pub fn validate_reference_video_count(
    task_type: &str,
    model_version: &str,
    count: usize,
) -> String {
    if count == 0 {
        return String::new();
    }
    if task_type != "character2video"
        || !["3.1_pro", "3.2_a", DRAMA_MODEL_VERSION, WAN_MODEL_VERSION].contains(&model_version)
    {
        return "Video input is only supported for character2video with model_version 3.1_pro, 3.2_a, viduq3.1-drama-std, or viduq2-drama-std".into();
    }
    if model_version == "3.1_pro" {
        // The workflow trims video/image references to its 48-slot budget.
        return String::new();
    }
    let max_count = if model_version == WAN_MODEL_VERSION {
        5
    } else {
        3
    };
    if count > max_count {
        return format!("Too many video inputs: {}. Max: {}", count, max_count);
    }
    String::new()
}

#[cfg(test)]
pub fn validate_reference_audio_file(path: &str) -> String {
    validate_reference_audio_file_for_model(path, DRAMA_MODEL_VERSION)
}

pub fn validate_reference_audio_file_for_model(path: &str, model_version: &str) -> String {
    let p = Path::new(path);
    if !p.is_file() {
        return format!("Audio file not found: {}", path);
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if model_version != "3.2_a" && !["mp3", "wav"].contains(&ext.as_str()) {
        return format!("Invalid audio format '{}'. Supported: mp3, wav", ext);
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if std::fs::File::open(p).is_err() {
        return format!("Cannot read audio file: {}", path);
    }
    let max_mb = if ["3.4", "3.2_a"].contains(&model_version) {
        20
    } else {
        15
    };
    if size == 0 || size > max_mb * 1024 * 1024 {
        return format!(
            "Audio file size must be greater than zero and at most {}MB (got {:.1}MB)",
            max_mb,
            size as f64 / 1024.0 / 1024.0
        );
    }
    String::new()
}

pub fn validate_reference_video_file(path: &str) -> String {
    let p = Path::new(path);
    if !p.is_file() {
        return format!("Video file not found: {}", path);
    }
    if std::fs::File::open(p).is_err() {
        return format!("Cannot read video file: {}", path);
    }
    String::new()
}

pub fn validate_wan_media_dimensions(width: u32, height: u32, max: u32) -> Result<(), String> {
    if width < 240
        || height < 240
        || width > max
        || height > max
        || u64::from(width) > 8 * u64::from(height)
        || u64::from(height) > 8 * u64::from(width)
    {
        return Err(format!(
            "Wan 3 media dimensions {}x{} require sides 240-{} and aspect ratio 1:8-8:1",
            width, height, max
        ));
    }
    Ok(())
}

#[cfg(test)]
pub fn validate_wan_image_file(path: &str) -> String {
    let p = Path::new(path);
    let size = match std::fs::metadata(p) {
        Ok(metadata) => metadata.len(),
        Err(_) => return format!("Image file not found: {}", path),
    };
    if size == 0 || size > 20 * 1024 * 1024 {
        return "Wan 3 image size must be greater than zero and at most 20MB".into();
    }
    let reader = match image::ImageReader::open(p).and_then(|reader| reader.with_guessed_format()) {
        Ok(reader) => reader,
        Err(_) => return "Cannot read Wan 3 image format".into(),
    };
    if !matches!(
        reader.format(),
        Some(
            image::ImageFormat::Jpeg
                | image::ImageFormat::Png
                | image::ImageFormat::Bmp
                | image::ImageFormat::WebP
        )
    ) {
        return "Wan 3 images must be JPEG, PNG, BMP or WEBP".into();
    }
    match reader.into_dimensions() {
        Ok((width, height)) => validate_wan_media_dimensions(width, height, 8000)
            .err()
            .unwrap_or_default(),
        Err(_) => "Cannot read Wan 3 image dimensions".into(),
    }
}

pub fn validate_wan_video_file(path: &str) -> String {
    let p = Path::new(path);
    if !p.is_file() {
        return format!("Video file not found: {}", path);
    }
    if std::fs::File::open(p).is_err() {
        return format!("Cannot read video file: {}", path);
    }
    let size = std::fs::metadata(p)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if size == 0 || size > 100 * 1024 * 1024 {
        return "Wan 3 video size must be greater than zero and at most 100MB".into();
    }
    String::new()
}

pub fn validate_lip_sync_text(text: &str) -> String {
    if text.is_empty() {
        return "Text content cannot be empty".into();
    }
    let has_cjk = text.chars().any(|c| {
        ('\u{4E00}'..='\u{9FFF}').contains(&c)
            || ('\u{3400}'..='\u{4DBF}').contains(&c)
            || ('\u{F900}'..='\u{FAFF}').contains(&c)
    });
    let char_count = text.chars().count();
    if has_cjk {
        if !(2..=1000).contains(&char_count) {
            return format!("Chinese text must be 2-1000 characters, got {}", char_count);
        }
    } else if !(4..=2000).contains(&char_count) {
        return format!("English text must be 4-2000 characters, got {}", char_count);
    }
    String::new()
}

pub fn validate_lip_sync_speed(speed: f64) -> String {
    if !(0.5..=2.0).contains(&speed) {
        return format!("speed must be between 0.5 and 2.0, got {}", speed);
    }
    String::new()
}

pub fn validate_lip_sync_volume(volume: f64) -> String {
    if !(0.1..=2.0).contains(&volume) {
        return format!("volume must be between 0.1 and 2.0, got {}", volume);
    }
    String::new()
}

pub fn tts_voices_grouped() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "English",
            vec![
                "English_expressive_narrator",
                "English_radiant_girl",
                "English_magnetic_voiced_man",
                "English_compelling_lady1",
                "English_Aussie_Bloke",
                "English_captivating_female1",
                "English_Upbeat_Woman",
                "English_Trustworth_Man",
                "English_CalmWoman",
                "English_UpsetGirl",
                "English_Gentle-voiced_man",
                "English_Whispering_girl",
                "English_Diligent_Man",
                "English_Graceful_Lady",
                "English_ReservedYoungMan",
                "English_PlayfulGirl",
                "English_ManWithDeepVoice",
                "English_MaturePartner",
                "English_FriendlyPerson",
                "English_MatureBoss",
                "English_Debator",
                "English_LovelyGirl",
                "English_Steadymentor",
                "English_Deep-VoicedGentleman",
                "English_Wiselady",
                "English_CaptivatingStoryteller",
                "English_DecentYoungMan",
                "English_SentimentalLady",
                "English_ImposingManner",
                "English_SadTeen",
                "English_PassionateWarrior",
                "English_WiseScholar",
                "English_Soft-spokenGirl",
                "English_SereneWoman",
                "English_ConfidentWoman",
                "English_PatientMan",
                "English_Comedian",
                "English_BossyLeader",
                "English_Strong-WilledBoy",
                "English_StressedLady",
                "English_AssertiveQueen",
                "English_AnimeCharacter",
                "English_Jovialman",
                "English_WhimsicalGirl",
                "English_Kind-heartedGirl",
            ],
        ),
        (
            "Chinese (Mandarin)",
            vec![
                "Chinese (Mandarin)_Reliable_Executive",
                "Chinese (Mandarin)_News_Anchor",
                "Chinese (Mandarin)_Unrestrained_Young_Man",
                "Chinese (Mandarin)_Mature_Woman",
                "Arrogant_Miss",
                "Robot_Armor",
                "Chinese (Mandarin)_Kind-hearted_Antie",
                "Chinese (Mandarin)_HK_Flight_Attendant",
                "Chinese (Mandarin)_Humorous_Elder",
                "Chinese (Mandarin)_Gentleman",
                "Chinese (Mandarin)_Warm_Bestie",
                "Chinese (Mandarin)_Stubborn_Friend",
                "Chinese (Mandarin)_Sweet_Lady",
                "Chinese (Mandarin)_Southern_Young_Man",
                "Chinese (Mandarin)_Wise_Women",
                "Chinese (Mandarin)_Gentle_Youth",
                "Chinese (Mandarin)_Warm_Girl",
                "Chinese (Mandarin)_Male_Announcer",
                "Chinese (Mandarin)_Kind-hearted_Elder",
                "Chinese (Mandarin)_Cute_Spirit",
                "Chinese (Mandarin)_Radio_Host",
                "Chinese (Mandarin)_Lyrical_Voice",
                "Chinese (Mandarin)_Straightforward_Boy",
                "Chinese (Mandarin)_Sincere_Adult",
                "Chinese (Mandarin)_Gentle_Senior",
                "Chinese (Mandarin)_Crisp_Girl",
                "Chinese (Mandarin)_Pure-hearted_Boy",
                "Chinese (Mandarin)_Soft_Girl",
                "Chinese (Mandarin)_IntellectualGirl",
                "Chinese (Mandarin)_Warm_HeartedGirl",
                "Chinese (Mandarin)_Laid_BackGirl",
                "Chinese (Mandarin)_ExplorativeGirl",
                "Chinese (Mandarin)_Warm-HeartedAunt",
                "Chinese (Mandarin)_BashfulGirl",
            ],
        ),
        (
            "Japanese",
            vec![
                "Japanese_IntellectualSenior",
                "Japanese_DecisivePrincess",
                "Japanese_LoyalKnight",
                "Japanese_DominantMan",
                "Japanese_SeriousCommander",
                "Japanese_ColdQueen",
                "Japanese_DependableWoman",
                "Japanese_GentleButler",
                "Japanese_KindLady",
                "Japanese_CalmLady",
                "Japanese_OptimisticYouth",
                "Japanese_GenerousIzakayaOwner",
                "Japanese_SportyStudent",
                "Japanese_InnocentBoy",
                "Japanese_GracefulMaiden",
            ],
        ),
        (
            "Cantonese",
            vec![
                "Cantonese_ProfessionalHost (F)",
                "Cantonese_GentleLady",
                "Cantonese_ProfessionalHost (M)",
                "Cantonese_PlayfulMan",
                "Cantonese_CuteGirl",
                "Cantonese_KindWoman",
            ],
        ),
        (
            "Korean",
            vec![
                "Korean_AirheadedGirl",
                "Korean_AthleticGirl",
                "Korean_AthleticStudent",
                "Korean_BraveAdventurer",
                "Korean_BraveFemaleWarrior",
                "Korean_BraveYouth",
                "Korean_CalmGentleman",
                "Korean_CalmLady",
                "Korean_CaringWoman",
                "Korean_CharmingElderSister",
                "Korean_CharmingSister",
                "Korean_CheerfulBoyfriend",
                "Korean_CheerfulCoolJunior",
                "Korean_CheerfulLittleSister",
                "Korean_ChildhoodFriendGirl",
                "Korean_CockyGuy",
                "Korean_ColdGirl",
                "Korean_ColdYoungMan",
                "Korean_ConfidentBoss",
                "Korean_ConsiderateSenior",
                "Korean_DecisiveQueen",
                "Korean_DominantMan",
                "Korean_ElegantPrincess",
                "Korean_EnchantingSister",
                "Korean_EnthusiasticTeen",
                "Korean_FriendlyBigSister",
                "Korean_GentleBoss",
                "Korean_GentleWoman",
                "Korean_HaughtyLady",
                "Korean_InnocentBoy",
                "Korean_IntellectualMan",
                "Korean_IntellectualSenior",
                "Korean_LonelyWarrior",
                "Korean_MatureLady",
                "Korean_MysteriousGirl",
                "Korean_OptimisticYouth",
                "Korean_PlayboyCharmer",
                "Korean_PossessiveMan",
                "Korean_QuirkyGirl",
                "Korean_ReliableSister",
                "Korean_ReliableYouth",
                "Korean_SassyGirl",
                "Korean_ShyGirl",
                "Korean_SoothingLady",
                "Korean_StrictBoss",
                "Korean_SweetGirl",
                "Korean_ThoughtfulWoman",
                "Korean_WiseElf",
                "Korean_WiseTeacher",
            ],
        ),
        (
            "Spanish",
            vec![
                "Spanish_SereneWoman",
                "Spanish_MaturePartner",
                "Spanish_CaptivatingStoryteller",
                "Spanish_Narrator",
                "Spanish_WiseScholar",
                "Spanish_Kind-heartedGirl",
                "Spanish_DeterminedManager",
                "Spanish_BossyLeader",
                "Spanish_ReservedYoungMan",
                "Spanish_ConfidentWoman",
                "Spanish_ThoughtfulMan",
                "Spanish_Strong-WilledBoy",
                "Spanish_SophisticatedLady",
                "Spanish_RationalMan",
                "Spanish_AnimeCharacter",
                "Spanish_Deep-tonedMan",
                "Spanish_Fussyhostess",
                "Spanish_SincereTeen",
                "Spanish_FrankLady",
                "Spanish_Comedian",
                "Spanish_Debator",
                "Spanish_ToughBoss",
                "Spanish_Wiselady",
                "Spanish_Steadymentor",
                "Spanish_Jovialman",
                "Spanish_SantaClaus",
                "Spanish_Rudolph",
                "Spanish_Intonategirl",
                "Spanish_Arnold",
                "Spanish_Ghost",
                "Spanish_HumorousElder",
                "Spanish_EnergeticBoy",
                "Spanish_WhimsicalGirl",
                "Spanish_StrictBoss",
                "Spanish_ReliableMan",
                "Spanish_SereneElder",
                "Spanish_AngryMan",
                "Spanish_AssertiveQueen",
                "Spanish_CaringGirlfriend",
                "Spanish_PowerfulSoldier",
                "Spanish_PassionateWarrior",
                "Spanish_ChattyGirl",
                "Spanish_RomanticHusband",
                "Spanish_CompellingGirl",
                "Spanish_PowerfulVeteran",
                "Spanish_SensibleManager",
                "Spanish_ThoughtfulLady",
            ],
        ),
        (
            "Portuguese",
            vec![
                "Portuguese_SentimentalLady",
                "Portuguese_BossyLeader",
                "Portuguese_Wiselady",
                "Portuguese_Strong-WilledBoy",
                "Portuguese_Deep-VoicedGentleman",
                "Portuguese_UpsetGirl",
                "Portuguese_PassionateWarrior",
                "Portuguese_AnimeCharacter",
                "Portuguese_ConfidentWoman",
                "Portuguese_AngryMan",
                "Portuguese_CaptivatingStoryteller",
                "Portuguese_Godfather",
                "Portuguese_ReservedYoungMan",
                "Portuguese_SmartYoungGirl",
                "Portuguese_Kind-heartedGirl",
                "Portuguese_Pompouslady",
                "Portuguese_Grinch",
                "Portuguese_Debator",
                "Portuguese_SweetGirl",
                "Portuguese_AttractiveGirl",
                "Portuguese_ThoughtfulMan",
                "Portuguese_PlayfulGirl",
                "Portuguese_GorgeousLady",
                "Portuguese_LovelyLady",
                "Portuguese_SereneWoman",
                "Portuguese_SadTeen",
                "Portuguese_MaturePartner",
                "Portuguese_Comedian",
                "Portuguese_NaughtySchoolgirl",
                "Portuguese_Narrator",
                "Portuguese_ToughBoss",
                "Portuguese_Fussyhostess",
                "Portuguese_Dramatist",
                "Portuguese_Steadymentor",
                "Portuguese_Jovialman",
                "Portuguese_CharmingQueen",
                "Portuguese_SantaClaus",
                "Portuguese_Rudolph",
                "Portuguese_Arnold",
                "Portuguese_CharmingSanta",
                "Portuguese_CharmingLady",
                "Portuguese_Ghost",
                "Portuguese_HumorousElder",
                "Portuguese_CalmLeader",
                "Portuguese_GentleTeacher",
                "Portuguese_EnergeticBoy",
                "Portuguese_ReliableMan",
                "Portuguese_SereneElder",
                "Portuguese_GrimReaper",
                "Portuguese_AssertiveQueen",
                "Portuguese_WhimsicalGirl",
                "Portuguese_StressedLady",
                "Portuguese_FriendlyNeighbor",
                "Portuguese_CaringGirlfriend",
                "Portuguese_PowerfulSoldier",
                "Portuguese_FascinatingBoy",
                "Portuguese_RomanticHusband",
                "Portuguese_StrictBoss",
                "Portuguese_InspiringLady",
                "Portuguese_PlayfulSpirit",
                "Portuguese_ElegantGirl",
                "Portuguese_CompellingGirl",
                "Portuguese_PowerfulVeteran",
                "Portuguese_SensibleManager",
                "Portuguese_ThoughtfulLady",
                "Portuguese_TheatricalActor",
                "Portuguese_FragileBoy",
                "Portuguese_ChattyGirl",
                "Portuguese_Conscientiousinstructor",
                "Portuguese_RationalMan",
                "Portuguese_WiseScholar",
                "Portuguese_FrankLady",
                "Portuguese_DeterminedManager",
            ],
        ),
        (
            "French",
            vec![
                "French_Male_Speech_New",
                "French_Female_News Anchor",
                "French_CasualMan",
                "French_MovieLeadFemale",
                "French_FemaleAnchor",
                "French_MaleNarrator",
            ],
        ),
        (
            "Indonesian",
            vec![
                "Indonesian_SweetGirl",
                "Indonesian_ReservedYoungMan",
                "Indonesian_CharmingGirl",
                "Indonesian_CalmWoman",
                "Indonesian_ConfidentWoman",
                "Indonesian_CaringMan",
                "Indonesian_BossyLeader",
                "Indonesian_DeterminedBoy",
                "Indonesian_GentleGirl",
            ],
        ),
        (
            "German",
            vec![
                "German_FriendlyMan",
                "German_SweetLady",
                "German_PlayfulMan",
            ],
        ),
        (
            "Russian",
            vec![
                "Russian_HandsomeChildhoodFriend",
                "Russian_BrightHeroine",
                "Russian_AmbitiousWoman",
                "Russian_ReliableMan",
                "Russian_CrazyQueen",
                "Russian_PessimisticGirl",
                "Russian_AttractiveGuy",
                "Russian_Bad-temperedBoy",
            ],
        ),
        (
            "Italian",
            vec![
                "Italian_BraveHeroine",
                "Italian_Narrator",
                "Italian_WanderingSorcerer",
                "Italian_DiligentLeader",
            ],
        ),
        (
            "Dutch",
            vec!["Dutch_kindhearted_girl", "Dutch_bossy_leader"],
        ),
        ("Vietnamese", vec!["Vietnamese_kindhearted_girl"]),
        ("Arabic", vec!["Arabic_CalmWoman", "Arabic_FriendlyGuy"]),
        (
            "Turkish",
            vec!["Turkish_CalmWoman", "Turkish_Trustworthyman"],
        ),
        (
            "Ukrainian",
            vec!["Ukrainian_CalmWoman", "Ukrainian_WiseScholar"],
        ),
        (
            "Thai",
            vec![
                "Thai_male_1_sample8",
                "Thai_male_2_sample2",
                "Thai_female_1_sample1",
                "Thai_female_2_sample2",
            ],
        ),
        (
            "Polish",
            vec![
                "Polish_male_1_sample4",
                "Polish_male_2_sample3",
                "Polish_female_1_sample1",
                "Polish_female_2_sample3",
            ],
        ),
        (
            "Romanian",
            vec![
                "Romanian_male_1_sample2",
                "Romanian_male_2_sample1",
                "Romanian_female_1_sample4",
                "Romanian_female_2_sample1",
            ],
        ),
        (
            "Greek",
            vec![
                "greek_male_1a_v1",
                "Greek_female_1_sample1",
                "Greek_female_2_sample3",
            ],
        ),
        (
            "Czech",
            vec!["czech_male_1_v1", "czech_female_5_v7", "czech_female_2_v2"],
        ),
        (
            "Finnish",
            vec![
                "finnish_male_3_v1",
                "finnish_male_1_v2",
                "finnish_female_4_v1",
            ],
        ),
        (
            "Hindi",
            vec!["hindi_male_1_v2", "hindi_female_2_v1", "hindi_female_1_v2"],
        ),
    ]
}

pub fn all_tts_voice_ids() -> Vec<&'static str> {
    tts_voices_grouped()
        .into_iter()
        .flat_map(|(_, v)| v)
        .collect()
}

pub fn validate_tts_voice_id(voice_id: &str) -> String {
    let valid: HashSet<String> = all_tts_voice_ids().iter().map(|s| s.to_string()).collect();
    let is_personal_voice = voice_id.strip_prefix("voice_").is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            && suffix.bytes().any(|b| b.is_ascii_alphanumeric())
    });
    if !valid.contains(voice_id) && !is_personal_voice {
        return format!(
            "Invalid voice_id '{}'. Use 'vidu-cli task tts-voices' for official voices or 'vidu-cli voice list' for personal voices.",
            voice_id
        );
    }
    String::new()
}

pub fn validate_tts_speed(speed: f64) -> String {
    if !(0.5..=2.0).contains(&speed) {
        return format!("speed must be between 0.5 and 2.0, got {}", speed);
    }
    String::new()
}

pub fn validate_tts_emotion(emotion: &str) -> String {
    if emotion.chars().count() > 100 {
        return "Emotion description too long. Maximum: 100 characters".to_string();
    }
    String::new()
}

pub fn validate_tts_volume(vol: i32) -> String {
    if !(0..=100).contains(&vol) {
        return format!("volume must be between 0 and 100, got {}", vol);
    }
    String::new()
}

pub fn validate_tts_language_boost(lang: &str) -> String {
    let valid = set(LANGUAGE_BOOSTS);
    if !valid.contains(lang) {
        let mut list: Vec<&str> = valid.iter().map(|s| s.as_str()).collect();
        list.sort();
        return format!(
            "Invalid language_boost '{}'. Valid: {}",
            lang,
            list.join(", ")
        );
    }
    String::new()
}

pub fn validate_timeline_clips(timeline: &Value) -> String {
    let track_clip_pairs = [
        ("video_tracks", "video_track_clips"),
        ("audio_tracks", "audio_track_clips"),
        ("subtitle_tracks", "subtitle_track_clips"),
    ];

    for (track_key, clip_key) in &track_clip_pairs {
        if let Some(tracks) = timeline.get(*track_key).and_then(|v| v.as_array()) {
            for (ti, track) in tracks.iter().enumerate() {
                if let Some(clips) = track.get(*clip_key).and_then(|v| v.as_array()) {
                    for (ci, clip) in clips.iter().enumerate() {
                        if clip.get("timeline_in").is_none_or(|v| v.is_null()) {
                            return format!(
                                "{}[{}].{}[{}]: missing required field 'timeline_in'",
                                track_key, ti, clip_key, ci
                            );
                        }
                        if clip.get("timeline_out").is_none_or(|v| v.is_null()) {
                            return format!(
                                "{}[{}].{}[{}]: missing required field 'timeline_out'",
                                track_key, ti, clip_key, ci
                            );
                        }
                    }
                }
            }
        }
    }

    if let Some(tracks) = timeline.get("effect_tracks").and_then(|v| v.as_array()) {
        for (ti, track) in tracks.iter().enumerate() {
            if let Some(items) = track.get("effect_track_items").and_then(|v| v.as_array()) {
                for (ci, item) in items.iter().enumerate() {
                    if item.get("timeline_in").is_none_or(|v| v.is_null()) {
                        return format!("effect_tracks[{}].effect_track_items[{}]: missing required field 'timeline_in'", ti, ci);
                    }
                    if item.get("timeline_out").is_none_or(|v| v.is_null()) {
                        return format!("effect_tracks[{}].effect_track_items[{}]: missing required field 'timeline_out'", ti, ci);
                    }
                }
            }
        }
    }

    String::new()
}

/// Read and validate a prompt file at `path`.
///
/// Files must exist, be UTF-8, and not exceed 1 MiB. A single trailing
/// newline (`\n` or `\r\n`) is stripped so editors that auto-append one do
/// not change the perceived prompt length.
pub fn read_prompt_file(path: &str) -> Result<String, String> {
    const MAX_PROMPT_FILE_BYTES: u64 = 1024 * 1024;

    let p = Path::new(path);
    if !p.is_file() {
        return Err(format!("Prompt file not found: {}", path));
    }
    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    if size > MAX_PROMPT_FILE_BYTES {
        return Err(format!(
            "Prompt file too large: {} ({:.1}MiB). Max: 1MiB",
            path,
            size as f64 / 1024.0 / 1024.0
        ));
    }
    let bytes = match std::fs::read(p) {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read prompt file {}: {}", path, e)),
    };
    let mut content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return Err(format!("Prompt file is not valid UTF-8: {}", path)),
    };
    if content.ends_with("\r\n") {
        content.truncate(content.len() - 2);
    } else if content.ends_with('\n') || content.ends_with('\r') {
        content.truncate(content.len() - 1);
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_body(
        task_type: &str,
        model: &str,
        duration: i64,
        images: usize,
        materials: usize,
    ) -> Value {
        let mut prompts: Vec<Value> = vec![json!({"type": "text", "content": "test"})];
        for _ in 0..images {
            prompts.push(json!({"type": "image", "content": "ssupload:?id=x"}));
        }
        for _ in 0..materials {
            prompts.push(
                json!({"type": "material", "name": "m", "material": {"id": "1", "version": "1"}}),
            );
        }
        json!({
            "type": task_type,
            "input": { "prompts": prompts, "enhance": true },
            "settings": {
                "duration": duration,
                "model_version": model,
                "resolution": "1080p",
                "schedule_mode": "normal",
            }
        })
    }

    #[test]
    fn text_counts_follow_task_specific_backend_rules() {
        let mut frame = make_body("img2video", "3.4", 5, 1, 0);
        frame["input"]["prompts"] = json!([
            {"type":"text", "content":"scene"},
            {"type":"text", "content":"blur", "negative":true},
            {"type":"image", "content":"ssupload:?id=1"}
        ]);
        assert_eq!(validate_task_body(&frame), "");
        frame["input"]["prompts"] = json!([
            {"type":"image", "content":"ssupload:?id=1"}
        ]);
        assert_eq!(validate_task_body(&frame), "");
        frame["type"] = json!("character2video");
        assert!(validate_task_body(&frame).contains("exactly one text"));

        for model in ["3.0", "3.1", "3.1_pro", "3.2", "3.2_a"] {
            let mut reference = make_body("character2video", model, 5, 1, 0);
            if model == "3.2" {
                reference["settings"]["transition"] = json!("speed");
            }
            reference["input"]["prompts"] = json!([
                {"type":"image", "content":"ssupload:?id=1"}
            ]);
            assert!(
                validate_task_body(&reference).contains("at least one text"),
                "{model}"
            );
        }
    }

    #[test]
    fn reference_image_limits_follow_models_and_server_material_expansion() {
        for (model, max) in [("3.4", 15), ("3.2_a", 9)] {
            assert_eq!(
                validate_task_body(&make_body("character2video", model, 5, max, 0)),
                ""
            );
            assert!(
                validate_task_body(&make_body("character2video", model, 5, max + 1, 0))
                    .contains(&format!("max {} images", max))
            );
        }
        let mut q4 = make_body("character2video", "3.4", 5, 0, 1);
        // A material ID must be resolved by unidu. A historical contents array
        // cannot establish its current type or number of image components.
        q4["input"]["prompts"][1]["material"]["contents"] = json!(vec!["stale"; 30]);
        assert_eq!(validate_task_body(&q4), "");
        q4["input"]["prompts"][1]["material"]["modality"] = json!("text");
        assert!(validate_task_body(&q4).contains("requires at least one"));
        q4["input"]["prompts"][1]["material"]["modality"] = json!("image");
        assert_eq!(validate_task_body(&q4), "");
        q4["input"]["prompts"][1]["material"]["modality"] = json!("video");
        assert!(validate_task_body(&q4).contains("only image or text materials"));
        assert_eq!(
            validate_task_body(&make_body("character2video", "3.4", 5, 15, 4)),
            ""
        );
        for model in [
            "3.1",
            "3.2_fast_m",
            "3.2_pro_m",
            "3.2_image_2",
            "3.2_image_2_5_fast",
            "3.2_image_2_5_pro",
        ] {
            assert_eq!(
                validate_task_body(&make_body("reference2image", model, 0, 19, 0)),
                "",
                "{}",
                model
            );
            assert!(
                validate_task_body(&make_body("reference2image", model, 0, 20, 0))
                    .contains("at most 20")
            );
        }
    }

    #[test]
    fn generation_text_length_is_left_to_backend_and_input_shapes_are_checked() {
        for (model, max) in [
            ("3.4", 20000),
            (WAN_MODEL_VERSION, 20000),
            (DRAMA_MODEL_VERSION, 5000),
            ("3.1", 5000),
        ] {
            let mut body = make_body("character2video", model, 5, 1, 0);
            body["input"]["prompts"][0]["content"] = json!("界".repeat(max));
            assert_eq!(validate_task_body(&body), "", "{}", model);
            body["input"]["prompts"][0]["content"] = json!("界".repeat(max + 1));
            assert_eq!(validate_task_body(&body), "", "{}", model);
            body["input"]["prompts"][0]["content"] = json!(format!(" \n{}\t", "界".repeat(max)));
            assert_eq!(validate_task_body(&body), "", "{}", model);
            body["input"]["prompts"][0]["content"] = json!(" \n\t");
            assert!(validate_task_body(&body).contains("must not be empty"));
        }
        for model in ["3.4", DRAMA_MODEL_VERSION, WAN_MODEL_VERSION] {
            let mut body = make_body("character2video", model, 5, 1, 0);
            body["input"]["prompts"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"text", "content":"second"}));
            assert!(validate_task_body(&body).contains("exactly one text"));
        }
        let mut legacy = make_body("character2video", "3.1", 5, 1, 0);
        legacy["input"]["prompts"] = json!([
            {"type":"text", "content":"a".repeat(5000)},
            {"type":"text", "content":"b".repeat(5000), "negative":true},
            {"type":"image", "content":"ssupload:?id=ref"}
        ]);
        assert_eq!(validate_task_body(&legacy), "");
        let mut text_only = legacy.clone();
        text_only["type"] = json!("text2video");
        text_only["input"]["prompts"].as_array_mut().unwrap().pop();
        assert_eq!(validate_task_body(&text_only), "");
        for value in [
            Value::Null,
            json!({}),
            json!({"type":"image", "content":null}),
        ] {
            legacy["input"]["prompts"][2] = value;
            assert!(!validate_task_body(&legacy).is_empty());
        }
    }

    #[test]
    fn material_identity_is_checked_without_trusting_expanded_media() {
        let mut body = make_body("character2video", "3.4", 5, 0, 1);
        for (id, version) in [
            (json!(1), json!(2)),
            (json!("1"), json!("9223372036854775807")),
        ] {
            body["input"]["prompts"][1]["material"]["id"] = id;
            body["input"]["prompts"][1]["material"]["version"] = version;
            assert_eq!(validate_task_body(&body), "");
        }
        for invalid in [
            json!(""),
            json!(0),
            json!(-1),
            json!(1.5),
            json!("9223372036854775808"),
            Value::Null,
        ] {
            body["input"]["prompts"][1]["material"]["id"] = invalid;
            assert!(validate_task_body(&body).contains("material.id"));
        }
        body["settings"]["model_version"] = json!(DRAMA_MODEL_VERSION);
        assert!(validate_task_body(&body).contains("does not support material"));
    }

    #[test]
    fn shared_settings_check_integer_bounds_without_inventing_generic_batch_limit() {
        let mut settings = make_body("character2video", "3.4", 5, 1, 0)["settings"].clone();
        for count in [1, 5, i64::from(i32::MAX)] {
            settings["sample_count"] = json!(count);
            assert_eq!(
                validate_generation_settings("character2video", &settings),
                ""
            );
        }
        for count in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!(i64::from(i32::MAX) + 1),
            Value::Null,
        ] {
            settings["sample_count"] = count;
            assert!(validate_generation_settings("character2video", &settings)
                .contains("32-bit integer"));
        }
        settings["sample_count"] = json!(5);
        settings["model_version"] = json!(DRAMA_MODEL_VERSION);
        assert!(validate_generation_settings("character2video", &settings).contains("1-4"));
        settings["sample_count"] = json!(4);
        assert_eq!(
            validate_generation_settings("character2video", &settings),
            ""
        );
        for codec in ["h264_pro", "h265_pro"] {
            settings["codec"] = json!(codec);
            assert_eq!(
                validate_generation_settings("character2video", &settings),
                ""
            );
        }
    }

    #[test]
    fn aspect_ratios_are_parsed_then_checked_per_model() {
        assert_eq!(parse_aspect_ratio("auto"), Ok(String::new()));
        assert_eq!(parse_aspect_ratio(""), Ok(String::new()));
        for bad in ["intelligent", "0:1", "1:-2", "NaN:1", "1:inf", "1:2:3"] {
            assert!(parse_aspect_ratio(bad).is_err(), "{}", bad);
        }
        let mut settings = make_body("text2image", "3.1", 0, 0, 0)["settings"].clone();
        for ratio in ["1:10", "10:1", "2.5:3", ""] {
            settings["aspect_ratio"] = json!(ratio);
            assert_eq!(validate_generation_settings("text2image", &settings), "");
        }
        settings["aspect_ratio"] = json!("10.01:1");
        assert!(
            validate_generation_settings("text2image", &settings).contains("Invalid aspect_ratio")
        );
        for model in [
            "3.2_fast_m",
            "3.2_pro_m",
            "3.2_image_2",
            "3.2_image_2_5_fast",
            "3.2_image_2_5_pro",
        ] {
            settings["model_version"] = json!(model);
            for ratio in ["3:2", "2:3", "21:9", "9:21", "17:6"] {
                settings["aspect_ratio"] = json!(ratio);
                assert_eq!(
                    validate_generation_settings("reference2image", &settings),
                    "",
                    "{} {}",
                    model,
                    ratio
                );
            }
        }
        for model in ["3.2_a", DRAMA_MODEL_VERSION] {
            settings = make_body("character2video", model, 5, 1, 0)["settings"].clone();
            settings["aspect_ratio"] = json!("9:21");
            assert!(validate_generation_settings("character2video", &settings)
                .contains("Invalid aspect_ratio"));
        }
    }

    #[test]
    fn sd_auto_duration_schedule_and_ad_transition_remain_model_specific() {
        let mut settings = make_body("character2video", "3.2_a", -1, 1, 0)["settings"].clone();
        assert_eq!(
            validate_generation_settings("character2video", &settings),
            ""
        );
        settings["schedule_mode"] = json!("nopeak");
        assert!(validate_generation_settings("character2video", &settings)
            .contains("does not support nopeak"));
        settings = make_body("character2video", "3.2", 3, 1, 0)["settings"].clone();
        settings["transition"] = json!("pro_ad");
        for duration in [3, 16] {
            settings["duration"] = json!(duration);
            assert_eq!(
                validate_generation_settings("character2video", &settings),
                ""
            );
        }
        settings["duration"] = json!(2);
        assert!(validate_generation_settings("character2video", &settings).contains("[3, 16]"));
        settings["duration"] = json!(5);
        assert_eq!(validate_generation_settings("img2video", &settings), "");
        assert_eq!(validate_generation_settings("text2video", &settings), "");
        settings["model_version"] = json!("3.4");
        assert_eq!(
            validate_generation_settings("character2video", &settings),
            ""
        );
        let mut editing = make_body("character2video", "3.1_pro", -1, 0, 0);
        for _ in 0..8 {
            editing["input"]["prompts"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"video", "content":"ssupload:?id=ref"}));
        }
        assert_eq!(validate_task_body(&editing), "");
    }

    #[test]
    fn reference_audio_files_use_model_budgets_and_keep_format_isolation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("audio.wav");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(20 * 1024 * 1024).unwrap();
        for model in ["3.4", "3.2_a"] {
            assert_eq!(
                validate_reference_audio_file_for_model(path.to_str().unwrap(), model),
                ""
            );
        }
        for model in [DRAMA_MODEL_VERSION, WAN_MODEL_VERSION] {
            assert!(
                validate_reference_audio_file_for_model(path.to_str().unwrap(), model)
                    .contains("15MB")
            );
        }
        assert!(validate_reference_audio_file(path.to_str().unwrap()).contains("15MB"));
        file.set_len(20 * 1024 * 1024 + 1).unwrap();
        assert!(
            validate_reference_audio_file_for_model(path.to_str().unwrap(), "3.4").contains("20MB")
        );
        let flac = directory.path().join("audio.flac");
        std::fs::write(&flac, b"fLaC").unwrap();
        assert_eq!(
            validate_reference_audio_file_for_model(flac.to_str().unwrap(), "3.2_a"),
            ""
        );
        assert!(
            validate_reference_audio_file_for_model(flac.to_str().unwrap(), "3.4")
                .contains("Supported: mp3, wav")
        );
    }

    #[test]
    fn protocol_transitions_are_preserved_across_supported_model_combinations() {
        let mut combinations = 0;
        for task_type in TASK_TYPES {
            for model in MODEL_VERSIONS {
                let supported = if *model == WAN_MODEL_VERSION {
                    WAN_TASK_TYPES.contains(task_type)
                } else {
                    model_support()
                        .get(*task_type)
                        .is_some_and(|models| models.contains(*model))
                };
                if !supported {
                    continue;
                }
                combinations += 1;
                let mut settings = json!({
                    "model_version": model, "duration": 5, "resolution": "1080p",
                    "sample_count": 1, "schedule_mode": "normal",
                });
                for transition in TRANSITIONS {
                    settings["transition"] = json!(transition);
                    assert_eq!(
                        validate_generation_settings(task_type, &settings),
                        "",
                        "{} {} {}",
                        task_type,
                        model,
                        transition
                    );
                }
                for transition in [json!("invented"), json!(17), Value::Null] {
                    settings["transition"] = transition;
                    let error = validate_generation_settings(task_type, &settings);
                    assert!(
                        error.contains("transition"),
                        "{} {}: {}",
                        task_type,
                        model,
                        error
                    );
                }
            }
        }
        assert_eq!(combinations, 37);
    }

    #[test]
    fn sd_output_options_follow_native_and_configured_upscale_paths() {
        for task_type in [
            "text2video",
            "img2video",
            "headtailimg2video",
            "character2video",
        ] {
            let mut settings = json!({"model_version":"3.2_a", "duration":5, "resolution":"720p"});
            for resolution in ["480p", "720p", "1080p", "2k", "4k"] {
                settings["resolution"] = json!(resolution);
                for ratio in ["16:9", "9:16", "1:1", "4:3", "3:4", "21:9", "adaptive", ""] {
                    settings["aspect_ratio"] = json!(ratio);
                    assert_eq!(
                        validate_generation_settings(task_type, &settings),
                        "",
                        "{} {} {}",
                        task_type,
                        resolution,
                        ratio
                    );
                }
            }
            for resolution in ["540p", "8k"] {
                settings["resolution"] = json!(resolution);
                assert!(validate_generation_settings(task_type, &settings)
                    .contains("Invalid resolution"));
            }
            settings["resolution"] = json!("720p");
            for ratio in ["9:21", "7:5", "3:2"] {
                settings["aspect_ratio"] = json!(ratio);
                assert!(validate_generation_settings(task_type, &settings)
                    .contains("Invalid aspect_ratio"));
            }
        }
    }

    #[test]
    fn ordinary_video_custom_ratios_do_not_widen_special_model_enums() {
        for model in ["3.0", "3.1", "3.1_pro", "3.2", "3.4"] {
            let mut settings = json!({"model_version":model, "duration":5, "resolution":"1080p", "transition":"speed"});
            for ratio in ["21:9", "7:5", "9:21", ""] {
                settings["aspect_ratio"] = json!(ratio);
                assert_eq!(
                    validate_generation_settings("character2video", &settings),
                    "",
                    "{} {}",
                    model,
                    ratio
                );
            }
            for ratio in ["adaptive", "foo", "0:1", "1:-2", "NaN:1", "1:inf"] {
                settings["aspect_ratio"] = json!(ratio);
                assert!(validate_generation_settings("character2video", &settings)
                    .contains("Invalid aspect_ratio"));
            }
        }
        for model in [DRAMA_MODEL_VERSION, WAN_MODEL_VERSION] {
            let mut settings = json!({"model_version":model, "duration":5, "resolution":"720p"});
            settings["aspect_ratio"] = json!("7:5");
            assert!(validate_generation_settings("character2video", &settings)
                .contains("Invalid aspect_ratio"));
        }
        let ordinary =
            json!({"model_version":"3.2", "duration":5, "resolution":"480p", "transition":"speed"});
        assert!(validate_generation_settings("character2video", &ordinary)
            .contains("Invalid resolution"));
    }

    #[test]
    fn editor_mode_follows_protocol_representation_and_professional_model_gate() {
        for model in [
            "3.0",
            "3.1",
            "3.1_pro",
            "3.2",
            "3.2_a",
            "3.4",
            DRAMA_MODEL_VERSION,
            WAN_MODEL_VERSION,
        ] {
            let mut body = make_body("character2video", model, 5, 1, 0);
            body["settings"]["transition"] = json!("speed");
            // Proto JSON accepts enum names, integer representations and null
            // (unset). The generated validator has no defined-only enum rule.
            for mode in [
                json!("unspecified"),
                json!("normal"),
                json!(0),
                json!(1),
                json!(3),
                Value::Null,
            ] {
                body["input"]["editor_mode"] = mode.clone();
                assert_eq!(validate_task_body(&body), "", "{} {}", model, mode);
            }
            for mode in [json!("professional"), json!(2)] {
                body["input"]["editor_mode"] = mode;
                if model == "3.0" {
                    assert_eq!(validate_task_body(&body), "");
                } else {
                    assert!(validate_task_body(&body).contains("professional editor_mode"));
                }
            }
            for mode in [json!("edit"), json!(1.5), json!(i64::from(i32::MAX) + 1)] {
                body["input"]["editor_mode"] = mode;
                assert!(!validate_task_body(&body).is_empty());
            }
        }
    }

    #[test]
    fn q4_basic_options_match_submit_and_cost() {
        for (task_type, min) in [("img2video", 3), ("character2video", 1)] {
            for duration in [min, 16] {
                for resolution in Q4_RESOLUTIONS {
                    let mut body = make_body(task_type, "3.4", duration, 1, 0);
                    body["settings"]["resolution"] = json!(resolution);
                    assert_eq!(validate_task_body(&body), "");
                    assert_eq!(validate_q4_cost(task_type, duration, resolution, None), "");
                }
            }
            for duration in [0, min - 1, 17] {
                assert!(
                    !validate_task_body(&make_body(task_type, "3.4", duration, 1, 0)).is_empty()
                );
                assert!(!validate_q4_cost(task_type, duration, "1080p", None).is_empty());
            }
        }
        assert!(!validate_q4_cost("text2video", 5, "1080p", None).is_empty());
        assert!(!validate_q4_cost("headtailimg2video", 5, "1080p", None).is_empty());
        assert!(!validate_q4_cost("img2video", 5, "8k", None).is_empty());
        assert_eq!(validate_q4_cost("img2video", 5, "1080p", Some("pro")), "");
        assert!(!validate_task_body(&make_body("character2video", "3.4", 5, 0, 0)).is_empty());
    }

    #[test]
    fn wan_models_settings_and_references_follow_backend_contract() {
        for (kind, images) in [
            ("text2video", 0),
            ("img2video", 1),
            ("headtailimg2video", 2),
            ("character2video", 10),
        ] {
            for duration in [2, 30] {
                for ratio in WAN_ASPECT_RATIOS {
                    let mut body = make_body(kind, WAN_MODEL_VERSION, duration, images, 0);
                    body["settings"]["resolution"] = json!("480p");
                    body["settings"]["aspect_ratio"] = json!(ratio);
                    assert_eq!(validate_task_body(&body), "");
                }
            }
        }
        for duration in [-1, 0, 1, 31] {
            assert!(
                validate_wan_cost("text2video", duration, "720p", None, None, 1)
                    .contains("out of range")
            );
        }
        for resolution in ["540p", "4k"] {
            assert!(!validate_wan_cost("text2video", 5, resolution, None, None, 1).is_empty());
        }
        for count in [0, 5] {
            assert!(!validate_wan_cost("text2video", 5, "720p", None, None, count).is_empty());
        }
        assert!(!validate_wan_cost("text2image", 5, "720p", None, None, 1).is_empty());
        assert_eq!(
            validate_wan_cost("text2video", 5, "720p", None, Some("speed"), 1),
            ""
        );
        assert_eq!(
            preferred_schedule_mode(Some(WAN_MODEL_VERSION), None),
            Ok(Some("normal".into()))
        );
        for mode in ["claw_pass", "nopeak"] {
            assert!(preferred_schedule_mode(Some(WAN_MODEL_VERSION), Some(mode)).is_err());
        }
        let mut body = make_body("character2video", WAN_MODEL_VERSION, 5, 10, 0);
        for kind in ["video", "audio"] {
            for _ in 0..5 {
                body["input"]["prompts"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"type":kind,"content":"ssupload:?id=ref"}));
            }
        }
        assert_eq!(validate_task_body(&body), "");
        body["input"]["prompts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"video","content":"ssupload:?id=extra"}));
        assert!(validate_task_body(&body).contains("max 10 images"));
        for (kind, images, material, expected) in [
            ("text2video", 1, 0, "text prompts only"),
            ("img2video", 2, 0, "exactly 1 image"),
            ("headtailimg2video", 1, 0, "exactly 2 images"),
            ("character2video", 0, 0, "at least one"),
            ("character2video", 0, 1, "does not support"),
        ] {
            assert!(
                validate_task_body(&make_body(kind, WAN_MODEL_VERSION, 5, images, material))
                    .contains(expected)
            );
        }
        let mut legacy = make_body("text2video", DRAMA_MODEL_VERSION, 5, 0, 0);
        legacy["settings"]["aspect_ratio"] = json!("adaptive");
        assert!(validate_task_body(&legacy).contains("Invalid aspect_ratio"));
    }

    #[test]
    fn wan_local_media_limits_do_not_use_drama_geometry_or_file_size() {
        assert!(validate_wan_media_dimensions(240, 1920, 4096).is_ok());
        for (width, height) in [(239, 240), (4097, 240), (240, 1921)] {
            assert!(validate_wan_media_dimensions(width, height, 4096).is_err());
        }
        let directory = tempfile::tempdir().unwrap();
        let video = directory.path().join("large.mp4");
        std::fs::File::create(&video)
            .unwrap()
            .set_len(60 * 1024 * 1024)
            .unwrap();
        assert_eq!(validate_wan_video_file(video.to_str().unwrap()), "");
        assert_eq!(validate_reference_video_file(video.to_str().unwrap()), "");
        let image = directory.path().join("small.png");
        image::RgbImage::new(240, 240).save(&image).unwrap();
        assert_eq!(validate_wan_image_file(image.to_str().unwrap()), "");
        image::RgbImage::new(239, 240).save(&image).unwrap();
        assert!(!validate_wan_image_file(image.to_str().unwrap()).is_empty());
    }

    #[test]
    fn wan_video_budget_is_independent_of_container_and_legacy_uploads() {
        let directory = tempfile::tempdir().unwrap();
        let video = directory.path().join("reference.mkv");
        let file = std::fs::File::create(&video).unwrap();
        file.set_len(100 * 1024 * 1024).unwrap();
        // Parsing/probing the actual media is the task layer's responsibility;
        // unidu's Wan metadata contract accepts video/* rather than extensions.
        assert_eq!(validate_wan_video_file(video.to_str().unwrap()), "");
        assert_eq!(validate_reference_video_file(video.to_str().unwrap()), "");
        file.set_len(100 * 1024 * 1024 + 1).unwrap();
        assert!(validate_wan_video_file(video.to_str().unwrap()).contains("100MB"));
        assert_eq!(validate_reference_video_file(video.to_str().unwrap()), "");
        file.set_len(0).unwrap();
        assert!(validate_wan_video_file(video.to_str().unwrap()).contains("greater than zero"));
    }

    #[test]
    fn drama_model_accepts_only_supported_task_types_and_duration_boundaries() {
        assert!(MODEL_VERSIONS.contains(&DRAMA_MODEL_VERSION));
        for (task_type, images) in [("text2video", 0), ("character2video", 1)] {
            for duration in [4, 30] {
                let mut body = make_body(task_type, DRAMA_MODEL_VERSION, duration, images, 0);
                body["settings"]["resolution"] = json!("480p");
                assert_eq!(validate_task_body(&body), "");
                assert_eq!(validate_drama_cost(task_type, duration, "720p", None), "");
            }
            for duration in [3, 31, -1] {
                let body = make_body(task_type, DRAMA_MODEL_VERSION, duration, images, 0);
                assert!(validate_task_body(&body).contains("out of range"));
                assert!(!validate_drama_cost(task_type, duration, "720p", None).is_empty());
            }
        }
        for task_type in [
            "img2video",
            "headtailimg2video",
            "text2image",
            "reference2image",
        ] {
            assert!(
                validate_task_body(&make_body(task_type, DRAMA_MODEL_VERSION, 5, 1, 0))
                    .contains("does not support")
            );
            assert!(!validate_drama_cost(task_type, 5, "720p", None).is_empty());
        }
    }

    #[test]
    fn drama_reference_rules_are_model_specific() {
        let mut character = make_body("character2video", DRAMA_MODEL_VERSION, 5, 9, 0);
        assert_eq!(validate_task_body(&character), "");
        character["input"]["prompts"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "type": "image", "content": "ssupload:?id=extra"
            }));
        assert!(validate_task_body(&character).contains("max 9 images"));

        let mut character_without_reference =
            make_body("character2video", DRAMA_MODEL_VERSION, 5, 0, 0);
        assert!(validate_task_body(&character_without_reference)
            .contains("requires at least one image, video or audio"));
        character_without_reference["input"]["prompts"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "type": "audio", "content": "ssupload:?id=audio"
            }));
        assert_eq!(validate_task_body(&character_without_reference), "");
        character_without_reference["input"]["prompts"]
            .as_array_mut()
            .unwrap()
            .pop();
        character_without_reference["input"]["prompts"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "type": "video", "content": "ssupload:?id=video"
            }));
        assert_eq!(validate_task_body(&character_without_reference), "");

        assert_eq!(
            validate_reference_video_count("character2video", DRAMA_MODEL_VERSION, 3),
            ""
        );
        assert!(
            validate_reference_video_count("character2video", DRAMA_MODEL_VERSION, 4)
                .contains("Max: 3")
        );
        assert!(!validate_reference_video_count("character2video", "3.2", 1).is_empty());
    }

    #[test]
    fn drama_text_tasks_reject_every_non_text_prompt_and_unsupported_options() {
        let mut body = make_body("text2video", DRAMA_MODEL_VERSION, 5, 1, 0);
        assert!(validate_task_body(&body).contains("text prompts only"));
        body["input"]["prompts"] = json!([
            {"type": "text", "content": "prompt"},
            {"type": "extend", "content": "anything"}
        ]);
        assert!(validate_task_body(&body).contains("does not support 'extend'"));

        let mut character = make_body("character2video", DRAMA_MODEL_VERSION, 5, 1, 0);
        character["input"]["prompts"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "type": "material", "name": "m", "material": {"id": "1", "version": "1"}
            }));
        assert!(validate_task_body(&character).contains("does not support material"));
        character["input"]["prompts"].as_array_mut().unwrap().pop();
        character["input"]["prompts"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "type": "audio", "content": "ssupload:?id=audio"
            }));
        assert_eq!(validate_task_body(&character), "");
        character["input"]["prompts"].as_array_mut().unwrap().pop();
        character["settings"]["transition"] = json!("pro");
        assert_eq!(validate_task_body(&character), "");

        let mut character = make_body("character2video", DRAMA_MODEL_VERSION, 5, 1, 0);
        character["input"]["prompts"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "type": "extend", "content": "ssupload:?id=video"
            }));
        assert!(validate_task_body(&character).contains("does not support 'extend'"));
        character["input"]["prompts"].as_array_mut().unwrap().pop();
        character["input"]["editor_mode"] = json!("edit");
        assert!(validate_task_body(&character).contains("Invalid editor_mode"));
        assert!(!validate_drama_cost("text2video", 5, "540p", None).is_empty());
        assert_eq!(
            validate_drama_cost("text2video", 5, "720p", Some("pro")),
            ""
        );
    }

    #[test]
    fn drama_task_body_allows_only_normal_credit_schedule_mode() {
        let mut body = make_body("character2video", DRAMA_MODEL_VERSION, 5, 1, 0);
        body["settings"]["schedule_mode"] = json!("normal");
        assert_eq!(validate_task_body(&body), "");
        for mode in ["nopeak", "claw_pass"] {
            body["settings"]["schedule_mode"] = json!(mode);
            assert!(validate_task_body(&body).contains("only --schedule-mode normal"));
        }
    }

    #[test]
    fn q4_defaults_to_credits_and_accepts_nopeak_but_not_claw_pass() {
        assert_eq!(
            preferred_schedule_mode(Some("3.4"), None),
            Ok(Some("normal".into()))
        );
        assert_eq!(
            preferred_schedule_mode(Some("3.4"), Some("normal")),
            Ok(Some("normal".into()))
        );
        assert_eq!(
            preferred_schedule_mode(Some("3.4"), Some("nopeak")),
            Ok(Some("nopeak".into()))
        );
        assert!(preferred_schedule_mode(Some("3.4"), Some("claw_pass"))
            .unwrap_err()
            .contains("Q4 does not support claw_pass"));
        let mut body = make_body("img2video", "3.4", 5, 1, 0);
        body["settings"]["schedule_mode"] = json!("claw_pass");
        assert!(validate_task_body(&body).contains("Q4 does not support claw_pass"));
        body["settings"]["schedule_mode"] = json!("nopeak");
        assert_eq!(validate_task_body(&body), "");
        for model in [None, Some("3.2"), Some("3.2_a")] {
            assert_eq!(preferred_schedule_mode(model, None), Ok(None));
            assert_eq!(
                preferred_schedule_mode(model, Some("claw_pass")),
                Ok(Some("claw_pass".into()))
            );
            if model == Some("3.2_a") {
                assert!(preferred_schedule_mode(model, Some("nopeak")).is_err());
            } else {
                assert_eq!(
                    preferred_schedule_mode(model, Some("nopeak")),
                    Ok(Some("nopeak".into()))
                );
            }
        }
    }

    #[test]
    fn drama_defaults_to_credits_without_pass_detection_and_rejects_claw_pass() {
        assert_eq!(
            preferred_schedule_mode(Some(DRAMA_MODEL_VERSION), None),
            Ok(Some("normal".into()))
        );
        assert_eq!(
            preferred_schedule_mode(Some(DRAMA_MODEL_VERSION), Some("normal")),
            Ok(Some("normal".into()))
        );
        assert_eq!(
            preferred_schedule_mode(Some(DRAMA_MODEL_VERSION), Some("nopeak")),
            Err("viduq3.1-drama-std supports only --schedule-mode normal (credits)".into())
        );
        assert!(
            preferred_schedule_mode(Some(DRAMA_MODEL_VERSION), Some("claw_pass"))
                .unwrap_err()
                .contains("only --schedule-mode normal")
        );
    }

    #[test]
    fn q4_accepts_audio_only_references() {
        for image_count in [0, 1] {
            for audio_count in [1, 3] {
                let mut body = make_body("character2video", "3.4", 8, image_count, 0);
                for index in 0..audio_count {
                    body["input"]["prompts"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({
                            "type": "audio",
                            "content": format!("ssupload:?id=audio-{}", index),
                        }));
                }
                assert_eq!(validate_task_body(&body), "");
                body["settings"]["schedule_mode"] = json!("claw_pass");
                assert!(validate_task_body(&body).contains("Q4 does not support claw_pass"));
            }
        }
    }

    #[test]
    fn reference_audio_count_is_model_specific() {
        for model in ["3.2_a", "3.4"] {
            assert_eq!(
                validate_reference_audio_count("character2video", model, 3),
                ""
            );
            assert!(validate_reference_audio_count("character2video", model, 4).contains("Max: 3"));
        }
        assert_eq!(
            validate_reference_audio_count("character2video", DRAMA_MODEL_VERSION, 10),
            ""
        );
        assert!(
            validate_reference_audio_count("character2video", DRAMA_MODEL_VERSION, 11)
                .contains("Max: 10")
        );
        for (task_type, model) in [("img2video", "3.4"), ("character2video", "3.2")] {
            assert!(!validate_reference_audio_count(task_type, model, 1).is_empty());
            assert_eq!(validate_reference_audio_count(task_type, model, 0), "");
        }
        let mut body = make_body("character2video", "3.4", 8, 1, 0);
        for _ in 0..4 {
            body["input"]["prompts"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "type": "audio", "content": "ssupload:?id=audio",
                }));
        }
        assert!(validate_task_body(&body).contains("Max: 3"));

        let mut drama_body = make_body("character2video", DRAMA_MODEL_VERSION, 5, 0, 0);
        for _ in 0..10 {
            drama_body["input"]["prompts"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "type": "audio", "content": "ssupload:?id=audio",
                }));
        }
        assert_eq!(validate_task_body(&drama_body), "");
        drama_body["input"]["prompts"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "type": "audio", "content": "ssupload:?id=audio",
            }));
        assert!(validate_task_body(&drama_body).contains("Max: 10"));
    }

    #[test]
    fn task_body_missing_type() {
        assert_eq!(
            validate_task_body(&json!({})),
            "Missing required field: type"
        );
    }

    #[test]
    fn task_body_invalid_type() {
        assert!(validate_task_body(&json!({"type": "bad"})).contains("Invalid type"));
    }

    #[test]
    fn task_body_missing_input() {
        assert_eq!(
            validate_task_body(&json!({"type": "text2video"})),
            "input must be an object"
        );
    }

    #[test]
    fn task_body_empty_prompts() {
        let body = json!({"type": "text2video", "input": {"prompts": []}});
        assert!(validate_task_body(&body).contains("must not be empty"));
    }

    #[test]
    fn task_body_valid_text2video() {
        assert_eq!(
            validate_task_body(&make_body("text2video", "3.2", 8, 0, 0)),
            ""
        );
    }

    #[test]
    fn task_body_img2video_wrong_image_count() {
        let mut body = make_body("img2video", "3.1", 5, 0, 0);
        body["settings"]["transition"] = json!("pro");
        assert!(validate_task_body(&body).contains("requires exactly 1 image"));
    }

    #[test]
    fn task_body_img2video_correct() {
        let mut body = make_body("img2video", "3.1", 5, 1, 0);
        body["settings"]["transition"] = json!("pro");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_headtail_wrong_count() {
        let mut body = make_body("headtailimg2video", "3.1", 5, 1, 0);
        body["settings"]["transition"] = json!("pro");
        assert!(validate_task_body(&body).contains("requires exactly 2 images"));
    }

    #[test]
    fn task_body_material_budget_counts_direct_images_without_guessing_components() {
        assert_eq!(
            validate_task_body(&make_body("character2video", "3.1", 5, 4, 4)),
            ""
        );
        assert_eq!(
            validate_task_body(&make_body("character2video", "3.1", 5, 8, 0)),
            ""
        );
        assert!(
            validate_task_body(&make_body("character2video", "3.1", 5, 8, 1)).contains("max 7")
        );
    }

    #[test]
    fn task_body_invalid_model() {
        assert!(validate_task_body(&make_body("text2video", "9.9", 5, 0, 0))
            .contains("Invalid model_version"));
    }

    #[test]
    fn task_body_model_not_support_task() {
        assert!(validate_task_body(&make_body("text2image", "3.0", 0, 0, 0))
            .contains("does not support"));
    }

    #[test]
    fn model_versions_include_image_2_5_variants() {
        assert!(MODEL_VERSIONS.contains(&"3.2_image_2_5_fast"));
        assert!(MODEL_VERSIONS.contains(&"3.2_image_2_5_pro"));
    }

    #[test]
    fn task_body_text2image_image_2_5_models_valid() {
        assert_eq!(
            validate_task_body(&make_body("text2image", "3.2_image_2_5_fast", 0, 0, 0)),
            ""
        );
        assert_eq!(
            validate_task_body(&make_body("text2image", "3.2_image_2_5_pro", 0, 0, 0)),
            ""
        );
    }

    #[test]
    fn task_body_text2image_image_2_5_2k_valid() {
        let mut body = make_body("text2image", "3.2_image_2_5_fast", 0, 0, 0);
        body["settings"]["resolution"] = json!("2k");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_reference2image_image_2_5_models_valid() {
        assert_eq!(
            validate_task_body(&make_body("reference2image", "3.2_image_2_5_fast", 0, 0, 0)),
            ""
        );
        assert_eq!(
            validate_task_body(&make_body("reference2image", "3.2_image_2_5_pro", 0, 0, 0)),
            ""
        );
    }

    #[test]
    fn task_body_image_2_5_models_reject_video_tasks() {
        assert!(
            validate_task_body(&make_body("text2video", "3.2_image_2_5_fast", 8, 0, 0))
                .contains("does not support")
        );
        assert!(
            validate_task_body(&make_body("img2video", "3.2_image_2_5_pro", 8, 1, 0))
                .contains("does not support")
        );
    }

    #[test]
    fn task_body_3_2_a_duration_accepts_auto() {
        assert_eq!(
            validate_task_body(&make_body("text2video", "3.2_a", -1, 0, 0)),
            ""
        );
    }

    #[test]
    fn task_body_3_2_a_duration_valid_boundaries() {
        assert_eq!(
            validate_task_body(&make_body("text2video", "3.2_a", 4, 0, 0)),
            ""
        );
        assert_eq!(
            validate_task_body(&make_body("text2video", "3.2_a", 15, 0, 0)),
            ""
        );
    }

    #[test]
    fn task_body_3_2_a_duration_invalid_boundaries() {
        assert!(
            validate_task_body(&make_body("text2video", "3.2_a", 3, 0, 0))
                .contains("invalid for 3.2_a")
        );
        assert!(
            validate_task_body(&make_body("text2video", "3.2_a", 16, 0, 0))
                .contains("invalid for 3.2_a")
        );
    }

    #[test]
    fn task_body_3_1_pro_duration_still_accepts_auto() {
        assert_eq!(
            validate_task_body(&make_body("character2video", "3.1_pro", -1, 1, 0)),
            ""
        );
    }

    #[test]
    fn task_body_duration_out_of_range() {
        assert!(
            validate_task_body(&make_body("text2video", "3.1", 20, 0, 0)).contains("out of range")
        );
    }

    #[test]
    fn task_body_char2video_3_2_requires_transition() {
        assert!(
            validate_task_body(&make_body("character2video", "3.2", 8, 1, 0))
                .contains("requires transition")
        );
    }

    #[test]
    fn task_body_char2video_3_2_with_transition() {
        let mut body = make_body("character2video", "3.2", 8, 1, 0);
        body["settings"]["transition"] = json!("pro");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_char2video_non_3_2_accepts_protocol_transition() {
        let mut body = make_body("character2video", "3.1", 5, 1, 0);
        body["settings"]["transition"] = json!("pro");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn schedule_mode_valid() {
        assert_eq!(validate_schedule_mode("normal"), "");
        assert_eq!(validate_schedule_mode("claw_pass"), "");
        assert_eq!(validate_schedule_mode("nopeak"), "");
    }

    #[test]
    fn schedule_mode_invalid() {
        let err = validate_schedule_mode("free");
        assert!(err.contains("Invalid schedule_mode"));
        assert!(err.contains("claw_pass, nopeak, normal"));
    }

    #[test]
    fn lip_sync_text_empty() {
        assert!(validate_lip_sync_text("").contains("cannot be empty"));
    }

    #[test]
    fn lip_sync_text_chinese_range() {
        assert!(validate_lip_sync_text("你").contains("2-1000"));
        assert_eq!(validate_lip_sync_text("你好"), "");
    }

    #[test]
    fn lip_sync_text_english_range() {
        assert!(validate_lip_sync_text("hi").contains("4-2000"));
        assert_eq!(validate_lip_sync_text("hello world"), "");
    }

    #[test]
    fn lip_sync_speed_bounds() {
        assert_eq!(validate_lip_sync_speed(0.5), "");
        assert_eq!(validate_lip_sync_speed(2.0), "");
        assert!(validate_lip_sync_speed(0.3).contains("must be between"));
    }

    #[test]
    fn lip_sync_volume_bounds() {
        assert_eq!(validate_lip_sync_volume(1.0), "");
        assert!(validate_lip_sync_volume(0.05).contains("must be between"));
    }

    #[test]
    fn element_preprocess_valid() {
        let body = json!({"name": "t", "type": "subject", "components": [{"type": "main"}]});
        assert_eq!(validate_element_preprocess(&body), "");
    }

    #[test]
    fn element_preprocess_too_many() {
        let body = json!({"name": "t", "type": "subject", "components": [
            {"type": "main"}, {"type": "a"}, {"type": "a"}, {"type": "a"}
        ]});
        assert!(validate_element_preprocess(&body).contains("at most 3"));
    }

    #[test]
    fn element_preprocess_no_main() {
        let body = json!({"name": "t", "type": "subject", "components": [{"type": "aux"}]});
        assert!(validate_element_preprocess(&body).contains("exactly one"));
    }

    #[test]
    fn timeline_valid() {
        let t = json!({
            "video_tracks": [{"video_track_clips": [
                {"timeline_in": 0, "timeline_out": 5}
            ]}],
            "audio_tracks": [{"audio_track_clips": [
                {"timeline_in": 0, "timeline_out": 5}
            ]}]
        });
        assert_eq!(validate_timeline_clips(&t), "");
    }

    #[test]
    fn timeline_missing_in() {
        let t = json!({"video_tracks": [{"video_track_clips": [{"timeline_out": 5}]}]});
        assert!(validate_timeline_clips(&t).contains("missing required field 'timeline_in'"));
    }

    #[test]
    fn timeline_missing_out() {
        let t = json!({"audio_tracks": [{"audio_track_clips": [{"timeline_in": 0}]}]});
        assert!(validate_timeline_clips(&t).contains("missing required field 'timeline_out'"));
    }

    #[test]
    fn timeline_null_value() {
        let t = json!({"video_tracks": [{"video_track_clips": [{"timeline_in": null, "timeline_out": 5}]}]});
        assert!(validate_timeline_clips(&t).contains("missing required field 'timeline_in'"));
    }

    #[test]
    fn timeline_subtitle_missing() {
        let t = json!({"subtitle_tracks": [{"subtitle_track_clips": [{"type": "Text"}]}]});
        assert!(validate_timeline_clips(&t).contains("subtitle_track_clips[0]"));
    }

    #[test]
    fn timeline_effect_missing() {
        let t = json!({"effect_tracks": [{"effect_track_items": [{"type": "VFX"}]}]});
        assert!(validate_timeline_clips(&t).contains("effect_track_items[0]"));
    }

    #[test]
    fn timeline_empty_tracks_ok() {
        assert_eq!(validate_timeline_clips(&json!({"video_tracks": []})), "");
    }

    #[test]
    fn timeline_error_has_indices() {
        let t = json!({"video_tracks": [{"video_track_clips": [
            {"timeline_in": 0, "timeline_out": 5},
            {"timeline_in": 5}
        ]}]});
        assert!(validate_timeline_clips(&t).contains("video_tracks[0].video_track_clips[1]"));
    }

    // --- validate_tts_voice_id (also used by lip-sync) ---

    #[test]
    fn tts_voice_id_valid() {
        assert_eq!(validate_tts_voice_id("English_expressive_narrator"), "");
    }

    #[test]
    fn tts_personal_voice_ids_are_accepted() {
        for voice in ["voice_123", "voice_abc-123", "voice_550e8400_e29b"] {
            assert_eq!(validate_tts_voice_id(voice), "", "{}", voice);
        }
    }

    #[test]
    fn tts_malformed_personal_voice_ids_are_rejected() {
        for voice in [
            "voice_",
            "voice___",
            "voice_--",
            "voice_ id",
            "voice_a/b",
            "voice_你好",
            "fake_voice",
        ] {
            assert!(
                validate_tts_voice_id(voice).contains("Invalid voice_id"),
                "{}",
                voice
            );
        }
    }

    #[test]
    fn tts_voice_id_invalid() {
        assert!(validate_tts_voice_id("fake_voice").contains("Invalid voice_id"));
    }

    // --- validate_tts_speed ---

    #[test]
    fn tts_speed_bounds() {
        assert_eq!(validate_tts_speed(0.5), "");
        assert_eq!(validate_tts_speed(2.0), "");
        assert!(validate_tts_speed(0.1).contains("must be between"));
        assert!(validate_tts_speed(3.0).contains("must be between"));
    }

    // --- validate_tts_volume ---

    #[test]
    fn tts_volume_bounds() {
        assert_eq!(validate_tts_volume(0), "");
        assert_eq!(validate_tts_volume(100), "");
        assert!(validate_tts_volume(-1).contains("must be between"));
        assert!(validate_tts_volume(101).contains("must be between"));
    }

    // --- validate_tts_emotion ---

    #[test]
    fn tts_emotion_valid() {
        assert_eq!(validate_tts_emotion("happy"), "");
    }

    #[test]
    fn tts_emotion_too_long() {
        let long = "a".repeat(101);
        assert!(validate_tts_emotion(&long).contains("too long"));
    }

    // --- validate_tts_language_boost ---

    #[test]
    fn tts_language_boost_valid() {
        assert_eq!(validate_tts_language_boost("English"), "");
        assert_eq!(validate_tts_language_boost("Chinese"), "");
        assert_eq!(validate_tts_language_boost("auto"), "");
    }

    #[test]
    fn tts_language_boost_invalid() {
        assert!(validate_tts_language_boost("Klingon").contains("Invalid language_boost"));
    }

    // --- validate_task_body: resolution, aspect_ratio, transition branches ---

    #[test]
    fn task_body_invalid_resolution() {
        let mut body = make_body("text2video", "3.2", 8, 0, 0);
        body["settings"]["resolution"] = json!("8k");
        assert!(validate_task_body(&body).contains("Invalid resolution"));
    }

    #[test]
    fn task_body_invalid_aspect_ratio() {
        let mut body = make_body("text2video", "3.2", 8, 0, 0);
        body["settings"]["aspect_ratio"] = json!("foo");
        assert!(validate_task_body(&body).contains("Invalid aspect_ratio"));
    }

    #[test]
    fn task_body_text2image_accepts_protocol_transition() {
        let mut body = make_body("text2image", "3.1", 0, 0, 0);
        body["settings"]["transition"] = json!("pro");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_text2video_non_3_2_accepts_protocol_transition() {
        let mut body = make_body("text2video", "3.1", 5, 0, 0);
        body["settings"]["transition"] = json!("pro");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_img2video_invalid_transition() {
        let mut body = make_body("img2video", "3.1", 5, 1, 0);
        body["settings"]["transition"] = json!("bad");
        assert!(validate_task_body(&body).contains("Invalid transition"));
    }

    #[test]
    fn task_body_img2video_valid_transition() {
        let mut body = make_body("img2video", "3.1", 5, 1, 0);
        body["settings"]["transition"] = json!("pro");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_img2video_3_1_requires_transition() {
        assert!(validate_task_body(&make_body("img2video", "3.1", 5, 1, 0))
            .contains("requires transition"));
    }

    #[test]
    fn task_body_img2video_3_2_requires_transition() {
        assert!(validate_task_body(&make_body("img2video", "3.2", 5, 1, 0))
            .contains("requires transition"));
    }

    #[test]
    fn task_body_headtailimg2video_3_1_accepts_missing_transition() {
        assert_eq!(
            validate_task_body(&make_body("headtailimg2video", "3.1", 5, 2, 0)),
            ""
        );
    }

    #[test]
    fn task_body_headtailimg2video_3_2_accepts_missing_transition() {
        assert_eq!(
            validate_task_body(&make_body("headtailimg2video", "3.2", 5, 2, 0)),
            ""
        );
    }

    #[test]
    fn task_body_headtailimg2video_valid_transition() {
        let mut body = make_body("headtailimg2video", "3.1", 5, 2, 0);
        body["settings"]["transition"] = json!("speed");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_headtailimg2video_preserves_protocol_transition() {
        let mut body = make_body("headtailimg2video", "3.2", 5, 2, 0);
        body["settings"]["transition"] = json!("creative");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_img2video_3_0_still_accepts_creative() {
        let mut body = make_body("img2video", "3.0", 5, 1, 0);
        body["settings"]["transition"] = json!("creative");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_img2video_3_2_a_keeps_current_transition_behavior() {
        assert_eq!(
            validate_task_body(&make_body("img2video", "3.2_a", 5, 1, 0)),
            ""
        );
        let mut body = make_body("img2video", "3.2_a", 5, 1, 0);
        body["settings"]["transition"] = json!("speed");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_missing_enhance() {
        let mut body = json!({
            "type": "text2video",
            "input": { "prompts": [{"type": "text", "content": "test"}] },
            "settings": { "duration": 5, "model_version": "3.2", "resolution": "1080p", "schedule_mode": "normal" }
        });
        assert_eq!(validate_task_body(&body), "");
        body["input"]["enhance"] = json!("true");
        assert!(validate_task_body(&body).contains("enhance must be true or false"));
    }

    #[test]
    fn task_body_missing_resolution() {
        let body = json!({
            "type": "text2video",
            "input": { "prompts": [{"type": "text", "content": "test"}], "enhance": true },
            "settings": { "duration": 5, "model_version": "3.2", "schedule_mode": "normal" }
        });
        assert!(validate_task_body(&body).contains("resolution is required"));
    }

    // --- tts_voices_grouped ---

    #[test]
    fn tts_voices_grouped_not_empty() {
        let groups = tts_voices_grouped();
        assert!(!groups.is_empty());
        assert!(all_tts_voice_ids().len() > 100);
    }

    // --- read_prompt_file ---

    fn write_temp_prompt(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let dir = tempfile::tempdir().unwrap().keep();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn read_prompt_file_strips_trailing_lf() {
        let path = write_temp_prompt("prompt.txt", b"hello from file\n");
        let out = read_prompt_file(path.to_str().unwrap()).unwrap();
        assert_eq!(out, "hello from file");
    }

    #[test]
    fn read_prompt_file_strips_trailing_crlf() {
        let path = write_temp_prompt("prompt_crlf.txt", b"line one\r\n");
        let out = read_prompt_file(path.to_str().unwrap()).unwrap();
        assert_eq!(out, "line one");
    }

    #[test]
    fn read_prompt_file_preserves_internal_newlines() {
        let path = write_temp_prompt("prompt_multi.txt", b"first\nsecond\n");
        let out = read_prompt_file(path.to_str().unwrap()).unwrap();
        assert_eq!(out, "first\nsecond");
    }

    #[test]
    fn read_prompt_file_missing_errors() {
        let err = read_prompt_file("/definitely/not/here.txt").unwrap_err();
        assert!(err.contains("Prompt file not found"), "got: {}", err);
    }

    #[test]
    fn read_prompt_file_too_large_errors() {
        let big = vec![b'a'; (1024 * 1024 + 1) as usize];
        let path = write_temp_prompt("big.txt", &big);
        let err = read_prompt_file(path.to_str().unwrap()).unwrap_err();
        assert!(err.contains("Prompt file too large"), "got: {}", err);
    }

    #[test]
    fn read_prompt_file_invalid_utf8_errors() {
        let path = write_temp_prompt("bad.bin", &[0xff, 0xfe, 0xfd]);
        let err = read_prompt_file(path.to_str().unwrap()).unwrap_err();
        assert!(err.contains("not valid UTF-8"), "got: {}", err);
    }
}
