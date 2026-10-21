<p align="center"><img src="assets/branding/repository-banner.svg" alt="Glim — Mac 全能文件内容查看器。文档、数据、图片和代码，一个工作空间。免费开源，支持 Apple Silicon 与 Intel。" width="100%" /></p>

# Glim — Mac 全能文件内容查看器

Glim 是免费开源的 Mac 全能文件内容查看器。Markdown、PDF、图片、JSON、CSV 和源代码，都可以放在同一个工作空间里阅读。

<p align="center">免费开源 · macOS · Apple Silicon 与 Intel · <a href="README.md">English</a> / <b>简体中文</b></p>

<p align="center"><a href="https://glimapp.github.io/">Website / 官网</a> · <a href="https://github.com/glimapp/glim/releases/latest"><strong>下载 Glim →</strong></a></p>

<p align="center"><a href="https://github.com/glimapp/glim/releases/latest">最新发布 · 更新说明与校验文件</a> · <a href="#下载与安装">安装帮助</a></p>

<p align="center"><sub>Mac 下载包目前尚未经过 Apple 公证，首次打开可能需要在系统设置中放行。<a href="#macos">查看步骤</a></sub></p>

<p align="center">
  <a href="https://github.com/glimapp/glim/releases"><img src="https://img.shields.io/github/downloads/glimapp/glim/total?style=flat-square&amp;color=268879&amp;label=%E6%80%BB%E4%B8%8B%E8%BD%BD%E9%87%8F" alt="发布附件总下载量" /></a>
  <a href="https://github.com/glimapp/glim/actions/workflows/ci.yml"><img src="https://github.com/glimapp/glim/actions/workflows/ci.yml/badge.svg" alt="桌面版构建状态" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="MIT 许可证" /></a>
  <a href="https://github.com/glimapp/glim/stargazers"><img src="https://img.shields.io/github/stars/glimapp/glim?style=social" alt="GitHub Star 数量" /></a>
</p>

## 文档、数据和代码，一个工作空间

<p align="center"><img src="assets/readme/workspace.png" alt="Glim 重新设计的 macOS 工作空间：左侧文件树、多个文档标签、居中的工作空间搜索，以及包含内嵌图示的 12-factor-agents Markdown 文档。" width="100%" /></p>

