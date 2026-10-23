# 阅读器边界统一管理

所有应用层阅读边界以 [assets/reader-limits.json](../assets/reader-limits.json) 为唯一数值来源。修改对应分类的 `value`，保持 `type`、`unit` 和 `effect` 的含义一致，然后重新构建安装。空值表示已取消对应总量门槛；有限值用于具体功能、分页或资源预算，实际行为见说明。

构建脚本通过 `scripts/generate-reader-limits.py` 在 Cargo OUT_DIR 生成 Rust 常量、Swift ReaderLimits 和嵌入式 JSON；图表 WebView 接收同一份 JSON。运行时不扫描或读取配置文件。配置只管理阈值，解析格式是否合法、编解码支持及 UI 布局仍由对应实现负责。

本页由同一配置自动生成。修改配置后执行 `python3 scripts/generate-reader-limits.py --docs` 更新中英文清单；普通构建只生成 OUT_DIR 文件。

## text

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `text.editable_bytes` | 32 MiB | bytes | 降级处理 | 内存编辑器工作预算；更大或长行文档使用可编辑分页及流式原子保存，不拒绝整个文件。 |
| `text.editable_newlines` | 400,000 | items | 降级处理 | 内存编辑器工作预算；更大或长行文档使用可编辑分页及流式原子保存，不拒绝整个文件。 |
| `text.line_bytes` | 64 KiB | bytes | 降级处理 | 内存编辑器工作预算；更大或长行文档使用可编辑分页及流式原子保存，不拒绝整个文件。 |
| `text.page_bytes` | 1 MiB | bytes | 处理／内存预算 | 普通文本每页读取字节数，后续页面保留其余完整内容。 |
| `text.full_bytes` | 256 MiB | bytes | 处理／内存预算 | 展开页的工作预算，不是完整文件门槛；上一页／下一页仍可访问其余内容，可编辑及流式来源还受各自工作预算管理。 |
| `text.full_newlines` | 4,000,000 | items | 处理／内存预算 | 展开页的工作预算，不是完整文件门槛；上一页／下一页仍可访问其余内容，可编辑及流式来源还受各自工作预算管理。 |
| `text.layout_run_bytes` | 1 KiB | bytes | 处理／内存预算 | 只读虚拟行单次排版的文本预算；超长逻辑行拆成后续显示行，源字节不丢失。 |

## binary

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `binary.preview_bytes` | 256 KiB | bytes | 分页，可继续访问 | 十六进制每页读取字节数，上一页／下一页可访问完整文件。 |

## image

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `image.input_bytes` | 取消 | bytes | 无应用层总量门槛 | 基于文件的原生图片读取不设输入字节或像素总量门槛，“文件字节”保留完整源文件访问。 |
| `image.pixels` | 取消 | pixels | 无应用层总量门槛 | 基于文件的原生图片读取不设输入字节或像素总量门槛，“文件字节”保留完整源文件访问。 |
| `image.decoded_bytes` | 512 MiB | bytes | 处理／内存预算 | 原生／QOI 当前预览图片的工作内存。QOI 流式采样像素，不拒绝较宽的源扫描行；原尺寸模式可能使用缩放图片，完整原始文件字节仍可访问。 |
| `image.preview_edge` | 4,000 | pixels | 处理／内存预算 | 初始 ImageIO 缩略图分辨率。原尺寸模式可在 image.decoded_bytes 内提高图片分辨率，不按原图尺寸拒绝文件。 |

## gif

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `gif.preview_edge` | 2,048 | pixels | 处理／内存预算 | 动画显示纹理边长，与源文件大小和总帧数无关。 |
| `gif.canvas_bytes` | 512 MiB | bytes | 处理／内存预算 | 合成工作内存预算；较大的源画布按行解码并采样，不拒绝整个动画。 |
| `gif.batch_bytes` | 16 MiB | bytes | 处理／内存预算 | 每批动画预取预算；已消费帧会释放，后续帧仍可继续读取。 |
| `gif.batch_frames` | 3 | items | 处理／内存预算 | 每批动画预取预算；已消费帧会释放，后续帧仍可继续读取。 |

## tiff

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `tiff.decode_buffer_bytes` | 取消 | bytes | 无应用层总量门槛 | ImageIO 按需读取所请求的 TIFF 页，不设应用层页数或累计预览总量门槛。 |
| `tiff.pages` | 取消 | items | 无应用层总量门槛 | ImageIO 按需读取所请求的 TIFF 页，不设应用层页数或累计预览总量门槛。 |
| `tiff.preview_decoded_bytes` | 取消 | bytes | 无应用层总量门槛 | ImageIO 按需读取所请求的 TIFF 页，不设应用层页数或累计预览总量门槛。 |
| `tiff.preview_encoded_bytes` | 取消 | bytes | 无应用层总量门槛 | ImageIO 按需读取所请求的 TIFF 页，不设应用层页数或累计预览总量门槛。 |

