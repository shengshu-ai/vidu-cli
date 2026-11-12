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
| `--duration` | Drama/Wan 3：5；旧视频模型：必填 | 视频时长（秒）。Drama 支持 4–30 秒；Wan 3 支持 2–30 秒；图片任务忽略此项 |
| `--resolution` | Drama/Wan 3：720p；旧版 submit 模型：必填 | 输出分辨率。旧模型的 cost 查询仍默认 1080p |
| `--aspect-ratio` | Drama/Wan：16:9；其他省略交后端 | 图片支持正数 `w:h` 或 `auto`；`3.1` 图片限 1:10–10:1。Image 2.5 含 `3:2`、`2:3`、`21:9`、`9:21`；普通 Vidu 视频允许透传正数 `w:h`；SD/Wan 使用各自枚举，含 `21:9` / `adaptive` |
| `--schedule-mode` | auto | `nopeak` 使用闲时额度，资格、剩余额度及任务/模型限制由服务端校验；`claw_pass` 使用通行证额度；`normal` 使用积分。Q4 默认 `normal`，不支持 `claw_pass`，并可按服务端规则使用 `nopeak`。SD 禁止 `nopeak`；Drama/Wan 3 只支持积分模式 `normal`。其他模型省略时自动检测有效 claw pass，否则使用 `normal` |
| `--with-audio` | 保持现有行为 | 控制支持模型的视频输出是否生成音频。`--with-audio` 开启，`--with-audio=false` 关闭；支持 `submit`、`cost`、`rerun`，与参考音频输入 `--audio` 分开使用 |
| `--audio` | - | `character2video` 的 SD（`3.2_a`）、Q4、Drama 或 Wan 3 参考音频。SD/Q4 最多 3 段，本地每段 ≤20 MiB；SD 支持可解析的音频，时长交给后端，Q4 仅 WAV/MP3、每段取整后 1–16 秒。Drama 最多 10 段，本地每段 ≤15 MiB、取整后每段 4–30 秒且合计 ≤30 秒。Wan 3 最多 5 段，每段 ≤15 MiB、1–15 秒且合计 ≤15 秒 |
| `--transition` | - | `img2video` / `headtailimg2video`：`3.0` 使用 `creative` / `stable`（图生默认 `stable`，首尾帧省略时不下发），`3.1` / `3.2` 通过 `pro` / `speed` 选择版本；3.0/3.1/3.2 图生必须有转场，首尾帧可省略；`character2video 3.2` 必须有转场（`pro` / `speed` / `pro_ad` 选择其版本）（营销模式时长 3–16 秒）；`text2video 3.2` 的 `pro` / `speed` 用于选择变体，其他已知协议枚举也会原样提交。已知枚举原样透传，由后端判断试用、权限和计费；Q4 在后续阶段清空，Drama/Wan 生成端不使用 |
| `--sample-count` | 1 | 正 int32；Drama/Wan 为 1–4，其他套餐及动态数量限制由后端判断 |
| `--codec` | h265 | `h264`、`h265`、`h264_pro`、`h265_pro`；pro 编码的会员资格由服务端校验 |
| `--movement-amplitude` | auto | 视频运动幅度 |

文本提示词必须非空，长度限制由后端校验。通过 `--prompt-path` / `--prompt-file` 读取的 UTF-8 提示词文件仍须不超过 1 MiB。普通任务最多 20 个 prompt 对象，Drama/Wan 使用各自独立的媒体数量规则。

输出编码不会因本机是否安装 `ffprobe` 而从 h265 自动改为 h264，submit/cost/rerun 保持相同编码选择。图片在任何上传前全部准备并校验，通用上传的最终字节为 512 B–10 MiB；较大的源图可以先压缩至该上限，模型格式和几何规则针对最终上传产物。素材 ID 和版本必须为正的 int64，名称仍可为空。

`--with-audio` 控制输出视频生成音频；`--audio` 仅提供参考文件，不会自动开启输出音频。查询 `task cost` 与提交 `task submit` 时请使用相同的音频选项。不传开关时保持现有行为；`task rerun` 默认保留历史音频选择，传开关可覆盖，`task rerun --with-audio=false` 也会关闭历史唇音同步设置。音频能力取决于模型，报价由后端返回。

**任务类型与约束：**

