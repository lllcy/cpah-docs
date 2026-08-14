# CPAH Docs

[![CI](https://github.com/lllcy/cpah-docs/actions/workflows/ci.yml/badge.svg)](https://github.com/lllcy/cpah-docs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/lllcy/cpah-docs?display_name=tag)](https://github.com/lllcy/cpah-docs/releases)

一个 Windows 单文件桌面工具：递归监控多个目录，将 Office、PDF、图片和文本转换为 Markdown，并在输出目录中保持原有文件夹结构。可选的 Agent 文档分类会从用户配置的候选类别中选择标签，并写入 Markdown 的 `cpah_categories` YAML 字段。

## 主要功能

- 多组“监控目录 → 输出目录”，新增、修改和删除会持续同步。
- 目录监听、格式转换和 Agent 分类分别控制：监听只发现文件并放入待执行，转换和分类分别消费自己的队列。
- 本地转换：Markdown 原样同步；DOCX、XLS、XLSX、PPTX、HTML、HTM、CSV、TXT 使用纯 Rust / anytomd。
- 云端解析：PDF、PNG、JPG、JPEG、WEBP、BMP、旧版 DOC、旧版 PPT 使用 MinerU。
- 可按目录配置单分类或多分类候选类别；在“分类任务”页独立开始、停止、重试并查看 Token 用量。
- SQLite 保存任务状态，程序重启后恢复队列和 MinerU 轮询；帮助页提供离线运行诊断和脱敏报告。
- Token 与 API Key 保存到 Windows 凭据管理器；关闭窗口后驻留系统托盘。

输出目录会镜像输入目录的子文件夹（包括空文件夹）：

```text
输入/reports/示例.pdf
输出/reports/示例.md
输出/reports/示例.assets/
输出/index.md
输出/reports/index.md
```

同目录存在同名不同格式文档时，为避免覆盖，会回退为 `示例.pdf.md`、`示例.docx.md`。Markdown 文件会按字节原样同步；Agent 分类启用后，分类步骤才会单独更新其 YAML。

输出根目录以及包含 Markdown 的每层子目录都会自动维护一个 `index.md`。根索引和子索引提供面包屑导航、文件夹入口、当前目录树的标签视图、待分类文档与最近更新。索引只扫描本地路径和 YAML，不调用模型、不消耗 Token；CPAH Docs 自身生成的索引也不会进入分类队列。

如果任意输出子目录原本已有 `index.md`，程序会保留用户内容，只更新以下托管标记之间的区域。文档删除后，纯自动生成且已无内容的子索引会删除；包含用户正文的索引只移除托管区域：

```markdown
<!-- cpah:index:start -->
自动生成的目录与标签索引
<!-- cpah:index:end -->
```

分类结果示例：

```yaml
---
source: example.pdf
converter: mineru
cpah_categories:
  - 培训材料
  - AI审计
---
```

已有合法 YAML 的其他字段、注释、正文、UTF-8 BOM 和换行风格会保留。YAML 非法或分类期间文件被修改时不会覆盖原文件。

## 隐私与运行边界

- CPAH Docs 不包含遥测、广告或后台更新检查。
- 本地转换不上传文件。
- 使用 MinerU 时，待解析文档会发送到设置中的 MinerU 服务。
- 开启 Agent 分类时，Markdown 内容会发送到设置中的 OpenAI Chat Completions Tool Calling 兼容服务。
- MinerU Token 和 Agent API Key 不写入 `settings.json`，仅保存在当前 Windows 用户的凭据管理器。
- 输入和输出目录不能相同、互相包含或与其他配置交叉；目录符号链接不会被跟随。
- 云端上传、下载、ZIP 条目及解压内容均设置大小和条目数量安全上限，拒绝路径穿越。
- 普通 Markdown、转换结果和设置文件使用同目录临时文件原子替换，设置损坏时会尝试恢复上一份有效备份。
- Release 运行日志写入应用数据目录的 `logs` 子目录，按 2 MiB 轮转并保留最近 3 份；日志不记录凭据或文档正文。

## 使用与分发

从 [GitHub Releases](https://github.com/lllcy/cpah-docs/releases) 下载 `CPAH-Docs-v<版本>-windows-x64.exe` 后即可运行，不需要 Python 或 Node.js。目标电脑需要 Microsoft Edge WebView2 Runtime；现代 Windows 10/11 通常已自带。

当前公开构建尚未进行商业代码签名，Windows SmartScreen 可能显示“未知发布者”。请只从本仓库 Release 下载，并使用同一页面提供的 `SHA256SUMS.txt` 校验文件完整性。

首次启动后：

1. 创建一个“原始文档”文件夹，作为监控目录，用来放 Word、PDF、Excel、PPT 等待转换文件。
2. 再创建一个独立的“Markdown 输出”文件夹，用来接收转换结果和附件资源。两个目录不能相同或互相包含。
3. 打开“监控目录”，分别选择这两个文件夹并保存；监听会扫描文件并放入“待执行”，不会立刻转换。
4. 在“格式说明”选择需要处理的扩展名。
5. PDF、图片或旧版 Office 需要在“设置”保存 MinerU Token。
6. 如需分类，在“设置”配置支持 Tool Calling 的模型，再为目录添加候选类别。
7. 确认待执行数量后，在“转换任务”点击“开始转换”；需要分类时，再在“分类任务”点击“开始分类”。

全新安装的默认状态是：目录监听运行、格式转换停止、Agent 分类停止。已有用户升级后继续保持设置文件中保存的状态。每个目录的“启用”是该目录参与监听、转换、分类和索引的总开关；“格式说明”中的扩展名开关只决定哪些文件会进入转换队列。

点击窗口关闭按钮只会隐藏到系统托盘；需要完全关闭时，请使用托盘菜单中的“退出”。

## 开发与验证

开发环境需要 Node.js 24+、Rust 1.97+，以及 Visual Studio Installer 的“使用 C++ 的桌面开发”（MSVC x64/x86 与 Windows SDK）。

```powershell
npm ci
npm run tauri dev
```

验证与生产构建：

```powershell
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run tauri build
```

执行完整发布检查并生成单文件 EXE 与 SHA-256：

```powershell
npm run release
```

产物位于 `release/CPAH-Docs-v<版本>-windows-x64.exe` 和 `release/SHA256SUMS.txt`；`src-tauri/target` 只是本机构建缓存。

真实 Agent Tool Calling 回归测试需要设置 `CPAHDOCS_AGENT_BASE_URL`、`CPAHDOCS_AGENT_MODEL` 和 `CPAHDOCS_AGENT_API_KEY`，再运行两个被忽略的 E2E 用例。所有真实 E2E 资料只能放在 Git 忽略的本机目录中。

第三方许可清单可通过 `scripts/generate-third-party-licenses.ps1` 重新生成，生成时需要 [cargo-about 0.9.1](https://github.com/EmbarkStudios/cargo-about/releases/tag/0.9.1)。

## 数据位置

设置、上一份有效设置备份、SQLite 任务数据库和滚动日志位于 Tauri 的应用数据目录 `com.cpah.docs`。从早期内部版本首次升级时，程序会迁移旧目录和 Windows 凭据。生成的 Markdown、分层索引与附件只写入对应监控配置的输出目录。

## 开源与安全

- 项目代码采用 [MIT License](LICENSE)，第三方组件许可证见 [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)。
- 参与开发请阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。
- 安全漏洞请按照 [SECURITY.md](SECURITY.md) 私下报告，不要发布包含凭据或真实文档的公开 Issue。
- CI 会在每次提交和 Pull Request 上自动构建前端，并执行 Rust 格式、测试和 Clippy 检查；依赖安全检查每周运行。

CPAH Docs 与 MinerU、OpenAI 及其他模型服务提供方不存在隶属或官方合作关系。使用云端解析或模型分类前，请自行确认对应服务条款、数据处理规则与费用。
