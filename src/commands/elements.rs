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

/// The gateway fixes ownership to the authenticated user. Keep the existing
/// --type/--modality spellings without allowing preset ownership writes.
#[derive(clap::Args)]
pub struct CreateArgs {
    #[arg(long)]
    name: String,
    #[arg(long, default_value = "image", value_parser = ["image"])]
    modality: String,
    #[arg(long = "type", default_value = "user", value_parser = ["user"])]
    elem_type: String,
    #[command(flatten)]
    media: super::element_storage::MediaArgs,
    /// One taxonomy ID from `element tags`.
    #[arg(long = "tag", value_name = "TAG_ID")]
    tag_id: String,
    /// Optional description; the service generates one when omitted.
    #[arg(long)]
    description: Option<String>,
}

#[derive(clap::Args)]
#[command(group(clap::ArgGroup::new("changes").required(true).multiple(true).args(["name", "labels", "clear_labels", "recaption", "images", "asset_ids"])))]
pub struct UpdateArgs {
    element_id: String,
    #[arg(long)]
    name: Option<String>,
    /// Replace legacy personal labels; these are not the element category.
    #[arg(long = "label", value_name = "LABEL_ID", action = clap::ArgAction::Append, conflicts_with_all = ["clear_labels", "images", "asset_ids"])]
    labels: Vec<String>,
    /// Clear legacy personal labels; does not change the element category.
    #[arg(long, conflicts_with_all = ["images", "asset_ids"])]
    clear_labels: bool,
    /// Replace description, preserving the existing style.
    #[arg(long)]
    recaption: Option<String>,
    #[command(flatten)]
    media: super::element_storage::MediaArgs,
}

/// Validate metadata before uploading; taxonomy selection stays server-side.
fn validate_metadata(name: Option<&str>, description: Option<&str>) -> Result<(), String> {
    if let Some(name) = name {
        if name.trim().is_empty() || name.chars().count() > 30 {
            return Err("name must contain 1-30 characters".into());
        }
    }
    if let Some(description) = description {
        if description.is_empty() || description.chars().count() > 1280 {
            return Err("description must contain 1-1280 characters".into());
        }
    }
    Ok(())
}

/// Protobuf int64 IDs stay strings on the wire to avoid JSON precision loss.
fn validate_tag_id(id: Option<&str>) -> Result<(), String> {
    if id.is_some_and(|id| id.parse::<i64>().map_or(true, |id| id <= 0)) {
        return Err("--tag requires a positive taxonomy ID from element tags".into());
    }
    Ok(())
}

fn require_valid(result: Result<(), String>) {
    result.unwrap_or_else(|err| client::fail("client_error", &err, None, None, None));
}

/// Ask the service for first-level tags; hierarchy, visibility and ordering are
/// business rules owned by the server. The CLI only projects display fields.
pub fn tags() -> Value {
    let params = HashMap::from([("parent_id".into(), "0".into())]);
    let data = client::request_json(
        "GET",
        &format!("{}/vidu/v1/material/elements/tags", client::base_url()),
        None,
        None,
        Some(&params),
    );
    let nodes = match data.get("tags") {
        None | Some(Value::Null) if data.is_object() => &[][..],
        Some(Value::Array(nodes)) => nodes.as_slice(),
        _ => client::fail(
            "parse_error",
            "Tag response must contain a tags array",
            None,
            None,
            None,
        ),
    };
    let mut items = Vec::new();
    for node in nodes {
        if !node.is_object() {
            client::fail("parse_error", "Tag must be a JSON object", None, None, None);
        }
        // The tag API exposes display_name_zh/en. Project only the public
        // CLI fields; name/name_en in a response are not API field aliases.
        items.push(json!({
            "id": extract_string_id(node, "id"),
            "name": node.get("display_name_zh").and_then(Value::as_str).unwrap_or(""),
            "name_en": node.get("display_name_en").and_then(Value::as_str).unwrap_or(""),
        }));
    }
    json!({"tags":items})
}

