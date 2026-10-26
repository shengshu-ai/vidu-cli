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

/// All accepted `--aspect-ratio` values.
pub const ASPECT_RATIOS: &[&str] = &["16:9", "9:16", "1:1", "4:3", "3:4", "21:9", "adaptive"];

/// All accepted `--transition` values across task types.
pub const TRANSITIONS: &[&str] = &["creative", "stable", "pro", "speed"];

/// All accepted `--schedule-mode` values.
pub const SCHEDULE_MODES: &[&str] = &["claw_pass", "nopeak", "normal"];

/// All accepted `--codec` values.
pub const CODECS: &[&str] = &["h264", "h265"];

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

pub fn validate_task_body(body: &Value) -> String {
    let task_type = body.get("type").and_then(|v| v.as_str()).unwrap_or("");
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

    let input_obj = body.get("input");
    let input = match input_obj {
        Some(Value::Object(_)) => input_obj.unwrap(),
        _ => return "input must be an object".into(),
    };
    let prompts = input.get("prompts");
    match prompts {
        Some(Value::Array(arr)) if !arr.is_empty() => {}
        Some(Value::Array(_)) => return "input.prompts is required and must not be empty".into(),
        _ => return "input.prompts is required and must not be empty".into(),
    }

    // Count images and materials
    let prompts_arr = prompts.unwrap().as_array().unwrap();
    let image_count = prompts_arr
        .iter()
        .filter(|p| p.get("type").and_then(|v| v.as_str()) == Some("image"))
        .count();
    let material_count = prompts_arr
        .iter()
        .filter(|p| p.get("type").and_then(|v| v.as_str()) == Some("material"))
        .count();
    let audio_count = prompts_arr
        .iter()
        .filter(|p| p.get("type").and_then(|v| v.as_str()) == Some("audio"))
        .count();
    let video_count = prompts_arr
        .iter()
        .filter(|p| p.get("type").and_then(|v| v.as_str()) == Some("video"))
        .count();
    let non_text_prompt_count = prompts_arr
        .iter()
        .filter(|p| p.get("type").and_then(|v| v.as_str()) != Some("text"))
        .count();
    let unsupported_prompt_type = prompts_arr.iter().find_map(|prompt| {
        let prompt_type = prompt.get("type").and_then(|v| v.as_str()).unwrap_or("");
        (!["text", "image", "video", "material", "audio"].contains(&prompt_type))
            .then_some(prompt_type)
    });

    let settings = match body.get("settings") {
        Some(Value::Object(_)) => body.get("settings").unwrap(),
        _ => return "settings must be an object".into(),
    };

    let model_version = settings
        .get("model_version")
        .and_then(|v| v.as_str())
        .unwrap_or("2.0");
    if !valid_model_versions().contains(model_version) {
        return format!(
            "Invalid model_version '{}'. Valid: {}",
            model_version,
            sorted_join(&valid_model_versions())
        );
    }

    if model_version == WAN_MODEL_VERSION {
        return validate_wan_task_body(body);
    }
    // The new Wan ratios must not widen legacy models' accepted inputs.
    if settings
        .get("aspect_ratio")
        .and_then(Value::as_str)
        .is_some_and(|ratio| ["adaptive", "21:9"].contains(&ratio))
    {
        return "Invalid aspect_ratio: adaptive and 21:9 are supported only for Wan 3".into();
    }

    let ms = model_support();
    if let Some(supported) = ms.get(task_type) {
        if !supported.contains(model_version) {
            return format!(
                "model_version {} does not support {}",
                model_version, task_type
            );
        }
    }

    // Validate input counts by task type and model version.
    match task_type {
        "img2video" if image_count != 1 => {
            return format!("img2video requires exactly 1 image, got {}", image_count)
        }
        "headtailimg2video" if image_count != 2 => {
            return format!(
                "headtailimg2video requires exactly 2 images, got {}",
                image_count
            )
        }
        "reference2image" if image_count + material_count > 7 => {
            return format!(
                "{} allows max 7 images+materials, got {}",
                task_type,
                image_count + material_count
            );
        }
        "character2video" if model_version == DRAMA_MODEL_VERSION && image_count > 9 => {
            return format!("{} allows max 9 images, got {}", task_type, image_count);
        }
        "character2video"
            if model_version != DRAMA_MODEL_VERSION && image_count + material_count > 7 =>
        {
            return format!(
                "{} allows max 7 images+materials, got {}",
                task_type,
                image_count + material_count
            );
        }
        _ => {}
    }

    let video_error = validate_reference_video_count(task_type, model_version, video_count);
    if !video_error.is_empty() {
        return video_error;
    }

    if model_version == DRAMA_MODEL_VERSION {
        if material_count > 0 {
            return "viduq3.1-drama-std does not support material references".into();
        }
        if let Some(prompt_type) = unsupported_prompt_type {
            return format!(
                "viduq3.1-drama-std does not support '{}' prompt references",
                prompt_type
            );
        }
        if input
            .get("editor_mode")
            .is_some_and(|mode| mode.as_str() != Some("normal"))
        {
            return "viduq3.1-drama-std supports only normal generation mode".into();
        }
        if task_type == "text2video" && non_text_prompt_count > 0 {
            return "viduq3.1-drama-std text2video accepts text prompts only; use character2video for references".into();
        }
        if task_type == "character2video" && image_count + video_count + audio_count == 0 {
            return "viduq3.1-drama-std character2video requires at least one image, video or audio reference".into();
        }
    }

    let audio_error = validate_reference_audio_count(task_type, model_version, audio_count);
    if !audio_error.is_empty() {
        return audio_error;
    }

    if model_version == "3.4"
        && task_type == "character2video"
        && image_count + material_count + audio_count == 0
    {
        return "Q4 character2video requires an image, material or audio reference".into();
    }

    let duration = settings
        .get("duration")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if model_version == "3.2_a" {
        if !(4..=15).contains(&duration) {
            return format!("duration {} is invalid for 3.2_a: must be 4-15", duration);
        }
    } else {
        let dr = duration_ranges();
        if let Some(type_ranges) = dr.get(task_type) {
            if let Some(&(min_d, max_d)) = type_ranges.get(model_version) {
                if min_d < 0 && duration != min_d && !(2..=max_d).contains(&duration) {
                    return format!(
                        "duration {} out of range [{}, {}] for {} with {}",
                        duration, min_d, max_d, task_type, model_version
                    );
                }
                if min_d > 0 && (duration < min_d || duration > max_d) {
                    return format!(
                        "duration {} out of range [{}, {}] for {} with {}",
                        duration, min_d, max_d, task_type, model_version
                    );
                }
            }
        }
    }

    if settings.get("resolution").is_none() {
        return format!("resolution is required for {}", task_type);
    }
    let res = settings
        .get("resolution")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let rs = resolution_support();
    let valid_res = if model_version == DRAMA_MODEL_VERSION {
        set(&["480p", "720p", "1080p"])
    } else if model_version == "3.2_a" {
        let mut valid = rs
            .get(task_type)
            .cloned()
            .unwrap_or_else(|| set(&["1080p"]));
        valid.insert("720p".into());
        valid
    } else if model_version == "3.4" {
        set(Q4_RESOLUTIONS)
    } else {
        rs.get(task_type)
            .cloned()
            .unwrap_or_else(|| set(&["1080p"]))
    };
    if !valid_res.contains(res) {
        return format!(
            "Invalid resolution '{}' for {}. Valid: {}",
            res,
            task_type,
            sorted_join(&valid_res)
        );
    }

    let no_ar_types = ["img2video", "headtailimg2video", "text2image"];
    if !no_ar_types.contains(&task_type) {
        if let Some(ar_val) = settings.get("aspect_ratio").and_then(|v| v.as_str()) {
            if !valid_aspect_ratios().contains(ar_val) {
                return format!(
                    "Invalid aspect_ratio '{}'. Valid: {}",
                    ar_val,
                    sorted_join(&valid_aspect_ratios())
                );
            }
        }
    }

    let transition = settings.get("transition").and_then(|v| v.as_str());

    if model_version == DRAMA_MODEL_VERSION && transition.is_some() {
        return "viduq3.1-drama-std does not support transition".into();
    }

    if model_version == "3.4" && transition.is_some() {
        return "Q4 should not include transition".into();
    }

    if transition.is_some() {
        let no_trans = ["reference2image", "text2image"];
        if no_trans.contains(&task_type) {
            return format!("{} should not include transition", task_type);
        }
        if task_type == "text2video" && model_version != "3.2" {
            return format!(
                "text2video with {} should not include transition (only 3.2 supports pro/speed)",
                model_version
            );
        }
    }

    if task_type == "img2video" || task_type == "headtailimg2video" {
        if model_version == "3.1" || model_version == "3.2" {
            match transition {
                None => {
                    return format!(
                        "{} with {} requires transition parameter (pro or speed)",
                        task_type, model_version
                    );
                }
                Some(trans) if trans != "pro" && trans != "speed" => {
                    return format!(
                        "Invalid transition '{}' for {} {}. Valid: pro, speed",
                        trans, task_type, model_version
                    );
                }
                _ => {}
            }
        } else if let Some(trans) = transition {
            let valid_trans = if model_version == "3.0" {
                set(&["creative", "stable"])
            } else {
                set(&["pro", "speed"])
            };
            if !valid_trans.contains(trans) {
                return format!(
                    "Invalid transition '{}' for {} {}. Valid: {}",
                    trans,
                    task_type,
                    model_version,
                    sorted_join(&valid_trans)
                );
            }
        }
    }

    if task_type == "character2video" {
        if model_version == "3.2" {
            match transition {
                None => {
                    return "character2video with 3.2 requires transition parameter (speed or pro)"
                        .into()
                }
                Some(trans) if trans != "speed" && trans != "pro" => {
                    return format!(
                        "Invalid transition '{}' for character2video 3.2. Valid: pro, speed",
                        trans
                    );
                }
                _ => {}
            }
        } else if transition.is_some() {
            return format!("character2video with {} should not include transition (only 3.2 supports pro/speed)", model_version);
        }
    }

    if input.get("enhance").is_none() {
        return "input.enhance is required (true or false)".into();
    }

    if let Err(err) = preferred_schedule_mode(
        Some(model_version),
        settings.get("schedule_mode").and_then(Value::as_str),
    ) {
        return err;
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
        if model_version == Some(DRAMA_MODEL_VERSION) && mode != "normal" {
            return Err("viduq3.1-drama-std supports only --schedule-mode normal (credits)".into());
        }
        if model_version == Some(WAN_MODEL_VERSION) && mode != "normal" {
            return Err("Wan 3 supports only --schedule-mode normal (credits)".into());
        }
    }
    Ok(mode.map(str::to_string))
}

