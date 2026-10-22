use crate::client;
use serde_json::{json, Value};
use std::collections::HashMap;

pub(crate) fn offset_params(
    page: i64,
    pagesz: i64,
) -> Result<HashMap<String, String>, &'static str> {
    if !(0..=i32::MAX as i64).contains(&page) {
        return Err("page must be between 0 and 2147483647");
    }
    if !(1..=100).contains(&pagesz) {
        return Err("pagesz must be between 1 and 100");
    }
    Ok(HashMap::from([
        ("pager.page".into(), page.to_string()),
        ("pager.pagesz".into(), pagesz.to_string()),
    ]))
}

fn format_records(data: &Value) -> Value {
    json!({
        "total": data.get("total").cloned().unwrap_or(json!(0)),
        "records": data.get("records").and_then(Value::as_array).cloned().unwrap_or_default(),
    })
}

pub fn records(page: i64, pagesz: i64) -> Value {
    let params = offset_params(page, pagesz)
        .unwrap_or_else(|message| client::fail("client_error", message, None, None, None));
    let data = client::request_json(
        "GET",
        &format!("{}/credit/v1/credits/records", client::base_url()),
        None,
        None,
        Some(&params),
    );
    format_records(&data)
}

pub fn format_claw_pass_json(data: &Value) -> Value {
    json!({
        "has_pass": data.get("has_pass").and_then(|v| v.as_bool()).unwrap_or(false),
        "package_id": data.get("package_id").and_then(|v| v.as_str()).unwrap_or(""),
        "tier": data.get("tier").and_then(|v| v.as_str()).unwrap_or(""),
        "daily_quota_seconds": data.get("daily_quota_seconds").and_then(|v| v.as_i64()).unwrap_or(0),
        "used_seconds": data.get("used_seconds").and_then(|v| v.as_i64()).unwrap_or(0),
        "remain_seconds": data.get("remain_seconds").and_then(|v| v.as_i64()).unwrap_or(0),
        "cycle_start_at": data.get("cycle_start_at").and_then(|v| v.as_str()).unwrap_or(""),
        "cycle_end_at": data.get("cycle_end_at").and_then(|v| v.as_str()).unwrap_or(""),
        "next_refresh_at": data.get("next_refresh_at").and_then(|v| v.as_str()).unwrap_or(""),
        "refresh_timezone": data.get("refresh_timezone").and_then(|v| v.as_str()).unwrap_or(""),
    })
}

pub fn claw_pass_status() {
    let base = client::base_url();
    let data = client::request_json(
        "GET",
        &format!("{}/credit/v1/claw-pass/status", base),
        None,
        None,
        None,
    );
    client::ok(format_claw_pass_json(&data));
}

pub fn credit_status() {
    let base = client::base_url();
    let data = client::request_json(
        "GET",
        &format!("{}/credit/v1/credits/me", base),
        None,
        None,
        None,
    );
    client::ok(json!({
        "credits": data.get("credits").and_then(|v| v.as_i64()).unwrap_or(0),
        "credits_expire_today": data.get("credits_expire_today").and_then(|v| v.as_i64()).unwrap_or(0),
        "credits_expire_monthly": data.get("credits_expire_monthly").and_then(|v| v.as_i64()).unwrap_or(0),
        "credits_permanent": data.get("credits_permanent").and_then(|v| v.as_i64()).unwrap_or(0),
        "concurrency": data.get("concurrency").and_then(|v| v.as_i64()).unwrap_or(0),
        "credits_free": data.get("credits_free").and_then(|v| v.as_i64()).unwrap_or(0),
        "credits_subscribed": data.get("credits_subscribed").and_then(|v| v.as_i64()).unwrap_or(0),
        "credits_purchased": data.get("credits_purchased").and_then(|v| v.as_i64()).unwrap_or(0),
        "credit_sub_expires_at": data.get("credit_sub_expires_at").and_then(|v| v.as_str()).unwrap_or(""),
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_pagination_validates_proto_ranges() {
        assert_eq!(offset_params(0, 20).unwrap()["pager.page"], "0");
        assert_eq!(offset_params(8, 100).unwrap()["pager.pagesz"], "100");
        assert!(offset_params(i32::MAX as i64, 1).is_ok());
        for (page, pagesz) in [
            (-1, 20),
            (i32::MAX as i64 + 1, 20),
            (0, 0),
            (0, -1),
            (0, 101),
        ] {
            assert!(offset_params(page, pagesz).is_err());
        }
    }

    #[test]
    fn records_preserve_ledger_fields_without_inventing_a_cursor() {
        let input =
            json!({"total": 2, "records": [{"amount": -12, "remark": "generate"}], "extra": true});
        assert_eq!(
            format_records(&input),
            json!({"total": 2, "records": input["records"]})
        );
        assert_eq!(
            format_records(&json!({})),
            json!({"total": 0, "records": []})
        );
    }
}
