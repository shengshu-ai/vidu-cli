use crate::{client, validators};
use image::GenericImageView;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;

const MAX_SIZE_MB: u64 = 10;

fn compress_image_if_needed(path: &str) -> (Vec<u8>, u32, u32, String) {
    let file_size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let img = match image::open(path) {
        Ok(i) => i,
        Err(e) => client::fail("client_error", &format!("Cannot read image: {}", e), None, None, None),
    };
    let (width, height) = img.dimensions();

    let mime = mime_guess::from_path(path).first_or_octet_stream().to_string();
    let mime = if mime.starts_with("image/") { mime } else { "image/jpeg".into() };

    if file_size <= MAX_SIZE_MB * 1024 * 1024 {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => client::fail("client_error", &format!("Cannot read file: {}", e), None, None, None),
        };
        return (bytes, width, height, mime);
    }

    // Compress with progressive quality reduction
    let rgb_img = img.to_rgb8();
    let mut quality = 95u8;
    while quality >= 10 {
        let mut buf = Cursor::new(Vec::new());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
        if rgb_img.write_with_encoder(encoder).is_ok() {
            let data = buf.into_inner();
            if (data.len() as u64) <= MAX_SIZE_MB * 1024 * 1024 {
                return (data, width, height, "image/jpeg".into());
            }
        }
        quality = quality.saturating_sub(5);
    }

    // Still too large, resize to 1920x1080
    let resized = img.resize(1920, 1080, image::imageops::FilterType::Lanczos3);
    let (rw, rh) = resized.dimensions();
    let rgb_resized = resized.to_rgb8();
    let mut buf = Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85);
    if let Err(e) = rgb_resized.write_with_encoder(encoder) {
        client::fail("client_error", &format!("Failed to compress image: {}", e), None, None, None);
    }
    (buf.into_inner(), rw, rh, "image/jpeg".into())
}

pub fn run(image_path: &str) {
    let uri = upload_and_get_uri(image_path);
    let upload_id = uri.strip_prefix("ssupload:?id=").unwrap_or("");
    client::ok(json!({"upload_id": upload_id, "ssupload_uri": uri}));
}

pub fn upload_and_get_uri(image_path: &str) -> String {
    let err = validators::validate_image_file(image_path);
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
    }

    let (image_bytes, width, height, mime) = compress_image_if_needed(image_path);

    // Step 1: Create upload
    let body = json!({
        "metadata": {
            "image-width": width.to_string(),
            "image-height": height.to_string(),
        },
        "scene": "vidu",
    });
    let base = client::base_url();
    let data = client::request_json(
        "POST",
        &format!("{}/tools/v1/files/uploads", base),
        Some("create_upload"),
        Some(&body),
        None,
    );
    let upload_id_str = data.get("id").map(|v| match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }).unwrap_or_default();
    let put_url = data.get("put_url").and_then(|v| v.as_str()).unwrap_or("");
    if upload_id_str.is_empty() || put_url.is_empty() {
        client::fail("parse_error", &format!("Unexpected create_upload response: {}", data), None, None, Some("create_upload"));
    }

    // Step 2: PUT image bytes
    let mut put_headers = HashMap::new();
    put_headers.insert("Content-Type".into(), mime);
    put_headers.insert("x-amz-meta-image-width".into(), width.to_string());
    put_headers.insert("x-amz-meta-image-height".into(), height.to_string());
    let (etag,) = client::put_raw(put_url, image_bytes, &put_headers, Some("put_image"));

    // Step 3: Finish upload
    let finish_body = json!({"etag": etag, "id": upload_id_str.clone()});
    client::request_json(
        "PUT",
        &format!("{}/tools/v1/files/uploads/{}/finish", base, upload_id_str),
        Some("finish_upload"),
        Some(&finish_body),
        None,
    );

    format!("ssupload:?id={}", upload_id_str)
}
