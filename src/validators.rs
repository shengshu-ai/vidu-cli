use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

fn set(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn valid_task_types() -> HashSet<String> {
    set(&["text2image", "text2video", "img2video", "headtailimg2video", "reference2image", "character2video"])
}

fn valid_model_versions() -> HashSet<String> {
    set(&["3.0", "3.1", "3.2", "3.2_fast_m", "3.2_pro_m"])
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
    m.insert("text2video".into(), tv.clone());
    m.insert("img2video".into(), tv.clone());
    m.insert("headtailimg2video".into(), tv);
    let mut cv = HashMap::new();
    cv.insert("3.0".into(), (5, 5));
    cv.insert("3.1".into(), (2, 8));
    cv.insert("3.1_pro".into(), (-1, 8));
    cv.insert("3.2".into(), (1, 16));
    m.insert("character2video".into(), cv);
    let mut ri = HashMap::new();
    ri.insert("3.1".into(), (0, 0));
    ri.insert("3.2_fast_m".into(), (0, 0));
    ri.insert("3.2_pro_m".into(), (0, 0));
    m.insert("reference2image".into(), ri.clone());
    m.insert("text2image".into(), ri);
    m
}

fn model_support() -> HashMap<String, HashSet<String>> {
    let mut m = HashMap::new();
    m.insert("text2image".into(), set(&["3.1", "3.2_fast_m", "3.2_pro_m"]));
    m.insert("text2video".into(), set(&["3.0", "3.1", "3.2"]));
    m.insert("img2video".into(), set(&["3.0", "3.1", "3.2"]));
    m.insert("headtailimg2video".into(), set(&["3.0", "3.1", "3.2"]));
    m.insert("reference2image".into(), set(&["3.1", "3.2_fast_m", "3.2_pro_m"]));
    m.insert("character2video".into(), set(&["3.0", "3.1", "3.1_pro", "3.2"]));
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
        return format!("Invalid type '{}'. Valid: {}", task_type, sorted_join(&valid_task_types()));
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
    let image_count = prompts_arr.iter().filter(|p| p.get("type").and_then(|v| v.as_str()) == Some("image")).count();
    let material_count = prompts_arr.iter().filter(|p| p.get("type").and_then(|v| v.as_str()) == Some("material")).count();

    // Validate counts by task type
    match task_type {
        "img2video" if image_count != 1 => return format!("img2video requires exactly 1 image, got {}", image_count),
        "headtailimg2video" if image_count != 2 => return format!("headtailimg2video requires exactly 2 images, got {}", image_count),
        "reference2image" | "character2video" if image_count + material_count > 7 => {
            return format!("{} allows max 7 images+materials, got {}", task_type, image_count + material_count);
        }
        _ => {}
    }

    let settings = match body.get("settings") {
        Some(Value::Object(_)) => body.get("settings").unwrap(),
        _ => return "settings must be an object".into(),
    };

    let model_version = settings.get("model_version").and_then(|v| v.as_str()).unwrap_or("2.0");
    if !valid_model_versions().contains(model_version) {
        return format!("Invalid model_version '{}'. Valid: {}", model_version, sorted_join(&valid_model_versions()));
    }

    let ms = model_support();
    if let Some(supported) = ms.get(task_type) {
        if !supported.contains(model_version) {
            return format!("model_version {} does not support {}", model_version, task_type);
        }
    }

    let dr = duration_ranges();
    if let Some(type_ranges) = dr.get(task_type) {
        let duration = settings.get("duration").and_then(|v| v.as_i64()).unwrap_or(0);
        if let Some(&(min_d, max_d)) = type_ranges.get(model_version) {
            if min_d > 0 && (duration < min_d || duration > max_d) {
                return format!("duration {} out of range [{}, {}] for {} with {}", duration, min_d, max_d, task_type, model_version);
            }
        }
    }

    if settings.get("resolution").is_none() {
        return format!("resolution is required for {}", task_type);
    }
    let res = settings.get("resolution").and_then(|v| v.as_str()).unwrap_or("");
    let rs = resolution_support();
    let valid_res = rs.get(task_type).cloned().unwrap_or_else(|| set(&["1080p"]));
    if !valid_res.contains(res) {
        return format!("Invalid resolution '{}' for {}. Valid: {}", res, task_type, sorted_join(&valid_res));
    }

    let no_ar_types = ["img2video", "headtailimg2video", "text2image"];
    if !no_ar_types.contains(&task_type) {
        if let Some(ar_val) = settings.get("aspect_ratio").and_then(|v| v.as_str()) {
            if !valid_aspect_ratios().contains(ar_val) {
                return format!("Invalid aspect_ratio '{}'. Valid: {}", ar_val, sorted_join(&valid_aspect_ratios()));
            }
        }
    }

    if settings.get("transition").is_some() {
        let no_trans = ["reference2image", "text2image"];
        if no_trans.contains(&task_type) {
            return format!("{} should not include transition", task_type);
        }
        if task_type == "text2video" && model_version != "3.2" {
            return format!("text2video with {} should not include transition (only 3.2 supports pro/speed)", model_version);
        }
        if task_type == "img2video" || task_type == "headtailimg2video" {
            let transition = settings.get("transition").and_then(|v| v.as_str()).unwrap_or("");
            let valid_trans = if model_version == "3.0" {
                set(&["creative", "stable"])
            } else {
                set(&["pro", "speed"])
            };
            if !valid_trans.contains(transition) {
                return format!("Invalid transition '{}' for {} {}. Valid: {}", transition, task_type, model_version, sorted_join(&valid_trans));
            }
        }
    }

    if task_type == "character2video" {
        if model_version == "3.2" {
            match settings.get("transition").and_then(|v| v.as_str()) {
                None => return "character2video with 3.2 requires transition parameter (speed or pro)".into(),
                Some(trans) if trans != "speed" && trans != "pro" => {
                    return format!("Invalid transition '{}' for character2video 3.2. Valid: pro, speed", trans);
                }
                _ => {}
            }
        } else if settings.get("transition").is_some() {
            return format!("character2video with {} should not include transition (only 3.2 supports pro/speed)", model_version);
        }
    }

    if input.get("enhance").is_none() {
        return "input.enhance is required (true or false)".into();
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
    let main_count = components.iter().filter(|c| c.get("type").and_then(|v| v.as_str()) == Some("main")).count();
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
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !["mp4", "mov", "avi"].contains(&ext.as_str()) {
        return format!("Invalid video format '{}'. Supported: mp4, mov, avi", ext);
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > 500 * 1024 * 1024 {
        return format!("Video file too large ({:.1}MB). Max: 500MB", size as f64 / 1024.0 / 1024.0);
    }
    String::new()
}

pub fn validate_audio_file(path: &str) -> String {
    let p = Path::new(path);
    if !p.is_file() {
        return format!("Audio file not found: {}", path);
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !["mp3", "wav", "aac", "m4a"].contains(&ext.as_str()) {
        return format!("Invalid audio format '{}'. Supported: mp3, wav, aac, m4a", ext);
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > 100 * 1024 * 1024 {
        return format!("Audio file too large ({:.1}MB). Max: 100MB", size as f64 / 1024.0 / 1024.0);
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
        if char_count < 2 || char_count > 1000 {
            return format!("Chinese text must be 2-1000 characters, got {}", char_count);
        }
    } else if char_count < 4 || char_count > 2000 {
        return format!("English text must be 4-2000 characters, got {}", char_count);
    }
    String::new()
}

pub fn all_voice_ids() -> Vec<&'static str> {
    [
        "male-qn-qingse", "male-qn-jingying", "male-qn-badao", "male-qn-daxuesheng",
        "female-shaonv", "female-yujie", "female-chengshu", "female-tianmei",
        "male-qn-qingse-jingpin", "male-qn-jingying-jingpin", "male-qn-badao-jingpin",
        "male-qn-daxuesheng-jingpin", "female-shaonv-jingpin", "female-yujie-jingpin",
        "female-chengshu-jingpin", "female-tianmei-jingpin",
        "clever_boy", "cute_boy", "lovely_girl", "cartoon_pig",
        "bingjiao_didi", "junlang_nanyou", "chunzhen_xuedi", "lengdan_xiongzhang",
        "badao_shaoye", "tianxin_xiaoling", "qiaopi_mengmei", "wumei_yujie",
        "diadia_xuemei", "danya_xuejie",
        "Chinese (Mandarin)_Reliable_Executive", "Chinese (Mandarin)_News_Anchor",
        "Chinese (Mandarin)_Mature_Woman", "Chinese (Mandarin)_Unrestrained_Young_Man",
        "Arrogant_Miss", "Robot_Armor",
        "Chinese (Mandarin)_Kind-hearted_Antie", "Chinese (Mandarin)_HK_Flight_Attendant",
        "Chinese (Mandarin)_Humorous_Elder", "Chinese (Mandarin)_Gentleman",
        "Chinese (Mandarin)_Warm_Bestie", "Chinese (Mandarin)_Male_Announcer",
        "Chinese (Mandarin)_Sweet_Lady", "Chinese (Mandarin)_Southern_Young_Man",
        "Chinese (Mandarin)_Wise_Women", "Chinese (Mandarin)_Gentle_Youth",
        "Chinese (Mandarin)_Warm_Girl", "Chinese (Mandarin)_Kind-hearted_Elder",
        "Chinese (Mandarin)_Cute_Spirit", "Chinese (Mandarin)_Radio_Host",
        "Chinese (Mandarin)_Lyrical_Voice", "Chinese (Mandarin)_Straightforward_Boy",
        "Chinese (Mandarin)_Sincere_Adult", "Chinese (Mandarin)_Gentle_Senior",
        "Chinese (Mandarin)_Stubborn_Friend", "Chinese (Mandarin)_Crisp_Girl",
        "Chinese (Mandarin)_Pure-hearted_Boy", "Chinese (Mandarin)_Soft_Girl",
        "Cantonese_ProfessionalHost（F)", "Cantonese_GentleLady",
        "Cantonese_ProfessionalHost（M)", "Cantonese_PlayfulMan",
        "Cantonese_CuteGirl", "Cantonese_KindWoman",
        "Grinch", "Rudolph", "Arnold", "Charming_Santa", "Charming_Lady",
        "Sweet_Girl", "Cute_Elf", "Attractive_Girl", "Serene_Woman",
        "English_Trustworthy_Man", "English_Graceful_Lady", "English_Aussie_Bloke",
        "English_Whispering_girl", "English_Diligent_Man", "English_Gentle-voiced_man",
    ].to_vec()
}

pub fn validate_voice_id(voice_id: &str) -> String {
    let valid: HashSet<String> = all_voice_ids().iter().map(|s| s.to_string()).collect();
    if !valid.contains(voice_id) {
        return format!("Invalid voice_id '{}'", voice_id);
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