pub fn validate_q4_cost(
    task_type: &str,
    duration: i64,
    resolution: &str,
    transition: Option<&str>,
) -> String {
    let ranges = duration_ranges();
    let Some(&(min, max)) = ranges.get(task_type).and_then(|r| r.get("3.4")) else {
        return format!("model_version 3.4 does not support {}", task_type);
    };
    if !(min..=max).contains(&duration) {
        return format!(
            "duration {} out of range [{}, {}] for Q4 {}",
            duration, min, max, task_type
        );
    }
    if !Q4_RESOLUTIONS.contains(&resolution) {
        return format!("Invalid resolution '{}' for Q4", resolution);
    }
    if transition.is_some() {
        return "Q4 should not include transition".into();
    }
    String::new()
}

pub fn validate_drama_cost(
    task_type: &str,
    duration: i64,
    resolution: &str,
    transition: Option<&str>,
) -> String {
    if !["text2video", "character2video"].contains(&task_type) {
        return format!(
            "model_version {} does not support {}",
            DRAMA_MODEL_VERSION, task_type
        );
    }
    if !(4..=30).contains(&duration) {
        return format!("Drama duration {} out of range [4, 30]", duration);
    }
    if !["480p", "720p", "1080p"].contains(&resolution) {
        return format!("Invalid resolution '{}' for Drama", resolution);
    }
    if transition.is_some() {
        return "viduq3.1-drama-std does not support transition".into();
    }
    String::new()
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
    if transition.is_some() {
        return "Wan 3 does not support transition".into();
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
        .get("editor_mode")
        .is_some_and(|mode| mode.as_str() != Some("normal"))
    {
        return "Wan 3 supports only normal generation mode".into();
    }
    if input.get("enhance").is_none() {
        return "input.enhance is required (true or false)".into();
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
        || !["3.2_a", DRAMA_MODEL_VERSION, WAN_MODEL_VERSION].contains(&model_version)
    {
        return "Video input is only supported for character2video with model_version 3.2_a, viduq3.1-drama-std, or viduq2-drama-std".into();
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

pub fn validate_reference_audio_file(path: &str) -> String {
    let p = Path::new(path);
    if !p.is_file() {
        return format!("Audio file not found: {}", path);
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["mp3", "wav"].contains(&ext.as_str()) {
        return format!("Invalid audio format '{}'. Supported: mp3, wav", ext);
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > 15 * 1024 * 1024 {
        return format!(
            "Audio file too large ({:.1}MB). Max: 15MB",
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
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["mp4", "mov"].contains(&ext.as_str()) {
        return format!("Invalid video format '{}'. Supported: mp4, mov", ext);
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > 50 * 1024 * 1024 {
        return format!(
            "Video file too large ({:.1}MB). Max: 50MB",
            size as f64 / 1024.0 / 1024.0
        );
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

pub fn validate_wan_image_file(path: &str) -> String {
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["jpg", "jpeg", "png", "bmp", "webp"].contains(&ext.as_str()) {
        return "Wan 3 images must be JPEG, PNG, BMP or WEBP".into();
    }
    let size = match std::fs::metadata(p) {
        Ok(metadata) => metadata.len(),
        Err(_) => return format!("Image file not found: {}", path),
    };
    if size == 0 || size > 20 * 1024 * 1024 {
        return "Wan 3 image size must be greater than zero and at most 20MB".into();
    }
    match image::image_dimensions(p) {
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
    let ext = p
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["mp4", "mov"].contains(&ext.as_str()) {
        return "Wan 3 video format must be MP4 or MOV".into();
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
        assert!(!validate_q4_cost("img2video", 5, "1080p", Some("pro")).is_empty());
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
        assert!(!validate_wan_cost("text2video", 5, "720p", None, Some("speed"), 1).is_empty());
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
        assert!(validate_task_body(&legacy).contains("only for Wan 3"));
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
        assert!(validate_reference_video_file(video.to_str().unwrap()).contains("50MB"));
        let image = directory.path().join("small.png");
        image::RgbImage::new(240, 240).save(&image).unwrap();
        assert_eq!(validate_wan_image_file(image.to_str().unwrap()), "");
        image::RgbImage::new(239, 240).save(&image).unwrap();
        assert!(!validate_wan_image_file(image.to_str().unwrap()).is_empty());
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
        assert!(validate_task_body(&character).contains("does not support transition"));

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
        assert!(validate_task_body(&character).contains("normal generation mode"));
        assert!(!validate_drama_cost("text2video", 5, "540p", None).is_empty());
        assert!(!validate_drama_cost("text2video", 5, "720p", Some("pro")).is_empty());
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
            assert_eq!(
                preferred_schedule_mode(model, Some("nopeak")),
                Ok(Some("nopeak".into()))
            );
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
        assert!(validate_task_body(&make_body("img2video", "3.1", 5, 0, 0))
            .contains("requires exactly 1 image"));
    }

    #[test]
    fn task_body_img2video_correct() {
        let mut body = make_body("img2video", "3.1", 5, 1, 0);
        body["settings"]["transition"] = json!("pro");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_headtail_wrong_count() {
        assert!(
            validate_task_body(&make_body("headtailimg2video", "3.1", 5, 1, 0))
                .contains("requires exactly 2 images")
        );
    }

    #[test]
    fn task_body_character2video_too_many_refs() {
        assert!(
            validate_task_body(&make_body("character2video", "3.1", 5, 4, 4)).contains("max 7")
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
    fn task_body_3_2_a_duration_rejects_auto() {
        assert!(
            validate_task_body(&make_body("text2video", "3.2_a", -1, 0, 0))
                .contains("must be 4-15")
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
    fn task_body_char2video_non_3_2_rejects_transition() {
        let mut body = make_body("character2video", "3.1", 5, 1, 0);
        body["settings"]["transition"] = json!("pro");
        assert!(validate_task_body(&body).contains("should not include transition"));
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
        body["settings"]["aspect_ratio"] = json!("21:9");
        assert!(validate_task_body(&body).contains("Invalid aspect_ratio"));
    }

    #[test]
    fn task_body_text2image_rejects_transition() {
        let mut body = make_body("text2image", "3.1", 0, 0, 0);
        body["settings"]["transition"] = json!("pro");
        assert!(validate_task_body(&body).contains("should not include transition"));
    }

    #[test]
    fn task_body_text2video_non_3_2_rejects_transition() {
        let mut body = make_body("text2video", "3.1", 5, 0, 0);
        body["settings"]["transition"] = json!("pro");
        assert!(validate_task_body(&body).contains("should not include transition"));
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
    fn task_body_headtailimg2video_3_1_requires_transition() {
        assert!(
            validate_task_body(&make_body("headtailimg2video", "3.1", 5, 2, 0))
                .contains("requires transition")
        );
    }

    #[test]
    fn task_body_headtailimg2video_3_2_requires_transition() {
        assert!(
            validate_task_body(&make_body("headtailimg2video", "3.2", 5, 2, 0))
                .contains("requires transition")
        );
    }

    #[test]
    fn task_body_headtailimg2video_valid_transition() {
        let mut body = make_body("headtailimg2video", "3.1", 5, 2, 0);
        body["settings"]["transition"] = json!("speed");
        assert_eq!(validate_task_body(&body), "");
    }

    #[test]
    fn task_body_headtailimg2video_invalid_transition() {
        let mut body = make_body("headtailimg2video", "3.2", 5, 2, 0);
        body["settings"]["transition"] = json!("creative");
        assert!(validate_task_body(&body).contains("Invalid transition"));
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
        let body = json!({
            "type": "text2video",
            "input": { "prompts": [{"type": "text", "content": "test"}] },
            "settings": { "duration": 5, "model_version": "3.2", "resolution": "1080p", "schedule_mode": "normal" }
        });
        assert!(validate_task_body(&body).contains("enhance is required"));
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
