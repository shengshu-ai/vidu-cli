//! Original element command surface. Kept separate so V2 parameters never leak
//! into default help, suggestions, or validation.
use crate::client;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum ElementAction {
    /// Get latest visible element
    Get { element_id: String },
    /// Update personal element metadata
    #[command(group(clap::ArgGroup::new("changes").required(true).multiple(true).args(["name", "labels", "clear_labels", "recaption"])))]
    Update {
        element_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long = "label", value_name = "LABEL_ID", help = "Replace labels with these label IDs. Repeatable.", action = clap::ArgAction::Append, conflicts_with = "clear_labels")]
        labels: Vec<String>,
        #[arg(long)]
        clear_labels: bool,
        #[arg(long)]
        recaption: Option<String>,
    },
    /// Delete personal element
    Delete { element_id: String },
    /// List favorite community elements
    Favorites {
        #[arg(long, default_value = "")]
        page_token: String,
        #[arg(long, default_value = "20", value_parser = clap::value_parser!(i64).range(1..=100))]
        pagesz: i64,
    },
    /// Favorite community element
    Favorite { element_id: String },
    /// Unfavorite community element
    Unfavorite { element_id: String },

    /// Check if element name exists
    Check {
        #[arg(long)]
        name: String,
    },
    /// Pre-process element
    Preprocess {
        #[arg(long)]
        name: String,
        #[arg(long = "type", default_value = "user")]
        elem_type: String,
        #[arg(long = "image", action = clap::ArgAction::Append)]
        images: Vec<String>,
    },
    /// Create element
    Create {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "image")]
        modality: String,
        #[arg(long = "type", default_value = "user")]
        elem_type: String,
        #[arg(long = "image", action = clap::ArgAction::Append)]
        images: Vec<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        style: Option<String>,
    },
    /// List personal elements
    List {
        #[arg(long)]
        keyword: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(i64).range(0..=i32::MAX as i64), conflicts_with = "page_token")]
        page: Option<i64>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long, default_value = "20", value_parser = clap::value_parser!(i64).range(10..=100))]
        pagesz: i64,
    },
    /// Search community elements
    Search {
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        keyword: String,
        #[arg(long, default_value = "20", value_parser = clap::value_parser!(i64).range(1..=100))]
        pagesz: i64,
        #[arg(long, default_value = "recommend")]
        sort_by: String,
        #[arg(long, default_value = "")]
        page_token: String,
    },
}

/// Dispatch the original protocol without rewriting its request or response contract.
pub fn run(action: ElementAction) {
    match action {
        ElementAction::Get { element_id } => {
            client::ok(crate::commands::elements_legacy::get(&element_id))
        }
        ElementAction::Update {
            element_id,
            name,
            labels,
            clear_labels,
            recaption,
        } => client::ok(crate::commands::elements_legacy::update(
            &element_id,
            name.as_deref(),
            &labels,
            clear_labels,
            recaption.as_deref(),
        )),
        ElementAction::Delete { element_id } => {
            client::ok(crate::commands::elements_legacy::delete(&element_id))
        }
        ElementAction::Favorites { page_token, pagesz } => client::ok(
            crate::commands::elements_legacy::favorites(&page_token, pagesz),
        ),
        ElementAction::Favorite { element_id } => client::ok(
            crate::commands::elements_legacy::favorite(&element_id, true),
        ),
        ElementAction::Unfavorite { element_id } => client::ok(
            crate::commands::elements_legacy::favorite(&element_id, false),
        ),
        ElementAction::Check { name } => {
            crate::commands::elements_legacy::check(&name);
        }
        ElementAction::Preprocess {
            name,
            elem_type,
            images,
        } => {
            crate::commands::elements_legacy::preprocess(&name, &elem_type, &images);
        }
        ElementAction::Create {
            name,
            modality,
            elem_type,
            images,
            description,
            style,
        } => {
            crate::commands::elements_legacy::create(
                &name,
                &modality,
                &elem_type,
                &images,
                description.as_deref(),
                style.as_deref(),
            );
        }
        ElementAction::List {
            keyword,
            page,
            pagesz,
            page_token,
        } => {
            crate::commands::elements_legacy::list_elements(
                keyword.as_deref(),
                page.unwrap_or(0),
                pagesz,
                page_token.as_deref(),
            );
        }
        ElementAction::Search {
            category,
            topic,
            keyword,
            pagesz,
            sort_by,
            page_token,
        } => {
            crate::commands::elements_legacy::search(
                &keyword,
                pagesz,
                &sort_by,
                &page_token,
                category.as_deref(),
                topic.as_deref(),
            );
        }
    }
}
