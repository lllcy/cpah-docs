# 更新日志

本项目的重要变更会记录在这里，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## [未发布]

## [1.3.0] - 2026-09-29

### 新增与改进

- Office 转换内核改为 anydoc 0.2.4，旧版 DOC / PPT 改为本地处理，并增加宏启用 Office、OpenDocument、EPUB、RTF 等格式；保留图片附件输出。
- HTML、HTM、TXT 继续使用 anytomd，Markdown 继续原样同步。
- PDF 优先本地提取，仅在明确需要 OCR 时整份转交 MinerU；保留大型 PDF 分片、断点恢复与重试，OCR 判断随源文件版本持久化。
- 更新格式设置、任务引擎显示和帮助说明；已有任务记录继续兼容。
- 候选类别支持粘贴 JSON 批量导入，整批校验并预览同名说明冲突，可统一保留、覆盖或取消；导入后自动保存，不启动历史分类。

### 修复

- 删除监控目录时取消转换、分类和索引任务，等待活动写入结束后清理任务记录与 MinerU 缓存，保留源文件和已生成文件。
- 防止旧窗口恢复已删除目录，或通过旧文件操作重新入队；决策模型迟到响应和 MinerU 解压写入同样受取消控制。
- 发布脚本支持自定义 Cargo 构建目录，并纳入 JSON 导入测试。

## [1.2.0] - 2026-09-28

### 新增

- 分类模型支持“大语言模型”和 System One 兼容的“决策模型”；决策模式最多提交前 32 KiB Markdown，并提供简短的模型说明。
- 监控目录增加完整文件树、文件名搜索、转换状态筛选和文件详情，支持未加入任务的文件与空文件夹。
- 文件详情新增单独转换与分类按钮：优先处理选中文件，结束后继续对应队列，支持恢复暂停中的队列。

### 改进与修复

- 主导航可收起为图标栏并记住状态；更换为透明背景的紫蓝色应用图标。
- 已保存的 Token / API Key 显示密码占位提示。
- 运行错误提示可关闭，同一错误不会随刷新反复弹出，顶部异常状态可重新查看。
- 单文件优先任务在失败、源文件移走或目录改变后释放队列；分类前检查转换结果与源文件状态。
- 更新 TLS 依赖 rustls，修复 RUSTSEC-2026-0285。

## [1.1.3] - 2026-08-16

### 修复

- 修复双平台 Release 工作流安装 `cargo-about` 时未启用 CLI，导致许可校验无法启动的问题。
- 发布脚本改用内置 .NET SHA-256 实现，兼容 GitHub Windows Runner 的 PowerShell 环境。

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
[1.1.3]: https://github.com/lllcy/cpah-docs/releases/tag/v1.1.3

[1.2.0]: https://github.com/lllcy/cpah-docs/releases/tag/v1.2.0
[1.3.0]: https://github.com/lllcy/cpah-docs/releases/tag/v1.3.0