紧凑界面将文档标签与可调整宽度的文件侧栏放在一起，工作空间搜索位于标题栏，预览与源码切换按钮随手可用。截图展示的是公开的 [12-factor-agents](https://github.com/humanlayer/12-factor-agents) 文档；其中的正文与插图属于该项目。

[下载 Glim](https://github.com/glimapp/glim/releases/latest) · [投票：下一步加什么格式](https://github.com/glimapp/glim/discussions/28) · [提出格式需求](https://github.com/glimapp/glim/issues/new?template=feature_request.yml)

| 想查看什么 | Glim 提供什么 |
| --- | --- |
| Markdown 文档 | 阅读 Mermaid 图表、表格和 LaTeX 公式，也可切回源码做小修改 |
| JSON 数据 | 逐层展开对象和数组，随时切回原文核对 |
| CSV / TSV 表格 | 按表格阅读，查看多行单元格完整内容，或切回源码 |
| PDF、图片与 Office 文档 | 在工作空间标签中只读预览；Office 排版取决于 macOS |
| EPUB 电子书与 SQLite 数据库 | 带书签阅读电子书，或通过只读快照浏览本地数据库表 |

## 支持的文件格式

<a href="docs/file-format-support.zh-CN.md">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/readme/formats-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="assets/readme/formats.svg" />
    <img src="assets/readme/formats.svg" alt="Glim 支持的代表文件格式；剑号为基础预览" width="100%" />
  </picture>
</a>

*代表格式一览：† 基础预览；具体能力与限制见下表及完整格式文档。*

| 内容 | 可以做什么 |
| --- | --- |
| 文档 | Markdown、静态 HTML、普通 UTF-8 文本、图表与公式 |
| 原生文档 | PDF、Word、Excel、PowerPoint、Keynote、Pages、Numbers、RTF 只读预览 |
| 图片 | PNG、JPEG、WebP、GIF 动画、SVG、多页 TIFF、BMP、ICO、ICNS、EXR / HDR |
| 媒体与工具 | 音视频播放与文件夹队列；字体及字体集合；SQLite 表、结构和关联 WAL 文件 |
| 思维导图 | XMind 只读预览，支持工作表切换、主题折叠、缩放与详情 |
| 代码与配置 | Rust、Python、JS/TS、Go、C/C++、Java、Swift、Ruby、INI 等；语法高亮与基础文本编辑 |
| 数据 | JSON / JSONC、CSV / TSV、YAML、TOML、SQL、GeoJSON 和 Notebook；结构树、表格或文本预览 |
| AI 模型元数据 | safetensors 张量名称、shape、精度和参数量，仅读取头部 |
| 探索格式首轮 | AsciiDoc / Org / RST、字幕、TSV / NDJSON / JSON5、D2 / Pikchr / Vega、OBJ / STL、归档目录与部分文本预览等 |

**平台范围：** Glim 仅维护 Apple Silicon 与 Intel 两种 macOS 架构，共用本文描述的文件阅读器与 Git 工作区。

<details>
<summary>格式详情与限制</summary>

Markdown 提供 Mermaid（含流程图、子图、Journey、Radar、ZenUML）、Graphviz/DOT、PlantUML（含部分 Salt 与 Chen ER 语法）、flowchart.js、WaveDrom、LaTeX，以及 GFM 表格、任务列表、脚注、GitHub Alerts 和 frontmatter。图表失败时保留源码。

较大或单行很长的文本使用只读分页。阅读器预算及具体影响见[共享阅读器边界](docs/reader-limits.zh-CN.md)。HTML 为静态预览，不执行脚本。归档支持目录及选定格式的文本预览，不提供通用解压或完整内容渲染；非 UTF-8 文本暂不提供文字解码。

Office 排版和音视频编码支持取决于 macOS。XLS/XLSX 显示本地表格及缓存公式值；完整样式、图表和复杂文档可通过默认应用查看。

</details>

[中文文档](docs/README.zh-CN.md) · [完整格式与限制](docs/file-format-support.zh-CN.md) · [未完成计划](docs/TODO.zh-CN.md)

## 下载与安装

在[最新 Release 页面](https://github.com/glimapp/glim/releases/latest)的 **Assets** 中，按设备选择对应安装包；版本号、更新说明和校验文件以该页面为准。

| 你的设备 | 安装包文件名后缀 |
| --- | --- |
| Mac · Apple Silicon（M 系列） | `-macos-arm64.dmg` |
| Mac · Intel | `-macos-x86_64.dmg` |

不确定 Mac 的类型？打开 **苹果菜单 → 关于本机**：看到 Apple M 系列芯片就选 Apple Silicon，看到 Intel 处理器就选 Intel。

### macOS

打开 DMG，将 Glim 拖入“应用程序”。目前使用 ad-hoc 签名，尚无 Developer ID 签名与 Apple 公证。若系统拦截，先尝试打开一次，再到 **系统设置 → 隐私与安全性 → 仍要打开**。请确认安装包来自本项目发布页，无需关闭系统安全保护。[Apple 官方说明](https://support.apple.com/zh-cn/102445)

[更新说明与 SHA-256 校验文件](https://github.com/glimapp/glim/releases/latest) · [历史版本](https://github.com/glimapp/glim/releases) · [从源码构建](docs/releasing.zh-CN.md)

## 参与项目

[报告问题](https://github.com/glimapp/glim/issues/new?template=bug_report.yml) · [申请格式或功能](https://github.com/glimapp/glim/issues/new?template=feature_request.yml) · [讨论与交流](https://github.com/glimapp/glim/discussions)，欢迎中文和英文。

[贡献指南](CONTRIBUTING.md#中文说明) · [构建说明](docs/releasing.zh-CN.md) · [未完成计划](docs/TODO.zh-CN.md) · [安全政策（英文）](SECURITY.md) · [MIT License](LICENSE)

## 贡献者

感谢每一位帮助 Glim 变得更好的贡献者。

<a href="https://github.com/glimapp/glim/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=glimapp/glim" alt="Glim 贡献者头像墙" />
</a>

## Star 增长曲线

<a href="https://star-history.com/#glimapp/glim&amp;Date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=glimapp/glim&amp;type=Date&amp;theme=dark" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=glimapp/glim&amp;type=Date" />
    <img src="https://api.star-history.com/svg?repos=glimapp/glim&amp;type=Date" alt="Glim 在 GitHub 上的 Star 增长曲线" width="100%" />
  </picture>
</a>

## 下载量增长曲线

<a href="https://github.com/glimapp/glim/tree/codex/download-stats">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/glimapp/glim/refs/heads/codex/download-stats/downloads-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/glimapp/glim/refs/heads/codex/download-stats/downloads.svg" />
    <img src="https://raw.githubusercontent.com/glimapp/glim/refs/heads/codex/download-stats/downloads.svg" alt="Glim 发布附件累计下载量曲线" width="100%" />
  </picture>
</a>

每日记录 GitHub 公开发布附件的累计下载次数，包含安装包、更新清单、校验文件及发布核验请求。从 **2026-10-12** 开始积累数据，不补造此前历史；下载次数不等于独立用户数或安装量。
