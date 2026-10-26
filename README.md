# vidu-cli

[中文文档](README.zh.md)

A command-line client for the [Vidu](https://www.vidu.io) video generation API.

## Agent Skill

vidu-cli has a companion Agent Skill that lets AI agents (Claude Code, Cursor, Copilot, OpenClaw, and more) call vidu-cli automatically:

```bash
# Install via npx skills (40+ agents supported)
npx skills add shengshu-ai/vidu-skills

# Install via ClawHub (OpenClaw ecosystem)
clawhub install github:shengshu-ai/vidu-skills
```

Once installed, the agent can generate videos and images from natural language — no manual CLI flags needed. See [vidu-skills](https://github.com/shengshu-ai/vidu-skills) for details.

## Installation

```bash
# Via npm (recommended)
npm install -g vidu-cli@latest

# Via cargo
cargo install vidu-cli
```

## Configuration

Set environment variables:

```bash
export VIDU_TOKEN=your_api_token
export VIDU_BASE_URL=https://service.vidu.cn  # China: service.vidu.cn, Global: service.vidu.com
```

## Usage

```
vidu-cli [--format json|table] <COMMAND>

Commands:
  upload   Upload image → get ssupload URI
  task     Task operations
  element  Element (reference material) operations
  creation Creation lists and favorites
  voice    Personal cloned voices
  quota    Quota, balance, and credit records
```

Output defaults to the existing JSON `ok` envelope. Use the global `--format table` for terminal summaries; scripts can keep the default JSON.

Replace `TASK_ID`, `VOICE_ID`, and pagination tokens below with values returned by the API. Numeric element ID `42`, creation ID `123`, and label IDs `1` and `2` are also examples; use real IDs you can access. Task IDs and creation IDs are separate resources.

---

### Upload

Upload a local image and get a reusable `ssupload:?id=xxx` URI:

```bash
vidu-cli upload ./photo.jpg
```

---

### Task

#### Submit a task

```bash
vidu-cli task submit --type <TYPE> --prompt <PROMPT> \
  --model-version <VERSION> [--duration <DURATION>] [--resolution <RESOLUTION>] [OPTIONS]
```

**Common options:**

| Option | Default | Description |
|--------|---------|-------------|
| `--duration` | Drama/Wan 3: 5; legacy video models: required | Video duration in seconds. Drama accepts 4-30; Wan 3 accepts 2-30. Image tasks ignore it |
| `--resolution` | Drama/Wan 3: 720p; legacy submit models: required | Supported output size. Cost queries keep the legacy 1080p default |
| `--aspect-ratio` | Drama/Wan 3: 16:9 | `16:9`, `9:16`, `1:1`, `4:3`, `3:4`; Wan 3 also supports `21:9` and `adaptive`, including img2video/headtailimg2video |
| `--schedule-mode` | auto | `nopeak` uses off-peak quota subject to server eligibility, remaining quota, and task/model rules; `claw_pass` uses pass quota; `normal` uses credits. Q4 defaults to `normal`, rejects `claw_pass`, and accepts `nopeak` subject to server rules. Drama supports only `normal` credits. Other models auto-detect an active claw pass, otherwise use `normal` |
| `--audio` | - | Reference audio for `character2video` with `3.2_a`, `3.4` (Q4), or Q3.1 Drama. `3.2_a`/Q4 allow up to 3 (2–15s each, 15s total for `3.2_a`; 1–16s each for Q4). Drama allows up to 10 and 30s total. Local Drama audio: 4–30s each without `--video`, 2–30s each with `--video`; the server determines edit/reference task type and enforces the 4s minimum for non-edit tasks. Local WAV/MP3 files are limited to 15MB; `ssupload` durations are validated by the server |
| `--transition` | - | For `img2video` / `headtailimg2video`: `3.0` uses `creative` / `stable`, `3.1` / `3.2` require `pro` / `speed`; `character2video 3.2` also requires `pro` / `speed`; `text2video` supports it only on `3.2` |
| `--sample-count` | 1 | Number of samples to generate |
| `--codec` | h265 | Output video codec |
| `--movement-amplitude` | auto | Video motion intensity |

**Task types and constraints:**

| Type | Models | Duration | Resolution | Aspect Ratio |
|------|--------|----------|------------|--------------|
| `text2video` | 3.0, 3.1, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→4-15s | 1080p | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `text2video` (Q3.1 Drama) | `viduq3.1-drama-std` | 4-30s (default 5) | 480p, 720p (default), 1080p | 16:9 (default), 9:16, 1:1, 4:3, 3:4 |
| `img2video` | 3.0, 3.1, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→4-15s | 1080p | — |
| `headtailimg2video` | 3.0, 3.1, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→4-15s | 1080p | — |
| `character2video` | 3.0, 3.1, 3.1_pro, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.1_pro→-1/2-8s, 3.2→1-16s, 3.2_a→4-15s | 1080p | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `character2video` (Q4) | 3.4 | 1-16s | 540p, 720p, 1080p, 2k, 4k | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `character2video` (Q3.1 Drama) | `viduq3.1-drama-std` | 4-30s (default 5) | 480p, 720p (default), 1080p | 16:9 (default), 9:16, 1:1, 4:3, 3:4 |
| `text2video`, `img2video`, `headtailimg2video`, `character2video` (Wan 3) | `viduq2-drama-std` | 2-30s (default 5) | 480p, 720p (default), 1080p | 16:9 (default), 9:16, 1:1, 4:3, 3:4, 21:9, adaptive |
| `text2image` | 3.1, 3.2_fast_m, 3.2_pro_m, 3.2_image_2, 3.2_image_2_5_fast, 3.2_image_2_5_pro | 0 (image) | 1080p, 2k, 4k | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `reference2image` | 3.1, 3.2_fast_m, 3.2_pro_m, 3.2_image_2, 3.2_image_2_5_fast, 3.2_image_2_5_pro | 0 (image) | 1080p, 2k, 4k | 16:9, 9:16, 1:1, 4:3, 3:4 |

Q4 reference audio: each local file must be 1-16 seconds. Up to 3 files are
allowed without the `3.2_a` combined 15-second limit. Q4 accepts audio-only
references with a text prompt; image/material references are optional when
audio is present. `3.2_a` keeps its per-file 2-15 second and combined
15-second limits. Local WAV/MP3 files remain limited to 15 MB each.
Audio validation runs before image uploads. Existing `ssupload` references
are checked by the server.

Q3.1 Drama `text2video` accepts text prompts only. Use `character2video` for
references; it requires at least one image, video, or audio, accepts up to 9
images, 3 videos, and 10 audio references, and does not support materials,
automatic duration, or transitions. The CLI does not expose an explicit
edit/extend task-type selector; the server determines the task type from the
references and prompt. Audio references
allow up to 30 seconds total: local clips must be 4–30 seconds each without
`--video`, or 2–30 seconds each with `--video`. With a video reference, the
server determines the task type and enforces a 4-second minimum for non-edit
tasks. Local WAV/MP3 files are limited to 15MB each; `ssupload` durations are
validated by the server. It accepts only `--schedule-mode normal`
(credits). Local MP4/MOV references must each be
4-30 seconds, average 24-60 fps, and total at most 30 seconds. The CLI checks
the average frame rate of each local video before submission. Existing `ssupload`
videos pass through for full server-side validation. Submit and cost use the same Drama
defaults; cost values are returned by the API.

Wan 3 defaults to 5 seconds, 720p, 16:9 and `normal` credits.
It accepts 1-4 samples and rejects
automatic duration (`-1`), transitions, materials, off-peak and pass modes.
`text2video` accepts text only; `img2video` requires one image;
`headtailimg2video` requires two images, in first/last order. Frame modes do not
accept audio/video references. `character2video` requires at least one reference
and allows up to 10 images, 5 videos and 5 audio files (20 media total), including
audio-only references. Local images must be JPEG/PNG/BMP/WEBP, ≤20MB, sides
240-8000 px and ratio 1:8-8:1. Local MP4/MOV videos must be ≤100MB, sides
240-4096 px, ratio 1:8-8:1 and average ≥16 fps. Each video/audio must be 1-15s,
with at most 15s total per kind; input video plus output duration must be ≤30s.
Local audio is WAV/MP3, ≤15MB. Existing `ssupload` references are checked by the
server, including the complete duration set when local and uploaded inputs mix.
Quotes are returned by the backend.

```bash
# Wan 3 text generation: 2 seconds, 480p
vidu-cli task submit --type text2video --prompt "a cat walking in the rain" \
  --model-version viduq2-drama-std --duration 2 --resolution 480p --wait

# Wan 3 multimodal references (local files are uploaded automatically)
vidu-cli task submit --type character2video --prompt "use the character and soundtrack" \
  --model-version viduq2-drama-std --image ./character.jpg \
  --video ./reference.mp4 --audio ./soundtrack.wav

# Quote uses identical model defaults; no generation or upload
vidu-cli task cost --type text2video --model-version viduq2-drama-std
```

**Examples:**

```bash
# Text to video
vidu-cli task submit \
  --type text2video \
  --prompt "a cat walking in the rain" \
  --model-version 3.2 \
  --duration 8 \
  --resolution 1080p \
  --aspect-ratio 16:9

# Q3.1 Drama (duration, resolution, and aspect ratio use model defaults)
vidu-cli task submit --type text2video --prompt "A quiet street at dawn" \
  --model-version viduq3.1-drama-std

# Image to video (local file or URL)
vidu-cli task submit \
  --type img2video \
  --prompt "the scene comes alive" \
  --model-version 3.1 \
  --duration 4 \
  --resolution 1080p \
  --transition pro \
  --image ./scene.jpg

# First-last frame video
vidu-cli task submit \
  --type headtailimg2video \
  --prompt "smooth transition" \
  --model-version 3.2 \
  --duration 5 \
  --resolution 1080p \
  --transition pro \
  --image ./start.jpg \
  --image ./end.jpg

# Character to video (with element material)
vidu-cli task submit \
  --type character2video \
  --prompt "character walks forward" \
  --model-version 3.2 \
  --duration 4 \
  --resolution 1080p \
  --transition pro \
  --image ./character.jpg \
  --material "mychar:42:1"
```

#### Get task result

```bash
vidu-cli task get <TASK_ID> [--details] [--output <DIR>]
```

Returns `task_id`, `state`, `type`, and `model`; failed tasks also include `err_code` and `err_msg`.

Use `--output` / `-o` to download media files to a local directory when the task is complete:

```bash
vidu-cli task get "$TASK_ID" --output ./results
```

If the task state is not `success`, the download is skipped and the response includes `download_skipped: "task not ready"`.

#### History, details, and waiting

```bash
vidu-cli task list --state success --pagesz 20
vidu-cli task get "$TASK_ID" --details
vidu-cli task wait "$TASK_ID" --timeout 1800 --poll-interval 5 --output ./results
vidu-cli --format table task list
```

Repeat `--state`, `--type`, or `--scene` to filter history; continue cursor pagination with `--page-token`. `get --details` adds raw `input`, `settings`, and `creations` containing actual `creation_id` values.

`submit`, `tts`, `lip-sync`, `compose`, `extend`, `upscale`, `upscale-image`, and `rerun` accept `--wait [--timeout 1800] [--output DIR]`. On submission commands, `--timeout` and `--output` require `--wait`. For example:

```bash
vidu-cli task submit --type text2video --prompt "A quiet street at dawn" \
  --model-version viduq3.1-drama-std --wait --output ./results
```

Progress goes to stderr and the final result goes to stdout. A timeout or interruption stops local waiting without canceling the server task; failure or timeout exits nonzero. `task get` remains a single query.

#### Manage tasks and reuse creations

```bash
vidu-cli task cancel "$TASK_ID"
vidu-cli task delete "$TASK_ID"
vidu-cli task rerun "$TASK_ID" --prompt "Change the scene to a rainy day" --wait
vidu-cli task extend --creation-id 123 --duration 4 --transition extendspeed --wait
vidu-cli task upscale-image --creation-id 123 --resolution 2k --wait
```

Cancellation is allowed before `processing`, except for `clip_compose`. Deletion soft-deletes history and rejects `queueing` or `processing` tasks. The server checks permissions and state.

`rerun` creates a new paid task from one of the six existing image/video submit types. Override the prompt (including `--prompt-path` / `--prompt-file`), model (`--model-version` / `--model`), duration, or resolution; historical references stay in place and a seed does not guarantee identical output. Extension requires an accessible completed 1080p video creation, uses model 3.1 and 1080p, and adds 1–7 seconds. The transition defaults to `extendspeed`, with `extendpro` also available; subscription and total-duration rules apply. Image upscaling takes an existing image creation and defaults to 2k; 720p, 1080p, 2k, and 4k are supported.

When `--model-version` actually changes, rerun preserves a transition accepted by the target type/model. Otherwise it selects `stable` for 3.0 image/first-last-frame video, selects `speed` where 3.1/3.2 requires `pro` or `speed`, or removes unsupported transitions. Incompatible scheduling falls back to the target model default (Q4/Drama: `normal`); the inherited vendor model is cleared for server selection. Historical style is preserved, with `general` used when switching to 3.0 without a style. Duration and resolution stay unchanged unless overridden, so adjust them when the target model requires different values.

#### Video upscaling and credit estimates

`task upscale` accepts an existing video creation or one video source. Exactly one of `--creation-id` and `--video` is required. `--resolution` is required and accepts `720p`, `1080p`, `2k`, or `4k`; source creations do not have to be 1080p.

```bash
# Existing creation: read duration automatically, wait, and download
vidu-cli task upscale --creation-id 123 --resolution 2k --wait --output ./results

# Local video: read duration and upload; returns task_id by default
vidu-cli task upscale --video ./input.mp4 --resolution 4k

# Remote source: specify the full source duration in positive integer seconds
vidu-cli task upscale --video 'https://example.com/input.mp4' --duration 8 --resolution 2k
vidu-cli task upscale --video 'ssupload:?id=123' --duration 8 --resolution 1080p

# Estimate with the same source, resolution, and duration rules
vidu-cli task upscale-cost --creation-id 123 --resolution 2k
vidu-cli task upscale-cost --video ./input.mp4 --resolution 4k
vidu-cli task upscale-cost --video 'ssupload:?id=123' --duration 8 --resolution 2k
```

Local files must be nonempty MP4/MOV/AVI videos, at most 500 MiB. Duration is read using the built-in MP4 parser, falling back to local `ffprobe`; an unreadable duration or missing video track is an error. Automatic durations round to integer seconds and must be in `1..2147483647`. `--duration` cannot override a creation or local file's duration. URL/ssupload inputs are not probed remotely: callers must supply the full source duration in that integer range. This value does not trim the video.

`upscale-cost` only requests a quote: it does not upload local files or create tasks, and does not accept `--wait`, `--timeout`, or `--output`. It returns the existing `cost_credits`, `current_credits`, `can_submit`, and `original_cost_credits` fields. Prices come from the server; `can_submit` reflects the balance at quote time and does not guarantee that source validation will pass.

Upscaling uses normal credit scheduling, with the provider selected by the server. Failed submissions are not automatically resubmitted. `--wait` defaults to 1800 seconds; a timeout leaves the task running and preserves its ID. Continue with `vidu-cli task wait "$TASK_ID" --timeout 1800 --output ./results`. Image upscaling remains available through the existing `upscale-image` command.

#### Lip sync

Drive mouth movement on an existing video using text or audio:

```bash
# Text mode
vidu-cli task lip-sync --video ./clip.mp4 --text "Hello, world!" --voice-id English_Aussie_Bloke

# Audio mode
vidu-cli task lip-sync --video ./clip.mp4 --audio ./speech.mp3
```

Supported video: MP4/MOV/AVI ≤500MB. Supported audio: MP3/WAV/AAC/M4A ≤100MB.

#### List available lip-sync voices

```bash
vidu-cli task lip-sync-voices
```

#### TTS (Text-to-Speech)

Convert text to speech audio:

```bash
# Basic usage (single segment)
vidu-cli task tts --prompt "Hello, this is a test." --voice-id English_Trustworth_Man

# Multi-segment with per-segment emotion
vidu-cli task tts \
  --text "Hello everyone!" \
  --text "This is exciting news." \
  --voice-id English_Trustworth_Man \
  --emotion "cheerful" \
  --emotion "excited" \
  --speed 1.2 \
  --volume 80 \
  --language-boost "English"
```

`--prompt` (single segment) and `--text` (repeatable, multi-segment) are mutually exclusive. When using `--text`, each `--emotion` flag pairs with the corresponding `--text` segment by order.

**Parameters:**

| Parameter | Required | Default | Range | Description |
|-----------|----------|---------|-------|-------------|
| `--prompt` | Yes* | - | 1-2000 chars | Single-segment text (*mutually exclusive with `--text`) |
| `--text` | Yes* | - | repeatable | Multi-segment text (*mutually exclusive with `--prompt`) |
| `--voice-id` | Yes | - | See voice list | Voice ID for speech synthesis |
| `--speed` | No | 1.0 | 0.5-2.0 | Speech speed multiplier |
| `--volume` | No | 80 | 0-100 | Audio volume level |
| `--emotion` | No | - | repeatable | Emotion per segment (paired by order with `--text`) |
| `--language-boost` | No | - | Chinese, English, auto, etc. | Enhance recognition of specific languages/dialects |
| `--schedule-mode` | No | auto | claw_pass, nopeak, normal | Billing/queue mode. `nopeak` uses off-peak quota subject to server rules; `claw_pass` uses pass quota; `normal` uses credits. If omitted, an active claw pass is auto-detected; otherwise normal is used |

#### List available TTS voices

```bash
vidu-cli task tts-voices
```

---

### Cost Estimation

Query credit cost before submitting a task:

```bash
# Video/image tasks
vidu-cli task cost --type text2video --model-version 3.2 --duration 8

# Q3.1 Drama defaults to duration 5, resolution 720p, and aspect ratio 16:9
vidu-cli task cost --type text2video --model-version viduq3.1-drama-std

# TTS tasks (priced by character count)
vidu-cli task tts-cost --text "Hello, this is a test." --voice-id English_Trustworth_Man --pitch 0

# Lip-sync tasks
vidu-cli task lip-sync-cost --duration 5 --voice-id English_Aussie_Bloke
```

---

### Quota

```bash
# Query claw-pass daily quota status
vidu-cli quota pass

# Query user credit balance
vidu-cli quota credit

# Credit transaction records (zero-based numeric pages)
vidu-cli quota records --page 0 --pagesz 20
```

---

### Element

Manage reusable character/material elements:

```bash
# Check if element name is available
vidu-cli element check --name "mychar"

# Pre-process element image
vidu-cli element preprocess --name "mychar" --image ./char.jpg --type user

# Create element (1-3 images supported; runs check → upload → preprocess → create internally)
vidu-cli element create --name "mychar" --image ./char.jpg \
  --description "My character" --style "anime"

# List your elements
vidu-cli element list --keyword "char" --page 0 --pagesz 20

# Search community elements
vidu-cli element search --keyword "anime girl" --sort-by recommend --pagesz 20
```

Personal element lists support `--keyword`. Use the returned `next_page_token` with `--page-token`, which conflicts with an explicit `--page`. Numeric page compatibility walks cursors sequentially, so deeper pages require more requests. Personal list `--pagesz` accepts 10–100 and defaults to 20.

```bash
vidu-cli element list --keyword "char" --page-token "$NEXT_PAGE_TOKEN" --pagesz 20
vidu-cli element get 42
vidu-cli element update 42 --name "New character" --label 1 --label 2 --recaption "A new character description"
vidu-cli element update 42 --clear-labels
vidu-cli element delete 42
vidu-cli element search --keyword "summer" --category "character" --topic "summer"
vidu-cli element favorite 42
vidu-cli element favorites --pagesz 20
vidu-cli element unfavorite 42
```

`--label` takes numeric label IDs and replaces the label set; it conflicts with `--clear-labels`. `--recaption` changes the description while preserving its style. Updates require at least one change; the server checks sharing, version, and deletion rules. Favorites take an element ID and resolve the share ID internally. `element favorites` is a separate cursor list accepting pagination only, without search filters.

### Creations

```bash
vidu-cli creation list --page 0 --pagesz 20
vidu-cli creation list --favorites-only --page 0 --pagesz 20
vidu-cli creation favorite 123
vidu-cli creation unfavorite 123
```

Lists use numeric pages (offset pagination) and return `total`. Favorites include creations you can access, including other creators' work. Use creation IDs returned by `task get --details` or creation lists.

### Personal cloned voices

```bash
vidu-cli voice create --audio ./voice-sample.wav --name "My narrator" --wait --timeout 1800
vidu-cli voice list --pagesz 20
vidu-cli voice get "$VOICE_ID"
vidu-cli voice update "$VOICE_ID" --name "New narrator" --description "Story narration"
vidu-cli voice wait "$VOICE_ID" --timeout 1800
vidu-cli task tts --prompt "Hello" --voice-id "$VOICE_ID" --wait
```

Set `VOICE_ID` to the personal `voice_...` ID returned by create/list. Supply a local recording of 10–300 seconds, at most 20 MiB. Upload completion performs synchronous moderation; only approved audio is submitted for cloning using its `ssupload` URI. Cloning is paid by default. `--use-trial` explicitly requests a trial and never automatically falls back to a paid submission.

Without `--wait`, create returns the ID after upload moderation and clone submission. With it, create also waits for cloning. `--timeout` requires `--wait`, applies only to clone waiting, and defaults to 1800 seconds. `voice wait` uses SSE and has no polling interval; timeout or interruption leaves server cloning running. Lists use `--page-token` cursors. Create does not accept a description; change it afterward with update. There is no voice delete command. Existing `task tts-voices` and `task lip-sync-voices` continue to list official static voices.

## License

MIT
