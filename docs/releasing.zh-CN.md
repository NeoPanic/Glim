# 构建与发布

[English](releasing.md) · **简体中文** · [文档目录](README.zh-CN.md)

## 平台与准备

| 目标 | 要求 |
| --- | --- |
| macOS Apple Silicon — `aarch64-apple-darwin` | Rust、包含 Swift 的 Apple Command Line Tools、Python 3 |
| macOS Intel — `x86_64-apple-darwin` | 同样的 macOS 工具及目标工具链 |

Rust 版本以 [CI](../.github/workflows/ci.yml) 固定值为准。当前仅维护并打包 macOS Apple Silicon 与 Intel；构建脚本拒绝非 macOS 目标，既有公开附件继续保留。

## 开发与验证

```sh
cargo run --locked -- /path/to/project
```

请求验证或准备发布时执行：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --locked
```

相关界面需要平台烟测，发布说明中写明未验证交互。纯文档修改检查内容／链接，不需要构建产品。操作日志与历史测试报告不放入仓库文档。

## 打包

保持 Cargo 工作空间版本、lockfile 中工作空间包版本与 `CFBundleShortVersionString` 一致。每次正式发布递增 `CFBundleVersion`，不替换已发布的同版本包。

```sh
scripts/bundle-macos.sh release
scripts/package-dmg.sh arm64
scripts/package-dmg.sh x86_64
```

未配置的 bundle 命令生成手动下载构建，不是启用更新器的发行包。默认输出 `dist/Glim.app`；分架构包输出 `dist/release-<arch>/Glim.app`、`Glim-<version>-macos-<arch>.dmg` 和 `.sha256`。`GLIM_BUNDLE_DIR` 覆盖路径，兼容旧 `GLIMPSE_BUNDLE_DIR`。

`target/` 与 `dist/` 是忽略的产物。正常构建使用已提交的图标及离线图表资源，不需要 Node.js。修改图表引擎源码／依赖时，在 `scripts/markdown/` 执行 `npm ci --ignore-scripts` 和 `npm run bundle`，同时提交生成资源及许可证。

macOS 先签嵌套可执行文件／bundle／framework，再签外层 app。`GLIM_CODESIGN_IDENTITY` 选择身份，默认 `-` 是 ad-hoc 签名，不是 Developer ID 签名或公证。最低系统版本从支持的 Mach-O 部署指令推导，原生 Swift 桥接至少要求 macOS 11。应核对实际包架构，不能一概按 ARM64 验收。

本地安装通常先备份旧 app，再替换、核对安装内容并启动；若任务是测试应用内升级，则保留旧版。首次打开的 Gatekeeper 步骤见[安装帮助](../README.zh-CN.md#macos)。

<a id="automatic-updates"></a>
## 自动更新配置

macOS 发行包使用固定 Sparkle 2.10.0，由 `scripts/fetch-sparkle.sh` 校验下载。各工作台共用复用的居中更新窗口。macOS 更新器从 0.1.12 开始提供，0.1.11 及更早版本需要先手动安装一次；未配置的本地构建保留 GitHub 下载入口。

### 一次性签名配置

1. 执行 `scripts/fetch-sparkle.sh`。
2. 使用 `target/sparkle/2.10.0/bin/generate_keys --account Glim` 生成或读取钥匙串密钥，保留受保护的离线备份。
3. 仓库变量 `GLIM_UPDATE_PUBLIC_KEY` 设置为 32 字节 Ed25519 公钥的 Base64 编码。
4. 创建 GitHub Environment `release`，设置可信维护者审批及允许的发布 tag。使用 `generate_keys --account Glim -x <受保护文件>` 导出，将 32 字节私钥 seed 存为环境 secret `GLIM_UPDATE_PRIVATE_KEY`。
5. 私钥不得提交、放入应用包或输出到日志／Issue。后续版本沿用既有密钥；没有支持的迁移方案时，换钥会破坏既有信任链。

本地启用更新器打包只需要公钥：

```sh
GLIM_ENABLE_UPDATER=1 GLIM_UPDATE_PUBLIC_KEY='<public-key>' scripts/package-dmg.sh arm64
```

脚本启用 `macos-updater`，内嵌 Sparkle 和许可证，写入分架构 feed／公钥及严格校验配置，并签名嵌套组件。更新签名凭据与 Apple 公证不同。

### 准备与发布版本

1. 提交最终源码及一致版本，推送稳定 `vX.Y.Z` tag。
2. 等待同一提交、同一版本标签的两个 `macOS builds` 工作全部成功。ARM 执行一次 fmt、release Clippy／测试及原生更新器检查；ARM 与 Intel 分别构建 release 并校验打包架构和签名。
3. 从发布 tag 手动运行 `Prepare signed release draft`，输入同一 tag 和含 `## English`／`## 中文` 的等价说明，并批准受保护环境。`bootstrap` 只用于前一版没有 appcast 的首次更新器发布，后续关闭。
4. 工作流下载同一提交的成功产物，核对全部 SHA-256、两种 DMG 的身份／公钥／feed／架构／build，验证上一版 feed，保留兼容历史并生成、验证新签名 feed 与安装包。
5. 将两个 Mac 安装包、两个校验文件和两个 appcast（共六个附件） 上传草稿，回下载核对字节，不修改已公开 Release，不覆盖既有资产。
6. 检查完整草稿，公开并标记 Latest，再检查两个公开 feed 地址、签名及版本／build。CI 成功本身不会自动公开版本。

