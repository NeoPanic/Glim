#!/usr/bin/env python3
"""Compile the shared reader policy into Rust, Swift and a JavaScript value map."""
import argparse
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent

def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f'Duplicate policy key: {key}')
        result[key] = value
    return result

def generate(output):
    policy = json.loads((ROOT / 'assets/reader-limits.json').read_text(), object_pairs_hook=unique)
    if policy['schema_version'] != 1:
        raise ValueError('Unsupported reader policy schema')
    values, rust, swift = {}, [], ['enum ReaderLimits {']
    types = {'usize': 2**63-1, 'u64': 2**64-1, 'u32': 2**32-1, 'i32': 2**31-1}
    for group, fields in policy['limits'].items():
        for name, entry in fields.items():
            key = f'{group}_{name}'.upper()
            if not re.fullmatch(r'[A-Z][A-Z0-9_]*', key) or key in values:
                raise ValueError(f'Invalid or duplicate policy name: {key}')
            value, kind = entry['value'], entry['type']
            if kind not in types or (value is not None and (type(value) is not int or not 0 < value <= min(types[kind], 2**53-1))):
                raise ValueError(f'Invalid value/type for {key}')
            if entry['effect'] not in ('reject', 'truncate', 'fallback', 'budget', 'timeout', 'page', 'unlimited'):
                raise ValueError(f'Invalid effect for {key}')
            if not entry['unit'] or not entry['description'] or not entry.get('description_zh'):
                raise ValueError(f'Missing policy documentation: {key}')
            values[key] = value
            if value is None:
                if entry['effect'] != 'unlimited':
                    raise ValueError(f'Null gate must be unlimited: {key}')
                continue
            rust.append(f'pub const {key}: {kind} = {value};')
            swift.append(f'    static let {key} = {value}')
    swift.append('}')
    encoded = json.dumps(values, separators=(',', ':'))
    rust.append(f'pub const JSON: &str = r#"{encoded}"#;')
    output.mkdir(parents=True, exist_ok=True)
    for name, contents in [('reader_limits.rs', '\n'.join(rust)), ('ReaderLimits.swift', '\n'.join(swift))]:
        target = output / name
        text = '// Generated from assets/reader-limits.json; edit the policy, not this file.\n' + contents + '\n'
        if not target.exists() or target.read_text() != text:
            target.write_text(text)

def document():
    policy = json.loads((ROOT / 'assets/reader-limits.json').read_text(), object_pairs_hook=unique)
    effects = {'reject': '拒绝该操作或视图', 'truncate': '截取显示', 'fallback': '降级处理', 'budget': '处理／内存预算', 'timeout': '超时', 'page': '分页，可继续访问', 'unlimited': '无应用层总量门槛'}
    for chinese in (False, True):
        title = '# 阅读器边界统一管理' if chinese else '# Shared reader policy'
        intro = (
            '所有应用层阅读边界以 [assets/reader-limits.json](../assets/reader-limits.json) 为唯一数值来源。修改对应分类的 `value`，保持 `type`、`unit` 和 `effect` 的含义一致，然后重新构建安装。空值表示已取消对应总量门槛；有限值用于具体功能、分页或资源预算，实际行为见说明。'
            if chinese else
            'All application reader boundaries take their numeric values from [assets/reader-limits.json](../assets/reader-limits.json). Edit a category’s `value`, preserving its `type`, `unit` and `effect`, then rebuild and install. Null values retire total-size/count gates. Finite values govern a specific feature, page or resource budget; see the behavior description.'
        )
        architecture = (
            '构建脚本通过 `scripts/generate-reader-limits.py` 在 Cargo OUT_DIR 生成 Rust 常量、Swift ReaderLimits 和嵌入式 JSON；图表 WebView 接收同一份 JSON。运行时不扫描或读取配置文件。配置只管理阈值，解析格式是否合法、编解码支持及 UI 布局仍由对应实现负责。'
            if chinese else
            'Build scripts use `scripts/generate-reader-limits.py` to generate Rust constants, Swift ReaderLimits and embedded JSON in Cargo OUT_DIR. The diagram WebView receives that same JSON. Rendering does not read the policy from disk. Format validity, codec support and UI geometry remain responsibilities of their implementations.'
        )
        instructions = ('本页由同一配置自动生成。修改配置后执行 `python3 scripts/generate-reader-limits.py --docs` 更新中英文清单；普通构建只生成 OUT_DIR 文件。' if chinese else 'This page is generated from the same policy. After editing it, run `python3 scripts/generate-reader-limits.py --docs` to update both language inventories. Ordinary builds write generated files only to OUT_DIR.')
        lines = [title, '', intro, '', architecture, '', instructions, '']
        for group, fields in policy['limits'].items():
            lines += ['## ' + group, '', '| Key | '+('当前值 | 单位 | 超限效果 | 实际行为 |' if chinese else 'Value | Unit | Effect | Behavior |'), '| --- | ---: | --- | --- | --- |']
            for name, entry in fields.items():
                value = entry['value']
                display = ("取消" if chinese else "None") if value is None else f"{value:,}"
                if value is not None and entry['unit'] == 'bytes':
                    for scale, label in ((1024**3, 'GiB'), (1024**2, 'MiB'), (1024, 'KiB')):
                        if value >= scale and value % scale == 0:
                            display = f'{value // scale} {label}'; break
                effect = effects[entry['effect']] if chinese else entry['effect']
                behavior = entry['description_zh'] if chinese else entry['description']
                lines.append(f"| `{group}.{name}` | {display} | {entry['unit']} | {effect} | {behavior.replace('|', '&#124;')} |")
            lines.append('')
        name = 'reader-limits.zh-CN.md' if chinese else 'reader-limits.md'
        (ROOT / 'docs' / name).write_text('\n'.join(lines))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out-dir', type=Path)
    parser.add_argument('--docs', action='store_true')
    args = parser.parse_args()
    if args.out_dir is None and not args.docs:
        parser.error('Provide --out-dir or --docs')
    if args.out_dir is not None:
        generate(args.out_dir)
    if args.docs:
        document()
