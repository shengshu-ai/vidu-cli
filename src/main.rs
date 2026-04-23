mod client;
mod commands;
mod validators;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "vidu-cli", about = "Vidu API CLI", version = env!("VIDU_CLI_VERSION"), propagate_version = true)]
struct Cli {
    #[arg(short = 'v', long = "version")]
    version: bool,
    #[command(subcommand)]
    group: Option<Group>,
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
    /// Quota / billing operations
    Quota {
        #[command(subcommand)]
        action: QuotaAction,
    },
}

#[derive(Subcommand)]
enum TaskAction {
    /// Submit task (see parameter constraints by type below)
    ///
    /// TYPE: text2image
    ///   Models: 3.1, 3.2_fast_m, 3.2_pro_m, 3.2_image_2
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
    ///   Models: 3.1, 3.2_fast_m, 3.2_pro_m, 3.2_image_2
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
        #[arg(long, help = "Model version: 3.0, 3.1, 3.2, 3.2_fast_m, 3.2_pro_m, 3.2_image_2")]
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
        #[arg(long, help = "Schedule mode: claw_pass (use daily quota) or normal (use credits). Auto-detected from claw-pass status if omitted.")]
        schedule_mode: Option<String>,
    },
    /// Get task result
    Get {
        task_id: String,
        #[arg(long, short = 'o', help = "Output directory for downloading media files")]
        output: Option<String>,
    },
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
        #[arg(long, help = "Schedule mode: claw_pass (use daily quota) or normal (use credits). Auto-detected from claw-pass status if omitted.")]
        schedule_mode: Option<String>,
    },
    /// List available voice IDs for lip-sync
    LipSyncVoices,
    /// TTS: Convert text to speech
    ///
    /// Single segment: --prompt "text" [--emotion happy]
    /// Multi segment:  --text "seg1" --emotion happy --text "seg2" --emotion sad --text "seg3"
    ///
    /// Required: --voice-id, and one of --prompt or --text
    /// Optional: --speed, --volume, --emotion, --language-boost
    Tts {
        #[arg(long, help = "Single text segment (mutually exclusive with --text)", conflicts_with = "texts")]
        prompt: Option<String>,
        #[arg(long = "text", action = clap::ArgAction::Append, help = "Text segment, repeatable for multi-segment TTS (mutually exclusive with --prompt)", conflicts_with = "prompt")]
        texts: Vec<String>,
        #[arg(long, help = "Voice ID (use 'vidu-cli task tts-voices' to list available voices)")]
        voice_id: String,
        #[arg(long, default_value = "1.0", help = "Speech speed: 0.5-2.0")]
        speed: f64,
        #[arg(long, default_value = "80", help = "Volume: 0-100")]
        volume: i32,
        #[arg(long, action = clap::ArgAction::Append, help = "Emotion per segment (paired by order with --text), or global emotion for --prompt")]
        emotion: Vec<String>,
        #[arg(long, help = "Language boost for small languages/dialects: Chinese, English, auto, etc. (optional)")]
        language_boost: Option<String>,
        #[arg(long, help = "Schedule mode: claw_pass (use daily quota) or normal (use credits). Auto-detected from claw-pass status if omitted.")]
        schedule_mode: Option<String>,
    },
    /// List available TTS voice IDs
    TtsVoices,
    /// Compose: export video from multi-track timeline
    ///
    /// --timeline accepts a JSON file path or inline JSON string.
    /// The timeline describes multi-track video/audio/subtitle/effect clips.
    /// media_url in timeline supports: ssupload:?id=xxx,
    /// http(s) URL, or local file path (auto-upload).
    /// Returns task_id — query with `task get <task_id>`.
    Compose {
        #[arg(long, help = "Timeline JSON (file path or inline JSON string)")]
        timeline: String,
        #[arg(long, help = "Output width in pixels")]
        width: Option<i32>,
        #[arg(long, help = "Output height in pixels")]
        height: Option<i32>,
        #[arg(long, help = "Schedule mode: claw_pass (use daily quota) or normal (use credits). Auto-detected from claw-pass status if omitted.")]
        schedule_mode: Option<String>,
    },
    /// Query credit cost for a task before submitting
    Cost {
        #[arg(long = "type", value_name = "TYPE", help = "Task type: text2image, text2video, img2video, headtailimg2video, reference2image, character2video")]
        task_type: String,
        #[arg(long, help = "Model version: 3.0, 3.1, 3.2, 3.2_fast_m, 3.2_pro_m, 3.2_image_2")]
        model_version: String,
        #[arg(long, help = "Duration in seconds")]
        duration: i64,
        #[arg(long, default_value = "1080p")]
        resolution: String,
        #[arg(long)]
        aspect_ratio: Option<String>,
        #[arg(long)]
        transition: Option<String>,
        #[arg(long, default_value = "1")]
        sample_count: i64,
        #[arg(long, default_value = "h265")]
        codec: String,
        #[arg(long, help = "Schedule mode: claw_pass (use daily quota) or normal (use credits). Auto-detected from claw-pass status if omitted.")]
        schedule_mode: Option<String>,
    },
    /// Query credit cost for a TTS task before submitting
    TtsCost {
        #[arg(long, help = "Text content (cost is calculated by character count)")]
        text: String,
        #[arg(long, help = "Voice ID (use 'vidu-cli task tts-voices' to list available voices)")]
        voice_id: String,
        #[arg(long, default_value = "1.0", help = "Speech speed: 0.5-2.0")]
        speed: f64,
        #[arg(long, default_value = "0", help = "Pitch adjustment")]
        pitch: i32,
        #[arg(long, default_value = "80", help = "Volume: 0-100")]
        volume: i32,
        #[arg(long, help = "Schedule mode: claw_pass (use daily quota) or normal (use credits). Auto-detected from claw-pass status if omitted.")]
        schedule_mode: Option<String>,
    },
    /// Query credit cost for a lip-sync task before submitting
    LipSyncCost {
        #[arg(long, help = "Duration in seconds")]
        duration: i64,
        #[arg(long, default_value = "English_Aussie_Bloke", help = "Voice ID")]
        voice_id: String,
        #[arg(long, default_value = "1.0", help = "Speech speed: 0.5-2.0")]
        speed: f64,
        #[arg(long, default_value = "0", help = "Volume [0.5,2], or 0 for server default")]
        volume: f64,
        #[arg(long, default_value = "h265")]
        codec: String,
        #[arg(long, help = "Schedule mode: claw_pass (use daily quota) or normal (use credits). Auto-detected from claw-pass status if omitted.")]
        schedule_mode: Option<String>,
    },
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

