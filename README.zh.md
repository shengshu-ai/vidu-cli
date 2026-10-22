# vidu-cli

[English](README.md)

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
vidu-cli [--format json|table] <COMMAND>

Commands:
  upload   上传图片 → 获取 ssupload URI
  task     任务操作
  element  素材（参考元素）操作
  creation 作品列表与收藏
  voice    个人克隆音色
  quota    配额、余额与积分流水
```

默认输出保持 JSON `ok` 信封。手动查看时可加全局 `--format table`；脚本可继续使用默认 JSON。

下文的 `TASK_ID`、`VOICE_ID` 和分页 token 必须替换为响应中的实际值；数字素材 ID `42`、作品 ID `123` 和标签 ID `1`、`2` 也是示例，请替换为有权限使用的真实 ID。任务 ID 与作品 ID 不能互换。

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
vidu-cli task submit --type <TYPE> --prompt <PROMPT> \
  --model-version <VERSION> [--duration <DURATION>] [--resolution <RESOLUTION>] [OPTIONS]
```

**常用选项：**

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `--duration` | Drama：5；旧视频模型：必填 | 视频时长（秒）。Drama 支持 4–30 秒；图片任务忽略此项 |
| `--resolution` | Drama：720p；旧版 submit 模型：必填 | 输出分辨率。旧模型的 cost 查询仍默认 1080p |
| `--aspect-ratio` | Drama：16:9 | `16:9`、`9:16`、`1:1`、`4:3`、`3:4`（img2video/headtailimg2video 不适用） |
| `--schedule-mode` | auto | `nopeak` 使用闲时额度，资格、剩余额度及任务/模型限制由服务端校验；`claw_pass` 使用通行证额度；`normal` 使用积分。Q4 默认 `normal`，不支持 `claw_pass`，并可按服务端规则使用 `nopeak`。Drama 只支持积分模式 `normal`。其他模型省略时自动检测有效 claw pass，否则使用 `normal` |
| `--audio` | - | `character2video` 的 `3.2_a`、`3.4`（Q4）或 Q3.1 Drama 参考音频。`3.2_a`/Q4 最多 3 个（`3.2_a` 每个 2–15 秒、合计不超过 15 秒；Q4 每个 1–16 秒）；Drama 最多 10 个、合计 30 秒。本地 Drama 音频：未传 `--video` 时每段 4–30 秒，传了 `--video` 时每段 2–30 秒；有视频时任务类型由服务端判断，非 edit 任务仍要求至少 4 秒。本地 WAV/MP3 文件最大 15MB；`ssupload` 时长由服务端校验 |
| `--transition` | - | `img2video` / `headtailimg2video`：`3.0` 使用 `creative` / `stable`，`3.1` / `3.2` 必须传 `pro` / `speed`；`character2video 3.2` 也必须传 `pro` / `speed`；`text2video` 仅 `3.2` 支持 |
| `--sample-count` | 1 | 生成样本数 |
| `--codec` | h265 | 输出视频编码 |
| `--movement-amplitude` | auto | 视频运动幅度 |

**任务类型与约束：**

| 类型 | 模型 | 时长 | 分辨率 | 宽高比 |
|------|------|------|--------|--------|
| `text2video` | 3.0, 3.1, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→4-15s | 1080p | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `text2video`（Q3.1 Drama） | `viduq3.1-drama-std` | 4–30 秒（默认 5 秒） | 480p、720p（默认）、1080p | 16:9（默认）、9:16、1:1、4:3、3:4 |
| `img2video` | 3.0, 3.1, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→4-15s | 1080p | — |
| `headtailimg2video` | 3.0, 3.1, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→4-15s | 1080p | — |
| `character2video` | 3.0, 3.1, 3.1_pro, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.1_pro→-1/2-8s, 3.2→1-16s, 3.2_a→4-15s | 1080p | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `character2video`（Q4） | 3.4 | 1-16s | 540p, 720p, 1080p, 2k, 4k | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `character2video`（Q3.1 Drama） | `viduq3.1-drama-std` | 4–30 秒（默认 5 秒） | 480p、720p（默认）、1080p | 16:9（默认）、9:16、1:1、4:3、3:4 |
| `text2image` | 3.1, 3.2_fast_m, 3.2_pro_m, 3.2_image_2, 3.2_image_2_5_fast, 3.2_image_2_5_pro | 0（图片） | 1080p, 2k, 4k | 16:9, 9:16, 1:1, 4:3, 3:4 |
| `reference2image` | 3.1, 3.2_fast_m, 3.2_pro_m, 3.2_image_2, 3.2_image_2_5_fast, 3.2_image_2_5_pro | 0（图片） | 1080p, 2k, 4k | 16:9, 9:16, 1:1, 4:3, 3:4 |

