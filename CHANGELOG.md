# 更新日志

本项目的重要变更会记录在这里，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## [1.1.2] - 2026-08-16

### 安全

- 阻止输出路径通过父级跳转、符号链接或 Windows junction 逃逸到配置目录之外。
- 远程 Agent 连接强制使用 HTTPS；仅本机回环地址允许 HTTP，并禁用 HTTP 重定向。
- 为 Agent 连接、请求、完整分类运行和 Tool Calling 探测增加超时上限。

### 修复

- 源目录离线、无权限或配置被禁用时不再清理已生成结果；源文件删除同步也会避开正在转换的任务。
- 将删除策略文案改为实际使用的输出目录 `.trash`，不再误称系统回收站。
- 第三方许可清单覆盖 Windows 与 macOS 依赖，并随两个平台的发布产物一同分发。

## [1.1.1] - 2026-08-15

### 新增

- 支持 macOS 运行，并提供同时兼容 Apple Silicon 与 Intel Mac 的通用 DMG。
- CI 与标签发布流程同时验证并生成 Windows、macOS 两个平台的产物。
- 增加本地 OpenAI 兼容服务模拟回归，覆盖 Agent Tool Calling 与 YAML 写入。

### 修复

- 移除前端依赖清单中写死的 Windows 原生绑定，使 macOS 可直接执行 `npm ci`。
- 修复旧版 XLS 在开启图片提取时被错误当作 OOXML ZIP 读取的问题。
- 将应用内凭据与路径提示改为 Windows/macOS 跨平台说明。

## [1.1.0] - 2026-08-14

### 新增

- 多组监控目录与输出目录的递归同步。
- Office、文本的本地 Markdown 转换，以及 MinerU 云端解析。
- 可独立暂停和恢复的目录监听、格式转换与 Agent 文档分类。
- 基于候选类别的单分类和多分类，结果写入 `cpah_categories` YAML。
- 输出目录的根索引与分层 `index.md`，支持按文件夹和类别浏览。
- 运行检查、脱敏诊断报告、滚动日志和异常退出后的任务恢复。
- Windows 单文件 EXE 发布脚本与 SHA-256 校验文件。

[1.1.0]: https://github.com/lllcy/cpah-docs/releases/tag/v1.1.0
[1.1.1]: https://github.com/lllcy/cpah-docs/releases/tag/v1.1.1
[1.1.2]: https://github.com/lllcy/cpah-docs/releases/tag/v1.1.2
