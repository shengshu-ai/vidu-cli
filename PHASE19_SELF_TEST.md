# 十九期 CLI 自查与自测

日期：2026-11-26。需求来源：https://shengshu.feishu.cn/wiki/D0z3wHSiFiTC9QkPOq0crAbWnwf ，仅十九期及其主体相关评论。
测试环境：https://vidu-api.test.s.shu.team 。认证通过 VIDU_TOKEN 环境变量提供；本文不保存 token。

## 自查修正

- 撤回无明确文档依据的 SD2.5 输出比例扩展，保留原有五种比例。SD2.0/Wan 的既有比例不变。
- 补齐文档明确规定的 SD2.0 本地参考视频像素总数 407,696–8,847,360 校验。已有上传引用由服务端校验。
- 保留非十九期模型原有缺少参考输入时的错误说明；将新增主体槽位计数限定到十九期模型，恢复 Q4 原有行为并补回归测试。

## 本地需求覆盖

| 文档要求 | 验证依据 |
| --- | --- |
| SD2.0 图片 9、视频 3、音频 3；SD2.5 图片 30、视频 10、音频 10 | validators 模型数量测试；phase19_seedance_and_wan_references_are_forwarded 提交结构测试 |
| SD 图片格式、大小、边长 300–6000、比例 0.4–2.5 | phase19_original_image_format_size_and_alpha_checks；phase19_seedance_image_geometry_boundaries |
| SD 视频 MP4/MOV ≤200 MiB；SD2.0 像素范围 | 格式验证测试；新增真实 CLI 低像素视频拒绝测试 |
| SD2.0 视频/音频每段 2–15 秒，每类合计 ≤15；SD2.5 每段 4–30，每类合计 ≤30 | tasks 模型时长边界单元测试 |
| 输出 SD2.0 4–15，SD2.5 4–30；480p/720p/1080p | validators 输出设置测试；真实 480p 报价/提交；SD2.0 原有自动时长保留 |
| Wan 图片 ≤20 MiB、PNG 无透明通道、边长 240–8000、比例 ≤8:1 | 图片格式/透明通道/几何测试 |
| Wan 视频 ≤100 MiB、边长 240–4096、每类时长额度 | Wan 文件和时长测试 |
| Wan 有视频可输出 1 秒，无视频至少 2 秒，输入+输出 ≤30 | 单元测试；真实 1 秒视频参考提交；无视频 1 秒真实 CLI 拒绝 |
| 图片/文本主体支持，视频主体不支持，图片槽位额度 | 本地 DTO 转发和槽位测试；真实主体提交结果见下表，服务端尚未符合全部要求 |

最终全量测试 362 项通过：239 单元测试、22 CLI 错误测试、2 主体契约测试、99 功能端到端测试。git diff --check 通过。

## 真实接口结果

统一使用 character2video、normal、480p；主体使用测试账号已有图片主体（element get 确认 modality=image）。

| 场景 | 结果 | 标识 |
| --- | --- | --- |
| SD2.0 4 秒报价 | can_submit=true，34 积分 | 原价 40 |
| SD2.5 4 秒报价 | can_submit=true，51 积分 | 原价 60 |
| Wan 4 秒报价 | can_submit=true，19 积分 | 原价 24 |
| SD2.0 5 秒视频参考，输出 4 秒 | 提交成功，最近状态 preparation | 3569224233077232 |
| SD2.5 5 秒视频参考，输出 4 秒 | 提交成功，最近状态 preparation | 3569224602024488 |
| Wan 5 秒视频参考，输出 1 秒 | 提交成功，执行 failed / InternalServiceFailure | 3569224647673043 |
| SD2.0 图片主体，输出 4 秒 | 提交成功，最近状态 preparation | 3569227248946036 |
| SD2.5 图片主体 | HTTP 400 FieldInvalid，input.prompts；提示仅接受 text/image/video/audio | trace e7eb15fd52a8c349afdc10a748badbfa |
| Wan 图片主体 | HTTP 400 FieldInvalid，input.prompts.material；提示 video subjects are not supported | trace b1a852330acbfe3c1840afb3aa2518e3 |

本地服务参考代码 q31_drama.go 的 prompt switch 也没有 material 分支，与 SD2.5 返回提示一致。该副本仍含 9 图/3 视频旧限制，但真实服务已接受 10 图及 4 视频，因此不能将本地副本当作部署版本。没有据此撤回文档明确要求的主体支持，也没有通过 CLI 猜测展开主体来绕过服务端规则。

## 第二轮真实边界核验

- SD2.5 10 图提交成功：3569231877771920，最近状态 preparation。
- SD2.5 4 段各 5 秒视频（合计 20 秒）提交成功：3569231992720854，最近状态 preparation。
- SD2.5 30 图使用同一个有效 ssupload 引用重复输入，CLI 通过；HTTP 400 FieldInvalid，fields=input，message=invalid field input: embedded message failed validation。此响应未返回 trace ID。
- SD2.5 31 图在请求前被 CLI 拒绝，message=character2video allows max 30 images, got 31。
- 本地 infer/v1/task.proto 的 Task.Input.prompts 仍标记 max_items=23，可能与 30 图被嵌套结构校验拒绝相关，但该副本不能证明部署中的具体限制。

## 尚未验证完成

- 三个模型真实生成产物尚未全部成功；不能将返回 task_id 视为生成通过。
- SD2.5/Wan 图片主体被测试服务拒绝，文本主体及主体展开/尺寸校验尚无成功的真实服务证据。
- 30 图真实输入被服务端拒绝；10 视频数量以本地和 mock 测试为依据。注意每段至少 4 秒、合计最多 30 秒意味着 10 段无法同时满足时长要求，CLI 按文档分别保留数量和时长校验。其他媒体格式/大小的全部组合未穷举真实生成。

## 输出边界真实报价矩阵

18 个组合均返回 ok=true、can_submit=true。报价通过只证明输出参数与价格链路，不证明参考输入或实际生成成功。

| 模型 | 输出时长 | 分辨率 | 积分 |
| --- | --- | --- | --- |
| 3.2_a | 4 | 480p | 34 |
| 3.2_a | 15 | 480p | 128 |
| 3.2_a | 4 | 720p | 68 |
| 3.2_a | 15 | 720p | 255 |
| 3.2_a | 4 | 1080p | 180 |
| 3.2_a | 15 | 1080p | 676 |
| viduq3.1-drama-std | 4 | 480p | 51 |
| viduq3.1-drama-std | 30 | 480p | 383 |
| viduq3.1-drama-std | 4 | 720p | 102 |
| viduq3.1-drama-std | 30 | 720p | 765 |
| viduq3.1-drama-std | 4 | 1080p | 272 |
| viduq3.1-drama-std | 30 | 1080p | 2040 |
| viduq2-drama-std | 2 | 480p | 10 |
| viduq2-drama-std | 30 | 480p | 144 |
| viduq2-drama-std | 2 | 720p | 16 |
| viduq2-drama-std | 30 | 720p | 240 |
| viduq2-drama-std | 2 | 1080p | 32 |
| viduq2-drama-std | 30 | 1080p | 480 |

最终状态复核：5 个 SD 提交任务仍为 preparation；Wan 视频参考任务为 failed / InternalServiceFailure。CLI 无法修复测试服务的请求校验或生成执行状态，需服务端确认部署及执行链路后复测。