Q4 本地参考音频每段 1-16 秒，最多 3 段，不套用 `3.2_a` 的合计 15 秒限制。
Q4 支持文本加纯音频参考，有音频时图片或 material 可不传。
`3.2_a` 保持每段 2-15 秒、合计不超过 15 秒的规则。
本地 WAV/MP3 文件仍限制为每个不超过 15 MB；音频校验在图片上传前完成，
已有 `ssupload` 引用的元数据由服务端校验。

Q3.1 Drama 的 `text2video` 只接受文本 prompt。多模态参考请使用
`character2video`，至少提供一张图、一个视频或一段音频；最多 9 张图、3 段视频、10 段音频。
音频合计最多 30 秒：未传 `--video` 时本地音频每段须为 4–30 秒，传了
`--video` 时为 2–30 秒；此时由服务端判断任务类型，并对非 edit 任务要求至少 4 秒。
本地 WAV/MP3 音频每段最大 15MB，`ssupload` 时长由服务端校验。
CLI 暂不提供显式指定 edit/extend 任务类型的参数，任务类型由服务端结合参考素材和 prompt 判断。
该模型不支持 material、自动时长或转场，且仅支持
`--schedule-mode normal` 积分模式。每段本地
MP4/MOV 参考视频须为 4–30 秒、平均帧率为 24–60 fps，合计不超过 30 秒。
CLI 会在提交前检查每段本地视频的平均帧率。已有 `ssupload` 视频直接透传，
由服务端校验完整集合。submit 和 cost 使用相同的 Drama 默认参数，
报价由后端返回。

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

# Q3.1 Drama（时长、分辨率和宽高比使用模型默认值）
vidu-cli task submit --type text2video --prompt "黎明时安静的街道" \
  --model-version viduq3.1-drama-std

# 图生视频（本地文件或 URL）
vidu-cli task submit \
  --type img2video \
  --prompt "画面缓缓动起来" \
  --model-version 3.1 \
  --duration 4 \
  --resolution 1080p \
  --transition pro \
  --image ./scene.jpg

# 首尾帧生视频
vidu-cli task submit \
  --type headtailimg2video \
  --prompt "平滑过渡" \
  --model-version 3.2 \
  --duration 5 \
  --resolution 1080p \
  --transition pro \
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
  --material "mychar:42:1"
```

#### 查询任务结果

```bash
vidu-cli task get <TASK_ID> [--details] [--output <DIR>]
```

返回 `task_id`、`state`、`type`、`model`；失败任务还包含 `err_code`、`err_msg`。

使用 `--output` / `-o` 在任务完成时下载媒体文件到本地目录：

```bash
vidu-cli task get "$TASK_ID" --output ./results
```

如果任务状态不是 `success`，下载会跳过，响应中包含 `download_skipped: "task not ready"`。

#### 历史、详情与等待

```bash
vidu-cli task list --state success --pagesz 20
vidu-cli task get "$TASK_ID" --details
vidu-cli task wait "$TASK_ID" --timeout 1800 --poll-interval 5 --output ./results
vidu-cli --format table task list
```

`task list` 的 `--state`、`--type`、`--scene` 可重复；用 `--page-token` 继续游标翻页。`get --details` 增加原始 `input`、`settings` 与包含实际 `creation_id` 的 `creations`。

`submit`、`tts`、`lip-sync`、`compose`、`extend`、`upscale-image`、`rerun` 都支持 `--wait [--timeout 1800] [--output DIR]`。提交命令上的 `--timeout`、`--output` 需要同时使用 `--wait`。例如：

```bash
vidu-cli task submit --type text2video --prompt "黎明时安静的街道" \
  --model-version viduq3.1-drama-std --wait --output ./results
