# vidu-cli

A command-line client for the [Vidu](https://www.vidu.io) video generation API.

## Installation

```bash
cargo install vidu-cli
```

## Configuration

Set the following environment variables before use:

```bash
export VIDU_TOKEN=your_api_token
export VIDU_BASE_URL=https://api.vidu.io  # optional, defaults to official endpoint
```

## Usage

```
vidu-cli <COMMAND>

Commands:
  upload   Upload image → get ssupload URI
  task     Task operations
  element  Element (主体) operations
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

**Task types and constraints:**

| Type | Models | Duration | Resolution | Aspect Ratio |
|------|--------|----------|------------|--------------|
| `text2video` | 3.0, 3.1, 3.2 | 3.0→5s, 3.1→2-8s, 3.2→1-16s | 1080p | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `img2video` | 3.0, 3.1, 3.2 | 3.0→5s, 3.1→2-8s, 3.2→1-16s | 1080p | — |
| `headtailimg2video` | 3.0, 3.1, 3.2 | 3.0→5s, 3.1→2-8s, 3.2→1-16s | 1080p | — |
| `character2video` | 3.0, 3.1, 3.1_pro, 3.2 | 3.0→5s, 3.1→2-8s, 3.2→1-16s | 1080p | 16:9, 9:16, 1:1, 4:3, 3:4 |
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
vidu-cli task get <TASK_ID>
```

#### Stream task state (SSE)

```bash
vidu-cli task sse <TASK_ID>
```

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

---

### Element (主体)

Manage reusable character/material elements:

```bash
# Check if element name is available
vidu-cli element check --name "mychar"

# Pre-process element image
vidu-cli element preprocess --image ./char.jpg

# Create element
vidu-cli element create --name "mychar" --image ./char.jpg

# List your elements
vidu-cli element list

# Search community elements
vidu-cli element search --query "anime girl"
```

## License

MIT
