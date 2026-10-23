<p align="center"><img src="assets/branding/repository-banner.svg" alt="Glim — 文档、数据和代码，一个 Mac 工作空间。免费开源，支持 Apple Silicon 与 Intel。" width="100%" /></p>


# Glim — 文档、数据和代码，一个 Mac 工作空间

Glim 是免费开源的 macOS 文件查看器。打开一个项目文件夹，在同一个窗口里阅读 Markdown、PDF、图片、JSON、CSV 和源代码。预览与原文随时切换，检查本地数据，也能顺手修改文本。

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

## 从一个文件，读到整个目录

<p align="center"><img src="assets/readme/workspace.png" alt="Glim 阅读内嵌 GitHub Star 图表的 Markdown 报告，左侧为项目文件树，顶部同时打开 PNG、Markdown 和 SVG 标签。" width="100%" /></p>

报告、图表、生成图表的脚本和原始数据库，可以放在同一个工作空间。截图展示 Markdown 报告中的内嵌图表，相邻标签保留相关 PNG 和 SVG 文件。

| 要完成的事 | 在 Glim 中 |
| --- | --- |
| 阅读报告与项目说明 | 排版 Markdown 表格、图表和公式，跟随本地链接查看相关文件与图片 |
| 检查导出数据或 API 响应 | 展开 JSON 对象，按表格阅读 CSV/TSV，查看单元格完整内容 |
| 查看本地数据库 | 在只读 SQLite 快照中浏览表、筛选和排序记录、检查结构 |
| 查阅文档与参考资料 | 将 PDF、支持的 Office 文档、图片和电子书保留在工作空间标签中 |
| 修改错别字或配置值 | 基础编辑 UTF-8 文本，支持撤销重做、自动保存和 ⌘S 显式保存 |

### 接着查看报告背后的数据

<p align="center"><img src="assets/readme/sqlite.png" alt="Glim 浏览 SQLite 只读快照，左侧选择数据表，右侧按 drupal 筛选记录，并提供列排序和分页导航。" width="100%" loading="lazy" /></p>

SQLite 通过独立的只读快照打开。选择表或视图，搜索全部记录的所有字段、排序，或双击值查看完整内容；源数据库保持不变。

## 用自己的文件开始

1. **打开文件**（⌘O），或**打开文件夹**（⌘⇧O）。从侧栏浏览相关文件；标题栏搜索按名称或路径查找工作空间文件夹中的文件。
2. **选择适合内容的视图。** Markdown、HTML 和支持的数据阅读器提供预览／原文切换；文档、图片使用各自的阅读控件。
3. **需要时做一点修改。** 已有 UTF-8 文件会自动保存。⌘N 创建未命名笔记，第一次 ⌘S 时选择保存位置。

[使用指南](docs/user-guide.zh-CN.md) · [格式与兼容范围](docs/file-format-support.zh-CN.md) · [下载 Glim](https://github.com/glimapp/glim/releases/latest)

## 格式一览

<a href="docs/file-format-support.zh-CN.md">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/readme/formats-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="assets/readme/formats.svg" />
    <img src="assets/readme/formats.svg" alt="Glim 支持的代表文件格式；剑号标记基础预览" width="100%" />
  </picture>
</a>

*† 基础预览。各阅读器的实际能力见[格式说明](docs/file-format-support.zh-CN.md)。*

| 内容 | 代表格式 |
| --- | --- |
| 文档与文本 | Markdown、HTML、UTF-8 文本和日志；Markdown 中的图表与公式 |
| 数据与代码 | JSON/JSONC/JSON5、CSV/TSV、YAML、TOML、XML、SQLite 及带高亮的源代码 |
| 图片与办公文档 | PNG、JPEG、WebP、GIF、SVG、TIFF、PDF、Word、Excel、PowerPoint 和 iWork |
| 电子书与思维导图 | EPUB、FB2、CBZ、XMind |
| 媒体与专用文件 | 系统支持的音视频、字体、safetensors 元数据、归档目录与部分文本预览 |

### 使用前了解这些行为

- **编辑与保存：** UTF-8 文本支持基础编辑和保存。表格、数据树、数据库快照、图片和文档预览保持只读；可编辑格式保留原文入口。大文本或超长行使用可编辑分页。
- **兼容范围：** Office 预览可能与原始排版不同；表格公式显示已保存的值，不重新计算。媒体播放取决于 macOS 编码支持，部分格式提供基础预览或源内容后备。
- **本地文件与联网内容：** 文件在本地打开。Markdown 可加载远程图片，HTML 预览支持 CSS、JavaScript 和网络资源。可选的匿名使用统计由设置控制，见[使用统计说明](docs/telemetry.zh-CN.md)。
- **平台与安装：** 支持 macOS Apple Silicon 和 Intel。目前安装包使用临时签名，尚未经过 Apple 公证，首次打开方式见下文。

[完整格式说明](docs/file-format-support.zh-CN.md) · [文档目录](docs/README.zh-CN.md) · [未完成事项](docs/TODO.zh-CN.md)

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
