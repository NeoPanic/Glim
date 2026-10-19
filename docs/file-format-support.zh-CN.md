# 文件格式与限制

[English](file-format-support.md) · **简体中文** · [首页](../README.zh-CN.md)

专注于 macOS 的文件查看器。

“支持”指下表中的实际行为，不代表完整兼容一种格式的全部功能。文件统一在工作空间标签中打开。当前支持范围为 macOS Apple Silicon 与 Intel，下表描述这两个架构的能力。

## 格式目录

| 格式 | 当前行为 |
| --- | --- |
| Rust、C/C++、Go、Swift、Java、C#、Kotlin、Zig、Python、Ruby、PHP、Lua、Scala、Elixir、Shell | 源码高亮、折叠、搜索、Minimap 与基础 UTF-8 编辑 |
| JS/TS/JSX/TSX、HTML、CSS、Astro、Svelte、ERB/EJS、Jinja/Jinja2/Nunjucks/Twig | 源码高亮；部分模板采用轻量着色，并非完整语法解析 |
| Vue、XML/plist、SCSS/LESS、Cython、CUDA/Metal、GLSL/OSL | 复用相关语言高亮，不提供专用语义校验 |
| Dockerfile/Containerfile、Makefile/CMake、Nix、INI、`.env`、忽略规则及相关配置 | 高亮或轻量着色的文本编辑，不执行内容 |
| Markdown | 原生阅读排版、表格、任务列表、脚注、Alerts、frontmatter、图表、公式、相对图片、本地链接与源码编辑 |
| HTML/HTM | WebKit 浏览器预览，支持 CSS/JavaScript 及源码编辑；所属 Git 项目内（无 Git 根目录时为文档目录）的相对本地资源分块加载 |
| TXT、日志及其他 UTF-8 文本 | 基础编辑；大文件或超长行使用可编辑 UTF-8 分页 |
| JSON/JSONC/JSON5、NDJSON/JSONL、GeoJSON、Notebook | 原文与按需加载的只读树。JSONC 支持注释和尾逗号，NDJSON 转为逐行记录数组；GeoJSON 不绘制地图，Notebook 不执行 |
| CSV/TSV | 虚拟化只读表格与原文编辑，支持引号、跨行字段、BOM 与不齐列 |
| XMind | 新版 JSON / 旧版 XML 容器；工作表、附属与游离主题树、折叠、缩放与详情。支持纯文本备注、标签、HTTP(S) 链接和关系端点，只读 |
| PNG/JPEG/WebP/GIF/BMP/TIFF/ICO/SVG/ICNS/EXR/HDR/QOI | 图片预览，GIF 动画、TIFF 多页、EXR/HDR 转 SDR；ICNS 需含 PNG 表示，SVG 不加载外部文件资源 |
| PDF — macOS | 标签内嵌 PDFKit 预览，翻页、缩放、文字选择与密码输入 |
| DOC/DOCX/PPT/PPTX/RTF — macOS | 标签内嵌 Quick Look 只读预览，可用性和排版取决于系统预览器 |
| XLS/XLSX — macOS | XLSX 流式建立单元格地址／值／公式索引并分页；XLS 及不支持的结构使用 Quick Look。不重算公式或还原样式、图表 |
| KEY/Pages/Numbers — macOS | Quick Look 只读预览，支持目录文档包，不提供编辑或演示动画 |
| MP3/MP4/MKV/M4A/M4V/MOV/AAC/WAV/AIF/AIFF/CAF/FLAC — macOS | 标签内嵌 AVKit 播放、文件夹顺序／随机队列。H.264／AAC-LC 的 MKV 无损重新封装为临时 MP4，包含误标为 `.mp4` 的 Matroska。其他 MKV 编码和内嵌字幕暂不支持，其余媒体解码取决于系统支持 |
| TTF/OTF/TTC/OTC/DFONT — macOS | CoreText 字体／集合查看、字形与自定义示例，不安装字体 |
| DB/DB3/SQLite/SQLite3 及可识别的 SQLite 文件头 — macOS | 只读临时快照，表／视图、结构、SQL 定义与分页记录；关联 WAL/SHM/journal 在主库存在时通过主库打开 |
| safetensors | 仅读取头部，显示张量名称、shape、精度、参数量与元数据，不加载或执行权重 |
| `.diff`/`.patch`、工作区／暂存区改动及提交历史 | 只读差异、单栏／双栏模式、显式暂存／取消暂存和仅提交暂存内容 |
| `.DS_Store` 等二进制点文件 | 有界十六进制／ASCII 预览，不提供语义解码或编辑 |

`.key` 也可能是文本密钥：仅 ZIP 容器和文档包进入 Keynote 阅读器。系统文件关联按后缀／类型生效，可能同时影响两种用途。

## 基础预览

