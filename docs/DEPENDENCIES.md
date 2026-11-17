# 依赖与第三方许可

岛仔原创代码采用 MIT。第三方依赖、字体、图标及品牌标识遵循各自许可；MIT 不授予第三方商标权。完整归属及许可文本见 [THIRD_PARTY_NOTICES.txt](../THIRD_PARTY_NOTICES.txt)。

## 本地补丁

- `vendor/eframe`：基于 0.31.1，处理 Windows WGL 上下文复用、岛体缩放和窗口焦点。
- `vendor/egui`：基于 0.31.1，为不同缩放倍率维护独立的字体纹理。
- `vendor/imap`：基于 2.4.1，关闭 `nom` 默认功能并保留 `std`，移除邮件协议解析不需要的 `lexical-core` 浮点解析依赖。服务器拒绝进入 IDLE 时直接回到轮询。
- `vendor/ureq`：基于 2.12.1。HTTP 代理的明文请求保持 absolute-form，HTTPS 的 CONNECT 隧道内改用 origin-form。原 MIT 与 Apache-2.0 许可证随源码保留。

- `vendor/imap-proto`：兼容腾讯企业邮箱 CAPABILITY 响应末尾的空格，保留 IMAP4rev1 校验和下一条响应边界。

## 安全检查

2026 年 11 月 3 日发布前，对 Cargo.lock 的 registry 依赖查询了 OSV 公告库，并运行完整测试。已移除受 [RUSTSEC-2023-0086](https://rustsec.org/advisories/RUSTSEC-2023-0086.html) 影响的 `lexical-core 0.7.6`。

当前仍有两项上游停止维护提示：

| 依赖 | 公告 | 当前处理 |
| --- | --- | --- |
| paste 1.0.15 | [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html) | 随现有 UI 依赖使用，后续跟随上游升级 |
| ttf-parser 0.25.1 | [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) | 随字体栈使用，后续评估维护中的替代版本 |

这两项属于维护状态提示，并非本次检查确认的可利用漏洞。公告库检查不能代替完整安全审计。`imap-proto` 还会产生 Rust 未来兼容性警告，当前编译和测试通过。
