# 生成参数对齐审计与实现方案

审计日期：2026-11-12。后端基准为 `/Users/saerdna/project/eng-be/unidu/main` 的提交 `5d7e73b86`；CLI 修改前基准为 `9ba5798`。下文的 unidu 路径均相对该后端根目录，行号以该提交为准。CLI 路径相对本仓库。

关联后端地图：`cap.video.task-submit`、`svc.vidu`、`svc.infer`、`cap.vidu.saas-surface`。结论以源码为准；模型试用 Case 的前端枚举、MCP 工具支持表、`GetModels` 返回值均不能替代普通任务接口的实际校验链。

审查后修正：生成提示词长度由后端校验，CLI 保留非空检查和 UTF-8 提示词文件不超过 1 MiB 的读取限制；`rerun` 将历史 `codec: unspecified` 按后端默认编码处理。下文原有本地与线上验证结果作为修正前版本的历史记录保留，本轮修正没有重新运行线上验证。

## 范围和对齐原则

覆盖当前 CLI 支持的六种生成任务及其所有已有模型组合：`text2video`、`img2video`、`headtailimg2video`、`character2video`、`text2image`、`reference2image`。同时检查 `submit`、`cost`、`rerun` 的设置构建、媒体预检、上传和帮助文案。`img2image` 仍归一为 `reference2image`。

本次不新增后端模型，不改变专用延长、增强、配音、语音复刻和剪辑命令的业务范围。后端新增模型与 CLI 当前支持列表的差集，应作为新增功能评估，不能全部视为现有参数校验错误。

对齐分为四层：

1. **任务契约**：模型与任务组合、输出参数、文本与媒体 prompt 形状、数量。可以静态确定的规则由共享校验器执行。
2. **本地媒体预检**：针对可读取的本地文件，尽量按后端相同的类型、大小和时长口径校验。`ssupload` 的完整元数据及混合素材集合仍交给后端。
3. **上传协议与客户端处理**：文件读取、媒体解析、上传格式转换属于客户端实现能力，不能表述为模型硬限制。原图大小和上传压缩后的大小也须分清。
4. **动态权限与执行策略**：会员、试用、闲时额度、通行证、素材展开与槽位筛选，以后端当前用户状态和执行链为准；不在客户端复制动态状态机。

## 已确认差异及修改