| 格式 | 当前行为与边界 |
| --- | --- |
| AsciiDoc/Org/RST | 基础标题、列表与代码块阅读投影，保留原文编辑，不完整兼容方言 |
| SRT/VTT | 按时间戳阅读字幕并编辑原文，不与媒体播放同步 |
| MATLAB/Stan/HTTP/HCL/Dhall/KDL | 基础词法着色与编辑，不执行脚本、请求或配置 |
| D2/Pikchr/`*.vega.json` | 离线静态图表，支持原文编辑，禁用外部导入、数据与图片 |
| OBJ/STL | 静态网格投影，支持 OBJ 面与 ASCII／二进制 STL，不提供交互旋转、材质或纹理 |
| GPX/KML | 离线坐标表与 XML 原文，不加载地图底图或外部资源 |
| BDF | 前 64 个位图字形，每个最多 64×64，不安装字体 |
| ZIP/TAR | 条目名称与大小，标识越界路径，不解压 |
| GZ/BZ2/XZ | 按需解压并提供完整源分页，使用有界滚动缓存 |
| EPUB / FB2 / FB2 ZIP / CBZ | 专用只读书籍阅读器：spine／目录导航、章节或漫画页、插图、字号／行距／纸张控件、书签和位置保存；不复刻出版者 CSS 或 EPUB 固定版式 |
| ODT/ODS/ODP | 容器目录与部分文档文本提取 |
| NPY | v1/v2/v3 头部及预览范围内的基础小端 f8/f4/i8/i4/u1 数值；其他类型保留元数据，不执行 pickle |

XMind 使用统一阅读布局，不还原原始手动位置或样式，也不展示图片、附件、边界和概要。加密或损坏文档显示错误；链接不会自动打开。

## Markdown 图表与公式

| 引擎 | 代码块及支持语法 |
| --- | --- |
| Mermaid | `mermaid`/`mmd`，流程图、子图、时序／类／状态／ER 图、甘特、饼图、思维导图、Journey、Radar、Treemap 等引擎支持的类型；也支持 `zenuml` |
| Graphviz | `dot`/`graphviz` |
| PlantUML | `plantuml`/`puml`/`uml`，可省略 `@startuml` 包裹；内置 C4／主题／emoji／OpenIconic，基础 Salt 与 Chen ER 适配器，`@startdot` 交给 Graphviz |
| flowchart.js | `flow`/`flowchart`/`flowchart.js`，明确的 Mermaid `flowchart`/`graph` 声明交给 Mermaid |
| WaveDrom | `wavedrom`/`wavejson`，接受 JSON/JSON5，不执行 JavaScript |
| LaTeX | `$…$`/`$$…$$`、独立 `$$` 块、独立段落 `\[ … \]` 及 `math`/`latex`/`tex`/`katex` 代码块 |
| GitHub Alerts | 引用块以 `[!NOTE]`、`[!TIP]`、`[!IMPORTANT]`、`[!WARNING]` 或 `[!CAUTION]` 开头 |

未标注语言的代码块可通过明确的 Mermaid、PlantUML 或 Graphviz 声明识别。图表转为静态图片，原文和保存内容不变；失败块显示错误与源码。引擎随 macOS 应用离线提供，使用系统 WebKit 渲染。

Salt 与 Chen ER 是部分适配器，并非完整 PlantUML 方言。暂不支持的预处理／include、富内容、外部资源及不兼容的 `foreignObject` 输出明确报错。Salt 支持常用线框控件；Chen 支持常见实体、属性、关系、基数与参与连接，不支持完整 EER 方言。不加载外部图片或未内置 include。

## 阅读边界统一管理

应用阈值统一在 [assets/reader-limits.json](../assets/reader-limits.json) 管理。[自动生成的清单](reader-limits.zh-CN.md) 列出每项数值、单位及超限效果。Rust、原生 Swift 和图表 JavaScript 使用同一份配置。输入上限、展示截取、分页阈值、预览尺寸及运行内存／时间预算分别管理；非 Git 总量门槛已改为流式、分页或完整源后备；Git/diff 边界留待专项迭代。

预览限制不截断原文件或暂存内容。普通非 UTF-8 文本目前不解码；Office 预览不编辑、不运行宏、不重新计算公式。

SQLite 复制主库、WAL 和回滚日志到私有快照，并检查源文件稳定性，不 checkpoint 原库。不支持加密数据库及任意写 SQL。

MKV 支持 H.264／AAC-LC 重新封装，包括 AVC 配置及 Annex-B 帧；暂不支持压缩／加密轨道、编码延迟及重复时间戳的帧交织。保留原文件与显示名称，临时媒体随原生阅读器释放。文件夹播放列表枚举直接子文件，跳过不支持的条目，恢复后保持暂停。

## 维护

`cargo run -p glim-services --example audit_formats --locked -- /path/to/project` 仅对明确的格式白名单抽样读取，输出计数，不输出私有路径或正文。它不是完整格式审计，也不能替代界面测试。

参见[架构](architecture.zh-CN.md)、[构建与发布](releasing.zh-CN.md)和[待完成事项](TODO.zh-CN.md)。

电子书各项边界见统一配置清单。只加载本地书内资源，不请求远程图片或执行活动内容。暂不支持 DRM EPUB、MOBI/AZW、PDF 书籍版式、出版者 CSS 的完整还原及 EPUB 固定版式。Space／Page Down 与 Shift-Space／Page Up 按视口翻页，Left／Right 切换章节。阅读偏好与书签单独保存，不改写原书。

XLSX 展示稀疏单元格索引，不复刻 Excel 网格。GPX/KML 坐标和图表批次提供继续浏览入口。复杂书籍、XMind、网格／字体投影及张量头保留完整源后备。原生图片可能采用缩放栅格，实际大小缩放不保证原分辨率解码。SQLite 完整值窗口不约束引擎内部表达式分配；ZIP 中央目录及 Quick Look/WebKit 仍受平台／库实现约束。非 Git 文件总量门槛改为流式、分页或完整源后备；Git/diff 边界留待专项迭代。