## inline_image

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `inline_image.preview_edge` | 2,048 | pixels | 处理／内存预算 | 内联缩略图的分辨率、解码及输出工作预算；原图仍可通过独立阅读器或源 URL 访问。 |
| `inline_image.encoded_bytes` | 取消 | bytes | 无应用层总量门槛 | 基于文件或流式准备图片，不设置应用编码文件总大小门槛。 |
| `inline_image.decoded_bytes` | 64 MiB | bytes | 处理／内存预算 | 内联缩略图的分辨率、解码及输出工作预算；原图仍可通过独立阅读器或源 URL 访问。 |
| `inline_image.output_overhead_bytes` | 1 MiB | bytes | 处理／内存预算 | 内联缩略图的分辨率、解码及输出工作预算；原图仍可通过独立阅读器或源 URL 访问。 |

## font

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `font.input_bytes` | 取消 | bytes | 无应用层总量门槛 | 系统基于文件读取字体元数据，不以应用总量门槛截掉字体面、资源或样本文本。 |
| `font.faces` | 取消 | items | 无应用层总量门槛 | 系统基于文件读取字体元数据，不以应用总量门槛截掉字体面、资源或样本文本。 |
| `font.resource_types` | 取消 | items | 无应用层总量门槛 | 系统基于文件读取字体元数据，不以应用总量门槛截掉字体面、资源或样本文本。 |
| `font.sample_chars` | 取消 | characters | 无应用层总量门槛 | 系统基于文件读取字体元数据，不以应用总量门槛截掉字体面、资源或样本文本。 |

## playlist

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `playlist.files` | 取消 | items | 无应用层总量门槛 | 虚拟化播放列表保留所有符合条件的文件，不设应用文件总数门槛。 |

## mkv

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `mkv.playlist_bytes` | 取消 | bytes | 无应用层总量门槛 | 从磁盘读取帧、用磁盘索引时间戳，并在可取消的辅助进程中为所选播放项写入碎片化 MP4；不设置总大小、总数量或总时长门槛。 |
| `mkv.input_bytes` | 取消 | bytes | 无应用层总量门槛 | 从磁盘读取帧、用磁盘索引时间戳，并在可取消的辅助进程中为所选播放项写入碎片化 MP4；不设置总大小、总数量或总时长门槛。 |
| `mkv.frame_bytes` | 16 MiB | bytes | 处理／内存预算 | 单个活动压缩帧／编解码元数据的分配预算，与影片总大小无关，原始文件字节仍可访问。 |
| `mkv.frames` | 取消 | items | 无应用层总量门槛 | 从磁盘读取帧、用磁盘索引时间戳，并在可取消的辅助进程中为所选播放项写入碎片化 MP4；不设置总大小、总数量或总时长门槛。 |
| `mkv.parser_read_bytes` | 2 GiB | bytes | 处理／内存预算 | 解析工作预算在每帧后重置，不累计限制影片总量；可选定位元数据通过流式扫描跳过。 |
| `mkv.parser_operations` | 20,000,000 | operations | 处理／内存预算 | 解析工作预算在每帧后重置，不累计限制影片总量；可选定位元数据通过流式扫描跳过。 |
| `mkv.tracks` | 取消 | items | 无应用层总量门槛 | 从磁盘读取帧、用磁盘索引时间戳，并在可取消的辅助进程中为所选播放项写入碎片化 MP4；不设置总大小、总数量或总时长门槛。 |
| `mkv.duration_seconds` | 取消 | seconds | 无应用层总量门槛 | 从磁盘读取帧、用磁盘索引时间戳，并在可取消的辅助进程中为所选播放项写入碎片化 MP4；不设置总大小、总数量或总时长门槛。 |
| `mkv.nal_units` | 取消 | items | 无应用层总量门槛 | 从磁盘读取帧、用磁盘索引时间戳，并在可取消的辅助进程中为所选播放项写入碎片化 MP4；不设置总大小、总数量或总时长门槛。 |
| `mkv.nesting` | 16 | levels | 处理／内存预算 | 轨道元数据解析栈预算；跳过可选章节、标签和定位索引，完整文件字节检查仍可用。 |
| `mkv.elements` | 取消 | items | 无应用层总量门槛 | 从磁盘读取帧、用磁盘索引时间戳，并在可取消的辅助进程中为所选播放项写入碎片化 MP4；不设置总大小、总数量或总时长门槛。 |

