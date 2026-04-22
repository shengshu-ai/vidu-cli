# vidu-cli

[English](./README.md)

[Vidu](https://www.vidu.cn) 视频生成 API 的命令行客户端。

## Agent Skill

vidu-cli 配套了 Agent Skill，让 AI 助手（Claude Code、Cursor、Copilot 等编程工具及 OpenClaw 类 Agent）可以自动调用 vidu-cli：

```bash
# 通过 npx skills（推荐，支持 40+ agents）
npx skills add shengshu-ai/vidu-skills

# 通过 ClawHub（OpenClaw 生态）
clawhub install github:shengshu-ai/vidu-skills
```

安装后，AI 助手可以直接通过自然语言生成视频和图像，无需手动拼 CLI 参数。详见 [vidu-skills](https://github.com/shengshu-ai/vidu-skills)。

## 安装

```bash
# 通过 npm（推荐）
npm install -g vidu-cli@latest

# 通过 cargo
cargo install vidu-cli
```

## 配置

设置环境变量：

```bash
export VIDU_TOKEN=your_api_token
export VIDU_BASE_URL=https://service.vidu.cn  # 中国大陆: service.vidu.cn，海外: service.vidu.com
```

## 用法

```
vidu-cli <COMMAND>

Commands:
  upload   上传图片 → 获取 ssupload URI
  task     任务操作
  element  素材（参考元素）操作
```

---

### 上传

上传本地图片，获取可复用的 `ssupload:?id=xxx` URI：

```bash
vidu-cli upload ./photo.jpg
```

---

### 任务

#### 提交任务

```bash
vidu-cli task submit --type <TYPE> --prompt <PROMPT> --duration <DURATION> \
  --model-version <VERSION> --resolution <RESOLUTION> [OPTIONS]
```

<!-- PLACEHOLDER_PART2 -->

**常用选项：**

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `--schedule-mode` | auto | `claw_pass`（每日配额）或 `normal`（积分），省略时自动检测 |
| `--transition` | - | `creative`、`stable`、`pro` 或 `speed`（取决于模型/类型） |
| `--sample-count` | 1 | 生成样本数 |
| `--codec` | h265 | 输出视频编码 |
| `--movement-amplitude` | auto | 视频运动幅度 |
| `--aspect-ratio` | - | `16:9`、`9:16`、`1:1`、`4:3`、`3:4`（img2video/headtailimg2video 不适用） |

**任务类型与约束：**

| 类型 | 模型 | 时长 | 分辨率 | 宽高比 |
|------|------|------|--------|--------|
| `text2video` | 3.0, 3.1, 3.2 | 3.0→5s, 3.1→2-8s, 3.2→1-16s | 1080p | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `img2video` | 3.0, 3.1, 3.2 | 3.0→5s, 3.1→2-8s, 3.2→1-16s | 1080p | — |
| `headtailimg2video` | 3.0, 3.1, 3.2 | 3.0→5s, 3.1→2-8s, 3.2→1-16s | 1080p | — |
| `character2video` | 3.0, 3.1, 3.1_pro, 3.2 | 3.0→5s, 3.1→2-8s, 3.1_pro→-1/2-8s, 3.2→1-16s | 1080p | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `text2image` | 3.1, 3.2_fast_m, 3.2_pro_m | 0（图片） | 1080p, 2k, 4k | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `reference2image` | 3.1, 3.2_fast_m, 3.2_pro_m | 0（图片） | 1080p, 2k, 4k | 16:9, 9:16, 1:1, 4:3, 3:4 |

**示例：**

```bash
# 文生视频
vidu-cli task submit \
  --type text2video \
  --prompt "一只猫在雨中漫步" \
  --model-version 3.2 \
  --duration 8 \
  --resolution 1080p \
  --aspect-ratio 16:9

# 图生视频（本地文件或 URL）
vidu-cli task submit \
  --type img2video \
  --prompt "画面缓缓动起来" \
  --model-version 3.1 \
  --duration 4 \
  --resolution 1080p \
  --image ./scene.jpg

# 首尾帧生视频
vidu-cli task submit \
  --type headtailimg2video \
  --prompt "平滑过渡" \
  --model-version 3.2 \
  --duration 5 \
  --resolution 1080p \
  --image ./start.jpg \
  --image ./end.jpg

# 参考素材生视频
vidu-cli task submit \
  --type character2video \
  --prompt "角色向前走" \
  --model-version 3.2 \
  --duration 4 \
  --resolution 1080p \
  --transition pro \
  --image ./character.jpg \
  --material "mychar:element-id:1"
```

#### 查询任务结果

```bash
vidu-cli task get <TASK_ID> [--output <DIR>]
```

返回：`task_id`、`state`、`type`、`model`、`err_code`、`err_msg`。

使用 `--output` / `-o` 在任务完成时下载媒体文件到本地目录：

```bash
vidu-cli task get <TASK_ID> --output ./results
```

如果任务状态不是 `success`，下载会跳过，响应中包含 `download_skipped: true`。

<!-- PLACEHOLDER_PART3 -->

#### 口型同步

用文本或音频驱动视频口型：

```bash
# 文本模式
vidu-cli task lip-sync --video ./clip.mp4 --text "你好，世界！" --voice-id English_Aussie_Bloke

# 音频模式
vidu-cli task lip-sync --video ./clip.mp4 --audio ./speech.mp3
```

支持视频格式：MP4/MOV/AVI ≤500MB。支持音频格式：MP3/WAV/AAC/M4A ≤100MB。

#### 列出口型同步可用声音

```bash
vidu-cli task lip-sync-voices
```

#### 文字转语音（TTS）

```bash
# 基本用法（单段）
vidu-cli task tts --prompt "你好，这是一个测试。" --voice-id "Chinese (Mandarin)_Reliable_Executive"

# 多段模式，每段可指定情绪
vidu-cli task tts \
  --text "大家好！" \
  --text "这是一个令人兴奋的消息。" \
  --voice-id "Chinese (Mandarin)_Reliable_Executive" \
  --emotion "开心" \
  --emotion "激动" \
  --speed 1.2 \
  --volume 80 \
  --language-boost "Chinese"
```

`--prompt`（单段）和 `--text`（可重复，多段）互斥。使用 `--text` 时，每个 `--emotion` 按顺序与对应的 `--text` 段配对。

**参数：**

| 参数 | 必填 | 默认值 | 范围 | 说明 |
|------|------|--------|------|------|
| `--prompt` | 是* | - | 1-2000 字符 | 单段文本（*与 `--text` 互斥） |
| `--text` | 是* | - | 可重复 | 多段文本（*与 `--prompt` 互斥） |
| `--voice-id` | 是 | - | 见声音列表 | 语音合成声音 ID |
| `--speed` | 否 | 1.0 | 0.5-2.0 | 语速倍率 |
| `--volume` | 否 | 80 | 0-100 | 音量 |
| `--emotion` | 否 | - | 可重复 | 每段情绪（按顺序与 `--text` 配对） |
| `--language-boost` | 否 | - | Chinese, English, auto 等 | 增强特定语言/方言识别 |
| `--schedule-mode` | 否 | auto | claw_pass, normal | 计费模式（省略时自动检测） |

#### 列出 TTS 可用声音

```bash
vidu-cli task tts-voices
```

---

### 预估消耗

提交任务前查询积分消耗：

```bash
# 视频/图片任务
vidu-cli task cost --type text2video --model-version 3.2 --duration 8

# TTS 任务（按字符数计费）
vidu-cli task tts-cost --text "你好，这是一个测试。" --voice-id "Chinese (Mandarin)_Reliable_Executive" --pitch 0

# 口型同步任务
vidu-cli task lip-sync-cost --duration 5 --voice-id English_Aussie_Bloke
```

---

### 配额

```bash
# 查询 claw-pass 每日配额
vidu-cli quota pass

# 查询用户积分余额
vidu-cli quota credit
```

---

### 素材

管理可复用的角色/参考素材：

```bash
# 检查素材名称是否可用
vidu-cli element check --name "mychar"

# 预处理素材图片
vidu-cli element preprocess --name "mychar" --image ./char.jpg --type user

# 创建素材（支持 1-3 张图片；内部自动执行 检查 → 上传 → 预处理 → 创建）
vidu-cli element create --name "mychar" --image ./char.jpg \
  --description "我的角色" --style "动漫"

# 列出个人素材
vidu-cli element list --keyword "char" --page 0 --pagesz 20

# 搜索社区素材
vidu-cli element search --keyword "动漫女孩" --sort-by recommend --pagesz 20
```

## 许可证

MIT
