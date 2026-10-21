#!/usr/bin/env python3
"""Create verified signed feeds from built release archives. Does not publish."""
import argparse
import hashlib
import os
from pathlib import Path
import re
import plistlib
import shutil
import subprocess
import tempfile
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET

SPARKLE = '{http://www.andymatuschak.org/xml-namespaces/sparkle}'
REPOSITORY = 'https://github.com/glimapp/glim'


def run(*args):
    return subprocess.run(list(map(str, args)), check=True, capture_output=True, text=True).stdout


def checksum(path):
    h = hashlib.sha256()
    with path.open('rb') as file:
        for block in iter(lambda: file.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def validate_bundle(archive, stage, version, arch, public_key):
    mount = stage / 'mounted'
    mount.mkdir()
    run('hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint', mount, archive)
    try:
        bundle = mount / 'Glim.app'
        if bundle.is_symlink() or not bundle.is_dir():
            raise ValueError('Missing application bundle')
        info_path = bundle / 'Contents/Info.plist'
        binary = bundle / 'Contents/MacOS/glim'
        for path in [info_path, binary]:
            path.resolve().relative_to(bundle.resolve())
        with info_path.open('rb') as file:
            info = plistlib.load(file)
        required = {
            'CFBundleIdentifier': 'io.github.jony4.glimpse',
            'CFBundleShortVersionString': version,
            'GlimUpdatesEnabled': True,
            'SUPublicEDKey': public_key.strip(),
            'SUFeedURL': f'{REPOSITORY}/releases/latest/download/appcast-macos-{arch}.xml',
            'SURequireSignedFeed': True,
            'SUVerifyUpdateBeforeExtraction': True,
            'SUSignedFeedFailureExpirationInterval': 0,
            'SUEnableSystemProfiling': False,
        }
        if any(info.get(key) != value for key, value in required.items()):
            raise ValueError('Bundle update identity, feed or signing configuration differs from the release')
        if run('lipo', '-archs', binary).strip() != arch:
            raise ValueError('Bundle architecture does not match its feed')
        run('codesign', '--verify', '--deep', '--strict', bundle)
        build = int(info['CFBundleVersion'])
        if build < 1 or not info.get('LSMinimumSystemVersion'):
            raise ValueError('Invalid build or minimum OS version')
        return build
    finally:
        run('hdiutil', 'detach', mount)


def create_feeds(archives, tag, notes, tools, key_file, public_key, bootstrap=False):
    if not re.fullmatch(r'v\d+\.\d+\.\d+', tag):
        raise ValueError('A stable vMAJOR.MINOR.PATCH tag is required')
    if not re.search(r'^## English\s*$', notes, re.M) or not re.search(r'^## 中文\s*$', notes, re.M):
        raise ValueError('Release notes must include ## English and ## 中文 sections')
    # The secret stays in a protected file. Confirm it matches the key embedded by CI.
    derived = run('xcrun', 'swift', Path(__file__).with_name('update-key-public.swift'), key_file).strip()
    if derived != public_key.strip():
        raise ValueError('Release signing key does not match GLIM_UPDATE_PUBLIC_KEY')
    version = tag[1:]
    required = [f'Glim-{version}-macos-{arch}.dmg' for arch in ('arm64', 'x86_64')]
    archives.mkdir(parents=True, exist_ok=True)
    allowed = set(required) | {name + '.sha256' for name in required} | {
        f'appcast-macos-{arch}.xml' for arch in ('arm64', 'x86_64')}
    unexpected = {path.name for path in archives.iterdir()} - allowed
    if unexpected:
        raise ValueError('Unexpected assets in macOS release: ' + ', '.join(sorted(unexpected)))
    for name in required:
        path = archives / name
        recorded = (archives / (name + '.sha256')).read_text().split()[0]
        if checksum(path) != recorded:
            raise ValueError('Artifact checksum mismatch: ' + name)
    builds = []
    for arch in ('arm64', 'x86_64'):
        name = f'appcast-macos-{arch}.xml'
        archive = archives / f'Glim-{version}-macos-{arch}.dmg'
        with tempfile.TemporaryDirectory(prefix='glim-appcast-') as directory:
            stage = Path(directory)
            feed = stage / name
            expected_build = validate_bundle(archive, stage, version, arch, public_key)
            try:
                with urllib.request.urlopen(f'{REPOSITORY}/releases/latest/download/{name}', timeout=30) as response:
                    content = response.read(4 * 1024 * 1024 + 1)
                if len(content) > 4 * 1024 * 1024:
                    raise ValueError('Previous feed exceeds size limit')
                feed.write_bytes(content)
                run(tools / 'sign_update', '--verify', '--ed-key-file', key_file, feed)
            except urllib.error.HTTPError as error:
                if error.code != 404 or not bootstrap:
                    raise
            previous = []
            if feed.exists():
                previous = [int(item.findtext(SPARKLE + 'version', '0')) for item in ET.parse(feed).findall('./channel/item')]
            shutil.copy2(archive, stage / archive.name)
            (stage / archive.with_suffix('.md').name).write_text(notes)
            run(tools / 'generate_appcast', '--ed-key-file', key_file,
                '--download-url-prefix', f'{REPOSITORY}/releases/download/{tag}/',
                '--embed-release-notes', '--maximum-deltas', '0', '--maximum-versions', '3', '-o', feed, stage)
            run(tools / 'sign_update', '--verify', '--ed-key-file', key_file, feed)
            items = ET.parse(feed).findall('./channel/item')
            matching = [item for item in items if item.findtext(SPARKLE + 'shortVersionString') == version]
            if len(matching) != 1:
                raise ValueError('Expected exactly one release entry for ' + arch)
            item = matching[0]
            build = int(item.findtext(SPARKLE + 'version', '0'))
            if build != expected_build:
                raise ValueError('Appcast and bundle builds differ')
            if build <= max(previous, default=0):
                raise ValueError('CFBundleVersion must increase beyond every published build')
            enclosure = item.find('enclosure')
            expected_url = f'{REPOSITORY}/releases/download/{tag}/{archive.name}'
            if enclosure is None or enclosure.get('url') != expected_url or int(enclosure.get('length', '0')) != archive.stat().st_size:
                raise ValueError('Invalid update enclosure')
            signature = enclosure.get(SPARKLE + 'edSignature')
            if not signature:
                raise ValueError('Update signature is missing')
            run(tools / 'sign_update', '--verify', '--ed-key-file', key_file, archive, signature)
            if not item.findtext(SPARKLE + 'minimumSystemVersion'):
                raise ValueError('Minimum macOS version missing')
            builds.append(build)
            shutil.copy2(feed, archives / name)
    if len(set(builds)) != 1:
        raise ValueError('Architecture build versions differ')
    print('Verified both macOS archives and signed feeds (six release assets).')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archives', type=Path, required=True)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--notes', type=Path, required=True)
    parser.add_argument('--bootstrap', action='store_true', help='Allow a missing previous feed for the first updater release only')
    args = parser.parse_args()
    sdk = Path(os.environ['GLIM_SPARKLE_DIR'])
    create_feeds(args.archives, args.tag, args.notes.read_text(), sdk / 'bin',
                 Path(os.environ['GLIM_UPDATE_PRIVATE_KEY_FILE']), os.environ['GLIM_UPDATE_PUBLIC_KEY'], args.bootstrap)