/// Upload once and use one-step creation. Material owns preprocessing, caption
/// generation, audit, and persistence; there is no temporary preprocess element.
pub fn create(args: &CreateArgs) {
    require_valid(validate_metadata(
        Some(&args.name),
        args.description.as_deref(),
    ));
    require_valid(validate_tag_id(Some(&args.tag_id)));
    require_valid(args.media.validate(true));

    let components = args.media.prepare_components();
    let mut body = json!({
        "name": args.name, "modality": args.modality,
        "tag_ids": [&args.tag_id], "components": components
    });
    // The public gateway sets type and creator_id from authentication. They
    // cannot be selected by an untrusted terminal caller.
    debug_assert_eq!(args.elem_type, "user");
    // Style is owned by the service, not a terminal input.
    if let Some(description) = &args.description {
        body["recaption"] = json!({"description":description});
    }
    let data = client::request_json(
        "POST",
        &format!("{}/vidu/v1/material/elements/create", client::base_url()),
        Some("create_element"),
        Some(&body),
        None,
    );
    let id = extract_string_id(&data, "id");
    let version = extract_string_id(&data, "version");
    if id.parse::<i64>().map_or(true, |id| id <= 0)
        || version.parse::<i64>().map_or(true, |version| version <= 0)
    {
        client::fail("parse_error", "Create response is missing a valid element ID or version; the write may have succeeded. Check element list before retrying.", None, None, Some("create_element"));
    }
    client::ok(json!({"id": id, "version": version}));
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
        if !data.is_object() {
            return Err("Server list response must be a JSON object".into());
        }
        if data
            .get("next_page_token")
            .is_some_and(|value| !value.is_null() && !value.is_string())
        {
            return Err("Server pagination cursor must be a string".into());
        }
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

/// Personal browsing retains list page-size limits; search uses the same query
/// with its own 1–100 limit. Both exclude presets and filter on the server.
pub fn list_elements(
    keyword: Option<&str>,
    tag: Option<&str>,
    page: i64,
    pagesz: i64,
    page_token: Option<&str>,
) {
    require_pagesz(pagesz, 10);
    client::ok(personal_elements(keyword, tag, page, pagesz, page_token));
}

pub fn personal_elements(
    keyword: Option<&str>,
    tag: Option<&str>,
    page: i64,
    pagesz: i64,
    page_token: Option<&str>,
) -> Value {
    require_pagesz(pagesz, 1);
    require_valid(validate_tag_id(tag));
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
    // Personal discovery excludes preset subjects, including the first page;
    // official discovery has its own source and endpoint.
    params.insert("without_preset".into(), "true".into());
    if let Some(tag) = tag {
        params.insert("tag_ids".into(), tag.into());
    }
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
    let items: Vec<Value> = data
        .get("elements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|e| list_item(e))
        .collect();
    json!({"elements": items, "next_page_token": data["next_page_token"]})
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

/// Conditions are explicit: a source or pagination alone is not a search.
/// Reject empty text instead of silently treating it as an omitted condition.
pub fn validate_search(
    keyword: Option<&str>,
    tag: Option<&str>,
    category: Option<&str>,
    topic: Option<&str>,
    sort_by: Option<&str>,
) {
    for (flag, value) in [
        ("--keyword", keyword),
        ("--category", category),
        ("--topic", topic),
    ] {
        if value.is_some_and(|value| value.trim().is_empty()) {
            client::fail(
                "client_error",
                &format!("{flag} must not be blank"),
                None,
                None,
                None,
            );
        }
    }
    if keyword.is_none()
        && tag.is_none()
        && category.is_none()
        && topic.is_none()
        && sort_by.is_none()
    {
        client::fail("client_error", "search requires --keyword, --tag or community --category/--topic/--sort-by; use list to browse without conditions", None, None, None);
    }
}

/// Reject filters that cannot be expressed by the selected library. Never drop
/// a user's filter silently or search a different source as a fallback.
pub fn reject_official_only_filters(has_official_filter: bool) {
    if has_official_filter {
        client::fail(
            "client_error",
            "--tag and collection --keyword filters require --source official",
            None,
            None,
            None,
        );
    }
}

/// Reject community discovery filters on the official library.
pub fn reject_community_only_filters(has_community_filter: bool) {
    if has_community_filter {
        client::fail(
            "client_error",
            "--category, --topic and --sort-by require --source community; personal/official support --keyword and --tag",
            None,
            None,
            None,
        );
    }
}

/// Official discovery and collections share Vidu's query contract. Filtering
/// and collection pagination stay on the server, rather than joining pages in
/// the CLI. Numeric list pages retain the existing cursor-walking behavior.
pub fn official_elements(
    keyword: Option<&str>,
    tag: Option<&str>,
    page: i64,
    page_token: &str,
    pagesz: i64,
    collected_only: bool,
) -> Value {
    require_pagesz(pagesz, 1);
    require_valid(validate_tag_id(tag));
    let mut params = HashMap::from([
        ("query.pager.pagesz".into(), pagesz.to_string()),
        ("collected_only".into(), collected_only.to_string()),
    ]);
    if let Some(keyword) = keyword {
        params.insert("query.keyword".into(), keyword.into());
    }
    if let Some(tag) = tag {
        params.insert("query.tag_ids".into(), tag.into());
    }
    let data = walk_personal_pages(page, page_token, |token| {
        params.insert("query.pager.page_token".into(), token.into());
        client::request_json(
            "GET",
            &format!("{}/vidu/v1/material/elements/official", client::base_url()),
            None,
            None,
            Some(&params),
        )
    })
    .unwrap_or_else(|err| client::fail("pagination_error", &err, None, None, None));
    // Match the personal list contract; full media details belong to get.
    let items = official_list_items(&data)
        .unwrap_or_else(|err| client::fail("parse_error", &err, None, None, None));
    json!({"elements":items,"next_page_token":data.get("next_page_token").and_then(Value::as_str).unwrap_or("")})
}

/// Both personal and official lists expose the same stable summary fields.
/// Identifiers stay strings so consumers can preserve int64 precision.
fn list_item(element: &Value) -> Value {
    json!({"id":extract_string_id(element,"id"),"version":extract_string_id(element,"version"),
        "name":element.get("name").and_then(Value::as_str).unwrap_or("")})
}

/// The gateway returns ListElementsReply with flat vidu.v1.Element items.
/// Missing repeated fields represent an empty list in protobuf JSON; malformed
/// fields are errors, not empty results or values that can panic on indexing.
fn official_list_items(data: &Value) -> Result<Vec<Value>, String> {
    if !data.is_object() {
        return Err("Official list response must be a JSON object".into());
    }
    let elements = match data.get("elements") {
        None | Some(Value::Null) => return Ok(vec![]),
        Some(Value::Array(elements)) => elements,
        _ => return Err("Official list elements must be an array".into()),
    };
    elements
        .iter()
        .map(|element| {
            if !element.is_object() {
                return Err("Official list item must be an element object".into());
            }
            Ok(list_item(element))
        })
        .collect()
}

/// Collection identities are different: official collections use Element IDs,
/// community collections resolve Share IDs through detail. In particular, do
/// not GET an official element before uncollecting: an offline element may no
/// longer be readable, but the user must still be able to remove its collection.
pub fn favorite_from_source(element_id: &str, enabled: bool, source: &str) -> Value {
    require_id(element_id);
    if source == "community" {
        return favorite(element_id, enabled);
    }
    if source != "official" {
        client::fail(
            "client_error",
            "Collection source must be community or official",
            None,
            None,
            None,
        );
    }
    let url = format!(
        "{}/tools/v1/votes/official_element/{element_id}",
        client::base_url()
    );
    if enabled {
        client::request_json(
            "PUT",
            &url,
            None,
            Some(&json!({"obj_type":"official_element","obj_id":element_id,"vote":"collect"})),
            None,
        )
    } else {
        let params = HashMap::from([("vote".into(), "collect".into())]);
        client::request_json("DELETE", &url, None, None, Some(&params))
    }
}

pub fn get(element_id: &str) -> Value {
    subject_output(get_detail(element_id))
}

/// Internal decisions need the server-owned edition before public projection.
/// Keep raw detail private so internal fields never leak through get/update output.
fn get_detail(element_id: &str) -> Value {
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

/// Detail wraps its subject in element; updates return the subject directly.
/// Hide internal usage/edition fields in V2 output without touching media or
/// the public version used for identity. Legacy output has its own unchanged path.
fn subject_output(mut data: Value) -> Value {
    if let Some(object) = data.as_object_mut() {
        object.remove("allowed_usages");
        object.remove("edition");
        if let Some(element) = object.get_mut("element").and_then(Value::as_object_mut) {
            element.remove("allowed_usages");
            element.remove("edition");
        }
    }
    data
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

fn update_legacy_labels(
    element_id: &str,
    name: Option<&str>,
    labels: &[String],
    clear_labels: bool,
    recaption: Option<&str>,
    current: Option<&Value>,
) -> Value {
    let body = update_body(element_id, name, labels, clear_labels, recaption, current)
        .unwrap_or_else(|err| client::fail("api_error", &err, None, None, None));
    subject_output(client::request_json(
        "PUT",
        &format!(
            "{}/vidu/v1/material/elements/{element_id}",
            client::base_url()
        ),
        None,
        Some(&body),
        None,
    ))
}

/// Build the V2 field mask without replaying unchanged component URLs. Metadata
/// edits never register assets or re-upload a subject's existing files.
fn library_update_body(args: &UpdateArgs, current: Option<&Value>) -> Result<Value, String> {
    let current = current
        .filter(|value| value.is_object())
        .ok_or("Missing current element in detail response; refusing to update")?;
    let name = args
        .name
        .as_deref()
        .or_else(|| current.get("name").and_then(Value::as_str))
        .ok_or("Missing current element name in detail response; refusing to update")?;
    let mut caption = current
        .get("recaption")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or("Missing valid current recaption in detail response; refusing to update")?;
    let mut element = json!({"id":args.element_id,"name":name});
    let mut mask = Vec::new();
    if args.name.is_some() {
        mask.push("name");
    }
    if let Some(description) = &args.recaption {
        caption["description"] = json!(description);
    }
    if args.recaption.is_some() {
        mask.push("recaption");
    }
    element["recaption"] = caption;
    if let Some(modality) = current.get("modality") {
        element["modality"] = modality.clone();
    }
    Ok(json!({"mask":mask.join(","),"element":element}))
}

/// Edition is the API discriminator: 1 is legacy, 2 uses platform assets.
/// Never infer it from tags, labels or media. Missing/unknown editions must not
/// silently authorize a legacy write. Accept numeric and protobuf string forms.
fn legacy_labels_allowed(current: Option<&Value>) -> Result<bool, &'static str> {
    let edition = current.and_then(|element| element.get("edition"));
    let edition = edition.and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(|value| value.parse::<i64>().ok()))
    });
    match edition {
        Some(1) => Ok(true),
        Some(2) => Ok(false),
        _ => Err("Missing or unsupported element edition; refusing personal label edits"),
    }
}

/// New edits use V2; legacy personal labels retain their compatible endpoint.
/// One command never splits into two writes that could partially succeed.
pub fn update(args: &UpdateArgs) -> Value {
    require_id(&args.element_id);
    require_valid(validate_metadata(
        args.name.as_deref(),
        args.recaption.as_deref(),
    ));
    require_valid(args.media.validate(false));
    let edits_labels = !args.labels.is_empty() || args.clear_labels;
    if edits_labels {
        require_valid(validate_update_args(
            args.name.as_deref(),
            &args.labels,
            args.clear_labels,
            args.recaption.as_deref(),
        ));
    }
    // Fetch exactly once before choosing a write branch. Public get() removes
    // edition, so it cannot be used to decide whether legacy labels are legal.
    let detail = get_detail(&args.element_id);
    let current = detail.get("element");
    if edits_labels {
        let allowed = legacy_labels_allowed(current)
            .unwrap_or_else(|error| client::fail("api_error", error, None, None, None));
        if !allowed {
            client::fail("client_error", "--label and --clear-labels are only supported for legacy elements; new elements do not allow personal label edits", None, None, None);
        }
        return update_legacy_labels(
            &args.element_id,
            args.name.as_deref(),
            &args.labels,
            args.clear_labels,
            args.recaption.as_deref(),
            current,
        );
    }
    let mut body = library_update_body(args, current)
        .unwrap_or_else(|err| client::fail("api_error", &err, None, None, None));
    if args.media.has_files() {
        // This editor replaces images only. Converting an old text/video element
        // requires a declared modality contract rather than implicit conversion.
        if current
            .and_then(|element| element.get("modality"))
            .and_then(Value::as_str)
            != Some("image")
        {
            client::fail(
                "client_error",
                "--image can only replace components of an image element",
                None,
                None,
                None,
            );
        }
        body["element"]["components"] = args.media.prepare_components();
        let mask = body["mask"].as_str().unwrap_or("");
        body["mask"] = json!(if mask.is_empty() {
            "components".into()
        } else {
            format!("{mask},components")
        });
    }
    subject_output(client::request_json(
        "PUT",
        &format!(
            "{}/vidu/v2/material/elements/{}",
            client::base_url(),
            args.element_id
        ),
        None,
        Some(&body),
        None,
    ))
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