```

等待进度写入 stderr，最终结果写入 stdout。超时或中断只结束本地等待，不取消服务端任务；失败或超时返回非零状态。`task get` 仍只查询一次。

#### 管理任务与复用作品

```bash
vidu-cli task cancel "$TASK_ID"
vidu-cli task delete "$TASK_ID"
vidu-cli task rerun "$TASK_ID" --prompt "改成雨天" --wait
vidu-cli task extend --creation-id 123 --duration 4 --transition extendspeed --wait
vidu-cli task upscale-image --creation-id 123 --resolution 2k --wait
```

取消只适用于进入 `processing` 前的任务，且不支持 `clip_compose`；删除是历史记录软删除，`queueing`、`processing` 状态不允许删除。具体权限由服务端校验。

`rerun` 从历史输入创建新的付费任务，支持原有六类图片/视频 submit 任务；可覆盖 prompt（含 `--prompt-path` / `--prompt-file`）、模型（`--model-version` / `--model`）、时长和分辨率，保留参考素材，不保证相同 seed 复现。延长要求已完成、可访问的 1080p 视频作品，固定模型 3.1、1080p，延长 1–7 秒，默认 `extendspeed`；也可选 `extendpro`，套餐和总时长仍由服务端校验。图片增强以已有图片作品为输入，默认 2k，可选 720p、1080p、2k、4k。

仅当 `--model-version` 实际改变时，rerun 保留目标类型/模型接受的转场；否则，3.0 图生/首尾帧使用 `stable`，需要 `pro` / `speed` 的 3.1/3.2 使用 `speed`，不支持转场的目标移除转场。不兼容的计费模式回到目标模型默认值（Q4/Drama 为 `normal`）；历史 vendor model 交由服务端重新选择。原 style 保留，切到 3.0 且 style 为空或缺失时使用 `general`。时长和分辨率继续沿用历史值，需要时请显式覆盖以符合新模型约束。

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
| `--schedule-mode` | 否 | auto | claw_pass, nopeak, normal | 计费/排队模式：`nopeak` 使用闲时额度并由服务端校验资格、剩余额度和任务/模型限制；`claw_pass` 使用通行证额度；`normal` 使用积分。省略时若有有效 claw pass 则自动选择，否则使用 `normal` |

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

# Q3.1 Drama 默认 duration 5、720p、16:9
vidu-cli task cost --type text2video --model-version viduq3.1-drama-std

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

# 积分流水（数字页，从 0 开始）
vidu-cli quota records --page 0 --pagesz 20
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

个人素材列表支持 `--keyword`，默认第一页；`--page-token` 使用响应中的 `next_page_token`，与显式 `--page` 互斥。数字页兼容模式由 CLI 顺序获取游标，较深页会产生多次请求。个人素材的 `--pagesz` 范围为 10–100，默认 20。

```bash
vidu-cli element list --keyword "char" --page-token "$NEXT_PAGE_TOKEN" --pagesz 20
vidu-cli element get 42
vidu-cli element update 42 --name "新角色" --label 1 --label 2 --recaption "新的角色描述"
vidu-cli element update 42 --clear-labels
vidu-cli element delete 42
vidu-cli element search --keyword "夏日" --category "character" --topic "summer"
vidu-cli element favorite 42
vidu-cli element favorites --pagesz 20
vidu-cli element unfavorite 42
```

`--label` 接收数字标签 ID 并整体替换标签集合，与 `--clear-labels` 互斥。`--recaption` 修改描述并保留原 style。更新至少需要一项变更；共享投稿、版本和删除权限由服务端校验。收藏命令接收素材 ID，由 CLI 内部查询 share ID。`element favorites` 是独立的游标列表，只接收分页参数，不接收搜索条件。

### 作品

```bash
vidu-cli creation list --page 0 --pagesz 20
vidu-cli creation list --favorites-only --page 0 --pagesz 20
vidu-cli creation favorite 123
vidu-cli creation unfavorite 123
```

列表使用数字页（offset）并返回 `total`；收藏列表包含当前用户有权访问的收藏作品，不限于本人作品。使用 `task get --details` 或作品列表返回的作品 ID。

### 个人克隆音色

```bash
vidu-cli voice create --audio ./voice-sample.wav --name "我的旁白" --wait --timeout 1800
vidu-cli voice list --pagesz 20
vidu-cli voice get "$VOICE_ID"
vidu-cli voice update "$VOICE_ID" --name "新旁白" --description "适合故事讲述"
vidu-cli voice wait "$VOICE_ID" --timeout 1800
vidu-cli task tts --prompt "你好" --voice-id "$VOICE_ID" --wait
```

`VOICE_ID` 须设为 create/list 返回的个人 `voice_...` ID。录音为本地文件，须为 10–300 秒、不超过 20 MiB。上传完成接口同步审核，通过后才使用 `ssupload` URI 发起复刻。默认付费；`--use-trial` 显式申请试用，失败不会自动改为付费重提。

不加 `--wait` 时，create 在上传审核后提交复刻并返回 ID；加上后继续等待复刻。`--timeout` 需要 `--wait`，只控制复刻等待，默认 1800 秒。`voice wait` 通过 SSE 等待，无轮询间隔参数；超时或中断不取消服务端复刻。列表使用 `--page-token` 游标。create 不接受 description，创建后用 update 修改。没有音色删除命令；原有 `task tts-voices`、`task lip-sync-voices` 继续列官方静态音色。

## 许可证

MIT