| 项目 | CLI 修改前行为 | 后端事实及本次处理 | 后端证据 |
|---|---|---|---|
| Q4 参考图片 | 通用 `image_count + material_count <= 7` | 3.4 参考生允许 15 张直接图片；素材展开图也共享后端 15 图预算。图片 material 的展开数量和模态不可在本地假定为1 | `infer/internal/service/task_validate.go:1275-1378`；`infer/internal/biz/task.go:1156-1227`，常量 `:189` |
| SD 参考图片 | 同样套用7项上限 | SD 最多9张图片，视频/音频各最多3；图片、视频素材的实际模态由后端解析后计数 | `infer/internal/service/task_validate.go:1945-1994`；`infer/internal/biz/task.go:1202-1208` |
| reference2image | 与参考生视频共用7项上限 | 无 material 时，普通 Infer 总 prompt 数≤20；CLI生成一个文本，故可接收19张直接图片。有 material 时，后端进入素材填充路径，默认图片预算7，超预算的直接图片会被拒绝，素材展开图按剩余额度分配。两种规则分开处理 | `infer/internal/service/task_validate.go:220-223,1852-1880`；`infer/internal/biz/task.go` 的 `FillMaterials`、`FillMaterialsByLimitCount:1196-1225` |
| 普通旧模型纯图数量 | 固定7张 | 没有material时不进入FillMaterialsByLimitCount，受普通20prompt限制（一个文本时19张）；含material时仅能确定直接图片超过7必拒绝。3.1_pro走FillAllMaterials，交工作流按槽位筛选引用 | `infer/internal/biz/task.go:2874-2885`；`infer/internal/service/task_validate.go:220-223` |
| 素材id/version | 仅检查name:id:version分段 | 按后端约束校验正int64，拒绝0、负数和超范围值；名称可为空 | `vidu/internal/biz/task.go:2023`；依赖apis v1.4.335的 `infer/v1/types.pb.go:1267-1268` |
| material 计数 | 固定每个material算一个图片额度 | 停止用 `image+material` 的本地简化式误拦；本地保留普通总prompt上限与明确的直接图片上限，后端解析素材模态、展开数量、所有权与历史版本 | `infer/internal/biz/task.go:1156-1227`；`infer/internal/service/task_validate.go:1340-1378` |
| 文本长度 | 对齐阶段曾增加本地5000/20000 Unicode字符预检 | 审查后移除生成提示词长度预检，统一交后端按其归一后的文本处理。CLI 仍检查非空，UTF-8提示词文件读取仍限1 MiB。后端当前普通文本上限5000字符、Q4图生/参考生及Wan上限20000字符，不在CLI复制 | `infer/internal/service/task_validate.go:43-51,546-566` |
| 普通prompt数量 | 缺少通用上限 | 普通Infer任务最多20个prompt；Q3.1 Drama/Wan先走各自专用分支，不能把这个20上限套给Drama的9图+3视频+10音频组合 | `infer/internal/service/task_validate.go:204-223` |
| Q4 transition | 本地按模型拒绝转场 | 所有已知协议枚举先原样提交Vidu。原值在转换为Infer前已参与试用、会员与积分上下文；Q4只在后端FromViduTask和Infer专用校验阶段清空，CLI不能提前代清 | `vidu/internal/service/task.go:267-277,302-311`；`vidu/internal/biz/task.go:1952-1961,207-215`；`infer/internal/service/task_validate.go:1179-1181,1306-1308` |
| Drama/Wan transition | 本地拒绝非空转场 | 普通设置校验不按模型拒绝该已知协议字段，原值仍可能影响Vidu试用、权益和积分上下文。CLI接受已知枚举并原样透传，不因最终生成builder不消费它就自行删除 | `vidu/internal/service/task.go:276`；`vidu/internal/biz/task.go:1952-1961`；`vidu/internal/service/q31_drama.go:20-64`；`infer/internal/service/wan_video.go:129` |
| 文本数量与任务隔离 | Q4全类型容易被同一规则收窄，旧参考生缺少文本下限 | Q4仅character2video要求恰好一个文本；普通character2video至少一个文本，普通text2video允许多个纯文本prompt。img2video以图片数量校验，不额外要求一个文本；Drama/Wan按专用规则恰好一个文本 | `infer/internal/service/task_validate.go:1085-1103,1144-1197,1200-1207,1283-1285,1931-1943`；`infer/internal/service/task_validate.go:376-429`；`pkg/xgen/wan_validation.go` |
| Q3文生 transition | 缺省可通过，其他枚举被CLI本地白名单拒绝 | 保留可省略，显式传值按协议枚举接受并透传。虽`:1119`有必填检查，但缺省style为空且不在general/anime时，`:1104-1109`先清style并return；有合法style的历史请求仍可能触发后端必填检查，不能将它提炼成所有请求的无条件CLI硬限 | `infer/internal/service/task_validate.go:1104-1120`；`infer/configs/config.yaml:142-143`；`pkg/xutil/utils.go:239-244` |
| Q1帧模式 transition/style | 缺省参数未补全，并用模型白名单收窄协议枚举 | 3.0图生缺省补stable，与既有跨模型rerun默认保持一致；首尾帧省略时仍不下发，文生缺省style补general。只有img2video的3.0/3.1/3.2及character2video的3.2有静态非空要求；headtailimg2video后端只检查图片数，CLI不额外强制转场非空。所有已知转场枚举原样下发 | `infer/internal/service/task_validate.go:1191-1195,1263-1266,1408-1418`；`pkg/xutil/utils.go` 的 `FillTaskRev` 与各版本rev路由 |
| 3.2营销转场 | 全局枚举缺pro_ad | 将pro_ad作为协议枚举补齐并透传；只有3.2+character2video时会命中后端营销分支，时长3–16秒。其他组合不因此被CLI臆断拒绝，外部路由/生成结果仍由后端决定 | `infer/internal/biz/task.go:2337-2350`；`infer/internal/biz/benefit.go:275-286`；`pkg/xutil/utils.go:255-264` |
| 普通Vidu视频比例 | 对3.0/3.1/3.1_pro/3.2/3.4强制五种常用比例 | 后端没有调用IsValidAspectRatio；普通SIP链把比例原样赋值并提交，当前固定mpf-apis版本字段也无验证规则。CLI允许有限正数w:h透传，保留空值；不将前端五比例枚举升级为后端硬限，也不承诺外部模型一定生成成功 | `infer/internal/conf/conf.go:72`（全仓仅定义）；`infer/internal/workflow/sip_task_activity.go:209-211` → `infer/internal/biz/sip_task.go:549` → `infer/internal/schedcli/client.go:81-85`；mpf-apis `v1.0.55-0.20260912080302-42ce843ad9fb` 的 `sip/sched/v2/task.pb.validate.go:3282,6566` |
| SD分辨率/比例 | 仅720p/1080p、五种常用比例 | 普通SD builder明确接受480p/720p/1080p/4k及五种比例+21:9/adaptive；2k可经配置启用的降档生成、StageVideoUpscale和腾讯2k模板完成。CLI开放480p/720p/1080p/2k/4k及七比例，是否启用超分交后端判断 | `infer/internal/workflow/seedance_activity.go:77-98,138-151`；`infer/internal/workflow/vidu_maas_task.go:383-388`；`infer/internal/workflow/seedance_task_wf.go:287-299`；`infer/internal/workflow/short_story_activity.go:733-752`；`pkg/tencetcloud/mps/upscale.go:21-25,71-98` |
| 编辑模式 | 部分专用校验仅接受normal | 接受后端协议的unspecified/normal；历史任务中的unspecified不再被CLI当作非法编辑模式误拦。professional仍受任务和后端权限规则约束 | `infer/internal/service/task_validate.go:238-248`；`vidu/internal/service/q31_drama.go:69-124`；依赖apis v1.4.335的 `infer/v1/task.pb.go:429-445` |
| SD智能输出时长 | 仅接受4–15秒 | `3.2_a` 接受-1智能时长或4–15秒，显式负数仍受模型校验；其他模型的-1支持范围不改变 | `infer/internal/biz/benefit.go:115-119,245-256` |
| SD闲时 | 允许提交nopeak后由后端拒绝 | 共享调度校验明确拒绝 `3.2_a+nopeak`，不受会员/VIP/额度绕过 | `infer/internal/service/task_validate.go:127-134` |
| SD/Q4音频大小 | 共用15 MiB | SD/Q4本地音频使用20 MiB；SD允许可识别audio/*，Q4/Drama/Wan仍限制WAV/MP3，Drama/Wan保留15 MiB。Q4仍逐段校验取整后的1–16秒和最多3段 | `infer/internal/service/task_validate.go:1381-1405`；`tools/internal/biz/file.go:632,671` |
| SD参考时长 | 强制每段2–15秒、合计15秒 | 普通SD链路当前已注释媒体时长预检，不把Drama客户端或供应商参数说明当作普通接口硬限；删除这层本地误拦，数量限制保留 | `infer/internal/service/task_validate.go:1996-2027` |
| Drama音频下限 | 有视频时放宽为2秒 | 普通Q3.1 Drama的参考音频无论有无视频，均逐段4–30秒，每类总长≤30秒；删除由供应商edit场景引入的2秒分支 | `vidu/internal/service/q31_drama.go:69-124`；`infer/internal/service/task_validate.go:476-502` |
| 时长取整 | 一部分本地探测直接拿小数比较 | Files metadata的duration按秒四舍五入；本地逐段取整后再检查区间、合计，避免3.6秒在后端为4秒却被CLI拒绝，或30.4秒口径相反 | `tools/internal/service/file.go:246-247`；`infer/internal/service/task_validate.go:466-502,1400-1402` |
| Drama图片和视频预检 | 额外限制几何、视频24–60fps等 | 仅保留普通任务实际执行的图像PNG/JPEG/WebP、≤50 MiB和视频可读metadata、取整时长规则；不复制未被普通链路调用的专用client约束 | `infer/internal/service/task_validate.go:451-502`；`infer/internal/workflow/seedance_activity.go` |
| Wan视频帧率 | 强制≥16fps | 普通Wan服务校验没有这条FPS硬限，删除本地帧率拒绝；其已核验的格式、大小、几何、数量和时长约束保留 | `infer/internal/service/wan_video.go:41-97` |
| Image2.5比例 | 固定五种常用比例 | Fast/Pro明确包含3:2、2:3、21:9、9:21；尺寸helper也可按正数w:h计算。所有已支持图片模型允许有限正数w:h/智能空值，3.1另限制比例范围 | `pkg/xgen/image25_size.go:12-95`；`infer/internal/workflow/image25_task_wf.go:215,357`；`infer/internal/workflow/image2_task_wf.go:310,626` |
| 3.1图片智能/自定义比例 | 全局固定枚举误拦 | `auto`或空值归一为空，交给意图分析；正数w:h范围1:10–10:1。先做语法解析，再按模型校验，不提交字面量intelligent/custom | `infer/internal/workflow/image_task_wf.go:545-554,630-650` |
| pro编码 | CLI枚举只有h264/h265 | 增加h264_pro/h265_pro，参数来源仍是共享枚举；后端执行会员准入 | `vidu/internal/biz/task.go:1985-1990`；`infer/internal/service/task.go:244-255` |
| 本地视频容器 | MP4/MOV白名单阻止其他可识别video/* | 先用内建mp4parse，失败后用已有ffprobe解析；缺少所需解析器时报本地解析能力错误，不声称模型拒绝该容器 | `infer/internal/service/task_validate.go:466-471`；`infer/internal/service/wan_video.go:66-74` |
| 输出codec一致性 | submit在无ffprobe时将h265改为h264 | 移除普通生成的环境相关codec覆写，submit/cost/rerun保留相同用户codec；输入视频metadata修复与输出codec选择分开 | `vidu/internal/biz/task.go:185`；`infer/internal/service/task.go:244-255` |
| 历史codec默认值 | 新增白名单会拒绝历史任务返回的unspecified | 后端将历史x264映射为unspecified；rerun在校验前将该值按后端默认编码处理，显式CLI编码枚举继续保留 | `infer/internal/biz/convert.go:77-78`；`infer/internal/service/task.go:244-250` |
| 参考视频颜色元数据 | 有ffprobe但注色失败时在客户端硬拒绝 | 普通上传和任务校验不要求颜色元数据。基础生成的参考媒体尽力注色，失败则上传原文件，由后端执行；其他独立命令的已有公共上传流程保持兼容 | `tools/internal/service/file.go:169-222`；`tools/internal/biz/file.go:662-663`；`infer/internal/service/task_validate.go:465-471`；`infer/internal/service/wan_video.go:67-73` |
| 3.1_pro视频输入 | 本地视频分支不接纳该模型 | 后端允许3.1_pro视频；工作流图片按4槽、视频按取得的duration转int后×4槽计入48槽；直接remix时长会截断小数，URI metadata路径可能已先四舍五入。放不下的视频prompt或素材元素整条移除，不截短视频。CLI保留总prompt约束，不复制槽位筛选 | `infer/internal/service/task_validate.go:1234-1241`；`infer/internal/biz/task.go:2881-2885`；`infer/internal/biz/prompt_slot.go:23-34,159-177,203-219`；`pkg/xutil/media.go:435-438` |

以上表格描述的是普通生成接口的可核验行为。源码中存在的其他供应商客户端检查，只有普通调用链实际调用时才计入本次对齐依据。

## 当前支持组合与保留范围

| 任务 | 当前支持模型 | 本次输出参数范围 |
|---|---|---|
| text2video | 3.0、3.1、3.2、3.2_a、Q3.1 Drama、Wan | Q1 5秒；Q2 2–8秒；Q3 1–16秒；SD -1或4–15秒；Drama4–30秒；Wan2–30秒 |
| img2video | 3.0、3.1、3.2、3.2_a、3.4、Wan | 一张直接图片；Q4 3–16秒；其余按对应模型范围 |
| headtailimg2video | 3.0、3.1、3.2、3.2_a、Wan | 两张直接图片，保持首尾顺序；不添加Q4组合 |
| character2video | 3.0、3.1、3.1_pro、3.2、3.2_a、3.4、Q3.1 Drama、Wan | 3.1_pro仅-1或2–8秒；Q4 1–16秒；3.2 pro_ad为3–16秒；其余按模型范围 |
| text2image / reference2image | 3.1、3.2_fast_m、3.2_pro_m、3.2_image_2、3.2_image_2_5_fast、3.2_image_2_5_pro | duration归一0；1080p/2k/4k；比例按模型校验 |

`Q3.1 Drama` 指 `viduq3.1-drama-std`，`Wan` 指 `viduq2-drama-std`，`SD` 指 `3.2_a`。

明确保留的规则及理由：

- Q4分辨率为540p/720p/1080p/2k/4k；Drama/Wan为480p/720p/1080p。SD接受480p/720p/1080p/2k/4k；2k依赖后端add_upscale配置和腾讯超分路径，不能因为SD原生builder没有2k就由CLI提前拦截。其余旧视频保留已有1080p支持范围。
- Q3普通文生/图生硬校验为1–16秒。Q2/3.1_pro除智能时长外继续由套餐配置的2–8秒范围控制，不把duration范围中-1与2之间的0、1误放行。证据：`infer/internal/biz/benefit.go:97-149`、`infer/configs/config.yaml:158-205`。
- 普通Vidu视频3.0/3.1/3.1_pro/3.2/3.4只校验有限正数w:h语法并原样透传，不以未调用的IsValidAspectRatio或前端枚举制造五比例硬限。3.1图片仍限定1:10–10:1，其他已有图片模型按其尺寸转换链接受正数w:h/智能空值。SD使用五种常用比例+21:9/adaptive，Drama使用五种常用比例，Wan使用其七种明确比例；这些专用枚举不被全局放宽。
- 普通 `sample_count` 保持正整数和协议int32范围；后端/共享归一的缺省或0值转为1，CLI显式参数仍只接受正数；Drama/Wan保留明确的1–4。没有把前端的1–4枚举升级为全局硬限。视频免费用户批量>1仍由后端拒绝；参考生图有单独权益例外。证据：`infer/internal/service/task_validate.go:581-598`、`vidu/internal/service/q31_drama.go:57-62`。
- 后端前端能力中的intelligent/custom是选择器标签，真实接口用空比例或数值w:h。adaptive只对有明确builder支持的SD/Wan保留；普通SIP视频不因此接受adaptive字面量。既有Wan参考媒体大小和几何校验继续执行。
- 已有专用任务命令沿用各自契约，本次只同步它们从共享枚举获得的pro编码能力。`rerun`仍是创建新任务，需要通过当前模型规则与用户权益；不保证seed复现。

## 默认值、归一与共享调用链

| 参数或入口 | 处理方式 |
|---|---|
| model_version | submit/cost继续要求显式模型，不替用户选择其他版本 |
| Drama/Wan默认值 | duration=5、resolution=720p、aspect_ratio=16:9、sample_count=1、schedule_mode=normal |
| 旧模型submit | duration/resolution继续要求调用者提供；图片duration统一0 |
| 旧模型cost | 保留已有resolution=1080p默认；报价不要求提供实际引用媒体，但仍验证输出设置 |
| 比例 | `parse_aspect_ratio`只负责有限正数w:h语法和auto→空；普通SIP视频保持透传，SD/Drama/Wan在共享settings中用明确枚举，3.1图片用比例范围。仅Drama/Wan缺省16:9，其他模型省略时交后端 |
| 转场处理 | 已知协议枚举原样下发，Q4/Drama/Wan也不提前清除。仅img2video的3.0/3.1/3.2与character2video的3.2做静态非空检查；headtail无静态必填。3.0图生缺省stable是CLI兼容默认，首尾帧省略不下发；3.0文生缺省style补general；3.2参考生pro_ad另按3–16秒校验 |
| codec | 缺省h265；共享枚举接受普通和pro版本；rerun将历史unspecified按后端默认编码处理 |
| 生成提示词 | CLI只检查非空，长度由后端校验；UTF-8提示词文件读取仍限1 MiB |
| schedule | Q4默认normal、允许按后端规则nopeak、拒绝claw_pass；SD拒绝nopeak；Drama/Wan仅normal；其他已有自动通行证检测保留 |
| submit | 在任何上传前执行共享设置、prompt形状/数量与本地媒体预检；上传后对最终body再校验 |
| cost | 用与submit相同的设置归一和静态校验，不强行构造虚假图片/音频，不创建任务 |
| rerun | 不换模型时保留历史显式转场原值（协议unspecified按缺省处理）并接受unspecified/normal编辑模式；换模型时仍可按既有客户端迁移策略重置旧转场、style、调度等兼容字段。迁移不是后端模型白名单规则，submit/cost不借此改用户显式转场；素材模态和48槽筛选交后端 |

代码职责：`src/validators.rs`维护模型契约与共享校验；`src/commands/tasks.rs`处理设置归一、提交和本地媒体准备；`src/commands/task_ops.rs`复用归一和校验处理历史任务；`src/main.rs`仅做参数语法解析和帮助展示。中英文README同步说明。

## 上传和动态权限边界

本地和HTTP图片先完成全套准备，再开始上传，避免前一张已上传而后一张才被本地预检拒绝。通用上传的最终字节范围为512 B–10 MiB（`tools/internal/biz/file.go:780`、上传完成图片最小字节检查）；较大的源图可先压缩至上限。模型图片格式与几何校验针对最终准备产物，不把原图未压缩的大小当作上传大小。普通上传的源文件检查与模型实际收到的上传产物检查须区分；不能把某一图片转换器的源图限制写成所有模型的50 MiB拒绝规则。音频大小按字节使用MiB（1024×1024），文案不再把15 MiB与20 MiB混为一条全局限制。

本地时长只能覆盖CLI能探测的文件。混合本地和 `ssupload` 时，完整集合的duration合计、文件类型、上传状态及素材解析最终由后端校验；CLI不额外下载远程媒体来猜测metadata。普通prompt上限只计算实际prompt对象，不能用某个素材的未知展开图片数预先拒绝。

本生成命令当前暴露的转场值为creative、stable、pro、speed、pro_ad；这些值按协议语法接受，不能再用任务/模型局部白名单误拦。专用延长命令的转场沿用其已有接口范围。

原始transition必须保留到Vidu接口：`Create`先匹配试用（`vidu/internal/service/task.go:276`）、做积分预检（`:302`）、执行权益校验（`vidu/internal/biz/task.go:1952`），之后才进入Q4清转场的FromViduTask。Vidu积分设置构建保留该值（`vidu/internal/biz/task.go:2506-2519`），Credit规则也读取转场（`credit/internal/biz/credit.go:422-424`）。因此“生成builder不消费字段”不代表该字段没有业务效果；CLI提前清空可能改变试用命中、费用或准入。

会员/试用/队列权限保留后端为准：

- Q3/Q4免费用户单段最多10秒；Q2/3.1_pro免费最多5秒。Q4 2k/4k会员专享，pro编码会员专享。
- Q3.1 Drama、SD、Wan、Image2.5的会员使用资格与试用剩余次数由服务端决定，CLI仅展示说明，不把本地缓存当作资格证明。
- 闲时取决于活动启用、套餐/购买额度、当日已用量与用户锁；通行证取决于模型资格和有效权益。Q4、SD、Drama/Wan静态不兼容模式提前拒绝，其他动态失败保留后端错误。
- 素材存在性、版本、所有权、模态、图片展开次序，以及3.1_pro的48槽引用筛选均不在CLI另建一份后端实现。

证据：`infer/internal/biz/benefit.go:97-207`；`vidu/internal/biz/task.go:1946-1990`；`infer/internal/service/task_validate.go:125-167,568-598`；`infer/internal/service/task_quota.go`。

## 本地验证结果与剩余边界

以下为2026-11-12审查修正前版本已完成的验证记录，保留原始结果；其中Unicode文本长度预检随后在本轮修正中移除。这些结果不代表修正后版本已重新执行全部检查。

| 检查 | 结果 |
|---|---|
| `cargo fmt --check` | 通过 |
| `cargo test --quiet` | 295项通过：225个单元测试、18个CLI错误测试、2个契约测试、50个本地HTTP回归；无失败或跳过 |
| `cargo clippy --all-targets -- -D warnings` | 通过，无警告 |
| `cargo build --release` | 通过 |
| release二进制的 `task submit --help` / `task cost --help` | 成功渲染，并核对协议转场、pro编码、数量说明及提交模型能力文案 |
| `git diff --check` | 通过 |

上述本地HTTP测试仅使用127.0.0.1上的模拟服务和固定测试凭据；沙箱默认禁止监听端口，因此完整测试在允许本地监听的执行环境完成。本地验证阶段没有读取真实token、调用线上接口或产生生成费用。后续经用户授权进行了下节所列真实token补测。unidu仍为原提交，工作区无改动；CLI变更未提交、未推送。

当时覆盖的关键回归包括：Q4 15/16图、SD9/10图；material与直接图片分开计数及reference2image 19/20直图；普通20/21prompt与Drama专用数量；Q4仅参考生exactly-one-text、图生不附加文本数量限制、普通参考生文本下限、unspecified/normal编辑模式；当时的Unicode文本长度预检边界（本轮已移除）；SD -1/非法0/1/2/3、nopeak；所有协议transition透传、Q4/Drama/Wan不提前清除；图生/参考生静态非空与headtail可省；Q3文生可省且原值透传；Q1图生默认stable、首尾帧不新增转场、文生默认general；3.2参考生pro_ad时长分支；普通SIP视频任意正数w:h透传、SD七比例与五档分辨率、Drama/Wan专用枚举；Image2.5四种附加比例及3.1自定义边界；pro编码枚举；Drama音频有无视频均4–30秒和逐段取整合计；20/15 MiB分模型边界；无FPS误拦、可识别视频容器和缺少ffprobe错误；material正int64；submit/cost/rerun codec与设置一致与所有预检早于上传。

回归同时验证37个现有任务/模型组合的已知转场枚举、模型间限制隔离、纯图与material路径、估价不要求媒体，以及已知输入错误在任何上传前返回。另有模拟ffmpeg失败的HTTP回归，断言参考视频按原字节上传。外部服务的生成执行能力不在这些本地测试的证明范围内。

普通SIP透传任意数值比例、已知但非惯用模型组合的transition，均仅证明本地协议与业务层没有相应硬拒绝。某些转场可产生空rev或进入回退路由；CLI不据此猜测外部SIP/供应商会拒绝，也不自动替用户改成speed。真正模型执行是否支持这些值，需要相应外部服务及部署配置证据。

上述源码审计与本地测试只核验两个本地checkout，不能证明线上已部署同一版本或外部供应商会成功执行所有透传值。后端规则变更后应按记录的提交基线重新核查。当前账号的部分线上行为另见下节，不扩展为所有账号、模型和参数的执行保证。

## 真实 token 线上验证（历史补测记录）

本节保留2026-11-12审查修正前的真实token补测记录，本轮修正没有重新执行这些线上操作或下述客户端拒绝用例。该次补测经用户授权，使用项目现有生产凭据，通过当时的release二进制访问 `https://service.vidu.cn`。凭据只由本地脚本载入子进程环境，不出现在命令行、报告或证据输出中。release版本为 `vidu-cli v0.2.37-dirty`，CLI基准提交为 `9ba579897`；当时检查确认没有源码文件晚于该次测试二进制。二进制SHA256为 `9dee663ceebd8bf5d4fc1687231fb9b4e0b709f193bec7f5480f24ee64593409`。

当时的最终结果：**真实生成2/2成功，线上报价14/14成功，本地拒绝边界7/7符合预期；实际扣费4积分、退款0、净消费4积分。** 该次线上补测没有修改CLI实现；本轮后续修正移除了其中两项生成提示词长度的客户端拒绝规则。

### 实际提交、等待、下载和 rerun

程序生成15张不同的512×512 PNG几何图案作为测试输入，每张内容SHA256不同。通过 `task submit` 一次上传15张本地图片，以 `character2video`、`model_version=3.4`、`duration=1`、`resolution=540p`、`sample_count=1`、`codec=h265`、`schedule_mode=normal`、`with_audio=false` 创建任务。原任务成功后，对该任务执行一次 `task rerun`，仅覆盖文本并显式保留上述时长、分辨率和输出音频选择。

| 场景 | 新任务ID | 最终状态 | 下载后实际视频 | 实际扣费 |
|---|---|---|---|---|
| Q4参考生，15张不同本地图片 | `3549683052645187` | success | HEVC，960×528，1.041667秒，44372字节 | 2积分 |
| 上述任务rerun，保留15图 | `3549688122203065` | success | HEVC，960×528，1.041667秒，37572字节 | 2积分 |

两任务均经过真实 `task wait` 等待终态、`task get --details` 回读和 `task get --output` 下载；`ffprobe`退出码均为0。请求和服务端保存的分辨率为540p，实际产物尺寸为960×528，不将分辨率档位写成严格540像素高。实际编码为HEVC，时长与请求1秒相差约一帧。

服务端回读的两组输入均为15个image prompt和1个text prompt；类型、模型、时长、分辨率、单样本、normal调度、h265及两个位置的 `with_audio=false` 均已核对。

最初直接比较两任务的 `image.content` 字符串时不相等。这是测试比较口径问题：后端 `StorageKey` 包含当前任务ID，`SaveUploads`会将图片复制到新任务的input目录并改写content，详情接口还会生成签名URL。证据为 `infer/internal/biz/task.go:224,845-849,960-974`、`infer/internal/service/task.go:517,947` 和 `infer/internal/biz/dto/types.go:49`。因此改为下载两组共30份参考图片，逐序比较字节SHA256；**本地15张原始PNG、原任务15图、rerun 15图完全一致，顺序一致，且每组均有15个不同哈希**。下载签名图片时不携带账户Authorization。

### 真实线上报价

全部使用normal调度，未列编码时沿用h265。下列14项均成功返回报价，当前账号 `can_submit=true`。报价成功只证明所列设置的费用接口可用，不等同于已完成相应模型的媒体输入或生成执行验证。

| 场景 | 参数重点 | 返回cost_credits |
|---|---|---|
| Q4 character2video基线 | 1秒、540p、关闭输出音频 | 2 |
| Q4 character2video转场 | 1秒、540p、speed | 2 |
| Q4 character2video pro编码 | 1秒、540p、h264_pro | 2 |
| Q4 img2video | 3秒、720p | 14 |
| SD character2video | 4秒、480p、21:9 | 48 |
| SD character2video | 4秒、2k、adaptive | 48 |
| SD character2video智能时长 | -1、720p、adaptive | 240 |
| Image2.5 Fast text2image | 1080p、3:2、sample_count=5 | 30 |
| 3.1 text2image | 1080p、auto比例 | 0 |
| Q3 text2video | 1秒、1080p、3:2、省略transition | 40 |
| 3.0 headtailimg2video | 5秒、1080p、省略transition | 20 |
| Q3 character2video营销转场 | 3秒、1080p、pro_ad | 18 |
| Drama text2video默认值 | 省略duration/resolution/aspect_ratio | 150 |
| Wan text2video默认值 | 省略duration/resolution/aspect_ratio | 50 |

`h264_pro`只做线上报价，实际生成任务使用h265；不据报价结果声明pro编码的提交权益已验证。SD 2k同样只验证报价，不声明超分流程已实际执行。报价为本轮账号和时间点的返回值，不作为固定价目表。

### 客户端拒绝边界（修正前行为）

当时以下7项均返回预期 `client_error`，错误信息包含对应限制，且无HTTP状态或网络步骤。为避免预检意外放行触发线上操作，这组子进程不携带真实token，端点设为不可用的本地地址。表中的两项提示词长度拒绝仅记录当时行为；本轮已移除CLI侧长度预检，现由后端处理，不再要求这两项在本地拒绝。

| 用例 | 已核对的拒绝原因 |
|---|---|
| Q4参考生16图 | 最多15图 |
| SD参考生10图 | 最多9图 |
| Q4提示词20001个Unicode字符 | 当时由CLI拒绝超过20000字符；本轮移除此预检 |
| 普通Q3提示词5001个Unicode字符 | 当时由CLI拒绝超过5000字符；本轮移除此预检 |
| SD nopeak报价 | SD不支持nopeak |
| Drama sample_count=5 | 范围1–4 |
| Q3参考生pro_ad、duration=2 | 范围3–16秒 |

底层执行日志把上述非零退出命令记为FAIL，测试层按预期错误重解释后7项均PASS；不能把预期拒绝计为7个产品故障。最终汇总显式检查所有目标用例、非负例线上操作及两个任务终态，不依赖中间阶段脚本的退出码推定整组通过。

### 账单、证据与边界

按两个新任务的精确 `task_id` 匹配账单，按record ID去重，并按任务确认记录存在；每个任务各1条扣费记录、扣2积分、无退款，合计净消费4积分。没有使用余额差估算费用。

私有证据目录为 `/private/tmp/vidu-live-write-prod-alignment-wyx0o5u_-online`，目录权限700、JSON权限600；包括 `manifest.json`、`summary.json`、报价与详情回读、账单、逐图哈希及两个下载视频。最终汇总为PASS，未发现非预期失败；证据JSON经过真实凭据及其常见编码表示扫描，无凭据残留。签名URL只保留在私有证据，不复制到报告。临时执行脚本为 `/private/tmp/vidu-alignment-live.py`；每个付费操作执行前持久记录名称，恢复时不会盲目重复提交。

两个新任务及其关联的15条上传保留供复核。本轮真实生成覆盖Q4的15图submit/rerun；SD、Drama、Wan、pro编码及其他任务参数仅完成表中所列报价，媒体大小/时长边界与更广的37组合仍以既有本地测试及源码审计为证，不宣称逐项线上生成通过。后端生产部署版本未独立确认；本轮账号的成功结果也不代表所有账号动态权益相同。unidu工作区仍干净，CLI变更未提交、未推送。

## 审查后修复的当前验证结果

2026-11-12，按用户要求修复历史编码兼容性，并将生成提示词长度交由后端处理：

- `rerun_body`在校验前移除历史 `codec: unspecified`，不强制替换成h264或h265，交给后端默认编码处理。四种显式编码继续保留，未知编码继续拒绝；六类生成任务的HTTP回归确认仍保留原媒体和文本覆盖行为。
- 共享生成请求校验器不再检查提示词字符数。普通模型、Q4、Drama和Wan的submit/rerun回归均确认原样发送长文本及首尾空白；模拟后端接受时返回任务ID，模拟后端拒绝时保留HTTP状态、错误码和错误消息。非空校验、文本数量/媒体形状规则及UTF-8提示词文件不超过1 MiB的读取限制继续执行。
- 中英文README和命令帮助已同步。上节两个真实任务、14项报价及7项边界检查均为修正前版本的历史证据；本轮没有新增线上请求或付费任务。

| 当前版本检查 | 结果 |
|---|---|
| `cargo test --quiet` | 297项全部通过：226个单元测试、18个CLI错误测试、2个契约测试、51个本地HTTP回归；无失败或跳过 |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过，无警告 |
| `cargo build --release` | 通过 |
| `git diff --check` | 通过 |
