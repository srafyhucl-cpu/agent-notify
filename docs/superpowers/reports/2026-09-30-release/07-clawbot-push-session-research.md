# ClawBot 推送会话机制 · 外部调研与 backlog 建议

- 日期：2026-10-01（2.1.0 发版期间）
- 性质：外部资料调研 + 可选增强建议；**不阻塞本次发版**，不改变既有设计。
- 背景：2026-10-01 实测推送会话「断了又续」：08:27 用户消息恢复 → 08:45/08:50 推送 `Sent` → 08:52 平台回 `ret=-2 errmsg="prepare failed"` 拒绝准备会话 → 后续推送跳过、弹「推送已断」→ 用户再发消息可恢复（09:11 实测再次刷新上下文）。

## 1. 文档现状

- iLink（ClawBot 底层协议，`ilinkai.weixin.qq.com`）**没有公开的协议级官方文档**：微信开放文档站仅有《clawbot相关接口》（属「智能对话/知识助理」平台的封装接口，含扫码与通道重置，不含推送会话机制）。
- 协议行为主要由官方插件（`@tencent-weixin/openclaw-weixin`）行为与社区整理、第三方 SDK 文档交叉印证。

## 2. 机制要点（多来源交叉）

- `context_token` 是会话路由锚点：由**用户入站消息**携带、出站必须原样回传；**不带 token 的发送**存在两种口径（「返回 200 但不投递」vs「降级可投递」），不可靠。
- 保活窗口：多个来源称**每 24h 至少一次用户消息**才能续期，超时后下发的消息被丢弃。
- 主动推送配额：有实践总结为**每个 token 窗口约 10 条**（腾讯云社区文，被动回复不计），用尽即 `ret=-2`。
- 错误语义：`-14` = 会话过期（需重新扫码）；`ret=-2 errmsg="prepare failed"` = 平台拒绝准备会话（本次实测）；另有第三方把 `-2` 记作「参数错误/限流（约 7 条/5 分钟）」，口径不一。
- `notifystart/notifystop`：社区文档未公开说明，仅见于官方插件行为（生命周期信号，best effort）。

## 3. 社区方案汇总

| 方案 | 内容 | 代表 |
| --- | --- | --- |
| 用户保活 | 每 24h 给 Bot 发一条消息续期（可做定时提醒/自动化） | 腾讯云社区文、博客园实践 |
| 客户端容错 | 把 `ret=-2` 按「token 失效」处理：清 token、跳过/退避、等用户消息重建 | hermes-agent #35949/#96416、QwenPaw #4477 |
| 会话守卫 | 官方插件 `session-guard`：`-14` 冷却 60 分钟；连续失败退避 | QwenPaw #2875（对照分析） |
| 中间件产品化 | 「自动续期 + 消息持久化补投」（本质 = 保活 + 排队） | OpeniLink Hub |
| 无 token 降级 | 声称 iLink 接受降级发送 | hermes-agent #17228（与反向工程实测矛盾，不采信） |

## 4. 与 2.1.0 现状对照

- 2.1.0 已实现社区公认的正确姿势：识别 `ret/-2` 且 `errmsg` 含 `prepare failed` → 清上下文 → **跳过投放**（不积压、不刷屏）→ 渠道显示「推送已断」→ 用户一条消息即重建（08:27 / 09:11 两次实测）。
- 部分 SDK 曾把该情形误判为限流并导致推送永久失败（hermes-agent #35949）；我们的判定更严格（同时校验 ret/errcode 与 errmsg）。

## 5. backlog（发版后可选，不阻塞 2.1.0）

1. **失败推送排队补投**：会话恢复后补发跳过的通知（需节流，避免恢复瞬间刷屏）。
2. **推送配额节流/合并**：按「每 token 约 10 条」的已知约束合并同类通知。
3. **保活提示产品化**：>24h 无互动时在渠道页提示「发条消息保持推送」（现有「推送已断」提醒的自然延伸）。
4. **失败推送文案去重**（2026-10-01 真机观测，不阻塞 2.1.0）：失败推送里前缀「第 N 步失败：」与 `blockReason` 开头「第 N 步执行失败：」语义重复；且 `blockReason` 句尾的「。」与模板后缀相连出现「。。」。统一为一处原因即可。
5. **渠道账号硬删除**（2026-10-01 真机观测）：渠道页「退出账号」= 登出并置 `enabled=false`（对话框明示「历史保留」），**无硬删除**；停用账号行只能保留。删除停用账号 `clawbot-0a1900c1ff2d12f9` 未能通过桌面端完成（UI 无该能力），留 M7/后续决定（加「移除」或保留）。

## 6. 参考链接

- 微信开放文档《clawbot相关接口》：https://developers.weixin.qq.com/doc/aispeech/knowledge/openapi/Clawbotrelated.html
- wechatbot.dev 协议文档：https://www.wechatbot.dev/zh/protocol
- 腾讯云社区《iLink ClawBot 通道全打通实战》：https://developer.cloud.tencent.com/article/2729953
- 博客园《微信上线 ClawBot》：https://www.cnblogs.com/AlayaNeW/articles/19757767
- OpeniLink：https://openilink.com ；背景：https://openilink.com/docs/guide/background
- hermes-agent issues：#17228 / #35949 / #96416（github.com/NousResearch/hermes-agent）
- QwenPaw issues：#4477 / #2875（github.com/agentscope-ai/QwenPaw）
- yage.ai 分析：https://yage.ai/share/tencent-wechat-agent-entry-20260321.html
