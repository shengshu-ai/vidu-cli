use super::{task_ops, tasks, upload};
use crate::{client, validators};
use serde_json::{json, Value};
use std::{collections::HashMap, path::Path, process::Command};

enum Source<'a> {
    Creation(i64),
    Remote(&'a str),
    Local { path: &'a str, seconds: f64 },
}

struct Prepared<'a> {
    source: Source<'a>,
    duration: i32,
}

fn invalid(message: &str) -> ! {
    client::fail("client_error", message, None, None, Some("task_upscale"))
}

fn endpoint(path: &str) -> String {
    format!(
        "{}/vidu/v1/{path}",
        client::base_url().trim_end_matches('/')
    )
}

fn rounded_duration(seconds: f64) -> Result<i32, String> {
    let rounded = seconds.round();
    if !seconds.is_finite() || seconds <= 0.0 || rounded < 1.0 || rounded > f64::from(i32::MAX) {
        return Err(
            "Source video duration must round to a positive 32-bit integer in seconds".into(),
        );
    }
    Ok(rounded as i32)
}

fn remote_input(input: &str) -> Result<bool, String> {
    if input.starts_with("ssupload:") {
        let url = reqwest::Url::parse(input).map_err(|_| "Invalid ssupload URI")?;
        let ids: Vec<_> = url.query_pairs().filter(|(key, _)| key == "id").collect();
        if !input.starts_with("ssupload:?")
            || url.fragment().is_some()
            || ids.len() != 1
            || ids[0].1.trim().is_empty()
        {
            return Err("Expected ssupload:?id=... with one nonempty upload ID".into());
        }
        return Ok(true);
    }
    if input.starts_with("http:") || input.starts_with("https:") {
        let url = reqwest::Url::parse(input).map_err(|_| "Invalid video URL")?;
        let authority = input
            .split_once("://")
            .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""));
        if url.host_str().is_none()
            || authority.is_none_or(str::is_empty)
            || input.chars().any(|c| c.is_whitespace() || c == '\\')
        {
            return Err("Video URL must be an HTTP/HTTPS address with a hostname".into());
        }
        return Ok(true);
    }
    Ok(false)
}

fn probe_duration(data: &Value) -> Option<f64> {
    let video = data["streams"]
        .as_array()?
        .iter()
        .find(|stream| stream["codec_type"].as_str() == Some("video"))?;
    let seconds = |value: &Value| {
        value
            .as_f64()
            .or_else(|| value.as_str()?.parse::<f64>().ok())
            .filter(|duration| rounded_duration(*duration).is_ok())
    };
    seconds(&video["duration"]).or_else(|| seconds(&data["format"]["duration"]))
}

fn local_duration(path: &str) -> Result<f64, String> {
    if let Some(seconds) =
        tasks::read_video_duration_f64(path).filter(|seconds| rounded_duration(*seconds).is_ok())
    {
        return Ok(seconds);
    }
    // Pass an absolute local path so ffprobe cannot interpret it as an option or URL.
    let absolute = Path::new(path).canonicalize().map_err(|e| e.to_string())?;
    let result = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,duration:format=duration",
            "-of",
            "json",
        ])
        .arg(absolute)
        .output();
    if let Ok(output) = result {
        if output.status.success() {
            if let Ok(data) = serde_json::from_slice::<Value>(&output.stdout) {
                if let Some(duration) = probe_duration(&data) {
                    return Ok(duration);
                }
            }
        }
    }
    Err(format!("Cannot read video duration from '{path}'. Ensure the file has a valid video track and install ffprobe for formats the built-in MP4 reader cannot parse."))
}

fn prepare<'a>(
    creation_id: Option<i64>,
    video: Option<&'a str>,
    resolution: &str,
    duration: Option<i32>,
) -> Prepared<'a> {
    if !["720p", "1080p", "2k", "4k"].contains(&resolution) {
        invalid("resolution must be 720p, 1080p, 2k or 4k");
    }
    match (creation_id, video) {
        (Some(id), None) => {
            if id <= 0 || duration.is_some() {
                invalid("creation_id must be positive; --duration cannot override a creation's duration");
            }
            let detail = client::request_json(
                "GET",
                &endpoint(&format!("creations/{id}/detail")),
                Some("task_upscale_source"),
                None,
                None,
            );
            let creation = &detail["creation"];
            if creation["type"].as_str() != Some("video")
                || detail["task"]["state"].as_str() != Some("success")
                || creation["is_deleted"].as_bool() == Some(true)
                || creation["err_code"]
                    .as_str()
                    .is_some_and(|code| !code.is_empty())
            {
                invalid("Source must be an accessible completed video creation");
            }
            let seconds = creation["duration"].as_f64().unwrap_or(f64::NAN);
            let duration = rounded_duration(seconds).unwrap_or_else(|e| {
                client::fail("parse_error", &e, None, None, Some("task_upscale_source"))
            });
            Prepared {
                source: Source::Creation(id),
                duration,
            }
        }
        (None, Some(video)) => {
            if remote_input(video).unwrap_or_else(|e| invalid(&e)) {
                let duration = duration.filter(|value| *value > 0)
                    .unwrap_or_else(|| invalid("--duration (positive integer seconds) is required for URL/ssupload video input; it must describe the full source video"));
                Prepared {
                    source: Source::Remote(video),
                    duration,
                }
            } else {
                if duration.is_some() {
                    invalid("--duration cannot override a local video's duration");
                }
                let error = validators::validate_video_file(video);
                if !error.is_empty() {
                    invalid(&error);
                }
                if std::fs::metadata(video).map(|m| m.len()).unwrap_or(0) == 0 {
                    invalid("Video file must not be empty");
                }
                let seconds = local_duration(video).unwrap_or_else(|e| invalid(&e));
                Prepared {
                    source: Source::Local {
                        path: video,
                        seconds,
                    },
                    duration: rounded_duration(seconds).unwrap_or_else(|e| invalid(&e)),
                }
            }
        }
        _ => invalid("Exactly one of --creation-id or --video is required"),
    }
}

