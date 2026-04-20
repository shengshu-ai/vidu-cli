use crate::client;
use serde_json::json;

pub fn claw_pass_status() {
    let base = client::base_url();
    let data = client::request_json("GET", &format!("{}/credit/v1/claw-pass/status", base), None, None, None);
    client::ok(json!({
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
    }));
}

pub fn credit_status() {
    let base = client::base_url();
    let data = client::request_json("GET", &format!("{}/credit/v1/credits/me", base), None, None, None);
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
