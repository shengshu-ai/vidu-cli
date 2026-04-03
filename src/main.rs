mod client;
mod commands;
mod validators;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "vidu-cli", about = "Vidu API CLI", version = env!("CARGO_PKG_VERSION"))]
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
    /// Submit task (see parameter constraints by type below)
    ///
    /// TYPE: text2image
    ///   Models: 3.1, 3.2_fast_m, 3.2_pro_m
    ///   Duration: 0 (image generation)
    ///   Resolution: 1080p, 2k, 4k
    ///   Aspect Ratio: 4:3, 3:4, 1:1, 9:16, 16:9
    ///
    /// TYPE: text2video
    ///   Models: 3.0, 3.1, 3.2
    ///   Duration: 3.0→5s, 3.1→2-8s, 3.2→1-16s
    ///   Resolution: 1080p
    ///   Aspect Ratio: 16:9, 9:16, 1:1, 4:3, 3:4
    ///   Transition: 3.2 only (pro/speed)
    ///
    /// TYPE: img2video
    ///   Models: 3.0, 3.1, 3.2
    ///   Duration: 3.0→5s, 3.1→2-8s, 3.2→1-16s
    ///   Resolution: 1080p
    ///   Transition: 3.0→creative/stable, 3.1+→pro/speed
    ///   Images: 1 required
    ///
    /// TYPE: headtailimg2video
    ///   Models: 3.0, 3.1, 3.2
    ///   Duration: 3.0→5s, 3.1→2-8s, 3.2→1-16s
    ///   Resolution: 1080p
    ///   Transition: 3.0→creative/stable, 3.1+→pro/speed
    ///   Images: 2 required (head + tail)
    ///
    /// TYPE: reference2image
    ///   Models: 3.1, 3.2_fast_m, 3.2_pro_m
    ///   Duration: 0 (image generation)
    ///   Resolution: 1080p, 2k, 4k
    ///   Aspect Ratio: 4:3, 3:4, 1:1, 9:16, 16:9
    ///   Inputs: image + material ≤ 7
    ///
    /// TYPE: character2video
    ///   Models: 3.0, 3.1, 3.1_pro, 3.2
    ///   Duration: 3.0→5s, 3.1→2-8s, 3.1_pro→-1/2-8s, 3.2→1-16s
    ///   Resolution: 1080p
    ///   Aspect Ratio: 16:9, 9:16, 1:1, 4:3, 3:4
    ///   Inputs: image + material ≤ 7
    Submit {
        #[arg(long = "type", value_name = "TYPE", help = "Task type: text2image, text2video, img2video, headtailimg2video, reference2image, character2video")]
        task_type: String,
        #[arg(long)]
        prompt: String,
        #[arg(long = "image", action = clap::ArgAction::Append, help = "Image input (local path, URL, or ssupload:?id=xxx). Repeatable.")]
        images: Vec<String>,
        #[arg(long = "material", action = clap::ArgAction::Append, help = "Material reference (format: name:id:version). Repeatable.")]
        materials: Vec<String>,
        #[arg(long, help = "Duration in seconds. Range depends on model: 3.0(5), 3.1(2-8), 3.2(1-16). Use 0 for images.")]
        duration: i64,
        #[arg(long, help = "Model version: 3.0, 3.1, 3.2, 3.2_fast_m, 3.2_pro_m")]
        model_version: String,
        #[arg(long, help = "Aspect ratio: 16:9, 9:16, 1:1, 4:3, 3:4 (not for img2video/headtailimg2video)")]
        aspect_ratio: Option<String>,
        #[arg(long, help = "Transition style. Required for character2video 3.2 (pro/speed). For img2video 3.0: creative/stable. For 3.1+: pro/speed. For text2video 3.2 only.")]
        transition: Option<String>,
        #[arg(long, help = "Resolution: 1080p (all), 2k/4k (text2image/reference2image only)")]
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
    /// Lip sync: drive video mouth movement with text or audio
    ///
    /// Text mode:  --video <path> --text "hello" [--voice-id <id>] [--speed 1.0] [--volume 1.0]
    /// Audio mode: --video <path> --audio <path>
    /// Video: MP4/MOV/AVI, ≤500MB. Audio: MP3/WAV/AAC/M4A, ≤100MB.
    /// voice-id default: English_Aussie_Bloke. speed range: [0.5,2]. volume range: [0.5,2] or 0 to omit.
    LipSync {
        #[arg(long)]
        video: String,
        #[arg(long, help = "Text for lip sync (mutually exclusive with --audio). Chinese: 2-1000 chars, English: 4-2000 chars.")]
        text: Option<String>,
        #[arg(long, help = "Audio file for lip sync (mutually exclusive with --text). MP3/WAV/AAC/M4A, ≤100MB.")]
        audio: Option<String>,
        #[arg(long, default_value = "English_Aussie_Bloke")]
        voice_id: String,
        #[arg(long, default_value = "1")]
        speed: f64,
        #[arg(long, default_value = "0", help = "Volume [0.5,2], or 0 to use server default")]
        volume: f64,
        #[arg(long, default_value = "true")]
        enhance: bool,
        #[arg(long, default_value = "h265")]
        codec: String,
    },
    /// List available voice IDs for lip-sync
    LipSyncVoices,
}

#[derive(Subcommand)]
enum ElementAction {
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
            TaskAction::LipSync { video, text, audio, voice_id, speed, volume, enhance, codec } => {
                commands::tasks::submit_lip_sync(
                    &video, text.as_deref(), audio.as_deref(),
                    &voice_id, speed, volume, enhance, &codec,
                );
            }
            TaskAction::LipSyncVoices => {
                commands::tasks::list_voices();
            }
        },
        Group::Element { action } => match action {
            ElementAction::Check { name } => {
                commands::elements::check(&name);
            }
            ElementAction::Preprocess { name, elem_type, images } => {
                commands::elements::preprocess(&name, &elem_type, &images);
            }
            ElementAction::Create { name, modality, elem_type, images, description, style } => {
                commands::elements::create(&name, &modality, &elem_type, &images, description.as_deref(), style.as_deref());
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
