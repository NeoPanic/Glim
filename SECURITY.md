# Security policy

## Supported versions

Security fixes target the latest published release. Older versions do not have a separate maintenance branch. Please update before checking whether an issue still occurs.

## Report a vulnerability privately

Use [Report a vulnerability](https://github.com/glimapp/glim/security/advisories/new) to contact the maintainers through GitHub's private vulnerability reporting. Do not open a public issue with exploit details or sensitive files.

Include the affected version and OS, impact, reproduction steps, and a minimal sanitized sample or proof of concept. Do not include credentials or other people's data. Please allow time to investigate before publishing details. This volunteer project does not promise a response deadline or offer a paid bounty.

File parsers, native preview helpers, HTML rendering, and Git command handling are relevant security surfaces. Ordinary crashes without a security impact can use the bug template.

## Distribution

Download binaries from [GitHub Releases](https://github.com/glimapp/glim/releases). Releases include SHA-256 checksums. Current releases target macOS Apple Silicon and Intel and use ad-hoc signing, without Developer ID notarization. Existing public assets remain historical downloads; only macOS receives maintained releases. Checksums verify file integrity, not publisher identity.

## 中文说明

安全修复面向最新版本。漏洞请通过上面的私密举报入口提交，附版本、系统、影响和脱敏复现步骤；不要在公开 Issue 中披露利用细节或敏感文件。项目目前不承诺响应时限，也没有付费漏洞奖励计划。