## json

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `json.input_bytes` | 16 MiB | bytes | 降级处理 | 结构树工作预算；源码保留完整可编辑文档，大文件使用分页。 |
| `json.field_chars` | 512 | characters | 处理／内存预算 | 仅控制树标签摘要；展开容器或双击查看完整键／值，源码保留全部内容。 |
| `json.nodes` | 250,000 | items | 降级处理 | 结构树工作预算；源码保留完整可编辑文档，大文件使用分页。 |
| `json.description_chars` | 80 | characters | 处理／内存预算 | 仅控制树标签摘要；展开容器或双击查看完整键／值，源码保留全部内容。 |
| `json.array_summary_chars` | 120 | characters | 处理／内存预算 | 仅控制树标签摘要；展开容器或双击查看完整键／值，源码保留全部内容。 |
| `json.auto_expand_children` | 16 | items | 处理／内存预算 | 仅控制初始自动展开，折叠的子项仍可手动展开。 |

## csv

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `csv.input_bytes` | 16 MiB | bytes | 处理／内存预算 | 当前页／记录解码内存预算，不是文件总大小门槛。过大的单条记录仍可通过可编辑源码分页完整访问。 |
| `csv.rows` | 100,000 | items | 分页，可继续访问 | 当前表格页／窗口大小，行列导航可访问其余全部记录。 |
| `csv.columns` | 256 | items | 分页，可继续访问 | 当前表格页／窗口大小，行列导航可访问其余全部记录。 |
| `csv.cells` | 1,000,000 | items | 分页，可继续访问 | 当前表格页／窗口大小，行列导航可访问其余全部记录。 |
| `csv.cell_chars` | 4,096 | characters | 处理／内存预算 | 可见单行单元格摘要；双击保留完整值，较大值使用分页查看。 |

## spreadsheet

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `spreadsheet.sheets` | 取消 | items | 无应用层总量门槛 | XLSX 单元格、工作表名称和已保存公式流式写入磁盘索引，值可分页查看；取消输入、工作表、行列、单元格和 HTML 总量门槛。旧版 XLS 及超出单条 XML 工作预算的情况使用无应用大小门槛的系统原生预览。 |
| `spreadsheet.rows` | 取消 | items | 无应用层总量门槛 | XLSX 单元格、工作表名称和已保存公式流式写入磁盘索引，值可分页查看；取消输入、工作表、行列、单元格和 HTML 总量门槛。旧版 XLS 及超出单条 XML 工作预算的情况使用无应用大小门槛的系统原生预览。 |
| `spreadsheet.columns` | 取消 | items | 无应用层总量门槛 | XLSX 单元格、工作表名称和已保存公式流式写入磁盘索引，值可分页查看；取消输入、工作表、行列、单元格和 HTML 总量门槛。旧版 XLS 及超出单条 XML 工作预算的情况使用无应用大小门槛的系统原生预览。 |
| `spreadsheet.cell_chars` | 取消 | characters | 无应用层总量门槛 | XLSX 单元格、工作表名称和已保存公式流式写入磁盘索引，值可分页查看；取消输入、工作表、行列、单元格和 HTML 总量门槛。旧版 XLS 及超出单条 XML 工作预算的情况使用无应用大小门槛的系统原生预览。 |
| `spreadsheet.input_bytes` | 取消 | bytes | 无应用层总量门槛 | XLSX 单元格、工作表名称和已保存公式流式写入磁盘索引，值可分页查看；取消输入、工作表、行列、单元格和 HTML 总量门槛。旧版 XLS 及超出单条 XML 工作预算的情况使用无应用大小门槛的系统原生预览。 |
| `spreadsheet.sheet_text_bytes` | 取消 | bytes | 无应用层总量门槛 | XLSX 单元格、工作表名称和已保存公式流式写入磁盘索引，值可分页查看；取消输入、工作表、行列、单元格和 HTML 总量门槛。旧版 XLS 及超出单条 XML 工作预算的情况使用无应用大小门槛的系统原生预览。 |
| `spreadsheet.html_bytes` | 取消 | bytes | 无应用层总量门槛 | XLSX 单元格、工作表名称和已保存公式流式写入磁盘索引，值可分页查看；取消输入、工作表、行列、单元格和 HTML 总量门槛。旧版 XLS 及超出单条 XML 工作预算的情况使用无应用大小门槛的系统原生预览。 |
| `spreadsheet.column_window` | 256 | columns | 分页，可继续访问 | 工作表每个展示页的列数，与 SQLite 浏览器无关。 |
| `spreadsheet.catalog_window` | 5,000 | items | 分页，可继续访问 | 工作表目录每页条目数。 |

