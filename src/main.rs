mod cli_errors;
mod client;
mod commands;
mod element_legacy_cli;
mod validators;

use clap::builder::PossibleValuesParser;
use clap::{Args, Parser, Subcommand};
use std::process::ExitCode;

const SAMPLE_COUNT_HELP: &str = "Positive sample count. Drama/Wan support 1-4; other models use the protocol's positive int32 range with entitlement and any dynamic limits checked by the server.";

// Share membership wording across submit, cost, and rerun so model help stays consistent.
const MODEL_VERSION_HELP: &str = concat!(
    "Model version (per-type compatibility enforced after parse; 3.4 = Q4). ",
    "Membership duration limits for video tasks: 3.1 / 3.1_pro require membership above 5s (max 8s); ",
    "3.2 (Q3) / 3.4 (Q4) require membership above 10s (max 16s). ",
    "Member features requiring active membership entitlements: 3.2_a (Seedance 2.0), ",
    "viduq3.1-drama-std (Seedance 2.5), viduq2-drama-std (Wan 3), ",
    "3.2_image_2_5_fast / 3.2_image_2_5_pro (Image 2.5). ",
    "Image 2.5 Fast/Pro free trials are available only with remaining trial quota; ",
    "active membership entitlements are required after trials are exhausted. ",
    "Trial eligibility and remaining quota are determined by the server. ",
    "Membership eligibility is validated by the server.",
);

#[derive(Parser)]
#[command(name = "vidu-cli", about = "Vidu API CLI", version = env!("VIDU_CLI_VERSION"), propagate_version = true)]
struct Cli {
    #[arg(long, global = true, default_value = "json", value_parser = ["json", "table"])]
    format: String,
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
        action: Box<TaskAction>,
    },
    /// Element (主体) operations
    Element {
        #[command(subcommand)]
        action: ElementAction,
    },
    /// Creation operations
    Creation {
        #[command(subcommand)]
        action: CreationAction,
    },
    /// Personal cloned voice operations
    Voice {
        #[command(subcommand)]
        action: VoiceAction,
    },
    /// Quota / billing operations
    Quota {
        #[command(subcommand)]
        action: QuotaAction,
    },
}

#[derive(Args)]
struct SubmissionOptions {
    #[arg(long)]
    wait: bool,
    #[arg(long, requires = "wait", value_parser = clap::value_parser!(u64).range(1..))]
    timeout: Option<u64>,
    #[arg(long, short = 'o', requires = "wait")]
    output: Option<String>,
}

impl SubmissionOptions {
    fn finish(self, data: serde_json::Value) {
        client::ok(commands::task_ops::finish_submission(
            data,
            self.wait,
            self.timeout.unwrap_or(1800),
            self.output.as_deref(),
        ));
    }
}

#[derive(Args)]
struct OutputAudioOptions {
    #[arg(
        long,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        action = clap::ArgAction::Set,
        help = "Generate output audio for supported video models. --with-audio enables; --with-audio=false disables. Omit to preserve existing behavior (historical choice for rerun). --audio supplies reference audio."
    )]
    with_audio: Option<bool>,
}

#[derive(Args)]
struct UpscaleOptions {
    /// Completed video creation (mutually exclusive with --video)
    #[arg(long, required_unless_present = "video", conflicts_with = "video", value_parser = clap::value_parser!(i64).range(1..))]
    creation_id: Option<i64>,
    /// Local MP4/MOV/AVI, HTTP/HTTPS URL, or ssupload:?id=...
    #[arg(long, required_unless_present = "creation_id")]
    video: Option<String>,
    #[arg(long, value_parser = ["720p", "1080p", "2k", "4k"])]
    resolution: String,
    /// Source duration in seconds: required only for URL/ssupload; does not trim video
    #[arg(long, conflicts_with = "creation_id", value_parser = clap::value_parser!(i32).range(1..))]
    duration: Option<i32>,
}

