//! Prepare ordered subject components through the platform asset upload service.
//! Existing asset IDs are reused directly. New material uploads are promoted to
//! assets by the subject service when it saves the components; upload IDs never
//! become asset IDs on the client.
use crate::client;
use clap::Args;
use serde_json::{json, Value};

#[derive(Args)]
pub struct MediaArgs {
    /// Ordered images; first is main, others auxiliary. Image replacement requires a second-generation element.
    #[arg(long = "image", action = clap::ArgAction::Append, conflicts_with = "asset_ids")]
    pub images: Vec<String>,
    /// Existing platform asset IDs; image replacement requires a second-generation element.
    #[arg(long = "asset-id", visible_alias = "asset_id", action = clap::ArgAction::Append, conflicts_with = "images")]
    pub asset_ids: Vec<String>,
}

impl MediaArgs {
    pub fn has_files(&self) -> bool {
        !self.images.is_empty() || !self.asset_ids.is_empty()
    }

    /// Validate before side effects. Ownership and asset readability are checked
    /// by the service; the CLI does not assign an owner or guess asset metadata.
    pub fn validate(&self, required: bool) -> Result<(), String> {
        if !self.images.is_empty() && !self.asset_ids.is_empty() {
            return Err("--image and --asset-id are mutually exclusive".into());
        }
        let count = self.images.len() + self.asset_ids.len();
        if (required && count == 0) || count > 3 {
            return Err("Must provide 1-3 images or platform asset IDs".into());
        }
        if self.images.iter().any(|image| image.trim().is_empty()) {
            return Err("--image must not be empty".into());
        }
        if self
            .asset_ids
            .iter()
            .any(|id| id.parse::<i64>().map_or(true, |id| id <= 0))
        {
            return Err("--asset-id requires a positive platform asset ID".into());
        }
        Ok(())
    }

    /// Preserve component order and roles for both create and full replacement.
    /// ID-only references deliberately omit URI fields: the server resolves the
    /// authorized asset, rather than trusting a separately supplied address.
    pub fn prepare_components(&self) -> Value {
        let inputs = if self.asset_ids.is_empty() {
            &self.images
        } else {
            &self.asset_ids
        };
        Value::Array(inputs.iter().enumerate().map(|(index, input)| {
            let mut component = json!({"type":if index == 0 { "main" } else { "auxiliary" },"content_type":"image"});
            if self.asset_ids.is_empty() {
                let uri = self.upload_image(input);
                component["content"] = json!(uri);
                component["src_img"] = component["content"].clone();
            } else {
                component["asset_id"] = json!(input);
            }
            component
        }).collect())
    }

    /// Reuse image preparation/compression, but upload through Tools Assets.
    /// Existing Tools material upload URIs are submitted for server promotion.
    fn upload_image(&self, input: &str) -> String {
        if input.starts_with("ssupload:") {
            if input
                .strip_prefix("ssupload:?id=")
                .and_then(|id| id.parse::<i64>().ok())
                .is_none_or(|id| id <= 0)
            {
                client::fail(
                    "client_error",
                    "--image requires a valid Tools material upload URI",
                    None,
                    None,
                    None,
                );
            }
            return input.into();
        }
        if input.starts_with("https://") || input.starts_with("http://") {
            let mut response = reqwest::blocking::get(input)
                .and_then(|response| response.error_for_status())
                .unwrap_or_else(|error| {
                    client::fail(
                        "network_error",
                        &client::network_error_message(error, input),
                        None,
                        None,
                        Some("download_element_image"),
                    )
                });
            let mut file = tempfile::NamedTempFile::new().unwrap_or_else(|error| {
                client::fail("client_error", &error.to_string(), None, None, None)
            });
            std::io::copy(&mut response, &mut file).unwrap_or_else(|error| {
                client::fail("client_error", &error.to_string(), None, None, None)
            });
            return super::upload::upload_asset_image(file.path().to_str().unwrap());
        }
        super::upload::upload_asset_image(input)
    }
}
