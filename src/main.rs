mod client;
mod commands;
mod validators;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "vidu-cli", about = "Vidu API CLI")]
struct Cli {
    #[command(subcommand)]
    group: Group,
}

#[derive(Subcommand)]
enum Group {
    /// Upload image → ssupload_uri
    Upload { image_path: String },
    /// Task operations
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// Element (主体) operations
    Element {
        #[command(subcommand)]
        action: ElementAction,
    },
}

#[derive(Subcommand)]
enum TaskAction {
    /// Submit task
    Submit {
        #[arg(long = "type", value_name = "TYPE")]
        task_type: String,
        #[arg(long)]
        prompt: String,
        #[arg(long = "image", action = clap::ArgAction::Append)]
        images: Vec<String>,
        #[arg(long = "material", action = clap::ArgAction::Append)]
        materials: Vec<String>,
        #[arg(long)]
        duration: i64,
        #[arg(long)]
        model_version: String,
        #[arg(long)]
        aspect_ratio: Option<String>,
        #[arg(long)]
        transition: Option<String>,
        #[arg(long)]
        resolution: String,
        #[arg(long, default_value = "1")]
        sample_count: i64,
        #[arg(long, default_value = "h265")]
        codec: String,
        #[arg(long, default_value = "auto")]
        movement_amplitude: String,
        #[arg(long, default_value = "normal")]
        schedule_mode: String,
    },
    /// Get task result
    Get { task_id: String },
    /// Stream SSE task state
    Sse { task_id: String },
}

#[derive(Subcommand)]
enum ElementAction {
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
        #[arg(long = "id")]
        elem_id: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "image")]
        modality: String,
        #[arg(long = "type", default_value = "user")]
        elem_type: String,
        #[arg(long = "image", action = clap::ArgAction::Append)]
        images: Vec<String>,
        #[arg(long, default_value = "0")]
        version: String,
        #[arg(long)]
        description: String,
    },
    /// List personal elements
    List {
        #[arg(long)]
        keyword: Option<String>,
        #[arg(long, default_value = "0")]
        page: i64,
        #[arg(long, default_value = "20")]
        pagesz: i64,
    },
    /// Search community elements
    Search {
        #[arg(long)]
        keyword: String,
        #[arg(long, default_value = "20")]
        pagesz: i64,
        #[arg(long, default_value = "recommend")]
        sort_by: String,
        #[arg(long, default_value = "")]
        page_token: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.group {
        Group::Upload { image_path } => {
            commands::upload::run(&image_path);
        }
        Group::Task { action } => match action {
            TaskAction::Submit {
                task_type, prompt, images, materials, duration,
                model_version, aspect_ratio, transition, resolution,
                sample_count, codec, movement_amplitude, schedule_mode,
            } => {
                commands::tasks::submit(
                    &task_type, &prompt, &images, &materials, duration,
                    &model_version, aspect_ratio.as_deref(), transition.as_deref(),
                    &resolution, sample_count, &codec, &movement_amplitude, &schedule_mode,
                );
            }
            TaskAction::Get { task_id } => commands::tasks::get(&task_id),
            TaskAction::Sse { task_id } => commands::tasks::sse(&task_id),
        },
        Group::Element { action } => match action {
            ElementAction::Preprocess { name, elem_type, images } => {
                commands::elements::preprocess(&name, &elem_type, &images);
            }
            ElementAction::Create { elem_id, name, modality, elem_type, images, version, description } => {
                commands::elements::create(&elem_id, &name, &modality, &elem_type, &images, &version, &description);
            }
            ElementAction::List { keyword, page, pagesz } => {
                commands::elements::list_elements(keyword.as_deref(), page, pagesz);
            }
            ElementAction::Search { keyword, pagesz, sort_by, page_token } => {
                commands::elements::search(&keyword, pagesz, &sort_by, &page_token);
            }
        },
    }
}