#[derive(Subcommand)]
enum TaskAction {
    /// Upscale a video creation or video file/URL/upload
    Upscale {
        #[command(flatten)]
        input: UpscaleOptions,
        #[command(flatten)]
        submission: SubmissionOptions,
    },
    /// Estimate video upscale credits without uploading or creating a task
    UpscaleCost {
        #[command(flatten)]
        input: UpscaleOptions,
    },
    /// Submit task (see parameter constraints by type below)
    ///
    /// TYPE: text2image
    ///   Models: 3.1, 3.2_fast_m, 3.2_pro_m, 3.2_image_2, 3.2_image_2_5_fast, 3.2_image_2_5_pro
    ///   Duration: 0 (image generation)
    ///   Resolution: 1080p, 2k, 4k
    ///   Aspect Ratio: positive w:h or auto; 3.1 requires 1:10 to 10:1; Image 2.5 includes 3:2/2:3/21:9/9:21
    ///
    /// TYPE: text2video
    ///   Models: 3.0, 3.1, 3.2, 3.2_a, viduq3.1-drama-std
    ///   Duration: 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→-1/4-15s, Drama→4-30s (default 5)
    ///   Resolution: 1080p; SD: 480p/720p/1080p; Drama→480p/720p/1080p (default 720p)
    ///   Aspect Ratio: Vidu positive w:h; SD2.0 uses 16:9/9:16/1:1/4:3/3:4/21:9/adaptive; Drama uses the five standard ratios
    ///   Transition: known values are forwarded; pro/speed select 3.2 variants
    ///
    /// TYPE: img2video
    ///   Models: 3.0, 3.1, 3.2, 3.2_a, 3.4 (Q4, normal/nopeak)
    ///   Duration: 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→-1/4-15s, 3.4→3-16s
    ///   Resolution: 1080p; SD: 480p/720p/1080p; Q4 also supports 540p, 720p, 2k, 4k
    ///   Transition: 3.0→creative/stable (default stable); img2video 3.1/3.2 requires a transition; known values are forwarded for backend policy
    ///   Images: 1 required
    ///
    /// TYPE: headtailimg2video
    ///   Models: 3.0, 3.1, 3.2, 3.2_a
    ///   Duration: 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→-1/4-15s
    ///   Resolution: 1080p; SD: 480p/720p/1080p
    ///   Transition: optional; 3.0 creative/stable and 3.1/3.2 pro/speed select variants
    ///   Images: 2 required (head + tail)
    ///
    /// TYPE: reference2image
    ///   Models: 3.1, 3.2_fast_m, 3.2_pro_m, 3.2_image_2, 3.2_image_2_5_fast, 3.2_image_2_5_pro
    ///   Duration: 0 (image generation)
    ///   Resolution: 1080p, 2k, 4k
    ///   Aspect Ratio: positive w:h or auto; 3.1 requires 1:10 to 10:1; Image 2.5 includes 3:2/2:3/21:9/9:21
    ///   Inputs: up to 19 direct images with one text prompt; material images are expanded/allocated by the server
    ///
    /// TYPE: character2video
    ///   Models: 3.0, 3.1, 3.1_pro, 3.2, 3.2_a, 3.4 (Q4, normal/nopeak), viduq3.1-drama-std
    ///   Duration: 3.0→5s, 3.1→2-8s, 3.1_pro→-1/2-8s, 3.2→1-16s, 3.2_a→-1/4-15s, 3.4→1-16s, Drama→4-30s (default 5)
    ///   Resolution: 1080p; SD: 480p/720p/1080p; Q4 also supports 540p, 720p, 2k, 4k; Drama supports 480p/720p/1080p (default 720p)
    ///   Aspect Ratio: Vidu positive w:h; SD2.0 uses 16:9/9:16/1:1/4:3/3:4/21:9/adaptive; Drama uses the five standard ratios
    ///   Inputs: direct images Q4 ≤15, 3.2_a ≤9; other legacy models fit the total 20-prompt limit (normally ≤19 images)
    ///   With materials, ordinary legacy models share a 7-image budget; Q4=15, SD2.0=9, 3.1_pro uses server frame slots
    ///   SD2.5: image ≤30, video ≤10, audio ≤10; image/text materials supported. SD2.0: video/audio ≤3 each; 3.1_pro references share a 48-slot server budget; references that do not fit may be omitted
    ///   Q4 accepts image/audio references; SD2.0/2.5 require an image/image-material/video reference, even when audio is present
    ///   3.2 character2video also accepts pro_ad (duration 3-16s). SD rejects nopeak
    ///
    /// MEMBERSHIP DURATION LIMITS (video tasks):
    ///   3.1 / 3.1_pro: free users max 5s; membership required for 6-8s
    ///   3.2 (Q3) / 3.4 (Q4): free users max 10s; membership required for 11-16s
    ///
    /// MEMBER FEATURES (require active membership entitlements; server validates eligibility):
    ///   Seedance 2.0: 3.2_a
    ///   Seedance 2.5: viduq3.1-drama-std (also labeled Q3.1 Drama)
    ///   Image 2.5: 3.2_image_2_5_fast, 3.2_image_2_5_pro
    ///     Free trials only with remaining trial quota; membership required after trials are exhausted
    ///     Fast/Pro and text2image/reference2image trial quotas are tracked separately
    ///     Trial eligibility and remaining quota are determined by the server
    ///
    /// WAN 3: viduq2-drama-std (member feature; requires active membership entitlements)
    ///   Types: text2video, img2video, headtailimg2video, character2video
    ///   Duration: 2-30s without video; 1-30s with video, input + output ≤30 (default 5); resolution: 480p/720p/1080p (default 720p)
    ///   Aspect Ratio: 16:9 (default), 9:16, 1:1, 4:3, 3:4, 21:9, adaptive
    ///   Credits only; transition is forwarded for backend policy. References: max 10 images, 5 videos, 5 audios, 20 total
    ///   Each video: 1-15s; audio has no specified minimum duration; total 15s per kind; input video + output ≤30s
    Submit {
        #[arg(long = "type", value_name = "TYPE",
            value_parser = validators::parse_task_type,
            help = "Task type: text2image, text2video, img2video, headtailimg2video, reference2image (alias: img2image), character2video")]
        task_type: String,
        #[arg(
            long,
            help = "Non-empty text prompt. Length limits are checked by the backend. Preferred over --prompt-path when both are given."
        )]
        prompt: Option<String>,
        #[arg(
            long = "prompt-path",
            alias = "prompt-file",
            value_name = "PATH",
            help = "Read the prompt from a UTF-8 file (≤1MiB). Ignored if --prompt is also set."
        )]
        prompt_path: Option<String>,
        #[arg(long = "image", action = clap::ArgAction::Append, help = "Image input (local path, URL, or ssupload:?id=xxx). Repeatable.")]
        images: Vec<String>,
        #[arg(long = "material", action = clap::ArgAction::Append, help = "Material reference (format: name:id:version). Repeatable.")]
        materials: Vec<String>,
        #[arg(
            long = "audio",
            action = clap::ArgAction::Append,
            help = "Reference audio for character2video + 3.2_a, Q4, Drama, or Wan 3. Local audio or ssupload URI. SD2.0 WAV/MP3: max 3, ≤15 MiB each, each 2-15s and total ≤15s. Q4 WAV/MP3: max 3, ≤20 MiB, each 1-16s. Drama WAV/MP3: max 10, local size ≤15 MiB, each 4-30s and total ≤30s after server-style rounding. Wan 3 WAV/MP3: max 5, local size ≤15 MiB, total ≤15s; no specified minimum duration."
        )]
        audios: Vec<String>,
        #[command(flatten)]
        audio_output: OutputAudioOptions,
        #[arg(long = "video", action = clap::ArgAction::Append, help = "Reference video (local path or ssupload URI), repeatable for character2video. SD2.0: max 3, each 2-15s and total ≤15s, 407696-8847360 pixels; SD2.5: max 10; 3.1_pro references are selected by the server slot budget. SD videos: MP4/MOV, ≤200 MiB. Local video uses built-in MP4 parsing with ffprobe fallback. SD2.5: rounded duration 4-30s each, total ≤30s; no additional local FPS/geometry checks. Wan 3: max 5, each 1-15s, total ≤15s, ≤100 MiB; input video + output ≤30s.")]
        videos: Vec<String>,
        #[arg(
            long,
            allow_negative_numbers = true,
            help = "Video duration: 3.0(5), 3.1(2-8), 3.1_pro(-1/2-8), 3.2(1-16; pro_ad 3-16), 3.2_a(-1/4-15), Q4 img2video(3-16)/character2video(1-16), Drama(4-30, default 5), Wan 3(2-30 without video; 1-30 with video, input + output ≤30, default 5). Ignored for images. Video membership limits: 3.1/3.1_pro above 5s; Q3/Q4 above 10s."
        )]
        duration: Option<i64>,
        #[arg(long,
            alias = "model",
            value_parser = PossibleValuesParser::new(validators::MODEL_VERSIONS),
            help = MODEL_VERSION_HELP)]
        model_version: String,
        #[arg(long,
            value_parser = validators::parse_aspect_ratio,
            help = "Aspect ratio (validated per model): images accept positive w:h or auto, 3.1 within 1:10 to 10:1; Image 2.5 includes 3:2/2:3/21:9/9:21; ordinary Vidu videos accept positive w:h; SD/Wan use their own ratio sets, including 21:9/adaptive")]
        aspect_ratio: Option<String>,
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::TRANSITIONS),
            help = "Transition forwarded unchanged for backend trials, entitlement and pricing. img2video 3.0/3.1/3.2 and character2video 3.2 require a value; headtailimg2video allows omission. creative/stable select Q1 variants, pro/speed select Q2/Q3 variants, and pro_ad selects Q3 reference marketing mode. Q4 clears it later; Drama/Wan generation does not use it.")]
        transition: Option<String>,
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::RESOLUTIONS),
            help = "Resolution: required for legacy submit models; Drama/Wan 3 default to 720p (480p/720p/1080p), Q4 supports 540p/720p/1080p/2k/4k")]
        resolution: Option<String>,
        #[arg(long, default_value = "1", value_parser = clap::value_parser!(i64).range(1..), help = SAMPLE_COUNT_HELP)]
        sample_count: i64,
        #[arg(long, default_value = "h265",
            value_parser = PossibleValuesParser::new(validators::CODECS))]
        codec: String,
        #[arg(long, default_value = "auto")]
        movement_amplitude: String,
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::SCHEDULE_MODES),
            help = "Q4 defaults to normal, allows nopeak subject to server rules, and rejects claw_pass; SD rejects nopeak; Drama/Wan 3 support only normal credits. Other models auto-detect pass status if omitted.")]
        schedule_mode: Option<String>,
        #[command(flatten)]
        submission: SubmissionOptions,
    },
    /// List task history
    List {
        #[arg(long = "state", action = clap::ArgAction::Append)]
        states: Vec<String>,
        #[arg(long = "type", action = clap::ArgAction::Append)]
        types: Vec<String>,
        #[arg(long = "scene", action = clap::ArgAction::Append)]
        scenes: Vec<String>,
        #[arg(long, default_value = "")]
        page_token: String,
        #[arg(long, default_value = "20", value_parser = clap::value_parser!(i64).range(1..=100))]
        pagesz: i64,
    },
    /// Cancel a queued task
    Cancel { task_id: String },
    /// Delete task history
    Delete { task_id: String },
    /// Wait for completion and optionally download results
    Wait {
        task_id: String,
        #[arg(long, default_value = "1800", value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
        #[arg(long, default_value = "5", value_parser = clap::value_parser!(u64).range(1..))]
        poll_interval: u64,
        #[arg(long, short = 'o')]
        output: Option<String>,
    },
    /// Extend a completed video creation
    Extend {
        #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
        creation_id: i64,
        #[arg(long, value_parser = clap::value_parser!(i64).range(1..=7))]
        duration: i64,
        #[arg(long, default_value = "extendspeed", value_parser = ["extendspeed", "extendpro"])]
        transition: String,
        #[command(flatten)]
        submission: SubmissionOptions,
    },
    /// Upscale an image creation
    UpscaleImage {
        #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
        creation_id: i64,
        #[arg(long, default_value = "2k", value_parser = ["720p", "1080p", "2k", "4k"])]
        resolution: String,
        #[command(flatten)]
        submission: SubmissionOptions,
    },
    /// Submit a new task using historical inputs
    Rerun {
        task_id: String,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long = "prompt-path", alias = "prompt-file")]
        prompt_path: Option<String>,
        #[arg(long = "model-version", alias = "model", value_parser = PossibleValuesParser::new(validators::MODEL_VERSIONS), help = MODEL_VERSION_HELP)]
        model: Option<String>,
        #[arg(
            long,
            allow_negative_numbers = true,
            help = "Override video duration. Membership required above 5s for 3.1/3.1_pro (max 8s), above 10s for 3.2 (Q3)/3.4 (Q4) (max 16s). Ignored for images."
        )]
        duration: Option<i64>,
        #[arg(long, value_parser = PossibleValuesParser::new(validators::RESOLUTIONS))]
        resolution: Option<String>,
        #[command(flatten)]
        audio_output: OutputAudioOptions,
        #[command(flatten)]
        submission: SubmissionOptions,
    },
    /// Get task result
    Get {
        task_id: String,
        #[arg(long)]
        details: bool,
        #[arg(
            long,
            short = 'o',
            help = "Output path for downloading media files. Existing directories and paths without an extension are treated as directories; file paths are supported for single-artifact tasks."
        )]
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
        #[arg(
            long,
            help = "Text for lip sync (mutually exclusive with --audio). Chinese: 2-1000 chars, English: 4-2000 chars."
        )]
        text: Option<String>,
        #[arg(
            long,
            help = "Audio file for lip sync (mutually exclusive with --text). MP3/WAV/AAC/M4A, ≤100MB."
        )]
        audio: Option<String>,
        #[arg(long, alias = "voice", default_value = "English_Aussie_Bloke")]
        voice_id: String,
        #[arg(long, default_value = "1")]
        speed: f64,
        #[arg(
            long,
            default_value = "0",
            help = "Volume [0.5,2], or 0 to use server default"
        )]
        volume: f64,
        #[arg(long, default_value = "true")]
        enhance: bool,
        #[arg(long, default_value = "h265",
            value_parser = PossibleValuesParser::new(validators::CODECS))]
        codec: String,
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::SCHEDULE_MODES),
            help = "Schedule mode: nopeak (off-peak quota; server checks eligibility, remaining quota and task/model rules), claw_pass (pass quota), or normal (credits). If omitted, an active claw pass is auto-detected; otherwise normal is used.")]
        schedule_mode: Option<String>,
        #[command(flatten)]
        submission: SubmissionOptions,
    },
    /// List available voice IDs for lip-sync
    LipSyncVoices,
    /// TTS: Convert text to speech
    ///
    /// Single segment: --prompt "text" [--emotion happy]
    /// Multi segment:  --text "seg1" --emotion happy --text "seg2" --emotion sad --text "seg3"
    ///
    /// Required: --voice-id, and one of --prompt, --prompt-path, or --text
    /// Optional: --speed, --volume, --emotion, --language-boost
    Tts {
        #[arg(
            long,
            help = "Single text segment (mutually exclusive with --text). Preferred over --prompt-path when both are given.",
            conflicts_with = "texts"
        )]
        prompt: Option<String>,
        #[arg(
            long = "prompt-path",
            alias = "prompt-file",
            value_name = "PATH",
            help = "Read the prompt from a UTF-8 file (≤1MiB). Ignored if --prompt is also set. Mutually exclusive with --text.",
            conflicts_with = "texts"
        )]
        prompt_path: Option<String>,
        #[arg(long = "text", action = clap::ArgAction::Append, help = "Text segment, repeatable for multi-segment TTS (mutually exclusive with --prompt)", conflicts_with = "prompt")]
        texts: Vec<String>,
        #[arg(
            long,
            alias = "voice",
            help = "Voice ID (use 'vidu-cli task tts-voices' to list available voices)"
        )]
        voice_id: String,
        #[arg(long, default_value = "1.0", help = "Speech speed: 0.5-2.0")]
        speed: f64,
        #[arg(long, default_value = "80", help = "Volume: 0-100")]
        volume: i32,
        #[arg(long, action = clap::ArgAction::Append, help = "Emotion per segment (paired by order with --text), or global emotion for --prompt")]
        emotion: Vec<String>,
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::LANGUAGE_BOOSTS),
            help = "Language boost for small languages/dialects: Chinese, English, auto, etc. (optional)")]
        language_boost: Option<String>,
        #[arg(long, visible_alias = "subtitle-enabled", action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true", default_value_t = true, value_parser = clap::value_parser!(bool), help = "Enable subtitle JSON output for single --prompt TTS")]
        subtitle_enable: bool,
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::SCHEDULE_MODES),
            help = "Schedule mode: nopeak (off-peak quota; server checks eligibility, remaining quota and task/model rules), claw_pass (pass quota), or normal (credits). If omitted, an active claw pass is auto-detected; otherwise normal is used.")]
        schedule_mode: Option<String>,
        #[command(flatten)]
        submission: SubmissionOptions,
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
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::SCHEDULE_MODES),
            help = "Schedule mode: nopeak (off-peak quota; server checks eligibility, remaining quota and task/model rules), claw_pass (pass quota), or normal (credits). If omitted, an active claw pass is auto-detected; otherwise normal is used.")]
        schedule_mode: Option<String>,
        #[command(flatten)]
        submission: SubmissionOptions,
    },
    /// Query credit cost for a task before submitting
    Cost {
        #[arg(long = "type", value_name = "TYPE",
            value_parser = validators::parse_task_type,
            help = "Task type: text2image, text2video, img2video, headtailimg2video, reference2image (alias: img2image), character2video")]
        task_type: String,
        #[arg(long,
            alias = "model",
            value_parser = PossibleValuesParser::new(validators::MODEL_VERSIONS),
            help = MODEL_VERSION_HELP)]
        model_version: String,
        #[command(flatten)]
        audio_output: OutputAudioOptions,
        #[arg(
            long,
            allow_negative_numbers = true,
            help = "Duration in seconds. Required for legacy video models; Drama defaults to 5 (range 4-30); Wan 3 defaults to 5 (2-30 without video; 1-30 quotes for character2video, input + output ≤30). Ignored for image tasks. Video membership limits: 3.1/3.1_pro above 5s; Q3/Q4 above 10s."
        )]
        duration: Option<i64>,
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::RESOLUTIONS),
            help = "Resolution. Defaults to 1080p for legacy models and 720p for Drama/Wan 3.")]
        resolution: Option<String>,
        #[arg(long,
            value_parser = validators::parse_aspect_ratio)]
        aspect_ratio: Option<String>,
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::TRANSITIONS))]
        transition: Option<String>,
        #[arg(long, default_value = "1", value_parser = clap::value_parser!(i64).range(1..), help = SAMPLE_COUNT_HELP)]
        sample_count: i64,
        #[arg(long, default_value = "h265",
            value_parser = PossibleValuesParser::new(validators::CODECS))]
        codec: String,
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::SCHEDULE_MODES),
            help = "Q4 defaults to normal, allows nopeak subject to server rules, and rejects claw_pass; SD rejects nopeak; Drama/Wan 3 support only normal credits. Other models auto-detect pass status if omitted.")]
        schedule_mode: Option<String>,
    },
    /// Query credit cost for a TTS task before submitting
    TtsCost {
        #[arg(long, help = "Text content (cost is calculated by character count)")]
        text: String,
        #[arg(
            long,
            alias = "voice",
            help = "Voice ID (use 'vidu-cli task tts-voices' to list available voices)"
        )]
        voice_id: String,
        #[arg(long, default_value = "1.0", help = "Speech speed: 0.5-2.0")]
        speed: f64,
        #[arg(long, default_value = "0", help = "Pitch adjustment")]
        pitch: i32,
        #[arg(long, default_value = "80", help = "Volume: 0-100")]
        volume: i32,
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::SCHEDULE_MODES),
            help = "Schedule mode: nopeak (off-peak quota; server checks eligibility, remaining quota and task/model rules), claw_pass (pass quota), or normal (credits). If omitted, an active claw pass is auto-detected; otherwise normal is used.")]
        schedule_mode: Option<String>,
    },
    /// Query credit cost for a lip-sync task before submitting
    LipSyncCost {
        #[arg(long, help = "Duration in seconds")]
        duration: i64,
        #[arg(
            long,
            alias = "voice",
            default_value = "English_Aussie_Bloke",
            help = "Voice ID"
        )]
        voice_id: String,
        #[arg(long, default_value = "1.0", help = "Speech speed: 0.5-2.0")]
        speed: f64,
        #[arg(
            long,
            default_value = "0",
            help = "Volume [0.5,2], or 0 for server default"
        )]
        volume: f64,
        #[arg(long, default_value = "h265",
            value_parser = PossibleValuesParser::new(validators::CODECS))]
        codec: String,
        #[arg(long,
            value_parser = PossibleValuesParser::new(validators::SCHEDULE_MODES),
            help = "Schedule mode: nopeak (off-peak quota; server checks eligibility, remaining quota and task/model rules), claw_pass (pass quota), or normal (credits). If omitted, an active claw pass is auto-detected; otherwise normal is used.")]
        schedule_mode: Option<String>,
    },
}

