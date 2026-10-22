use crate::{client, commands::quota::offset_params};
use serde_json::{json, Value};
use std::collections::HashMap;

fn list_params(
    favorites_only: bool,
    page: i64,
    pagesz: i64,
) -> Result<HashMap<String, String>, &'static str> {
    let mut params = offset_params(page, pagesz)?;
    params.insert("is_favor".into(), favorites_only.to_string());
    Ok(params)
}

fn format_list(data: &Value) -> Value {
    let creations: Vec<Value> = data
        .get("creations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .cloned()
        .map(|mut creation| {
            // Keep the API's actual creation ID and its numeric/string representation.
            // A task ID is never a substitute for a missing creation ID.
            if let Some(id) = creation
                .get("id")
                .filter(|id| {
                    id.as_i64().is_some_and(|id| id > 0)
                        || id
                            .as_str()
                            .is_some_and(|id| id.parse::<i64>().is_ok_and(|id| id > 0))
                })
                .cloned()
            {
                creation["creation_id"] = id;
            }
            creation
        })
        .collect();
    json!({
        "total": data.get("total").cloned().unwrap_or(json!(0)),
        "creations": creations,
    })
}

pub fn list(favorites_only: bool, page: i64, pagesz: i64) -> Value {
    let params = list_params(favorites_only, page, pagesz)
        .unwrap_or_else(|message| client::fail("client_error", message, None, None, None));
    let data = client::request_json(
        "GET",
        &format!("{}/vidu/v1/my-creations", client::base_url()),
        None,
        None,
        Some(&params),
    );
    format_list(&data)
}

fn favorite_request(
    creation_id: i64,
    enabled: bool,
) -> Result<(&'static str, Option<Value>), &'static str> {
    if creation_id <= 0 {
        return Err("creation_id must be positive");
    }
    Ok(if enabled {
        (
            "PUT",
            Some(json!({"obj_id": creation_id, "obj_type": "creation"})),
        )
    } else {
        ("DELETE", None)
    })
}

pub fn favorite(creation_id: i64, enabled: bool) -> Value {
    let (method, body) = favorite_request(creation_id, enabled)
        .unwrap_or_else(|message| client::fail("client_error", message, None, None, None));
    client::request_json(
        method,
        &format!(
            "{}/tools/v1/favors/creation/{creation_id}",
            client::base_url()
        ),
        None,
        body.as_ref(),
        None,
    );
    json!({"creation_id": creation_id, "is_favor": enabled})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_switches_server_branch_without_extra_filters() {
        let mine = list_params(false, 0, 20).unwrap();
        let favorites = list_params(true, 3, 50).unwrap();
        assert_eq!(mine["is_favor"], "false");
        assert_eq!(favorites["is_favor"], "true");
        assert_eq!(favorites["pager.page"], "3");
        assert_eq!(favorites["pager.pagesz"], "50");
        assert_eq!(favorites.len(), 3);
        assert!(list_params(true, -1, 20).is_err());
        assert!(list_params(false, 0, 101).is_err());
    }

    #[test]
    fn output_keeps_real_creation_ids_and_does_not_use_task_ids() {
        let data = json!({"total": 4, "creations": [
            {"id": 91, "task_id": 1, "uri": "video"},
            {"id": "9223372036854775807", "task_id": "2"},
            {"task_id": 3},
            {"id": 0, "task_id": 4}
        ]});
        let output = format_list(&data);
        assert_eq!(output["total"], 4);
        assert_eq!(output["creations"][0]["creation_id"], 91);
        assert_eq!(output["creations"][0]["uri"], "video");
        assert_eq!(output["creations"][1]["creation_id"], "9223372036854775807");
        assert!(output["creations"][2].get("creation_id").is_none());
        assert!(output["creations"][3].get("creation_id").is_none());
        assert_eq!(
            format_list(&json!({})),
            json!({"total": 0, "creations": []})
        );
    }

    #[test]
    fn creation_favorite_uses_favors_and_validates_ids() {
        assert_eq!(
            favorite_request(7, true).unwrap(),
            ("PUT", Some(json!({"obj_id": 7, "obj_type": "creation"})))
        );
        assert_eq!(favorite_request(7, false).unwrap(), ("DELETE", None));
        assert!(favorite_request(0, true).is_err());
        assert!(favorite_request(-1, false).is_err());
    }
}