#[derive(Subcommand)]
enum QuotaAction {
    /// Query claw-pass daily quota status
    Pass,
    /// Query user credit balance
    Credit,
}

fn main() {
    let cli = Cli::parse();

    if cli.version {
        println!("vidu-cli {}", env!("VIDU_CLI_VERSION"));
        return;
    }

    let group = match cli.group {
        Some(g) => g,
        None => {
            use clap::CommandFactory;
            Cli::command().print_help().unwrap();
            println!();
            return;
        }
    };

    match group {
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
                    &resolution, sample_count, &codec, &movement_amplitude, schedule_mode.as_deref(),
                );
            }
            TaskAction::Get { task_id, output } => commands::tasks::get(&task_id, output.as_deref()),
            TaskAction::LipSync { video, text, audio, voice_id, speed, volume, enhance, codec, schedule_mode } => {
                commands::tasks::submit_lip_sync(
                    &video, text.as_deref(), audio.as_deref(),
                    &voice_id, speed, volume, enhance, &codec, schedule_mode.as_deref(),
                );
            }
            TaskAction::LipSyncVoices => {
                commands::tasks::list_voices();
            }
            TaskAction::Tts { prompt, texts, voice_id, speed, volume, emotion, language_boost, schedule_mode } => {
                commands::tasks::submit_tts(prompt.as_deref(), &texts, &emotion, &voice_id, speed, volume, language_boost.as_deref(), schedule_mode.as_deref());
            }
            TaskAction::TtsVoices => {
                commands::tasks::list_tts_voices();
            }
            TaskAction::Compose { timeline, width, height, schedule_mode } => {
                commands::tasks::compose(&timeline, width, height, schedule_mode.as_deref());
            }
            TaskAction::Cost {
                task_type, model_version, duration, resolution,
                aspect_ratio, transition, sample_count, codec, schedule_mode,
            } => {
                commands::tasks::query_credits(
                    &task_type, &model_version, duration, &resolution,
                    aspect_ratio.as_deref(), transition.as_deref(),
                    sample_count, &codec, schedule_mode.as_deref(),
                );
            }
            TaskAction::TtsCost { text, voice_id, speed, pitch, volume, schedule_mode } => {
                commands::tasks::query_tts_credits(
                    &text, &voice_id, speed, pitch, volume, schedule_mode.as_deref(),
                );
            }
            TaskAction::LipSyncCost { duration, voice_id, speed, volume, codec, schedule_mode } => {
                commands::tasks::query_lip_sync_credits(
                    duration, &voice_id, speed, volume, &codec, schedule_mode.as_deref(),
                );
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
        Group::Quota { action } => match action {
            QuotaAction::Pass => commands::quota::claw_pass_status(),
            QuotaAction::Credit => commands::quota::credit_status(),
        },
    }
}