| 类型 | 模型 | 时长 | 分辨率 | 宽高比 | 会员功能 |
|------|------|------|--------|--------|--------|
| `text2video` | 3.0, 3.1, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→-1/4-15s | 1080p；SD：480p/720p/1080p/2k/4k | 正数 w:h；SD 按自身枚举 | Seedance 2.0；3.1 超过 5 秒、Q3 超过 10 秒需会员 |
| `text2video`（Q3.1 Drama） | `viduq3.1-drama-std` | 4–30 秒（默认 5 秒） | 480p、720p（默认）、1080p | 16:9（默认）、9:16、1:1、4:3、3:4 | Seedance 2.5 |
| `img2video` | 3.0, 3.1, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→-1/4-15s | 1080p；SD：480p/720p/1080p/2k/4k | — | Seedance 2.0；3.1 超过 5 秒、Q3 超过 10 秒需会员 |
| `img2video`（Q4） | 3.4 | 3-16s | 540p, 720p, 1080p, 2k, 4k | 按输入图片确定 | 超过 10 秒及 2k/4k 需会员 |
| `headtailimg2video` | 3.0, 3.1, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.2→1-16s, 3.2_a→-1/4-15s | 1080p；SD：480p/720p/1080p/2k/4k | — | Seedance 2.0；3.1 超过 5 秒、Q3 超过 10 秒需会员 |
| `character2video` | 3.0, 3.1, 3.1_pro, 3.2, 3.2_a | 3.0→5s, 3.1→2-8s, 3.1_pro→-1/2-8s, 3.2→1-16s, 3.2_a→-1/4-15s | 1080p；SD：480p/720p/1080p/2k/4k | 正数 w:h；SD 按自身枚举 | Seedance 2.0；3.1 / 3.1_pro 超过 5 秒、Q3 超过 10 秒需会员 |
| `character2video`（Q4） | 3.4 | 1-16s | 540p, 720p, 1080p, 2k, 4k | 正数 w:h | 超过 10 秒需会员 |
| `character2video`（Q3.1 Drama） | `viduq3.1-drama-std` | 4–30 秒（默认 5 秒） | 480p、720p（默认）、1080p | 16:9（默认）、9:16、1:1、4:3、3:4 | Seedance 2.5 |
| `text2video`、`img2video`、`headtailimg2video`、`character2video`（Wan 3） | `viduq2-drama-std` | 2–30 秒（默认 5 秒） | 480p、720p（默认）、1080p | 16:9（默认）、9:16、1:1、4:3、3:4、21:9、adaptive | Wan 3 |
| `text2image` | 3.1, 3.2_fast_m, 3.2_pro_m, 3.2_image_2, 3.2_image_2_5_fast, 3.2_image_2_5_pro | 0（图片） | 1080p, 2k, 4k | 正数w:h/auto；3.1限1:10–10:1 | Image 2.5 Fast / Pro |
| `reference2image` | 3.1, 3.2_fast_m, 3.2_pro_m, 3.2_image_2, 3.2_image_2_5_fast, 3.2_image_2_5_pro | 0（图片） | 1080p, 2k, 4k | 正数w:h/auto；3.1限1:10–10:1 | Image 2.5 Fast / Pro |

**会员功能**：Seedance 2.0（`3.2_a`）、Seedance 2.5（`viduq3.1-drama-std`，下文也称 Q3.1 Drama）、Wan 3（`viduq2-drama-std`）和 Image 2.5（`3.2_image_2_5_fast` / `3.2_image_2_5_pro`）需有效会员权益，使用资格由服务端校验。会员权益要求同样适用于 `task rerun` 使用这些模型生成新任务；`task cost --help` 也会提示模型的会员要求。积分计费说明不代表已具备模型使用权益。表中“—”表示此处未标注会员要求。

**Image 2.5 Fast / Pro 免费试用**：会员功能；有剩余试用额度时可免费试用，额度用完后需有效会员权益。试用资格及剩余额度以服务端为准。Fast / Pro 的文生图（`text2image`）、参考生图（`reference2image`）试用额度分别计算。

**视频时长会员限制**：`3.1` / `3.1_pro` 免费账号最长 5 秒，6–8 秒需会员；Q3（`3.2`）/ Q4（`3.4`）免费账号最长 10 秒，11–16 秒需会员。此说明不适用于图片任务。