feed 地址是 `/releases/latest/download/appcast-macos-<arch>.xml`，安装包使用固定 `/releases/download/<tag>/Glim-<version>-macos-<arch>.dmg`。运行时 macOS 客户端不需要 GitHub Token，也不查询 Releases API。自动更新下载会进入附件计数；README 曲线每天记录全部公开附件下载量，包含 feed、校验文件和核验请求，不等于用户数或安装量。

### 发布构建与缓存

CI 的 Cargo 步骤统一使用 `--release --target <arch> --features macos-updater --locked`，不构建 dev／debug，不在打包前执行 `cargo clean`。CI 保持 release 优化，设置 `CARGO_PROFILE_RELEASE_LTO=false` 和 `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` 以缩短链接／代码生成；本地默认 release profile 不受影响。

main 上相关源码／构建资源变更会预热两个架构的发布缓存，手动运行工作流也可预热；这些运行不上传安装包。版本 tag 复用默认分支的可见缓存并生成 DMG。不同 tag 的缓存不能直接互用，因此应在创建发布 tag 前让 main 预热完成。发布工作流只选择同一 tag 的成功 push 运行，避免误选没有安装包的 main 预热运行。

缓存按架构和工具链／Cargo 配置区分，保留依赖及工作空间编译产物；Sparkle 仅缓存固定压缩包，每次从验过摘要的字节重新解压。DMG 已压缩，上传不再重复压缩，构建附件保留七天。Job Summary 记录缓存命中和磁盘占用，Actions 步骤记录耗时。目标是热缓存下约 1–5 分钟，冷缓存、工具链／依赖变化、排队和网络波动不保证五分钟内完成。Intel 通过实际 release 构建和包校验，ARM 测试不能视为 Intel 实机交互验收。

### 隔离更新验证

```sh
scripts/fetch-sparkle.sh
export GLIM_SPARKLE_DIR="$PWD/target/sparkle/2.10.0"
cargo clippy --workspace --all-targets --features macos-updater --locked -- -D warnings
cargo test --workspace --features macos-updater --locked
scripts/test-updater.sh
python3 scripts/test-updater-install.py
```

专项检查覆盖原生批准／取消、两种部署指令格式及未签名 Intel 嵌套包签名。安装测试使用临时 app／密钥和 loopback feed，检查篡改拒绝、手动重启与正常退出安装，不替换用户已安装 app 或生成正式密钥；它不能证明所有实机／管理员授权场景。

参见[更新行为与恢复](auto-update-design.zh-CN.md)、[架构](architecture.zh-CN.md)和[未完成事项](TODO.zh-CN.md)。