#[derive(Subcommand)]
enum ElementAction {
    /// Get latest visible element
    Get { element_id: String },
    /// Update personal element metadata or replace its ordered images
    Update(commands::elements::UpdateArgs),
    /// Get first-level element tags (for --tag IDs)
    Tags,
    /// Delete personal element
    Delete { element_id: String },
    /// List collected community or official elements
    Favorites {
        #[command(flatten)]
        generation: commands::elements::EditionArgs,
        #[arg(long, default_value = "community", value_parser = ["community", "official"])]
        source: String,
        /// Filter official collections by keyword.
        #[arg(long)]
        keyword: Option<String>,
        /// Filter official collections by a single category ID.
        #[arg(long)]
        tag: Option<String>,
        #[arg(long, default_value = "")]
        page_token: String,
        #[arg(long, default_value = "20", value_parser = clap::value_parser!(i64).range(1..=100))]
        pagesz: i64,
    },
    /// Collect a community or official element (not a like)
    Favorite {
        element_id: String,
        #[arg(long, default_value = "community", value_parser = ["community", "official"])]
        source: String,
    },
    /// Remove a community or official element from collections
    Unfavorite {
        element_id: String,
        #[arg(long, default_value = "community", value_parser = ["community", "official"])]
        source: String,
    },

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
    /// Create a second-generation image element
    Create(commands::elements::CreateArgs),
    /// List personal or official elements
    List {
        #[command(flatten)]
        generation: commands::elements::EditionArgs,
        #[arg(long, default_value = "personal", value_parser = ["personal", "official"])]
        source: String,
        /// Filter personal or official elements by a single category ID.
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        keyword: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(i64).range(0..=i32::MAX as i64), conflicts_with = "page_token")]
        page: Option<i64>,
        #[arg(long)]
        page_token: Option<String>,
        #[arg(long, default_value = "20", value_parser = clap::value_parser!(i64).range(10..=100))]
        pagesz: i64,
    },
    /// Search personal, community or official elements by conditions
    ///
    /// Supply at least one keyword, tag, edition or community category/topic/sort-by.
    /// Source defaults to community; a tag does not automatically change it.
    /// Second-generation elements cannot currently be published to the community.
    Search {
        #[command(flatten)]
        generation: commands::elements::EditionArgs,
        #[arg(long, default_value = "community", value_parser = ["community", "official", "personal"])]
        source: String,
        /// Filter personal or official elements by a single category ID.
        #[arg(long)]
        tag: Option<String>,
        /// Community category filter.
        #[arg(long)]
        category: Option<String>,
        /// Community topic filter.
        #[arg(long)]
        topic: Option<String>,
        /// Optional keyword; must not be blank when supplied.
        #[arg(long)]
        keyword: Option<String>,
        #[arg(long, default_value = "20", value_parser = clap::value_parser!(i64).range(1..=100))]
        pagesz: i64,
        /// Community sort order (default recommend); not supported for personal/official.
        #[arg(long, value_parser = ["recommend", "time", "unspecified"])]
        sort_by: Option<String>,
        #[arg(long, default_value = "")]
        page_token: String,
    },
}