参考数量按模型分别限制。`character2video` 的 Q4 最多 **15 张直接图片**，SD 最多 **9 张**；其他已支持旧模型的纯图片参考受总共 20 个 prompt 限制（一个文本时最多 **19 张**）。混用 material 时，普通旧模型的图片预算为 **7 张**，Q4/SD 为15/9张，`3.1_pro` 使用后端槽位分配。一个图片 material 可能展开成多张图片，也可能是其他模态；后端负责解析及分配图片额度，CLI 不再把每个 material 固定算成一张图。Q4 允许文本加纯音频参考；SD 即使有音频，仍需图片、material 或视频参考。SD 最多 3 段视频、3 段音频。`3.1_pro` 由后端在 48 个槽位内筛选引用：图片占 4 槽，视频每秒占 4 槽，具体时长与取整由后端计算；放不下的引用可能整条被移除。这不表示允许 48 段视频，也不表示会截短视频保留部分帧。

`reference2image` 不共用旧参考生视频的 7 张校验。只传一个文本 prompt 时，最多 **19 张直接图片**可满足后端总共 20 个 prompt 的限制。混用 material 时，后端另按 7 张额度分配直接图片和素材展开图；此时超过 7 张直接图片会被拒绝，素材展开图按剩余额度分配。CLI 的报价和提交都不在本地解析素材。

Q4 本地参考音频支持 WAV/MP3，每段 ≤20 MiB、取整后 1–16 秒，最多 3 段，没有合计 15 秒的限制。SD 支持本地可识别音频文件、每段 ≤20 MiB；普通后端链路已停用参考时长检查，因此 CLI 移除旧的每段 2–15 秒、合计 15 秒硬限制。SD 输出时长支持 `-1` 智能时长或 4–15 秒，并禁止 `nopeak`；分辨率支持 480p/720p/1080p/2k/4k，比例为 16:9、9:16、1:1、4:3、3:4、21:9、adaptive，超分可用性由后端配置决定。音频校验在上传前完成；已有 `ssupload` 引用和混合输入的完整元数据交给服务端校验。

Q3.1 Drama 的 `text2video` 只接受文本。多模态参考使用 `character2video`，至少提供图片、视频或音频之一，最多 9 张图、3 段视频、10 段音频。不支持 material 或智能输出时长，转场参数透传给后端策略判断，且仅支持 `--schedule-mode normal`。本地 WAV/MP3 每段 ≤15 MiB；参考音频和视频均逐段四舍五入到整数秒后校验每段 4–30 秒，每类合计最多 30 秒。混入视频也不会将音频下限降至 2 秒。准备后实际上传的图片支持 PNG/JPEG/WebP、模型上限 50 MiB，且仍须满足通用上传限制；不额外限制图片几何或参考视频帧率、几何。本地视频优先使用内建 MP4 解析器，其他容器或解析失败时使用 `ffprobe`；缺少所需解析器时提示本地能力错误。已有 `ssupload` 输入由服务端完整校验。submit 和 cost 使用相同的 Drama 默认参数，报价由后端返回。

Image 2.5 Fast/Pro 在五种常用比例外支持 `3:2`、`2:3`、`21:9`、`9:21`。`3.1` 生图支持 `--aspect-ratio auto`（以空比例交给后端意图分析），以及 1:10–10:1 内的正数 `w:h` 自定义比例。其他图片模型也接受可解析的正数 `w:h` 或智能空比例；普通 Vidu 视频比例允许透传，SD/Drama/Wan 仍使用各自明确的比例集合。语法校验通过不代表外部执行服务已验证支持每种自定义比例。

Wan 3 默认 5 秒、720p、16:9、`normal` 积分模式。
支持 1–4 个样本，不支持自动时长 `-1`、角色素材、闲时或通行证模式；转场参数透传给后端策略判断。
`text2video` 只接受文本；`img2video` 必须有一张图片；`headtailimg2video` 必须有两张图片，
顺序为首帧、尾帧。首帧/首尾帧模式不能混入视频或音频参考。
`character2video` 至少提供一个参考，最多 10 张图、5 段视频、5 段音频，总计 20 个媒体，支持纯音频参考。
准备后实际上传的图片支持 JPEG/PNG/BMP/WEBP、模型上限 20 MiB，边长 240–8000px，宽高比 1:8–8:1，且仍须满足通用上传限制。
本地可解析视频最大 100 MiB，边长 240–4096px，宽高比 1:8–8:1。
视频/音频每段 1–15 秒，每类总时长最多 15 秒；输入视频总时长加输出时长最多 30 秒。
本地音频支持 WAV/MP3，最大 15 MiB。已有 `ssupload` 素材由服务端校验；混合输入时也由服务端校验完整时长集合。
报价由后端返回。

