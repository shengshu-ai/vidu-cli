# vidu-cli

[中文文档](./README.zh.md)

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
vidu-cli <COMMAND>

Commands:
  upload   Upload image → get ssupload URI
  task     Task operations
  element  Element (reference material) operations
```

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
vidu-cli task submit --type <TYPE> --prompt <PROMPT> --duration <DURATION> \
  --model-version <VERSION> --resolution <RESOLUTION> [OPTIONS]
```

**Common options:**

| Option | Default | Description |
|--------|---------|-------------|
| `--schedule-mode` | auto | `claw_pass` (daily quota) or `normal` (credits). Auto-detected if omitted |
| `--transition` | - | `creative`, `stable`, `pro`, or `speed` (model/type dependent) |
| `--sample-count` | 1 | Number of samples to generate |
| `--codec` | h265 | Output video codec |
| `--movement-amplitude` | auto | Video motion intensity |
| `--aspect-ratio` | - | `16:9`, `9:16`, `1:1`, `4:3`, `3:4` (not for img2video/headtailimg2video) |

**Task types and constraints:**

| Type | Models | Duration | Resolution | Aspect Ratio |
|------|--------|----------|------------|--------------|
| `text2video` | 3.0, 3.1, 3.2 | 3.0→5s, 3.1→2-8s, 3.2→1-16s | 1080p | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `img2video` | 3.0, 3.1, 3.2 | 3.0→5s, 3.1→2-8s, 3.2→1-16s | 1080p | — |
| `headtailimg2video` | 3.0, 3.1, 3.2 | 3.0→5s, 3.1→2-8s, 3.2→1-16s | 1080p | — |
| `character2video` | 3.0, 3.1, 3.1_pro, 3.2 | 3.0→5s, 3.1→2-8s, 3.1_pro→-1/2-8s, 3.2→1-16s | 1080p | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `text2image` | 3.1, 3.2_fast_m, 3.2_pro_m | 0 (image) | 1080p, 2k, 4k | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `reference2image` | 3.1, 3.2_fast_m, 3.2_pro_m | 0 (image) | 1080p, 2k, 4k | 16:9, 9:16, 1:1, 4:3, 3:4 |

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

# Image to video (local file or URL)
vidu-cli task submit \
  --type img2video \
  --prompt "the scene comes alive" \
  --model-version 3.1 \
  --duration 4 \
  --resolution 1080p \
  --image ./scene.jpg

# First-last frame video
vidu-cli task submit \
  --type headtailimg2video \
  --prompt "smooth transition" \
  --model-version 3.2 \
  --duration 5 \
  --resolution 1080p \
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
  --material "mychar:element-id:1"
```

#### Get task result

```bash
vidu-cli task get <TASK_ID> [--output <DIR>]
```

Returns: `task_id`, `state`, `type`, `model`, `err_code`, `err_msg`.

Use `--output` / `-o` to download media files to a local directory when the task is complete:

```bash
vidu-cli task get <TASK_ID> --output ./results
```

If the task state is not `success`, the download is skipped and the response includes `download_skipped: true`.

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
| `--schedule-mode` | No | auto | claw_pass, normal | Billing mode (auto-detected if omitted) |

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

## License

MIT