#[derive(Subcommand)]
enum QuotaAction {
    /// List credit transaction records
    Records {
        #[arg(long, default_value = "0", value_parser = clap::value_parser!(i64).range(0..=i32::MAX as i64))]
        page: i64,
        #[arg(long, default_value = "20", value_parser = clap::value_parser!(i64).range(1..=100))]
        pagesz: i64,
    },
    /// Query claw-pass daily quota status
    Pass,
    /// Query user credit balance
    Credit,
}

#[derive(Subcommand)]
enum CreationAction {
    /// List personal or favorite creations
    List {
        #[arg(long)]
        favorites_only: bool,
        #[arg(long, default_value = "0", value_parser = clap::value_parser!(i64).range(0..=i32::MAX as i64))]
        page: i64,
        #[arg(long, default_value = "20", value_parser = clap::value_parser!(i64).range(1..=100))]
        pagesz: i64,
    },
    Favorite {
        #[arg(value_parser = clap::value_parser!(i64).range(1..))]
        creation_id: i64,
    },
    Unfavorite {
        #[arg(value_parser = clap::value_parser!(i64).range(1..))]
        creation_id: i64,
    },
}

#[derive(Subcommand)]
enum VoiceAction {
    /// Create a personal cloned voice
    Create {
        #[arg(long)]
        audio: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        use_trial: bool,
        #[arg(long)]
        wait: bool,
        #[arg(long, requires = "wait", value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
    },
    /// List personal cloned voices
    List {
        #[arg(long, default_value = "")]
        page_token: String,
        #[arg(long, default_value = "20", value_parser = clap::value_parser!(i64).range(1..=100))]
        pagesz: i64,
    },
    Get {
        voice_id: String,
    },
    #[command(group(clap::ArgGroup::new("changes").required(true).multiple(true).args(["name", "description"])))]
    Update {
        voice_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },
    Wait {
        voice_id: String,
        #[arg(long, default_value = "1800", value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
    },
}