## xmind

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `xmind.title_bytes` | 16 KiB | bytes | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |
| `xmind.nesting` | 64 | levels | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |
| `xmind.content_bytes` | 16 MiB | bytes | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |
| `xmind.topics` | 10,000 | items | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |
| `xmind.input_bytes` | 取消 | bytes | 无应用层总量门槛 | 从磁盘打开 ZIP 容器，不以文件总大小或条目总数拒绝打开。 |
| `xmind.archive_entries` | 取消 | items | 无应用层总量门槛 | 从磁盘打开 ZIP 容器，不以文件总大小或条目总数拒绝打开。 |
| `xmind.sheets` | 100 | items | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |
| `xmind.relationships` | 10,000 | items | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |
| `xmind.note_bytes` | 64 KiB | bytes | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |
| `xmind.labels` | 128 | items | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |
| `xmind.xml_nodes` | 200,000 | items | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |
| `xmind.xml_nesting` | 256 | levels | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |
| `xmind.link_bytes` | 16 KiB | bytes | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |
| `xmind.labels_bytes` | 16 KiB | bytes | 降级处理 | 思维导图工作预算；超出（包括解析栈预算耗尽）后，以可取消的按需解压分页打开完整 content.json/content.xml。 |

## archive

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `archive.entries` | 取消 | items | 无应用层总量门槛 | 枚举全部条目并落盘生成报告，界面按有界页面读取，不设置条目总数门槛。 |
| `archive.bytes` | 32 MiB | bytes | 处理／内存预算 | 结构化 XML 投影的工作缓冲预算，较大内容完整流式写入分页源码报告。 |

## numpy

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `numpy.header_bytes` | 64 KiB | bytes | 处理／内存预算 | 头部字符串 token 的工作缓冲预算；字段扫描覆盖完整流式头部，分页报告保留完整头部及支持类型的全部数值。 |
| `numpy.values` | 取消 | items | 无应用层总量门槛 | 支持类型的数值流式写入磁盘分页报告，不再只保留前缀数量。 |

## geometry

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `geometry.input_bytes` | 16 MiB | bytes | 降级处理 | 静态网格投影工作预算；较大投影改为完整源码／十六进制分页，不拒绝或静默截掉文件。 |
| `geometry.vertices` | 100,000 | items | 降级处理 | 静态网格投影工作预算；较大投影改为完整源码／十六进制分页，不拒绝或静默截掉文件。 |
| `geometry.faces` | 100,000 | items | 降级处理 | 静态网格投影工作预算；较大投影改为完整源码／十六进制分页，不拒绝或静默截掉文件。 |
| `geometry.polygon_vertices` | 256 | items | 降级处理 | 静态网格投影工作预算；较大投影改为完整源码／十六进制分页，不拒绝或静默截掉文件。 |

## bdf

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `bdf.glyph_edge` | 64 | pixels | 降级处理 | 位图字体投影工作预算；超出后仍提供完整 BDF 源码分页，不静默缩短字形列表。 |
| `bdf.glyphs` | 64 | items | 降级处理 | 位图字体投影工作预算；超出后仍提供完整 BDF 源码分页，不静默缩短字形列表。 |

## executable

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `executable.input_bytes` | 取消 | bytes | 无应用层总量门槛 | 不设文件、名称、条目数或报告总量门槛。元数据读取使用 io.cache_bytes 工作缓存并落盘生成报告；缓存预算耗尽时打开完整文件字节分页。CString 不采用库内的前缀截取。 |
| `executable.name_chars` | 取消 | characters | 无应用层总量门槛 | 不设文件、名称、条目数或报告总量门槛。元数据读取使用 io.cache_bytes 工作缓存并落盘生成报告；缓存预算耗尽时打开完整文件字节分页。CString 不采用库内的前缀截取。 |
| `executable.architectures` | 取消 | items | 无应用层总量门槛 | 不设文件、名称、条目数或报告总量门槛。元数据读取使用 io.cache_bytes 工作缓存并落盘生成报告；缓存预算耗尽时打开完整文件字节分页。CString 不采用库内的前缀截取。 |
| `executable.hex_bytes` | 4 KiB | bytes | 处理／内存预算 | 元数据报告中的十六进制摘要，完整文件字节仍可通过原始字节分页访问。 |
| `executable.segments` | 取消 | items | 无应用层总量门槛 | 不设文件、名称、条目数或报告总量门槛。元数据读取使用 io.cache_bytes 工作缓存并落盘生成报告；缓存预算耗尽时打开完整文件字节分页。CString 不采用库内的前缀截取。 |
| `executable.sections` | 取消 | items | 无应用层总量门槛 | 不设文件、名称、条目数或报告总量门槛。元数据读取使用 io.cache_bytes 工作缓存并落盘生成报告；缓存预算耗尽时打开完整文件字节分页。CString 不采用库内的前缀截取。 |
| `executable.report_bytes` | 取消 | bytes | 无应用层总量门槛 | 不设文件、名称、条目数或报告总量门槛。元数据读取使用 io.cache_bytes 工作缓存并落盘生成报告；缓存预算耗尽时打开完整文件字节分页。CString 不采用库内的前缀截取。 |
| `executable.libraries` | 取消 | items | 无应用层总量门槛 | 不设文件、名称、条目数或报告总量门槛。元数据读取使用 io.cache_bytes 工作缓存并落盘生成报告；缓存预算耗尽时打开完整文件字节分页。CString 不采用库内的前缀截取。 |
| `executable.dynamic_entries` | 取消 | items | 无应用层总量门槛 | 不设文件、名称、条目数或报告总量门槛。元数据读取使用 io.cache_bytes 工作缓存并落盘生成报告；缓存预算耗尽时打开完整文件字节分页。CString 不采用库内的前缀截取。 |
| `executable.name_scan_bytes` | 取消 | bytes | 无应用层总量门槛 | 不设文件、名称、条目数或报告总量门槛。元数据读取使用 io.cache_bytes 工作缓存并落盘生成报告；缓存预算耗尽时打开完整文件字节分页。CString 不采用库内的前缀截取。 |

