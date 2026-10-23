# Build and release

**English** · [简体中文](releasing.zh-CN.md) · [Documentation](README.md)

## Platforms and prerequisites

| Target | Requirements |
| --- | --- |
| macOS Apple Silicon — `aarch64-apple-darwin` | Rust, Apple Command Line Tools with Swift, Python 3 |
| macOS Intel — `x86_64-apple-darwin` | Same macOS tooling and target toolchain |

Use the Rust version pinned in [CI](../.github/workflows/ci.yml). Only macOS Apple Silicon and Intel are maintained and packaged. The build script rejects non-macOS targets; existing public release assets are retained.

## Development and validation

```sh
cargo run --locked -- /path/to/project
```

Run validation when requested or preparing a release:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --locked
```

Relevant UI behavior needs platform smoke checks; state unverified interactions in release notes. Documentation-only changes need content/link checks, not a product build. Keep operational logs and prior test reports out of repository documentation.

## Packaging

Keep the workspace Cargo version, lockfile workspace-package versions and `CFBundleShortVersionString` aligned. Increase `CFBundleVersion` for every public release; never replace a published same-version package.

```sh
scripts/bundle-macos.sh release
scripts/package-dmg.sh arm64
scripts/package-dmg.sh x86_64
```

The unconfigured bundle command creates a local manual-download build, not an updater-enabled release. Default output is `dist/Glim.app`; architecture packages use `dist/release-<arch>/Glim.app`, `Glim-<version>-macos-<arch>.dmg` and `.sha256`. `GLIM_BUNDLE_DIR` overrides the bundle path; legacy `GLIMPSE_BUNDLE_DIR` remains accepted.

`target/` and `dist/` are ignored outputs. Normal builds use committed icons and offline diagram assets, not Node.js. To update diagram engine sources/dependencies, run `npm ci --ignore-scripts` and `npm run bundle` in `scripts/markdown/`, then commit the generated assets and licenses.

macOS signing proceeds inside-out through nested executables/bundles/frameworks, then the outer app. `GLIM_CODESIGN_IDENTITY` selects an identity; the default `-` is ad-hoc signing, not Developer ID signing or notarization. The package minimum OS is derived from supported Mach-O deployment commands, with the native Swift bridges requiring at least macOS 11. Verify the actual packaged architecture rather than assuming ARM64.

Local installation normally backs up the existing app before replacement, verifies the installed bundle and launches it. Keep the old version when the requested task is an in-app upgrade test. First-launch Gatekeeper instructions are in [installation help](../README.md#macos).

<a id="automatic-updates"></a>
## Automatic update setup

macOS release builds use pinned Sparkle 2.10.0, verified by `scripts/fetch-sparkle.sh`. The reusable centered update window shares state across workspaces. macOS updating first shipped in 0.1.12: 0.1.11 and older need one manual installation. Unconfigured local builds retain the GitHub download entry.

### One-time signing configuration

1. Run `scripts/fetch-sparkle.sh`.
2. Generate or retrieve a Keychain-backed key with `target/sparkle/2.10.0/bin/generate_keys --account Glim` and keep an offline protected backup.
3. Set repository variable `GLIM_UPDATE_PUBLIC_KEY` to the Base64 32-byte Ed25519 public key.
4. Create GitHub Environment `release` with trusted reviewer approval and allowed release tags. Export with `generate_keys --account Glim -x <protected-file>` and store the private 32-byte seed as environment secret `GLIM_UPDATE_PRIVATE_KEY`.
5. Never commit the private seed, include it in app bundles, or print it in logs/issues. Preserve the existing key for subsequent releases; changing it breaks the established trust chain unless a supported migration is designed.

Local updater-enabled packaging needs only the public key:

```sh
GLIM_ENABLE_UPDATER=1 GLIM_UPDATE_PUBLIC_KEY='<public-key>' scripts/package-dmg.sh arm64
```

The bundle script enables `macos-updater`, embeds Sparkle and its license, writes the architecture-specific feed/public key and strict verification settings, and signs nested components. Signing credentials are separate from Apple notarization.

### Prepare and publish a release

1. Commit final sources and aligned versions; push a stable `vX.Y.Z` tag.
2. Wait for both `macOS builds` jobs from the exact commit and version tag to succeed. ARM runs formatting, release Clippy/tests and native updater checks once; ARM and Intel each build an optimized release and verify the packaged architecture and signature.
3. Dispatch `Prepare signed release draft` from the release tag, provide that tag and equivalent notes under `## English` and `## 中文`, and approve the protected environment. Enable `bootstrap` only for the first updater release whose predecessor has no appcast; leave it off thereafter.
4. The workflow downloads successful same-commit artifacts, checks all archive SHA-256 files and both DMGs' identity/key/feed/architecture/build metadata, verifies predecessor feeds, retains compatible history, and signs/verifies the new feeds and packages.
5. It uploads two Mac installers, two checksum files and two appcasts (six assets total) to a draft, then downloads them to verify exact bytes. It neither modifies published releases nor overwrites existing assets.
6. Inspect the complete draft, publish and mark it Latest. Check both public feed URLs, signatures and release/build values. CI success alone never publishes a release.

Feeds use `/releases/latest/download/appcast-macos-<arch>.xml`; enclosures use immutable `/releases/download/<tag>/Glim-<version>-macos-<arch>.dmg` URLs. Runtime macOS clients need no GitHub token or Releases API calls. Downloads include automatic updates; README history is a daily snapshot of all public asset download counts, including feeds/checksums/verification requests, not users or installations.

### Release builds and caching

CI Cargo steps consistently use `--release --target <arch> --features macos-updater --locked`. There are no dev/debug builds or pre-packaging `cargo clean` steps. CI keeps release optimization and sets `CARGO_PROFILE_RELEASE_LTO=false` and `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` to shorten linking/code generation; the default local release profile is unchanged.

Relevant source/build-resource pushes to main warm both architecture caches; manual workflow runs can also warm them. These runs do not upload installers. Version tags reuse visible default-branch caches and produce DMGs. Caches from different tags cannot be reused directly, so let main warmup finish before creating a release tag. The signing workflow selects a successful push run for the exact tag, avoiding a main warmup run with no installers.

Caches distinguish architectures and toolchain/Cargo configuration, retaining dependencies and workspace outputs. Only the pinned Sparkle archive is cached; extraction always follows checksum verification. DMGs are already compressed, so artifact upload uses no additional compression and retains build artifacts for seven days. Job summaries record cache hits and disk usage; Actions steps record durations. Aim for roughly 1–5 minutes with warm caches; cold caches, toolchain/dependency changes, queueing and network delays are not guaranteed to fit five minutes. Intel's actual release build and package checks remain required; ARM tests do not certify Intel hardware interactions.

### Isolated updater validation

```sh
scripts/fetch-sparkle.sh
export GLIM_SPARKLE_DIR="$PWD/target/sparkle/2.10.0"
cargo clippy --workspace --all-targets --features macos-updater --locked -- -D warnings
cargo test --workspace --features macos-updater --locked
scripts/test-updater.sh
python3 scripts/test-updater-install.py
```

The focused checks cover native approval/cancellation, both deployment-command formats and unsigned nested Intel signing. The installer test uses temporary apps/keys and a loopback feed to exercise tamper rejection, manual restart and installation on normal quit; it does not replace the user's installed app or create production keys. Those checks do not prove every real hardware/admin-permission scenario.

See [update behavior and recovery](auto-update-design.md), [architecture](architecture.md) and [remaining work](TODO.md).
