# 离线 Markdown 渲染资源

`renderer.js` 随二进制内嵌，由系统 WebView 在独立、不可见的子进程中执行。Markdown 文档只作为 JSON 数据传入，页面不接收文档 HTML；网络连接、外部导航和下载关闭。渲染结果为 SVG，由 services 在后台转为 PNG，再交给原生 TextView 扩展显示。

更新资源：在 `scripts/markdown` 执行 `npm ci --ignore-scripts` 和 `npm run bundle`。Node/npm 只用于维护资源，应用运行不依赖它们。提交生成文件及两个 lockfile。

`bootstrap.js` 只加载轻量调度器。调度器先分析当前图表批次的类型及 PlantUML 子类型，再并发导入所需引擎；模块 Promise 按引擎复用，单个引擎失败不阻塞无关类型。引擎及其依赖按 ESM 拆分到 `chunks/`，应用构建时将全部资源内嵌到二进制，不运行时下载。PlantUML 保留官方 ES module，themes/C4 仅在需要该引擎时加载；资源生成器检查所有生成模块与 PlantUML 的 JavaScript 语法。

引擎版本由 `scripts/markdown/package-lock.json` 固定：Mermaid 及官方 ZenUML 插件、Viz.js / Graphviz、PlantUML 官方 TeaVM 引擎、flowchart.js、WaveDrom、MathJax、D2、Pikchr WASM 与 Vega。第三方完整许可证见 `THIRD_PARTY_LICENSES.txt`，构建工具生成的代码归属见 `renderer.js.LEGAL.txt` 及 `chunks/*.LEGAL.txt`。

PlantUML 的 themes / emoji / OpenIconic 来自固定的 `@plantuml/core` 包。C4 标准库 v2.14.0（MIT）来自 [PlantUML 官方生成资源](https://plantuml.github.io/plantuml/js-plantuml/c4.min.js)，库自身附带版本、来源和许可证信息；文件已提交，运行时不下载。标准库来源：[C4-PlantUML](https://github.com/plantuml-stdlib/C4-PlantUML/tree/release/v2.14.0)。其他云厂商图标库及外部 include 未内置。

当前维护的 macOS Apple Silicon 与 Intel 构建使用系统 WebKit。不包含其他平台的 WebView 实现。

Salt 常用线框由 `scripts/markdown/salt.js` 的本地 SVG 适配器绘制，不依赖 PlantUML TeaVM 中缺失的 Salt 模块。支持范围和限制见 `docs/file-format-support.md`。`@startdot` 使用已内置 Graphviz；Journey 指定纯 SVG 文字。

Chen ER 的常用语法通过 `scripts/markdown/chen.js` 转为 DOT，并由内置 Graphviz 布局为 SVG；未支持的 EER 指令会明确报错，适配范围见格式文档。

结果按块独立原子回传，services 只读取新结果并复用系统字体库。Journey 的任务与分区文字根据实际背景选择对比色，避免 SVG 文字模式中的低对比度。

图表栅格化启用字体内存映射，避免重复读取中文字体文件；成功 PNG 按图表类型、完整源和栅格尺寸在进程内复用，缓存按共享 IO 工作预算淘汰，不缓存错误。