## tensor

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `tensor.header_bytes` | 取消 | bytes | 无应用层总量门槛 | 元数据记录及 dtype 计数流式落盘；过大的单条记录改为完整头部分页，不读取张量权重。不设置头部、名称、形状、条目数量或报告总量门槛。 |
| `tensor.rows` | 取消 | items | 无应用层总量门槛 | 元数据记录及 dtype 计数流式落盘；过大的单条记录改为完整头部分页，不读取张量权重。不设置头部、名称、形状、条目数量或报告总量门槛。 |
| `tensor.report_bytes` | 取消 | bytes | 无应用层总量门槛 | 元数据记录及 dtype 计数流式落盘；过大的单条记录改为完整头部分页，不读取张量权重。不设置头部、名称、形状、条目数量或报告总量门槛。 |
| `tensor.shape_dimensions` | 取消 | items | 无应用层总量门槛 | 元数据记录及 dtype 计数流式落盘；过大的单条记录改为完整头部分页，不读取张量权重。不设置头部、名称、形状、条目数量或报告总量门槛。 |
| `tensor.metadata_entries` | 取消 | items | 无应用层总量门槛 | 元数据记录及 dtype 计数流式落盘；过大的单条记录改为完整头部分页，不读取张量权重。不设置头部、名称、形状、条目数量或报告总量门槛。 |
| `tensor.name_chars` | 取消 | characters | 无应用层总量门槛 | 元数据记录及 dtype 计数流式落盘；过大的单条记录改为完整头部分页，不读取张量权重。不设置头部、名称、形状、条目数量或报告总量门槛。 |
| `tensor.dtype_chars` | 取消 | characters | 无应用层总量门槛 | 元数据记录及 dtype 计数流式落盘；过大的单条记录改为完整头部分页，不读取张量权重。不设置头部、名称、形状、条目数量或报告总量门槛。 |
| `tensor.metadata_key_chars` | 取消 | characters | 无应用层总量门槛 | 元数据记录及 dtype 计数流式落盘；过大的单条记录改为完整头部分页，不读取张量权重。不设置头部、名称、形状、条目数量或报告总量门槛。 |
| `tensor.metadata_value_chars` | 取消 | characters | 无应用层总量门槛 | 元数据记录及 dtype 计数流式落盘；过大的单条记录改为完整头部分页，不读取张量权重。不设置头部、名称、形状、条目数量或报告总量门槛。 |

## sqlite

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `sqlite.snapshot_bytes` | 取消 | bytes | 无应用层总量门槛 | 按固定大小缓冲块复制快照，不设置应用数据库总大小门槛。 |
| `sqlite.page_rows` | 500 | rows | 分页，可继续访问 | 表格／结构每页缓冲行数，与表总行数无关；常驻分页栏提供行数统计、页码跳转及首页／末页导航。 |
| `sqlite.tables` | 取消 | items | 无应用层总量门槛 | 列出全部表和视图，不设置应用目录条目上限。 |
| `sqlite.columns` | 取消 | columns | 无应用层总量门槛 | 所有 SQLite 列均可访问；通过横向虚拟化限制工作集。 |
| `sqlite.value_bytes` | 取消 | bytes | 无应用层总量门槛 | 沿用原生 SQLite 格式约束；完整值及定义按 text.page_bytes 的窗口读取，并提供上一页／下一页。 |
| `sqlite.text_bytes` | 512 | bytes | 处理／内存预算 | 仅控制单元格摘要；双击打开完整值分页，二进制值以十六进制显示。 |
| `sqlite.schema_text_bytes` | 取消 | bytes | 无应用层总量门槛 | 沿用原生 SQLite 格式约束；完整值及定义按 text.page_bytes 的窗口读取，并提供上一页／下一页。 |
| `sqlite.blob_bytes` | 16 | bytes | 处理／内存预算 | 仅控制单元格摘要；双击打开完整值分页，二进制值以十六进制显示。 |
| `sqlite.query_seconds` | 5 | seconds | 超时 | 初始可取消查询时间预算，“延长时间重试”可明确增加预算。 |
| `sqlite.busy_milliseconds` | 1,000 | milliseconds | 超时 | 原生 SQLite 锁等待时间；数据库忙碌时可重试，与文件大小无关。 |
| `sqlite.cache_kib` | 8,192 | KiB | 处理／内存预算 | SQLite 页缓存工作内存，较大的数据库继续从磁盘读取页面。 |
| `sqlite.column_window` | 32 | columns | 分页，可继续访问 | 横向视口每次读取的列数，不限制总列数。 |

