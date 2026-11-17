# Kiro 额度适配

通过本机 Kiro 登录缓存查询 GetUsageLimits。只读登录文件，不保存令牌副本，不回写，不使用 refreshToken 自动续期。

## 登录与身份

默认路径为 `%USERPROFILE%\.aws\sso\cache\kiro-auth-token.json`，也可选择其他登录 JSON。连接与重连在后台核验成功、配置保存成功后生效。取消或关闭窗口后，晚到结果不会保存账号。

- `accessToken`：Bearer 凭据，仅发往下列固定官方端点。
- `profileArn`：有值时随请求发送；缺少时允许空请求体，兼容企业 IdC 登录。
- `region`：与 ARN 区域一致时使用；缺少时从 ARN 提取，两者都缺少时使用美国东部区域。
- `expiresAt`：RFC3339 格式的客户端续期时间，登录失效以服务端 401 / 403 为准。
- `userInfo.userId`：使用响应中的用户标识绑定账号，配置和缓存仅保存其 SHA-256 摘要。缺少可核验用户标识时不显示额度。

客户端注册 `clientIdHash`、profile ARN 和访问令牌均不作为用户身份。原版本尚未核验的身份需要重新连接；已经核验的账号重连时必须仍是原用户。重连保留账号 ID、备注、配色及常驻设置。

## 请求边界

| 区域 | 固定端点 |
| --- | --- |
| us-east-1 | https://codewhisperer.us-east-1.amazonaws.com/ |
| eu-central-1 | https://q.eu-central-1.amazonaws.com/ |

其他区域暂不支持，不根据登录文件拼接任意主机。请求使用 POST、`application/x-amz-json-1.0` 和 `X-Amz-Target: AmazonCodeWhispererService.GetUsageLimits`。禁用重定向，使用既有代理与请求超时；解压后响应上限为 1 MiB。

401 / 403 提示重新登录；429 保留服务端 Retry-After 并按账号退避。查询期间登录文件发生变化时丢弃结果。Kiro 请求独立执行，避免阻塞其他额度服务。

## 点数与时间

从 `usageBreakdownList` 的 CREDIT 项读取套餐用量。优先精度字段，缺少精度字段时兼容对应整数；负数、非有限值及损坏字段不按零处理。

- 月度套餐：总额减去套餐内消耗。套餐内消耗使用总用量减超额用量，不因赠送列表是否非空切换公式。
- 试用：仅计入 ACTIVE 且未过期的 `freeTrialInfo`。
- 赠送 / 加购：仅计入 ACTIVE 且未过期的 `bonuses`，兼容单个对象和数组，逐项保留名称及到期时间。
- 可用点数：套餐、有效试用和有效赠送 / 加购的剩余量合计。
- 超额上限余量：启用超额且 CREDIT 项返回明确点数上限时单独显示，不加进可用点数；不把 `capForWorkloadUsage` 等金额字段换算成点数。

月度重置与试用、赠送的到期时间分别显示。秒与毫秒时间戳均做范围校验。额度环中心显示 Kiro 标识，剩余点数显示在卡片右侧及常驻栏图标右侧；设置页保留各积分池明细。旧的其他供应商缓存可以继续读取。

## 参考

- [Kiro 计费与试用](https://kiro.dev/docs/billing/)
- [Kiro 加购点数](https://kiro.dev/docs/billing/add-on-credits/)
- [AWS 官方 GetUsageLimits 响应模型](https://github.com/aws/amazon-q-developer-cli/blob/15cc8f3cd18c4272925ce1c7053268eedff1ea0a/crates/amzn-codewhisperer-client/src/operation/get_usage_limits/_get_usage_limits_output.rs)
- [AWS 官方用量字段模型](https://github.com/aws/amazon-q-developer-cli/blob/15cc8f3cd18c4272925ce1c7053268eedff1ea0a/crates/amzn-codewhisperer-client/src/types/_usage_breakdown.rs)

真实账号还需核对 Kiro 客户端中的点数、用户身份、区域及到期时间。合成测试不代表所有登录形态均已完成真机验收。
