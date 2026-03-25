use crate::{client, validators};
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn preprocess(name: &str, elem_type: &str, images: &[String]) {
    if images.is_empty() || images.len() > 3 {
        client::fail("client_error", "Must provide 1-3 images", None, None, None);
    }

    let components: Vec<Value> = images.iter().enumerate().map(|(i, img)| {
        json!({
            "content": img,
            "src_img": img,
            "content_type": "image",
            "type": if i == 0 { "main" } else { "auxiliary" }
        })
    }).collect();

    let body = json!({
        "components": components,
        "name": name,
        "type": elem_type
    });

    let err = validators::validate_element_preprocess(&body);
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
    }

    let base = client::base_url();
    let data = client::request_json("POST", &format!("{}/vidu/v1/material/elements/pre-process", base), None, Some(&body), None);
    client::ok(data);
}

pub fn create(
    elem_id: &str, name: &str, modality: &str, elem_type: &str,
    images: &[String], version: &str, description: &str,
) {
    if images.is_empty() || images.len() > 3 {
        client::fail("client_error", "Must provide 1-3 images", None, None, None);
    }

    let components: Vec<Value> = images.iter().enumerate().map(|(i, img)| {
        json!({
            "content": img,
            "src_img": img,
            "content_type": "image",
            "type": if i == 0 { "main" } else { "auxiliary" }
        })
    }).collect();

    let body = json!({
        "id": elem_id,
        "name": name,
        "modality": modality,
        "type": elem_type,
        "components": components,
        "version": version,
        "recaption": { "description": description }
    });

    let err = validators::validate_element_create(&body);
    if !err.is_empty() {
        client::fail("client_error", &err, None, None, None);
    }

    let base = client::base_url();
    let data = client::request_json("POST", &format!("{}/vidu/v1/material/elements", base), None, Some(&body), None);
    let id = data.get("id").map(|v| match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }).unwrap_or_default();
    let ver = data.get("version").map(|v| match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }).unwrap_or_default();
    client::ok(json!({"id": id, "version": ver, "raw": data}));
}

pub fn list_elements(keyword: Option<&str>, page: i64, pagesz: i64) {
    let mut params = HashMap::new();
    params.insert("pager.page".into(), page.to_string());
    params.insert("pager.pagesz".into(), pagesz.to_string());
    params.insert("modalities".into(), "image".into());
    if let Some(kw) = keyword {
        params.insert("keyword".into(), kw.to_string());
    }

    let base = client::base_url();
    let data = client::request_json("GET", &format!("{}/vidu/v1/material/elements/personal", base), None, None, Some(&params));
    let elements = data.get("elements").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let items: Vec<Value> = elements.iter().map(|e| {
        json!({
            "id": e.get("id").map(|v| match v { Value::String(s) => s.clone(), Value::Number(n) => n.to_string(), _ => String::new() }).unwrap_or_default(),
            "version": e.get("version").map(|v| match v { Value::String(s) => s.clone(), Value::Number(n) => n.to_string(), _ => String::new() }).unwrap_or_default(),
            "name": e.get("name").and_then(|v| v.as_str()).unwrap_or("")
        })
    }).collect();
    let next_token = data.get("next_page_token").and_then(|v| v.as_str()).unwrap_or("");
    client::ok(json!({"elements": items, "next_page_token": next_token}));
}

pub fn search(keyword: &str, pagesz: i64, sort_by: &str, page_token: &str) {
    if keyword.is_empty() {
        client::fail("client_error", "--keyword is required for search", None, None, None);
    }
    let mut params = HashMap::new();
    params.insert("keyword".into(), keyword.into());
    params.insert("pager.page_token".into(), page_token.into());
    params.insert("pager.pagesz".into(), pagesz.to_string());
    params.insert("modalities".into(), "image".into());
    params.insert("sort_by".into(), sort_by.into());
    params.insert("is_like".into(), "false".into());
    params.insert("is_collect".into(), "false".into());

    let base = client::base_url();
    let data = client::request_json("GET", &format!("{}/vidu/v1/material/share_elements/feed", base), None, None, Some(&params));
    let share_elements = data.get("share_elements").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let items: Vec<Value> = share_elements.iter().map(|se| {
        let empty_obj = json!({});
        let empty_arr = json!([]);
        let el = se.get("element").unwrap_or(&empty_obj);
        let sh = se.get("share").unwrap_or(&empty_obj);
        let recaption = el.get("recaption").unwrap_or(&empty_obj);
        let desc: String = recaption.get("description").and_then(|v| v.as_str()).unwrap_or("").chars().take(100).collect();
        json!({
            "id": el.get("id").map(|v| match v { Value::String(s) => s.clone(), Value::Number(n) => n.to_string(), _ => String::new() }).unwrap_or_default(),
            "version": el.get("version").map(|v| match v { Value::String(s) => s.clone(), Value::Number(n) => n.to_string(), _ => String::new() }).unwrap_or_default(),
            "name": el.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            "description": desc,
            "category": sh.get("category_display").unwrap_or(&empty_arr)
        })
    }).collect();
    let next_token = data.get("next_page_token").and_then(|v| v.as_str()).unwrap_or("");
    client::ok(json!({"elements": items, "next_page_token": next_token}));
}