fn settings(resolution: &str, duration: i32) -> Value {
    json!({"resolution":resolution,"duration":duration,"sample_count":1,"schedule_mode":"normal"})
}

pub fn submit(
    creation_id: Option<i64>,
    video: Option<&str>,
    resolution: &str,
    duration: Option<i32>,
) -> Value {
    let prepared = prepare(creation_id, video, resolution, duration);
    let input = match prepared.source {
        Source::Creation(id) => json!({"creation_id":id}),
        source => {
            let uri = match source {
                Source::Remote(uri) => uri.to_owned(),
                Source::Local { path, seconds } => {
                    let mut metadata = serde_json::Map::new();
                    metadata.insert("duration".into(), json!(seconds.to_string()));
                    upload::upload_media_and_get_uri_with_metadata(path, Some(metadata))
                }
                Source::Creation(_) => unreachable!(),
            };
            json!({"prompts":[{"type":"video","content":uri}]})
        }
    };
    let body =
        json!({"type":"upscale","input":input,"settings":settings(resolution, prepared.duration)});
    // A paid creation is never retried after an ambiguous transport/server failure.
    task_ops::submission(client::request_json(
        "POST",
        &endpoint("tasks/tool"),
        Some("task_upscale"),
        Some(&body),
        None,
    ))
}

pub fn cost(
    creation_id: Option<i64>,
    video: Option<&str>,
    resolution: &str,
    duration: Option<i32>,
) {
    let prepared = prepare(creation_id, video, resolution, duration);
    let mut params = HashMap::from([("type".to_owned(), "upscale".to_owned())]);
    for (key, value) in settings(resolution, prepared.duration).as_object().unwrap() {
        params.insert(
            format!("settings.{key}"),
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        );
    }
    let data = client::request_json(
        "GET",
        &endpoint("tasks/credits"),
        Some("task_upscale_cost"),
        None,
        Some(&params),
    );
    tasks::output_credits_result(&data);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_rounding_and_boundaries() {
        for (seconds, expected) in [
            (0.5, 1),
            (5.49, 5),
            (5.5, 6),
            (f64::from(i32::MAX), i32::MAX),
        ] {
            assert_eq!(rounded_duration(seconds).unwrap(), expected);
        }
        for seconds in [
            0.0,
            -1.0,
            0.49,
            f64::NAN,
            f64::INFINITY,
            f64::from(i32::MAX) + 0.5,
        ] {
            assert!(rounded_duration(seconds).is_err());
        }
    }

    #[test]
    fn probe_requires_video_and_finite_duration() {
        assert_eq!(
            probe_duration(&json!({"streams":[{"codec_type":"audio"}],"format":{"duration":"8"}})),
            None
        );
        assert_eq!(
            probe_duration(
                &json!({"streams":[{"codec_type":"video","duration":"N/A"}],"format":{"duration":"5.5"}})
            ),
            Some(5.5)
        );
        assert_eq!(
            probe_duration(
                &json!({"streams":[{"codec_type":"video","duration":"NaN"}],"format":{"duration":"inf"}})
            ),
            None
        );
    }

    #[test]
    fn remote_uri_validation() {
        for uri in [
            "ssupload:",
            "ssupload:?id=",
            "ssupload:?id=a&id=b",
            "ssupload:?id=%20",
            "ssupload:?id=a#b",
            "https:///a",
            "https://",
            "http:foo",
            "https://exa mple.com/a",
        ] {
            assert!(remote_input(uri).is_err(), "{uri}");
        }
        for uri in [
            "ssupload:?id=123",
            "https://example.com/v.mp4?signature=a%2Bb",
            "http://example.com/v",
        ] {
            assert_eq!(remote_input(uri), Ok(true));
        }
        assert_eq!(remote_input("./video.mp4"), Ok(false));
    }
}
