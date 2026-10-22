#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
preview_test_dir=$(mktemp -d)
trap 'rm -rf "$preview_test_dir"' EXIT
python3 scripts/generate-reader-limits.py --out-dir "$preview_test_dir"
xcrun swiftc -swift-version 5 \
    "$preview_test_dir/ReaderLimits.swift" native/Preview.swift native/ImagePreview.swift native/ImageTextExtraction.swift native/PreviewCopy.swift native/FloatingReaderToolbar.swift native/ReaderActions.swift native/HTMLPreview.swift native/WorkbookPreview.swift native/WordPreview.swift native/EmbeddedPreview.swift \
    native/tests/EmbeddedPreviewTests.swift -o "$preview_test_dir/preview-tests"
"$preview_test_dir/preview-tests"