```bash
# Wan 3 文生视频：2 秒、480p
vidu-cli task submit --type text2video --prompt "一只猫在雨中行走" \
  --model-version viduq2-drama-std --duration 2 --resolution 480p --wait

# Wan 3 生成有声视频
vidu-cli task submit --type text2video --prompt "海浪拍打沙滩的声音" \
  --model-version viduq2-drama-std --duration 3 --with-audio --wait

# Wan 3 多模态参考：本地文件会自动上传
vidu-cli task submit --type character2video --prompt "参考人物、镜头和配乐生成视频" \
  --model-version viduq2-drama-std --image ./character.jpg \
  --video ./reference.mp4 --audio ./soundtrack.wav --with-audio

# 使用与生成相同的默认参数报价，不创建任务或上传素材
vidu-cli task cost --type text2video --model-version viduq2-drama-std
```

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

`submit`、`tts`、`lip-sync`、`compose`、`extend`、`upscale`、`upscale-image`、`rerun` 都支持 `--wait [--timeout 1800] [--output DIR]`。提交命令上的 `--timeout`、`--output` 需要同时使用 `--wait`。例如：

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

仅当 `--model-version` 实际改变时，rerun 沿用已有的版本设置迁移行为：保留兼容选择；需要替换时，3.0 图生/首尾帧使用 `stable`，需要 `pro` / `speed` 的 3.1/3.2 使用 `speed`，生成端不消费转场的目标移除转场。这是 CLI 的跨模型迁移默认值，不表示后端禁止该字段；未切换模型时保留历史转场原值，由后端执行试用、权限和计费判断。历史 `codec: unspecified` 在 rerun 时按后端默认编码处理。不兼容的计费模式回到目标模型默认值（Q4/Drama 为 `normal`）；历史 vendor model 交由服务端重新选择。原 style 保留，切到 3.0 且 style 为空或缺失时使用 `general`。时长和分辨率继续沿用历史值，需要时请显式覆盖以符合新模型约束。

#### 视频超分与积分预估

`task upscale` 接受已有视频作品或单个视频素材；`--creation-id` 和 `--video` 必须二选一。`--resolution` 必填，支持 `720p`、`1080p`、`2k`、`4k`，源作品无需是 1080p。

```bash
# 已有作品：自动读取时长，等待完成并下载
vidu-cli task upscale --creation-id 123 --resolution 2k --wait --output ./results

# 本地视频：自动读取时长并上传，默认返回 task_id
vidu-cli task upscale --video ./input.mp4 --resolution 4k

# 远程素材：必须填写完整源视频的时长（正整数秒，不负责裁剪）
vidu-cli task upscale --video 'https://example.com/input.mp4' --duration 8 --resolution 2k
vidu-cli task upscale --video 'ssupload:?id=123' --duration 8 --resolution 1080p

# 预估与提交使用相同输入、分辨率和时长规则
vidu-cli task upscale-cost --creation-id 123 --resolution 2k
vidu-cli task upscale-cost --video ./input.mp4 --resolution 4k
vidu-cli task upscale-cost --video 'ssupload:?id=123' --duration 8 --resolution 2k
```

本地文件支持 MP4/MOV/AVI，大小不超过 500 MiB，必须包含视频轨且非空。优先使用内置 MP4 解析器读取时长，无法解析时使用本机 `ffprobe`；两者都无法读取时会报错。自动读取的时长四舍五入为整数秒，必须处于 `1..2147483647` 范围，作品和本地输入不允许通过 `--duration` 覆盖。URL/ssupload 不做远程时长探测，调用者负责提供准确的完整源视频时长，且须在上述整数范围内。

`upscale-cost` 只查询报价，不上传本地文件、不创建任务，也不接受 `--wait`、`--timeout`、`--output`。输出沿用 `cost_credits`、`current_credits`、`can_submit` 和 `original_cost_credits`；价格由服务端返回，`can_submit` 仅表示报价时的余额判断，不保证素材一定通过任务校验。

超分使用普通积分模式，供应商由服务端选择。提交失败不会自动重提；`--wait` 默认等待 1800 秒，超时不会取消后台任务，可使用返回的任务 ID 继续执行 `vidu-cli task wait "$TASK_ID" --timeout 1800 --output ./results`。图片超分仍使用原有 `upscale-image` 命令。

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