## markup

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `markup.scan_bytes` | 取消 | bytes | 无应用层总量门槛 | 扫描当前文本中的图片引用；大文档使用源码分页，不以引用扫描总量截掉后半部分。 |

## remote_image

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `remote_image.input_bytes` | 取消 | bytes | 无应用层总量门槛 | HTTP 下载流式写入临时文件，再生成有界缩略图，不以源输入总大小拒绝文件。 |
| `remote_image.timeout_seconds` | 20 | seconds | 超时 | 空闲／无进度超时，持续有进度的下载不设固定总耗时上限。 |
| `remote_image.first_attempt_seconds` | 8 | seconds | 超时 | 连接建立时间预算，包括 GitHub 原始 URL 首次尝试后转用 API 的连接等待。 |
| `remote_image.svg_embedded_bytes` | 64 MiB | bytes | 处理／内存预算 | SVG 准备过程中单个内嵌图片的工作内存，原始 SVG 内容仍可通过源码或文件字节访问。 |
| `remote_image.redirects` | 5 | items | 处理／内存预算 | 重定向链预算用于避免请求循环，不限制图片文件总大小。 |
| `remote_image.connect_seconds` | 8 | seconds | 超时 | 连接建立时间预算，包括 GitHub 原始 URL 首次尝试后转用 API 的连接等待。 |
| `remote_image.minimum_bytes_per_second` | 1 | bytes/second | 超时 | 空闲超时使用的最低进度判定，不是带宽上限。 |

## diagram

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `diagram.blocks` | 8 | items | 处理／内存预算 | 后台每批图表渲染工作预算；可见图块自动排队，不限制文档公式总数，也无需手动翻页。 |
| `diagram.source_bytes` | 256 KiB | bytes | 降级处理 | 单个图表的结构化渲染工作预算；失败时保留完整图块源码，其余图块仍可按需访问。 |
| `diagram.svg_bytes` | 8 MiB | bytes | 降级处理 | 单个图表的结构化渲染工作预算；失败时保留完整图块源码，其余图块仍可按需访问。 |
| `diagram.aggregate_encoded_bytes` | 64 MiB | bytes | 处理／内存预算 | 可视区域图表缓存与当前渲染批次的编码／解码工作预算；旧资源可淘汰后再次按需生成，不限制文档图块总数。 |
| `diagram.aggregate_decoded_bytes` | 128 MiB | bytes | 处理／内存预算 | 可视区域图表缓存与当前渲染批次的编码／解码工作预算；旧资源可淘汰后再次按需生成，不限制文档图块总数。 |
| `diagram.preview_edge` | 2,400 | pixels | 处理／内存预算 | 当前图片的分辨率／缩放预算；每页图片尺寸还按聚合工作内存调整，与文档总长度无关。 |
| `diagram.startup_seconds` | 15 | seconds | 超时 | 可取消的渲染器启动／单块时间预算；未完成图块显示完整源码，不拒绝整个 Markdown 文档。 |
| `diagram.block_seconds` | 8 | seconds | 超时 | 可取消的渲染器启动／单块时间预算；未完成图块显示完整源码，不拒绝整个 Markdown 文档。 |
| `diagram.ipc_bytes` | 128 MiB | bytes | 处理／内存预算 | 一页请求或单图块的 IPC 工作缓冲预算；文档按页处理，过大的渲染结果保留完整图块源码。 |
| `diagram.slot_bytes` | 16 MiB | bytes | 处理／内存预算 | 一页请求或单图块的 IPC 工作缓冲预算；文档按页处理，过大的渲染结果保留完整图块源码。 |
| `diagram.max_upscale` | 2 | scale | 处理／内存预算 | 当前图片的分辨率／缩放预算；每页图片尺寸还按聚合工作内存调整，与文档总长度无关。 |
| `diagram.request_bytes` | 64 MiB | bytes | 处理／内存预算 | 一页请求或单图块的 IPC 工作缓冲预算；文档按页处理，过大的渲染结果保留完整图块源码。 |
| `diagram.message_bytes` | 16388 KiB | bytes | 处理／内存预算 | 一页请求或单图块的 IPC 工作缓冲预算；文档按页处理，过大的渲染结果保留完整图块源码。 |
| `diagram.mermaid_edges` | 1,000 | edges | 降级处理 | 单个图表的结构化渲染工作预算；失败时保留完整图块源码，其余图块仍可按需访问。 |
| `diagram.math_macros` | 1,000 | expansions | 处理／内存预算 | 公式解释器的展开工作预算，控制递归工作量；无法完成渲染时仍保留完整公式源码。 |

