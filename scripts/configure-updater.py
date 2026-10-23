#!/usr/bin/env python3
"""Configure only the staged bundle, never source plist or a user's installation."""
import base64
import os
from pathlib import Path
import plistlib
import re
import subprocess
import sys

bundle = Path(sys.argv[1])
enabled = os.environ.get('GLIM_ENABLE_UPDATER') == '1'
path = bundle / 'Contents/Info.plist'
with path.open('rb') as file:
    info = plistlib.load(file)
info['GlimUpdatesEnabled'] = enabled
if enabled:
    public = os.environ.get('GLIM_UPDATE_PUBLIC_KEY', '').strip()
    if len(base64.b64decode(public, validate=True)) != 32:
        raise SystemExit('GLIM_UPDATE_PUBLIC_KEY must be a base64-encoded 32-byte Ed25519 public key')
    binary = bundle / 'Contents/MacOS/glim'
    arch = subprocess.check_output(['lipo', '-archs', str(binary)], text=True).strip()
    if arch not in ('arm64', 'x86_64'):
        raise SystemExit('Updater bundles require one known architecture')
    deployment = subprocess.check_output(['xcrun', 'vtool', '-show-build', str(binary)], text=True)
    minimums = re.findall(r'\bminos\s+([0-9.]+)', deployment)
    minimums += re.findall(
        r'\bcmd\s+LC_VERSION_MIN_MACOSX\s+cmdsize\s+\d+\s+version\s+([0-9.]+)', deployment)
    if not minimums:
        raise SystemExit('Cannot determine minimum macOS version from the executable')
    # Both native Swift bridges target macOS 11, even when Rust's Intel
    # executable retains an older deployment load command.
    minimums.append('11.0')
    info.update({
        'LSMinimumSystemVersion': max(minimums, key=lambda v: tuple(map(int, v.split('.')))),
        'SUFeedURL': f'https://github.com/glimapp/glim/releases/latest/download/appcast-macos-{arch}.xml',
        'SUPublicEDKey': public,
        'SUEnableAutomaticChecks': True,
        'SUScheduledCheckInterval': 86400,
        'SUAutomaticallyUpdate': True,
        'SUEnableSystemProfiling': False,
        'SUVerifyUpdateBeforeExtraction': True,
        'SURequireSignedFeed': True,
        'SUSignedFeedFailureExpirationInterval': 0,
    })
with path.open('wb') as file:
    plistlib.dump(info, file, sort_keys=False)
