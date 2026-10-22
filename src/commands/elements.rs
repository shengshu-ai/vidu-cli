use crate::{client, validators};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub fn check(name: &str) {
    let base = client::base_url();
    let mut params = HashMap::new();
    params.insert("name".into(), name.into());
    let data = client::request_json(
        "GET",
        &format!("{}/vidu/v1/material/elements/create/check", base),
        None,
        None,
        Some(&params),
    );
    client::ok(data);
}

pub fn preprocess(name: &str, elem_type: &str, images: &[String]) {
    if images.is_empty() || images.len() > 3 {
        client::fail("client_error", "Must provide 1-3 images", None, None, None);
    }

    let components: Vec<Value> = images
        .iter()
        .enumerate()
        .map(|(i, img)| {
            json!({
                "content": img,
                "src_img": img,
                "content_type": "image",
                "type": if i == 0 { "main" } else { "auxiliary" }
            })
        })
        .collect();

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
    let data = client::request_json(
        "POST",
        &format!("{}/vidu/v1/material/elements/pre-process", base),
        None,
        Some(&body),
        None,
    );

    let id = data
        .get("id")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();

    let creator_id = data
        .get("creator_id")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();

    let style = data
        .get("recaption")
        .and_then(|r| r.get("style"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let description = data
        .get("recaption")
        .and_then(|r| r.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    client::ok(json!({
        "id": id,
        "creator_id": creator_id,
        "style": style,
        "description": description
    }));
}

fn extract_string_id(data: &Value, key: &str) -> String {
    data.get(key)
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default()
}

pub fn create(
    name: &str,
    modality: &str,
    elem_type: &str,
    images: &[String],
    description: Option<&str>,
    style: Option<&str>,
) {
    if images.is_empty() || images.len() > 3 {
        client::fail("client_error", "Must provide 1-3 images", None, None, None);
    }

    // Validate user-provided recaption fields
    if let Some(s) = style {
        if s.chars().count() > 64 {
            client::fail(
                "client_error",
                "recaption.style must not exceed 64 characters",
                None,
                None,
                None,
            );
        }
    }
    if let Some(d) = description {
        let char_count = d.chars().count();
        if d.is_empty() || char_count > 1280 {
            client::fail(
                "client_error",
                "recaption.description must be 1-1280 characters",
                None,
                None,
                None,
            );
        }
    }

    let base = client::base_url();

    // Step 1: Check name availability
    let mut check_params = HashMap::new();
    check_params.insert("name".into(), name.into());
    let _check = client::request_json(
        "GET",
        &format!("{}/vidu/v1/material/elements/create/check", base),
        Some("check_name"),
        None,
        Some(&check_params),
    );

    // Step 2: Process images (auto-upload local files / URLs)
    let processed_images: Vec<String> = images
        .iter()
        .map(|img| crate::commands::tasks::process_image_input(img))
        .collect();

    // Step 3: Preprocess
    let components: Vec<Value> = processed_images
        .iter()
        .enumerate()
        .map(|(i, img)| {
            json!({
                "content": img,
                "src_img": img,
                "content_type": "image",
                "type": if i == 0 { "main" } else { "auxiliary" }
            })
        })
        .collect();

    let preprocess_body = json!({
        "components": components,
        "name": name,
        "type": elem_type
    });

    let prep_data = client::request_json(
        "POST",
        &format!("{}/vidu/v1/material/elements/pre-process", base),
        Some("preprocess"),
        Some(&preprocess_body),
        None,
    );

    let elem_id = extract_string_id(&prep_data, "id");
    let creator_id = extract_string_id(&prep_data, "creator_id");

    let prep_style = prep_data
        .get("recaption")
        .and_then(|r| r.get("style"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let prep_desc = prep_data
        .get("recaption")
        .and_then(|r| r.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Use user-provided or preprocess values
    let final_style = style.unwrap_or(prep_style);
    let final_desc = description.unwrap_or(prep_desc);

    // Step 4: Create
    let mut recaption = json!({"description": final_desc});
    if !final_style.is_empty() {
        recaption["style"] = json!(final_style);
    }

    let create_body = json!({
        "id": elem_id,
        "name": name,
        "modality": modality,
        "type": elem_type,
        "components": components,
        "creator_id": creator_id,
        "recaption": recaption
    });

    let data = client::request_json(
        "POST",
        &format!("{}/vidu/v1/material/elements", base),
        Some("create"),
        Some(&create_body),
        None,
    );

    let id = extract_string_id(&data, "id");
    let ver = extract_string_id(&data, "version");
    client::ok(json!({"id": id, "version": ver}));
}

fn require_pagesz(pagesz: i64, minimum: i64) {
    if !(minimum..=100).contains(&pagesz) {
        client::fail(
            "client_error",
            &format!("--pagesz must be between {minimum} and 100"),
            None,
            None,
            None,
        );
    }
}

fn require_id(id: &str) {
    if id.parse::<i64>().map_or(true, |id| id <= 0) {
        client::fail(
            "client_error",
            "element ID must be a positive integer from the API response",
            None,
            None,
            None,
        );
    }
}

// Numeric page compatibility walks real cursors; never send an offset to this API.
fn walk_personal_pages<F>(page: i64, first_token: &str, mut fetch: F) -> Result<Value, String>
where
    F: FnMut(&str) -> Value,
{
    let mut token = first_token.to_string();
    let mut seen = HashSet::new();
    seen.insert(token.clone());
    for current_page in 0..=page {
        let mut data = fetch(&token);
        let next = data
            .get("next_page_token")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if !next.is_empty() && !seen.insert(next.clone()) {
            return Err("Server returned a repeated pagination cursor".into());
        }
        if current_page == page {
            // Always expose a string, even when the server omits the final cursor.
            data["next_page_token"] = json!(next);
            return Ok(data);
        }
        if next.is_empty() {
            return Ok(json!({"elements": [], "next_page_token": ""}));
        }
        token = next;
    }
    unreachable!()
}

pub fn list_elements(keyword: Option<&str>, page: i64, pagesz: i64, page_token: Option<&str>) {
    require_pagesz(pagesz, 10);
    if page < 0 || (page != 0 && page_token.is_some()) {
        client::fail(
            "client_error",
            "--page must be nonnegative and cannot be combined with --page-token",
            None,
            None,
            None,
        );
    }
    let mut params = HashMap::new();
    params.insert("pager.pagesz".into(), pagesz.to_string());
    params.insert("modalities".into(), "image".into());
    if let Some(kw) = keyword {
        params.insert("keyword".into(), kw.to_string());
    }
    let base = client::base_url();
    let data = walk_personal_pages(page, page_token.unwrap_or(""), |token| {
        params.insert("pager.page_token".into(), token.into());
        client::request_json(
            "GET",
            &format!("{base}/vidu/v1/material/elements/personal"),
            None,
            None,
            Some(&params),
        )
    })
    .unwrap_or_else(|err| client::fail("pagination_error", &err, None, None, None));
    let items: Vec<Value> = data.get("elements").and_then(Value::as_array).into_iter().flatten().map(|e| {
        json!({"id": extract_string_id(e, "id"), "version": extract_string_id(e, "version"), "name": e.get("name").and_then(Value::as_str).unwrap_or("")})
    }).collect();
    client::ok(json!({"elements": items, "next_page_token": data["next_page_token"]}));
}

fn feed_params(
    pagesz: i64,
    page_token: &str,
    search: Option<(&str, &str, Option<&str>, Option<&str>)>,
) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("pager.page_token".into(), page_token.into());
    params.insert("pager.pagesz".into(), pagesz.to_string());
    params.insert("is_collect".into(), search.is_none().to_string());
    if let Some((keyword, sort_by, category, topic)) = search {
        params.insert("keyword".into(), keyword.into());
        params.insert("modalities".into(), "image".into());
        params.insert("sort_by".into(), sort_by.into());
        params.insert("is_like".into(), "false".into());
        if let Some(category) = category {
            params.insert("category".into(), category.into());
        }
        if let Some(topic) = topic {
            params.insert("topic".into(), topic.into());
        }
    }
    params
}

fn feed_result(data: &Value) -> Value {
    let items: Vec<Value> = data.get("share_elements").and_then(Value::as_array).into_iter().flatten().map(|se| {
        let empty_obj = json!({});
        let empty_arr = json!([]);
        let el = se.get("element").unwrap_or(&empty_obj);
        let sh = se.get("share").unwrap_or(&empty_obj);
        let desc: String = el.get("recaption").and_then(|r| r.get("description")).and_then(Value::as_str).unwrap_or("").chars().take(100).collect();
        json!({"id": extract_string_id(el, "id"), "version": extract_string_id(el, "version"), "name": el.get("name").and_then(Value::as_str).unwrap_or(""), "description": desc, "category": sh.get("category_display").unwrap_or(&empty_arr)})
    }).collect();
    json!({"elements": items, "next_page_token": data.get("next_page_token").and_then(Value::as_str).unwrap_or("")})
}

fn fetch_feed(params: &HashMap<String, String>) -> Value {
    let data = client::request_json(
        "GET",
        &format!(
            "{}/vidu/v1/material/share_elements/feed",
            client::base_url()
        ),
        None,
        None,
        Some(params),
    );
    feed_result(&data)
}

pub fn search(
    keyword: &str,
    pagesz: i64,
    sort_by: &str,
    page_token: &str,
    category: Option<&str>,
    topic: Option<&str>,
) {
    require_pagesz(pagesz, 1);
    if keyword.is_empty() {
        client::fail(
            "client_error",
            "--keyword is required for search",
            None,
            None,
            None,
        );
    }
    client::ok(fetch_feed(&feed_params(
        pagesz,
        page_token,
        Some((keyword, sort_by, category, topic)),
    )));
}

pub fn favorites(page_token: &str, pagesz: i64) -> Value {
    require_pagesz(pagesz, 1);
    fetch_feed(&feed_params(pagesz, page_token, None))
}

pub fn get(element_id: &str) -> Value {
    require_id(element_id);
    client::request_json(
        "GET",
        &format!(
            "{}/vidu/v1/material/elements/detail/{element_id}",
            client::base_url()
        ),
        None,
        None,
        None,
    )
}

fn validate_update_args(
    name: Option<&str>,
    labels: &[String],
    clear_labels: bool,
    recaption: Option<&str>,
) -> Result<(), String> {
    if clear_labels && !labels.is_empty() {
        return Err("--label and --clear-labels are mutually exclusive".into());
    }
    if name.is_none() && labels.is_empty() && !clear_labels && recaption.is_none() {
        return Err("Specify at least one update".into());
    }
    if labels.len() > 20
        || labels
            .iter()
            .any(|id| id.parse::<i64>().map_or(true, |id| id <= 0))
    {
        return Err("--label requires a positive label ID; at most 20 labels are supported".into());
    }
    if let Some(description) = recaption {
        if description.is_empty() || description.chars().count() > 1280 {
            return Err("--recaption must contain 1-1280 characters".into());
        }
    }
    Ok(())
}

fn update_body(
    element_id: &str,
    name: Option<&str>,
    labels: &[String],
    clear_labels: bool,
    recaption: Option<&str>,
    current: Option<&Value>,
) -> Result<Value, String> {
    validate_update_args(name, labels, clear_labels, recaption)?;
    let current = current
        .filter(|value| value.is_object())
        .ok_or("Missing current element in detail response; refusing to update")?;
    let current_name = name
        .or_else(|| current.get("name").and_then(Value::as_str))
        .ok_or("Missing current element name in detail response; refusing to update")?;
    let mut caption = current
        .get("recaption")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or("Missing valid current recaption in detail response; refusing to update")?;
    // PatchAudit reads name and recaption even when they are absent from the mask.
    // Carry the current values for audit context without widening the update mask.
    let mut element = json!({"id": element_id, "name": current_name});
    let mut mask = Vec::new();
    if name.is_some() {
        mask.push("name");
    }
    if !labels.is_empty() || clear_labels {
        element["labels"] = json!(labels
            .iter()
            .map(|id| json!({"id": id}))
            .collect::<Vec<_>>());
        mask.push("labels");
    }
    if let Some(description) = recaption {
        // The backend supports the whole recaption mask only. Preserve its existing style.
        caption["description"] = json!(description);
        mask.push("recaption");
    }
    element["recaption"] = caption;
    if let Some(modality) = current.get("modality") {
        element["modality"] = modality.clone();
    }
    // Protobuf JSON encodes FieldMask as a comma-separated string.
    Ok(json!({"mask": mask.join(","), "element": element}))
}

pub fn update(
    element_id: &str,
    name: Option<&str>,
    labels: &[String],
    clear_labels: bool,
    recaption: Option<&str>,
) -> Value {
    require_id(element_id);
    // Validate arguments before any read or mutation.
    validate_update_args(name, labels, clear_labels, recaption)
        .unwrap_or_else(|err| client::fail("client_error", &err, None, None, None));
    let detail = get(element_id);
    let current = detail.get("element");
    let body = update_body(element_id, name, labels, clear_labels, recaption, current)
        .unwrap_or_else(|err| client::fail("api_error", &err, None, None, None));
    client::request_json(
        "PUT",
        &format!(
            "{}/vidu/v1/material/elements/{element_id}",
            client::base_url()
        ),
        None,
        Some(&body),
        None,
    )
}

pub fn delete(element_id: &str) -> Value {
    require_id(element_id);
    client::request_json(
        "DELETE",
        &format!(
            "{}/vidu/v1/material/elements/{element_id}",
            client::base_url()
        ),
        None,
        None,
        None,
    )
}

pub fn favorite(element_id: &str, enabled: bool) -> Value {
    let detail = get(element_id);
    let share_id = detail
        .get("share")
        .map(|s| extract_string_id(s, "id"))
        .unwrap_or_default();
    if share_id.parse::<i64>().map_or(true, |id| id <= 0) {
        client::fail(
            "client_error",
            "Element has no shared-library ID and cannot be favorited",
            None,
            None,
            None,
        );
    }
    let url = format!(
        "{}/tools/v1/votes/share_element/{share_id}",
        client::base_url()
    );
    if enabled {
        client::request_json(
            "PUT",
            &url,
            None,
            Some(&json!({"obj_type": "share_element", "obj_id": share_id, "vote": "collect"})),
            None,
        )
    } else {
        let params = HashMap::from([("vote".to_string(), "collect".to_string())]);
        client::request_json("DELETE", &url, None, None, Some(&params))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_preserves_caption_style_and_replaces_labels() {
        let body = update_body(
            "123",
            Some("新名称"),
            &["91".into(), "92".into()],
            false,
            Some("新描述"),
            Some(&json!({"recaption":{"style":"写实", "description":"旧描述"},"modality":"image"})),
        )
        .unwrap();
        assert_eq!(body["mask"], "name,labels,recaption");
        assert_eq!(
            body["element"]["recaption"],
            json!({"style":"写实","description":"新描述"})
        );
        assert_eq!(body["element"]["labels"], json!([{"id":"91"},{"id":"92"}]));
        assert_eq!(body["element"]["modality"], "image");
    }

    #[test]
    fn update_clear_and_validation() {
        let body = update_body(
            "123",
            None,
            &[],
            true,
            None,
            Some(
                &json!({"name":"current", "recaption":{"style":"realistic","description":"keep"}}),
            ),
        )
        .unwrap();
        assert_eq!(
            body,
            json!({"mask":"labels","element":{"id":"123","name":"current","labels":[],"recaption":{"style":"realistic","description":"keep"}}})
        );
        assert!(update_body("123", None, &[], false, None, None).is_err());
        assert!(update_body("123", None, &["91".into()], true, None, None).is_err());
        assert!(update_body("123", None, &["name".into()], false, None, None).is_err());
        assert!(update_body("123", None, &[], false, Some(""), None).is_err());
    }

    #[test]
    fn every_update_carries_audit_context_without_widening_mask() {
        let current = json!({
            "name":"current", "modality":"image",
            "recaption":{"style":"realistic","description":"keep","extra":"preserved"}
        });
        for (name, labels, clear_labels, description, expected_mask) in [
            (Some("renamed"), vec![], false, None, "name"),
            (None, vec!["91".to_string()], false, None, "labels"),
            (None, vec![], true, None, "labels"),
            (None, vec![], false, Some("new caption"), "recaption"),
        ] {
            let body = update_body(
                "123",
                name,
                &labels,
                clear_labels,
                description,
                Some(&current),
            )
            .unwrap();
            assert_eq!(body["mask"], expected_mask);
            assert_eq!(body["element"]["name"], name.unwrap_or("current"));
            assert_eq!(body["element"]["modality"], "image");
            let mut expected_caption = current["recaption"].clone();
            if let Some(description) = description {
                expected_caption["description"] = json!(description);
            }
            assert_eq!(body["element"]["recaption"], expected_caption);
            assert_eq!(
                body["element"].get("labels").is_some(),
                expected_mask == "labels"
            );
        }
    }

    #[test]
    fn update_rejects_missing_or_invalid_audit_context() {
        for current in [
            None,
            Some(json!(null)),
            Some(json!({"name":"current"})),
            Some(json!({"name":"current","recaption":null})),
            Some(json!({"name":"current","recaption":"invalid"})),
            Some(json!({"name":"current","recaption":[]})),
        ] {
            assert!(update_body("123", Some("new"), &[], false, None, current.as_ref()).is_err());
            assert!(
                update_body("123", None, &[], true, Some("caption"), current.as_ref()).is_err()
            );
        }
        assert!(update_body("123", None, &[], true, None, Some(&json!({"recaption":{}}))).is_err());
        assert!(validate_update_args(Some("new"), &[], false, None).is_ok());
        assert!(validate_update_args(None, &[], true, None).is_ok());
    }

    #[test]
    fn collection_params_do_not_include_search_filters() {
        let favorites = feed_params(1, "cursor", None);
        assert_eq!(favorites["pager.pagesz"], "1");
        assert_eq!(favorites["is_collect"], "true");
        assert_eq!(favorites.len(), 3);
        let search = feed_params(
            1,
            "cursor",
            Some(("猫", "hot", Some("character"), Some("summer"))),
        );
        assert_eq!(search["is_collect"], "false");
        assert_eq!(search["pager.pagesz"], "1");
        assert_eq!(search["category"], "character");
        assert_eq!(search["topic"], "summer");
        assert_eq!(search["keyword"], "猫");
    }

    #[test]
    fn numeric_page_walks_cursors_and_stops_at_end() {
        let mut visited = Vec::new();
        let result = walk_personal_pages(8, "", |token| {
            visited.push(token.to_string());
            match token {
                "" => json!({"elements":[{"id":"1"}],"next_page_token":"next"}),
                "next" => json!({"elements":[{"id":"2"}]}),
                _ => panic!("Unexpected cursor"),
            }
        })
        .unwrap();
        assert_eq!(visited, vec!["", "next"]);
        assert_eq!(result, json!({"elements":[],"next_page_token":""}));
    }

    #[test]
    fn cursor_cycles_are_rejected() {
        assert!(walk_personal_pages(3, "", |_| json!({"next_page_token":"same"})).is_err());
        assert!(walk_personal_pages(0, "same", |_| json!({"next_page_token":"same"})).is_err());
    }

    #[test]
    fn requested_cursor_page_retains_result_and_next_cursor() {
        let result = walk_personal_pages(1, "", |token| {
            if token.is_empty() {
                json!({"elements":[{"id":"1"}], "next_page_token":"second"})
            } else {
                json!({"elements":[{"id":"2"}], "next_page_token":"third"})
            }
        })
        .unwrap();
        assert_eq!(result["elements"][0]["id"], "2");
        assert_eq!(result["next_page_token"], "third");
    }
}