## book

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `book.chapter_bytes` | 4 MiB | bytes | 降级处理 | 章节结构化排版工作预算。超出时保留章节导航并提供完整源码分页，支持的旧编码通过流式转码读取，不静默截掉章节。 |
| `book.xml_nodes` | 250,000 | nodes | 降级处理 | 章节结构化排版工作预算。超出时保留章节导航并提供完整源码分页，支持的旧编码通过流式转码读取，不静默截掉章节。 |
| `book.resource_bytes` | 取消 | bytes | 无应用层总量门槛 | 所请求的包内图片流式写入私有临时文件，再由原生接口生成缩略图；不按资源字节总量或原图边长拒绝打开，仅保留当前章节／页面预览。 |
| `book.preview_edge` | 2,400 | pixels | 处理／内存预算 | 当前插图／漫画页的缩略图分辨率，不以此拒绝原始图片。 |
| `book.input_bytes` | 取消 | bytes | 无应用层总量门槛 | 取消书籍、压缩包条目、章节、目录和元数据总量门槛。结构化解析保留工作预算；复杂元数据改为列出全部包内成员，提供完整源码或字节分页。 |
| `book.text_bytes` | 64 MiB | bytes | 降级处理 | 章节结构化排版工作预算。超出时保留章节导航并提供完整源码分页，支持的旧编码通过流式转码读取，不静默截掉章节。 |
| `book.archive_entries` | 取消 | items | 无应用层总量门槛 | 取消书籍、压缩包条目、章节、目录和元数据总量门槛。结构化解析保留工作预算；复杂元数据改为列出全部包内成员，提供完整源码或字节分页。 |
| `book.chapters` | 取消 | items | 无应用层总量门槛 | 取消书籍、压缩包条目、章节、目录和元数据总量门槛。结构化解析保留工作预算；复杂元数据改为列出全部包内成员，提供完整源码或字节分页。 |
| `book.metadata_bytes` | 取消 | bytes | 无应用层总量门槛 | 取消书籍、压缩包条目、章节、目录和元数据总量门槛。结构化解析保留工作预算；复杂元数据改为列出全部包内成员，提供完整源码或字节分页。 |
| `book.toc_entries` | 取消 | items | 无应用层总量门槛 | 取消书籍、压缩包条目、章节、目录和元数据总量门槛。结构化解析保留工作预算；复杂元数据改为列出全部包内成员，提供完整源码或字节分页。 |
| `book.toc_depth` | 32 | levels | 处理／内存预算 | 仅控制目录的视觉缩进，保留全部嵌套条目及实际层级，不截掉目录。 |
| `book.fb2_depth` | 64 | levels | 降级处理 | 章节结构化排版工作预算。超出时保留章节导航并提供完整源码分页，支持的旧编码通过流式转码读取，不静默截掉章节。 |
| `book.html_depth` | 128 | levels | 降级处理 | 章节结构化排版工作预算。超出时保留章节导航并提供完整源码分页，支持的旧编码通过流式转码读取，不静默截掉章节。 |
| `book.chapter_decoded_bytes` | 128 MiB | bytes | 降级处理 | 章节结构化排版工作预算。超出时保留章节导航并提供完整源码分页，支持的旧编码通过流式转码读取，不静默截掉章节。 |
| `book.chapter_images` | 128 | items | 降级处理 | 章节结构化排版工作预算。超出时保留章节导航并提供完整源码分页，支持的旧编码通过流式转码读取，不静默截掉章节。 |
| `book.image_decoded_bytes` | 取消 | bytes | 无应用层总量门槛 | 所请求的包内图片流式写入私有临时文件，再由原生接口生成缩略图；不按资源字节总量或原图边长拒绝打开，仅保留当前章节／页面预览。 |
| `book.image_edge` | 取消 | pixels | 无应用层总量门槛 | 所请求的包内图片流式写入私有临时文件，再由原生接口生成缩略图；不按资源字节总量或原图边长拒绝打开，仅保留当前章节／页面预览。 |

