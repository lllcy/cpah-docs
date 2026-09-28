# CPAH Docs

[![CI](https://github.com/lllcy/cpah-docs/actions/workflows/ci.yml/badge.svg)](https://github.com/lllcy/cpah-docs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/lllcy/cpah-docs?display_name=tag)](https://github.com/lllcy/cpah-docs/releases)

一个支持 Windows 和 macOS 的桌面工具：递归监控多个目录，将 Office、PDF、图片和文本转换为 Markdown，并在输出目录中保持原有文件夹结构。可选的 Agent 文档分类会从用户配置的候选类别中选择标签，并写入 Markdown 的 `cpah_categories` YAML 字段。

## 主要功能

- 多组“监控目录 → 输出目录”，新增、修改和删除会持续同步。
- 目录监听、格式转换和 Agent 分类分别控制：监听只发现文件并放入待执行，转换和分类分别消费自己的队列。
- 本地转换：Office（含旧版 DOC / XLS / PPT、宏启用文档）、OpenDocument、EPUB、RTF、CSV 使用 anydoc 0.2.4；HTML、HTM、TXT 保留 anytomd；Markdown 原样同步。
- PDF 优先使用 anydoc 本地提取；只有解析器明确返回需要 OCR 时，才将整份 PDF 交给 MinerU。损坏、加密或超限等错误不会触发上传。PNG、JPG、JPEG、WEBP、BMP 继续使用 MinerU。
- 需要 OCR 的大型 PDF 会自动预检：超过 200 页时按页段提交，超过 MinerU 200 MB 限制时在本地无损拆分；原文件最大支持 512 MiB，最终仍只生成一份 Markdown。
- 可按目录配置单分类或多分类候选类别，支持从 JSON 批量导入；在“分类任务”页独立开始、停止、重试并查看 Token 用量。
- SQLite 保存任务状态，程序重启后恢复队列和 MinerU 轮询；帮助页提供离线运行诊断和脱敏报告。
- 本地 PDF 保留解析器输出的文字、表格和图片占位，不导出 PDF 图片附件；Office 等格式的可读取内嵌图片仍写入 `.assets`。anydoc 使用固定版本源码，补充渲染器导出与 Excel 图片提取，详见 [来源与补丁记录](src-tauri/vendor/anydoc/UPSTREAM.md)。
- Token 与 API Key 保存到系统凭据库（Windows 凭据管理器或 macOS 钥匙串）；关闭窗口后驻留 Windows 系统托盘或 macOS 菜单栏。

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

### 批量导入候选类别

在“监控目录”中选择目标目录，点击“Agent 文档分类”旁的“导入 JSON”，在弹出的编辑框中粘贴纯 JSON。顶层是数组，每项只能包含 `name` 和 `description`：

```json
[
  {
    "name": "PDF凭证",
    "description": "PDF格式的会计记账凭证及附件。"
  }
]
```

`name` 必须是非空且不重复的字符串，不能使用保留名称“未分类”；`description` 必须是字符串，可以为空。粘贴内容不要包含 Markdown 代码围栏，也不需要 ID、分类模式或启用状态；旧字段 `value` 不受支持。整批校验失败时不会改动原有类别。新类别追加，同名且说明相同的跳过；说明不同时可统一保留原说明、覆盖说明或取消。确认后会自动保存分类规则，但不会启动分类或重跑历史任务。

## 隐私与运行边界

- CPAH Docs 不包含遥测、广告或后台更新检查。
- 本地转换不上传文件。
- 使用 MinerU 时，待解析文档会发送到设置中的 MinerU 服务。
- 开启文档分类时，Markdown 内容会发送到设置中选择的大语言模型或决策模型服务。
- MinerU Token 和 Agent API Key 不写入 `settings.json`，仅保存在系统凭据库（Windows 凭据管理器或 macOS 钥匙串）。
- 输入和输出目录不能相同、互相包含或与其他配置交叉；目录符号链接不会被跟随。
- 云端上传、下载、ZIP 条目及解压内容均设置大小和条目数量安全上限，拒绝路径穿越。
- 普通 Markdown、转换结果和设置文件使用同目录临时文件原子替换，设置损坏时会尝试恢复上一份有效备份。
- Release 运行日志写入应用数据目录的 `logs` 子目录，按 2 MiB 轮转并保留最近 3 份；日志不记录凭据或文档正文。

## 使用与分发

从 [GitHub Releases](https://github.com/lllcy/cpah-docs/releases) 下载对应平台的产物即可运行，不需要 Python、Node.js 或 Rust：

- Windows x64：`CPAH-Docs-v<版本>-windows-x64.exe`。需要 Microsoft Edge WebView2 Runtime，现代 Windows 10/11 通常已自带。
- macOS 通用版：`CPAH-Docs-v<版本>-macos-universal.dmg`，同时支持 Apple Silicon 和 Intel Mac。打开 DMG 后将 CPAH Docs 拖入“应用程序”。

当前公开构建尚未使用商业/Apple Developer 证书签名，macOS 版本也尚未公证。Windows SmartScreen 可能显示“未知发布者”；macOS Gatekeeper 可能要求在 Finder 中右键选择“打开”，或在“系统设置 → 隐私与安全性”确认打开。请只从本仓库 Release 下载，并使用同一页面提供的 `SHA256SUMS.txt` 校验文件完整性。

首次启动后：

1. 创建一个“原始文档”文件夹，作为监控目录，用来放 Word、PDF、Excel、PPT 等待转换文件。
2. 再创建一个独立的“Markdown 输出”文件夹，用来接收转换结果和附件资源。两个目录不能相同或互相包含。
3. 打开“监控目录”，分别选择这两个文件夹并保存；监听会扫描文件并放入“待执行”，不会立刻转换。
4. 在“格式说明”选择需要处理的扩展名。
5. 图片和需要 OCR 的 PDF 需要在“设置”保存 MinerU Token；普通文本 PDF 与 Office 无需 Token。
6. 如需分类，在“设置 → 分类模型”选择“大语言模型”或“决策模型”，填写对应服务的地址、模型名称及 API Key 并测试连接，再为目录逐项添加候选类别，或粘贴符合上述格式的 JSON。
7. 确认待执行数量后，在“转换任务”点击“开始转换”；需要分类时，再在“分类任务”点击“开始分类”。

全新安装的默认状态是：目录监听运行、格式转换停止、Agent 分类停止。已有用户升级后继续保持设置文件中保存的状态。每个目录的“启用”是该目录参与监听、转换、分类和索引的总开关；“格式说明”中的扩展名开关只决定哪些文件会进入转换队列。

左上角可收起主导航为图标栏，收起状态会记住；鼠标悬停图标可查看名称。已保存的 Token 和 API Key 显示密码占位提示，输入新值即可替换。底部错误提示可关闭，同一错误不会因自动刷新反复弹出，点击顶部异常状态可重新查看；运行错误与连接错误分别显示。若索引输出目录已移动或删除，请在对应目录的“目录设置”更新输出路径，或停用不再使用的目录。

### 浏览目录文件

在“监控目录”选择已保存的目录后，默认进入“文件”页签；路径、删除策略和分类规则放在“目录设置”。文件树按输入目录的实际内容显示，包括空文件夹、未加入任务、未启用格式和不支持转换的文件；文件夹按需展开，链接显示但不跟随。状态关联完整转换记录，不受任务页最近 5,000 条记录的限制，PDF 分片不会混入文件树。

搜索文件名和筛选转换状态会遍历所选输入目录的全部子文件夹，并保留匹配文件的目录层级。搜索结果的任务状态继续更新，新增匹配文件通过重新搜索或点击“刷新文件”显示。普通浏览会刷新已展开目录，并在当前会话记住展开位置；大量文件采用可见行渲染。

点击文件可查看路径、大小、转换进度和错误，打开原文件、所在文件夹或已有结果，并用详情底部的“转换 / 分类”按钮处理单个文件。已完成文件若大小或修改时间改变，会提醒“源文件已变化”；输出不存在或无法访问时提示“结果缺失”。浏览、搜索和刷新不会加入任务、生成输出或调用模型，监听停止或目录停用时仍可浏览。详情中的文件操作会恢复对应队列，优先处理所选文件，结束或失败后继续其他同类任务；已执行的任务不会被中断。转换和分类的暂停状态分别控制，目录监听状态不变。分类要求已完成转换、结果存在且源文件未变化，并已启用目录分类和配置模型。任务页原有“重试”仍沿用队列的开始/停止状态。

两种分类模型共用目录的候选类别、单分类/多分类规则和任务队列：

| 模型类型 | 接口与配置 |
| --- | --- |
| 大语言模型（默认） | OpenAI Chat Completions Tool Calling 兼容接口；沿用原有读取文档和提交标签的工具调用流程。 |
| 决策模型 | System One 兼容接口。阿里百炼示例 Base URL：`https://<业务空间ID>.cn-beijing.maas.aliyuncs.com/compatible-mode/v1`，模型：`decision-model-preview`；Jev 示例：`https://api.typesafe.ai/v1`，模型：`jev-latest`。也支持完整的 `/systemone` 地址。 |

切换类型后需填写对应服务的连接参数并保存；当前保存一组分类模型配置，API Key 留空会继续使用已保存的 Key。已有配置未记录模型类型时按大语言模型处理。保存或测试连接不会自动启动已停止的分类队列；已完成文档需要在分类任务页手动重新分类。

决策模型由程序提交最多前 32 KiB 的 Markdown（去除已有 `cpah_categories`），长文档会标记为截断，读取量可在任务详情查看。单分类使用 `choice`（最多 254 个自定义类别，另加“未分类”）；多分类对每个标签使用独立的 `noul` 判断，每批最多 16 个问题，匹配概率高于 0.5 时选中，没有命中则写入“未分类”。0.5 是选择边界，不代表准确率保证。所有批次成功且结果合法后才写入文件；仍会检查原文件是否被修改，并保留其他 YAML 字段及正文。连接测试只提交内置示例，验证单分类与多标签判断能力。

点击窗口关闭按钮只会隐藏到 Windows 系统托盘或 macOS 菜单栏；需要完全关闭时，请使用图标菜单中的“退出”。

## 开发与验证

应用使用“知识方块”图标，源图为 `app-icon.png`。更新后运行 `npm run tauri icon app-icon.png` 生成 `src-tauri/icons/` 中的各平台资源，再将 `128x128@2x.png` 同步到 `public/app-icon.png`，用于应用内标识和浏览器图标。图案说明与生成提示词见 [图标说明](docs/app-icon.md)。

开发环境需要 Node.js 24.15+ 和 Rust 1.97+。Windows 还需要 Visual Studio Installer 的“使用 C++ 的桌面开发”（MSVC x64/x86 与 Windows SDK）；macOS 需要 Xcode Command Line Tools 或完整 Xcode。

```shell
npm ci
npm run tauri dev
```

验证与生产构建：

```shell
npm run build
npm run test:files
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run tauri build
```

执行完整发布检查、校验第三方许可清单，并生成当前平台的发布产物与 SHA-256：

```shell
npm run release
```

Windows 生成 `release/CPAH-Docs-v<版本>-windows-x64.exe`；macOS 生成 `release/CPAH-Docs-v<版本>-macos-universal.dmg`。发布目录同时包含 `LICENSE.txt`、`THIRD_PARTY_LICENSES.md` 和 `SHA256SUMS.txt`，`src-tauri/target` 只是本机构建缓存。推送与版本一致的 `v*` 标签后，GitHub Actions 会并行构建 Windows x64 和 macOS 通用版，并在两个构建都成功后发布同一个 GitHub Release。

真实 MinerU 回归测试需要设置 `CPAHDOCS_MINERU_E2E` 和 `CPAHDOCS_MINERU_TOKEN`（也可使用应用已保存的 Token）。大型 PDF 的两个忽略用例分别使用 `CPAHDOCS_MINERU_PAGE_RANGES_E2E`（超过 200 页且不超过 200 MB）和 `CPAHDOCS_MINERU_PHYSICAL_E2E`（200–512 MiB）。真实 Agent Tool Calling 回归测试需要设置 `CPAHDOCS_AGENT_BASE_URL`、`CPAHDOCS_AGENT_MODEL` 和 `CPAHDOCS_AGENT_API_KEY`，再运行被忽略的 E2E 用例。所有真实 E2E 资料只能放在 Git 忽略的本机目录中。

第三方许可清单可通过 `node scripts/generate-third-party-licenses.mjs` 重新生成；Windows 也可使用 PowerShell 包装脚本 `scripts/generate-third-party-licenses.ps1`。生成时需要 [cargo-about 0.9.1](https://github.com/EmbarkStudios/cargo-about/releases/tag/0.9.1)。

## 数据位置

设置、上一份有效设置备份、SQLite 任务数据库和滚动日志位于 Tauri 的应用数据目录 `com.cpah.docs`（Windows 的用户应用数据目录或 macOS 的 `~/Library/Application Support/com.cpah.docs`）。从早期内部版本首次升级时，程序会迁移旧目录和系统凭据。生成的 Markdown、分层索引与附件只写入对应监控配置的输出目录。

## 开源与安全

- 项目原创代码采用 [MIT License](LICENSE)，第三方组件许可与声明见 [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)。
- 参与开发请阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。
- 安全漏洞请按照 [SECURITY.md](SECURITY.md) 私下报告，不要发布包含凭据或真实文档的公开 Issue。
- CI 会在 Windows 和 macOS 上自动构建前端，并执行 Rust 格式、测试和 Clippy 检查；依赖安全检查每周运行。

CPAH Docs 与 MinerU、OpenAI 及其他模型服务提供方不存在隶属或官方合作关系。使用云端解析或模型分类前，请自行确认对应服务条款、数据处理规则与费用。