/// Select the command contract before parsing so help and completion metadata
/// describe exactly the protocol used for execution. No public switch is added.
fn protocol_command(v2: bool) -> clap::Command {
    use clap::CommandFactory;
    let command = Cli::command();
    if v2 {
        command
    } else {
        command.mut_subcommand("element", |_| {
            element_legacy_cli::ElementAction::augment_subcommands(
                clap::Command::new("element").about("Element (主体) operations"),
            )
            .subcommand_required(true)
            .arg_required_else_help(true)
        })
    }
}

fn main() -> ExitCode {
    use clap::FromArgMatches;
    // An absent switch retains the original contract. Invalid configuration must
    // never silently select a different write protocol.
    let v2 = match std::env::var("VIDU_ELEMENT_PLATFORM_ASSETS") {
        Err(std::env::VarError::NotPresent) => false,
        Ok(value) if value == "false" => false,
        Ok(value) if value == "true" => true,
        _ => {
            eprintln!("VIDU_ELEMENT_PLATFORM_ASSETS must be true or false (default false)");
            return ExitCode::from(2);
        }
    };
    // Only the new protocol adds this diagnostic. Legacy debug output must
    // remain identical to main, just like its requests and JSON responses.
    if v2 && std::env::var_os("VIDU_DEBUG").is_some() {
        eprintln!("element protocol: v2");
    }
    let mut command = protocol_command(v2);
    let matches = match command.clone().try_get_matches() {
        Ok(matches) => matches,
        Err(error) => return cli_errors::print_parse_error(&mut command, error),
    };
    if !v2 {
        if let Some(("element", args)) = matches.subcommand() {
            let action = match element_legacy_cli::ElementAction::from_arg_matches(args) {
                Ok(action) => action,
                Err(error) => return cli_errors::print_parse_error(&mut command, error),
            };
            client::set_output_format(
                matches
                    .get_one::<String>("format")
                    .is_some_and(|format| format == "table"),
            );
            element_legacy_cli::run(action);
            return ExitCode::SUCCESS;
        }
    }
    let cli = match cli_errors::parse_or_exit(|| Cli::from_arg_matches(&matches)) {
        Ok(c) => c,
        Err(code) => return code,
    };

    client::set_output_format(cli.format == "table");

    let group = match cli.group {
        Some(g) => g,
        None => {
            protocol_command(v2).print_help().unwrap();
            println!();
            return ExitCode::from(0);
        }
    };

    match group {
        Group::Upload { image_path } => {
            commands::upload::run(&image_path);
        }
        Group::Task { action } => match *action {
            TaskAction::Submit {
                task_type,
                prompt,
                prompt_path,
                images,
                materials,
                audios,
                audio_output,
                videos,
                duration,
                model_version,
                aspect_ratio,
                transition,
                resolution,
                sample_count,
                codec,
                movement_amplitude,
                schedule_mode,
                submission,
            } => {
                let data = commands::tasks::submit(
                    &task_type,
                    prompt.as_deref(),
                    prompt_path.as_deref(),
                    &images,
                    &materials,
                    &audios,
                    &videos,
                    duration,
                    &model_version,
                    aspect_ratio.as_deref(),
                    transition.as_deref(),
                    resolution.as_deref(),
                    sample_count,
                    &codec,
                    &movement_amplitude,
                    schedule_mode.as_deref(),
                    audio_output.with_audio,
                );
                submission.finish(data);
            }
            TaskAction::Get {
                task_id,
                output,
                details,
            } => {
                client::ok(commands::tasks::get_result(
                    &task_id,
                    output.as_deref(),
                    details,
                ));
            }
            TaskAction::List {
                states,
                types,
                scenes,
                page_token,
                pagesz,
            } => client::ok(commands::task_ops::list(
                &states,
                &types,
                &scenes,
                &page_token,
                pagesz,
            )),
            TaskAction::Cancel { task_id } => client::ok(commands::task_ops::cancel(&task_id)),
            TaskAction::Delete { task_id } => client::ok(commands::task_ops::delete(&task_id)),
            TaskAction::Wait {
                task_id,
                timeout,
                poll_interval,
                output,
            } => client::ok(commands::task_ops::wait(
                &task_id,
                timeout,
                poll_interval,
                output.as_deref(),
            )),
            TaskAction::Extend {
                creation_id,
                duration,
                transition,
                submission,
            } => submission.finish(commands::task_ops::extend(
                creation_id,
                duration,
                &transition,
            )),
            TaskAction::Upscale { input, submission } => {
                submission.finish(commands::upscale::submit(
                    input.creation_id,
                    input.video.as_deref(),
                    &input.resolution,
                    input.duration,
                ));
            }
            TaskAction::UpscaleCost { input } => commands::upscale::cost(
                input.creation_id,
                input.video.as_deref(),
                &input.resolution,
                input.duration,
            ),
            TaskAction::UpscaleImage {
                creation_id,
                resolution,
                submission,
            } => submission.finish(commands::task_ops::upscale_image(creation_id, &resolution)),
            TaskAction::Rerun {
                task_id,
                prompt,
                prompt_path,
                model,
                duration,
                resolution,
                audio_output,
                submission,
            } => submission.finish(commands::task_ops::rerun(
                &task_id,
                prompt.as_deref(),
                prompt_path.as_deref(),
                model.as_deref(),
                duration,
                resolution.as_deref(),
                audio_output.with_audio,
            )),
            TaskAction::LipSync {
                video,
                text,
                audio,
                voice_id,
                speed,
                volume,
                enhance,
                codec,
                schedule_mode,
                submission,
            } => {
                let data = commands::tasks::submit_lip_sync(
                    &video,
                    text.as_deref(),
                    audio.as_deref(),
                    &voice_id,
                    speed,
                    volume,
                    enhance,
                    &codec,
                    schedule_mode.as_deref(),
                );
                submission.finish(data);
            }
            TaskAction::LipSyncVoices => {
                commands::tasks::list_voices();
            }
            TaskAction::Tts {
                prompt,
                prompt_path,
                texts,
                voice_id,
                speed,
                volume,
                emotion,
                language_boost,
                subtitle_enable,
                schedule_mode,
                submission,
            } => {
                let data = commands::tasks::submit_tts(
                    prompt.as_deref(),
                    prompt_path.as_deref(),
                    &texts,
                    &emotion,
                    &voice_id,
                    speed,
                    volume,
                    language_boost.as_deref(),
                    subtitle_enable,
                    schedule_mode.as_deref(),
                );
                submission.finish(data);
            }
            TaskAction::TtsVoices => {
                commands::tasks::list_tts_voices();
            }
            TaskAction::Compose {
                timeline,
                width,
                height,
                schedule_mode,
                submission,
            } => {
                let data =
                    commands::tasks::compose(&timeline, width, height, schedule_mode.as_deref());
                submission.finish(data);
            }
            TaskAction::Cost {
                task_type,
                model_version,
                audio_output,
                duration,
                resolution,
                aspect_ratio,
                transition,
                sample_count,
                codec,
                schedule_mode,
            } => {
                commands::tasks::query_credits(
                    &task_type,
                    &model_version,
                    duration,
                    resolution.as_deref(),
                    aspect_ratio.as_deref(),
                    transition.as_deref(),
                    sample_count,
                    &codec,
                    schedule_mode.as_deref(),
                    audio_output.with_audio,
                );
            }
            TaskAction::TtsCost {
                text,
                voice_id,
                speed,
                pitch,
                volume,
                schedule_mode,
            } => {
                commands::tasks::query_tts_credits(
                    &text,
                    &voice_id,
                    speed,
                    pitch,
                    volume,
                    schedule_mode.as_deref(),
                );
            }
            TaskAction::LipSyncCost {
                duration,
                voice_id,
                speed,
                volume,
                codec,
                schedule_mode,
            } => {
                commands::tasks::query_lip_sync_credits(
                    duration,
                    &voice_id,
                    speed,
                    volume,
                    &codec,
                    schedule_mode.as_deref(),
                );
            }
        },
        Group::Element { action } => match action {
            ElementAction::Get { element_id } => client::ok(commands::elements::get(&element_id)),
            ElementAction::Update(args) => client::ok(commands::elements::update(&args)),
            ElementAction::Tags => client::ok(commands::elements::tags()),
            ElementAction::Delete { element_id } => {
                client::ok(commands::elements::delete(&element_id))
            }
            ElementAction::Favorites {
                generation,
                source,
                keyword,
                tag,
                page_token,
                pagesz,
            } => {
                if source == "official" {
                    client::ok(commands::elements::official_elements(
                        keyword.as_deref(),
                        tag.as_deref(),
                        0,
                        &page_token,
                        pagesz,
                        true,
                        generation.edition,
                    ));
                }
                commands::elements::reject_community_edition(generation.edition);
                commands::elements::reject_official_only_filters(
                    keyword.is_some() || tag.is_some(),
                );
                client::ok(commands::elements::favorites(&page_token, pagesz));
            }
            ElementAction::Favorite { element_id, source } => {
                client::ok(commands::elements::favorite_from_source(
                    &element_id,
                    true,
                    &source,
                ));
            }
            ElementAction::Unfavorite { element_id, source } => {
                client::ok(commands::elements::favorite_from_source(
                    &element_id,
                    false,
                    &source,
                ));
            }
            ElementAction::Check { name } => {
                commands::elements::check(&name);
            }
            ElementAction::Preprocess {
                name,
                elem_type,
                images,
            } => {
                commands::elements::preprocess(&name, &elem_type, &images);
            }
            ElementAction::Create(args) => commands::elements::create(&args),
            ElementAction::List {
                generation,
                source,
                tag,
                keyword,
                page,
                pagesz,
                page_token,
            } => {
                if source == "official" {
                    client::ok(commands::elements::official_elements(
                        keyword.as_deref(),
                        tag.as_deref(),
                        page.unwrap_or(0),
                        page_token.as_deref().unwrap_or(""),
                        pagesz,
                        false,
                        generation.edition,
                    ));
                }
                commands::elements::list_elements(
                    keyword.as_deref(),
                    tag.as_deref(),
                    page.unwrap_or(0),
                    pagesz,
                    page_token.as_deref(),
                    generation.edition,
                );
            }
            ElementAction::Search {
                generation,
                source,
                tag,
                category,
                topic,
                keyword,
                pagesz,
                sort_by,
                page_token,
            } => {
                commands::elements::validate_search(
                    keyword.as_deref(),
                    tag.as_deref(),
                    category.as_deref(),
                    topic.as_deref(),
                    sort_by.as_deref(),
                    generation.edition,
                );
                if source != "community" {
                    commands::elements::reject_community_only_filters(
                        category.is_some() || topic.is_some() || sort_by.is_some(),
                    );
                    let data = if source == "personal" {
                        commands::elements::personal_elements(
                            keyword.as_deref(),
                            tag.as_deref(),
                            0,
                            pagesz,
                            Some(&page_token),
                            generation.edition,
                        )
                    } else {
                        commands::elements::official_elements(
                            keyword.as_deref(),
                            tag.as_deref(),
                            0,
                            &page_token,
                            pagesz,
                            false,
                            generation.edition,
                        )
                    };
                    client::ok(data);
                }
                commands::elements::reject_community_edition(generation.edition);
                if tag.is_some() {
                    client::fail("client_error", "--tag requires --source personal or official; community supports --keyword/--category/--topic/--sort-by", None, None, None);
                }
                commands::elements::search(
                    keyword.as_deref().unwrap_or(""),
                    pagesz,
                    sort_by.as_deref().unwrap_or("recommend"),
                    &page_token,
                    category.as_deref(),
                    topic.as_deref(),
                );
            }
        },
        Group::Creation { action } => match action {
            CreationAction::List {
                favorites_only,
                page,
                pagesz,
            } => client::ok(commands::creations::list(favorites_only, page, pagesz)),
            CreationAction::Favorite { creation_id } => {
                client::ok(commands::creations::favorite(creation_id, true))
            }
            CreationAction::Unfavorite { creation_id } => {
                client::ok(commands::creations::favorite(creation_id, false))
            }
        },
        Group::Voice { action } => match action {
            VoiceAction::Create {
                audio,
                name,
                use_trial,
                wait,
                timeout,
            } => client::ok(commands::voices::create(
                &audio,
                name.as_deref(),
                use_trial,
                wait,
                timeout.unwrap_or(1800),
            )),
            VoiceAction::List { page_token, pagesz } => {
                client::ok(commands::voices::list(&page_token, pagesz))
            }
            VoiceAction::Get { voice_id } => client::ok(commands::voices::get(&voice_id)),
            VoiceAction::Update {
                voice_id,
                name,
                description,
            } => client::ok(commands::voices::update(
                &voice_id,
                name.as_deref(),
                description.as_deref(),
            )),
            VoiceAction::Wait { voice_id, timeout } => {
                client::ok(commands::voices::wait(&voice_id, timeout))
            }
        },
        Group::Quota { action } => match action {
            QuotaAction::Records { page, pagesz } => {
                client::ok(commands::quota::records(page, pagesz))
            }
            QuotaAction::Pass => commands::quota::claw_pass_status(),
            QuotaAction::Credit => commands::quota::credit_status(),
        },
    }
    ExitCode::from(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse_tts(args: &[&str]) -> TaskAction {
        let cli = Cli::try_parse_from(args).expect("parse cli");
        match cli.group.expect("group") {
            Group::Task { action } => *action,
            _ => unreachable!(),
        }
    }

    #[test]
    fn tts_subtitle_enable_defaults_true() {
        let action = parse_tts(&[
            "vidu-cli",
            "task",
            "tts",
            "--prompt",
            "hello",
            "--voice-id",
            "English_expressive_narrator",
        ]);

        match action {
            TaskAction::Tts {
                subtitle_enable, ..
            } => assert!(subtitle_enable),
            _ => unreachable!(),
        }
    }

    #[test]
    fn tts_subtitle_enable_flag_sets_true() {
        let action = parse_tts(&[
            "vidu-cli",
            "task",
            "tts",
            "--prompt",
            "hello",
            "--voice-id",
            "English_expressive_narrator",
            "--subtitle-enable",
        ]);

        match action {
            TaskAction::Tts {
                subtitle_enable, ..
            } => assert!(subtitle_enable),
            _ => unreachable!(),
        }
    }

    #[test]
    fn tts_subtitle_enable_explicit_true_sets_true() {
        let action = parse_tts(&[
            "vidu-cli",
            "task",
            "tts",
            "--prompt",
            "hello",
            "--voice-id",
            "English_expressive_narrator",
            "--subtitle-enable",
            "true",
        ]);

        match action {
            TaskAction::Tts {
                subtitle_enable, ..
            } => assert!(subtitle_enable),
            _ => unreachable!(),
        }
    }

    #[test]
    fn tts_subtitle_enable_explicit_false_sets_false() {
        let action = parse_tts(&[
            "vidu-cli",
            "task",
            "tts",
            "--prompt",
            "hello",
            "--voice-id",
            "English_expressive_narrator",
            "--subtitle-enable",
            "false",
        ]);

        match action {
            TaskAction::Tts {
                subtitle_enable, ..
            } => assert!(!subtitle_enable),
            _ => unreachable!(),
        }
    }

    #[test]
    fn tts_subtitle_enabled_alias_explicit_false_sets_false() {
        let action = parse_tts(&[
            "vidu-cli",
            "task",
            "tts",
            "--prompt",
            "hello",
            "--voice-id",
            "English_expressive_narrator",
            "--subtitle-enabled",
            "false",
        ]);

        match action {
            TaskAction::Tts {
                subtitle_enable, ..
            } => assert!(!subtitle_enable),
            _ => unreachable!(),
        }
    }

    #[test]
    fn clap_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn generation_ratio_parser_and_shared_video_options_are_available() {
        for command in ["submit", "cost"] {
            for (ratio, expected) in [("9:21", "9:21"), ("7:5", "7:5"), ("auto", "")] {
                let cli = Cli::try_parse_from([
                    "vidu-cli",
                    "task",
                    command,
                    "--type",
                    "text2image",
                    "--model-version",
                    "3.1",
                    "--aspect-ratio",
                    ratio,
                    "--codec",
                    "h265_pro",
                    "--transition",
                    "pro_ad",
                ])
                .expect("clap parses syntax; shared settings validate model compatibility");
                let Some(Group::Task { action }) = cli.group else {
                    unreachable!()
                };
                match *action {
                    TaskAction::Submit {
                        aspect_ratio,
                        codec,
                        transition,
                        ..
                    }
                    | TaskAction::Cost {
                        aspect_ratio,
                        codec,
                        transition,
                        ..
                    } => {
                        assert_eq!(aspect_ratio.as_deref(), Some(expected));
                        assert_eq!(codec, "h265_pro");
                        assert_eq!(transition.as_deref(), Some("pro_ad"));
                    }
                    _ => unreachable!(),
                }
            }
            for ratio in ["0:1", "NaN:1", "invalid"] {
                assert!(Cli::try_parse_from([
                    "vidu-cli",
                    "task",
                    command,
                    "--type",
                    "text2image",
                    "--model-version",
                    "3.1",
                    "--aspect-ratio",
                    ratio,
                ])
                .is_err());
            }
        }
    }

    #[test]
    fn new_root_commands_and_global_format_parse() {
        for args in [
            vec!["vidu-cli", "--format", "table", "creation", "list"],
            vec![
                "vidu-cli", "creation", "favorite", "123", "--format", "table",
            ],
            vec!["vidu-cli", "voice", "get", "voice_123", "--format", "table"],
            vec!["vidu-cli", "task", "list", "--format", "table"],
        ] {
            assert_eq!(Cli::try_parse_from(args).unwrap().format, "table");
        }
        assert_eq!(
            Cli::try_parse_from(["vidu-cli", "quota", "records"])
                .unwrap()
                .format,
            "json"
        );
    }

    #[test]
    fn element_cursor_conflicts_only_with_explicit_page() {
        assert!(
            Cli::try_parse_from(["vidu-cli", "element", "list", "--page-token", "next"]).is_ok()
        );
        assert!(Cli::try_parse_from([
            "vidu-cli",
            "element",
            "list",
            "--page",
            "0",
            "--page-token",
            "next"
        ])
        .is_err());
    }

    #[test]
    fn submission_wait_requirements_and_aliases_parse() {
        for command in ["rerun", "tts"] {
            let base = if command == "rerun" {
                vec![
                    "vidu-cli",
                    "task",
                    "rerun",
                    "task_1",
                    "--model",
                    "3.1",
                    "--prompt-file",
                    "prompt.txt",
                ]
            } else {
                vec![
                    "vidu-cli",
                    "task",
                    "tts",
                    "--voice",
                    "voice_1",
                    "--prompt-file",
                    "prompt.txt",
                ]
            };
            assert!(Cli::try_parse_from(base.clone()).is_ok());
            for flag in ["--output", "--timeout"] {
                let value = if flag == "--output" { "renders" } else { "10" };
                let mut args = base.clone();
                args.extend([flag, value]);
                assert!(Cli::try_parse_from(args.clone()).is_err());
                args.push("--wait");
                assert!(Cli::try_parse_from(args).is_ok());
            }
        }
        assert!(
            Cli::try_parse_from(["vidu-cli", "voice", "create", "--audio", "voice.wav"]).is_ok()
        );
        assert!(Cli::try_parse_from([
            "vidu-cli",
            "voice",
            "create",
            "--audio",
            "voice.wav",
            "--timeout",
            "10"
        ])
        .is_err());
    }

    #[test]
    fn metadata_updates_require_changes_and_label_clear_conflicts() {
        for args in [
            vec!["vidu-cli", "element", "update", "element_1"],
            vec!["vidu-cli", "voice", "update", "voice_1"],
            vec![
                "vidu-cli",
                "element",
                "update",
                "element_1",
                "--label",
                "1",
                "--clear-labels",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
        assert!(Cli::try_parse_from([
            "vidu-cli",
            "element",
            "update",
            "element_1",
            "--recaption",
            "new caption",
            "--label",
            "1",
            "--label",
            "2"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "vidu-cli",
            "voice",
            "update",
            "voice_1",
            "--description",
            "new description"
        ])
        .is_ok());
    }

    #[test]
    fn numeric_ranges_are_rejected_during_parsing() {
        for args in [
            vec!["vidu-cli", "creation", "favorite", "0"],
            vec!["vidu-cli", "creation", "favorite", "-1"],
            vec![
                "vidu-cli",
                "task",
                "extend",
                "--creation-id",
                "1",
                "--duration",
                "8",
            ],
            vec!["vidu-cli", "task", "wait", "task_1", "--timeout", "0"],
            vec!["vidu-cli", "task", "wait", "task_1", "--poll-interval", "0"],
            vec!["vidu-cli", "quota", "records", "--page", "-1"],
            vec!["vidu-cli", "task", "list", "--pagesz", "0"],
            vec!["vidu-cli", "voice", "list", "--pagesz", "101"],
            vec!["vidu-cli", "element", "list", "--pagesz", "9"],
            vec!["vidu-cli", "voice", "wait", "voice_1", "--timeout", "0"],
        ] {
            assert!(
                Cli::try_parse_from(args.clone()).is_err(),
                "unexpectedly accepted: {args:?}"
            );
        }
    }
}