## book_state

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `book_state.bookmarks` | 取消 | items | 无应用层总量门槛 | 保存全部书签、标签和阅读状态记录，不设置应用书签总数或输入总大小门槛。 |
| `book_state.label_bytes` | 取消 | bytes | 无应用层总量门槛 | 保存全部书签、标签和阅读状态记录，不设置应用书签总数或输入总大小门槛。 |
| `book_state.input_bytes` | 取消 | bytes | 无应用层总量门槛 | 保存全部书签、标签和阅读状态记录，不设置应用书签总数或输入总大小门槛。 |
| `book_state.label_chars` | 120 | characters | 处理／内存预算 | 派生书签、书籍标题和目录标签摘要；章节目标及完整元数据／文本弹窗保留全部内容。 |

## geo

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `geo.points` | 10,000 | items | 分页，可继续访问 | 每页坐标数量；上一页／下一页可访问全部 GPX/KML 点，源码保留完整 XML。 |

## workspace

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `workspace.search_results` | 60 | items | 分页，可继续访问 | 可见页／批次大小，后续搜索结果或已保存工作空间可通过继续浏览入口访问。 |
| `workspace.search_entries` | 取消 | items | 无应用层总量门槛 | 搜索遍历全部符合条件的条目并提供可取消的结果分页，不设置扫描条目总数截断。 |
| `workspace.recent_workspaces` | 30 | items | 分页，可继续访问 | 可见页／批次大小，后续搜索结果或已保存工作空间可通过继续浏览入口访问。 |

## update

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `update.session_bytes` | 取消 | bytes | 无应用层总量门槛 | 流式读写会话 JSON，校验身份及活动索引，保留全部实际窗口、根目录和标签页。 |
| `update.workspaces` | 取消 | items | 无应用层总量门槛 | 流式读写会话 JSON，校验身份及活动索引，保留全部实际窗口、根目录和标签页。 |
| `update.roots_per_workspace` | 取消 | items | 无应用层总量门槛 | 流式读写会话 JSON，校验身份及活动索引，保留全部实际窗口、根目录和标签页。 |
| `update.tabs_per_workspace` | 取消 | items | 无应用层总量门槛 | 流式读写会话 JSON，校验身份及活动索引，保留全部实际窗口、根目录和标签页。 |

## audio_artwork

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `audio_artwork.input_bytes` | 取消 | bytes | 无应用层总量门槛 | 原生元数据封面不设置应用层字节门槛；“打开完整封面”提供完整原始快照。 |
| `audio_artwork.preview_edge` | 600 | pixels | 处理／内存预算 | 当前封面缩略图分辨率；“打开完整封面”保留完整原图访问入口。 |

## io

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `io.buffer_bytes` | 64 KiB | bytes | 处理／内存预算 | 可取消流式读取、复制及报告生成的固定工作缓冲，与源文件总大小无关。 |
| `io.cache_bytes` | 64 MiB | bytes | 处理／内存预算 | 解码文本、可执行文件元数据及网格／字体投影缓冲的工作缓存预算。旧文本范围可重新生成；较大的表示仍保留完整源码／字节分页。 |
| `io.record_bytes` | 64 MiB | bytes | 处理／内存预算 | 单条活动 XML／结构化元数据记录或值的预算，与文件总大小无关；过大的记录仍可通过完整源码／头部分页或原生预览访问。 |

## svg

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `svg.event_bytes` | 64 MiB | bytes | 处理／内存预算 | 清理器中单条活动 XML 标记／文本记录的工作预算，不是文档总量门槛；超出准备预算时，原生阅读器“文件字节”仍保留完整源文件。 |

## word

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `word.zoom_min_percent` | 25 | percent | 处理／内存预算 | Word 阅读视图最小显示缩放，不约束源文件内容。 |
| `word.zoom_max_percent` | 400 | percent | 处理／内存预算 | Word 阅读视图最大显示缩放，不约束源文件内容。 |
| `word.zoom_step_percent` | 25 | percent | 处理／内存预算 | Word 阅读视图缩放按钮的百分比步长；重置恢复 100%。 |

## drawio

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `drawio.xml_bytes` | 32 MiB | bytes | 处理／内存预算 | 预览源文件或解压后单页 XML 的预算；超过预算时回退到源码分页。 |
| `drawio.nesting` | 64 | levels | 处理／内存预算 | 限制父级遍历深度并检测循环分组。 |
| `drawio.preview_bytes` | 128 MiB | bytes | 处理／内存预算 | 所有 PNG 预览页的总编码内存预算；超过时回退到源码分页。 |

## opml

| Key | 当前值 | 单位 | 超限效果 | 实际行为 |
| --- | ---: | --- | --- | --- |
| `opml.xml_bytes` | 16 MiB | bytes | 处理／内存预算 | OPML XML 预览预算；超过时回退到源码分页。 |
| `opml.nodes` | 100,000 | items | 处理／内存预算 | 预览驻留大纲节点预算。 |
| `opml.nesting` | 256 | levels | 处理／内存预算 | 大纲嵌套深度预算。 |
