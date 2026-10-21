#!/usr/bin/env python3
"""Record one daily GitHub release-download snapshot and render README charts.

Counts include every asset of published releases, including prereleases and
checksum files. They are GitHub's reported totals, not unique users or installs.
Only actual observations are recorded; historical downloads are not estimated.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, timedelta, timezone
from html import escape
import json
import math
import os
from pathlib import Path
import re
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


SHANGHAI = timezone(timedelta(hours=8))


def github_pages(path):
    """Read all pages; an incomplete API response must never become a snapshot."""
    values = []
    for page in range(1, 1001):
        url = f"https://api.github.com/{path}?per_page=100&page={page}"
        headers = {
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "Glim-download-history",
        }
        if os.environ.get("GITHUB_TOKEN"):
            headers["Authorization"] = "Bearer " + os.environ["GITHUB_TOKEN"]
        for attempt in range(3):
            try:
                with urlopen(Request(url, headers=headers), timeout=25) as response:
                    batch = json.load(response)
                break
            except HTTPError as error:
                if error.code not in (429, 500, 502, 503, 504) or attempt == 2:
                    raise
                time.sleep(2 ** attempt)
            except (URLError, TimeoutError):
                if attempt == 2:
                    raise
                time.sleep(2 ** attempt)
        if not isinstance(batch, list):
            raise ValueError("Expected a GitHub API list; no snapshot was written")
        values.extend(batch)
        if len(batch) < 100:
            return values
    raise ValueError("GitHub pagination limit reached; no snapshot was written")


def download_totals(repository):
    releases = [r for r in github_pages(f"repos/{repository}/releases") if not r["draft"]]
    # Asset lists are paginated independently of releases.
    def assets_for(release):
        return github_pages(f"repos/{repository}/releases/{int(release['id'])}/assets")

    with ThreadPoolExecutor(max_workers=4) as pool:
        assets = [asset for batch in pool.map(assets_for, releases) for asset in batch]
    counts = [asset["download_count"] for asset in assets]
    if any(type(count) is not int or count < 0 for count in counts):
        raise ValueError("Invalid asset count; no snapshot was written")
    return sum(counts), len(releases), len(assets)


def read_history(path, repository):
    if not path.exists():
        return {"schema_version": 1, "repository": repository, "timezone": "Asia/Shanghai",
                "metric": "published-release-asset-downloads", "snapshots": []}
    history = json.loads(path.read_text(encoding="utf-8"))
    if (history.get("schema_version") != 1 or history.get("repository") != repository
            or history.get("timezone") != "Asia/Shanghai"
            or history.get("metric") != "published-release-asset-downloads"):
        raise ValueError("History schema, repository, timezone, or metric mismatch")
    previous = None
    for snapshot in history["snapshots"]:
        observed = date.fromisoformat(snapshot["date"])
        if previous is not None and observed <= previous:
            raise ValueError("History must have one observation per date, in ascending order")
        if type(snapshot["total"]) is not int or snapshot["total"] < 0:
            raise ValueError("Invalid historical download count")
        previous = observed
    return history


def chart(history, dark=False):
    samples = history["snapshots"][-90:]
    latest = samples[-1]
    bg, fg, muted, grid, accent = (
        ("#0d1117", "#e6edf3", "#919ba7", "#26313c", "#64d9bd") if dark else
        ("#ffffff", "#243d48", "#60717a", "#e8eef1", "#238673")
    )
    left, top, width, height = 78, 128, 834, 226
    dates = [date.fromisoformat(s["date"]).toordinal() for s in samples]
    span = max(1, dates[-1] - dates[0])
    peak = max(1, max(s["total"] for s in samples))
    unit = 10 ** math.floor(math.log10(peak / 4))
    step = next(n * unit for n in (1, 2, 2.5, 5, 10) if n * unit >= peak / 4)
    step = max(1, math.ceil(step))
    ceiling = step * 4
    points = [
        (left + width / 2 if len(samples) == 1 else left + (day - dates[0]) / span * width,
         top + height - sample["total"] / ceiling * height)
        for day, sample in zip(dates, samples)
    ]
    parts = [f'''<svg xmlns="http://www.w3.org/2000/svg" width="960" height="430" viewBox="0 0 960 430" role="img" aria-labelledby="title desc">
<title id="title">{escape(history['repository'])} release download history</title>
<desc id="desc">Daily observed totals for published GitHub release assets. Tracking began {history['snapshots'][0]['date']}. Latest: {latest['total']:,} on {latest['date']}. No earlier history is estimated.</desc>
<rect x=".5" y=".5" width="959" height="429" rx="16" fill="{bg}" stroke="{grid}"/>
<g font-family="-apple-system, BlinkMacSystemFont, Arial, sans-serif">
<text x="32" y="43" fill="{fg}" font-size="21" font-weight="650">Release download history</text>
<text x="32" y="69" fill="{muted}" font-size="13">{escape(history['repository'])} · Daily snapshots · Asia/Shanghai</text>
<text x="916" y="47" text-anchor="end" fill="{accent}" font-size="32" font-weight="700">{latest['total']:,}</text>
<text x="916" y="70" text-anchor="end" fill="{muted}" font-size="12">total attachment downloads</text>''']
    for tick in range(5):
        value = tick * step
        y = top + height - tick / 4 * height
        parts.append(f'<path d="M{left} {y:g}H{left + width}" stroke="{grid}"/>')
        parts.append(f'<text x="{left - 12}" y="{y + 4:g}" text-anchor="end" fill="{muted}" font-size="12">{value:,}</text>')
    if len(points) > 1:
        path = "M" + " L".join(f"{x:.2f} {y:.2f}" for x, y in points)
        parts.append(f'<path d="{path} L{points[-1][0]:.2f} {top + height} L{points[0][0]:.2f} {top + height}Z" fill="{accent}" fill-opacity=".08"/>')
        parts.append(f'<path d="{path}" fill="none" stroke="{accent}" stroke-width="3" stroke-linejoin="round" stroke-linecap="round"/>')
    else:
        parts.append(f'<text x="495" y="282" text-anchor="middle" fill="{muted}" font-size="14">First snapshot recorded. The curve grows with each daily observation.</text>')
    for i in sorted({round(n * (len(samples) - 1) / 4) for n in range(5)}):
        x, _ = points[i]
        parts.append(f'<text x="{x:.2f}" y="381" text-anchor="middle" fill="{muted}" font-size="12">{samples[i]["date"]}</text>')
    x, y = points[-1]
    parts.append(f'<circle cx="{x:.2f}" cy="{y:.2f}" r="9" fill="{accent}" fill-opacity=".15"/><circle cx="{x:.2f}" cy="{y:.2f}" r="4.5" fill="{accent}"/>')
    parts.append(f'<text x="32" y="409" fill="{muted}" font-size="11">Recorded since {history["snapshots"][0]["date"]} · All release assets, including checksums · Not unique users or installs</text>')
    parts.append('</g></svg>\n')
    return "\n".join(parts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", default="glimapp/glim")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repository):
        parser.error("repository must be owner/name")
    path = args.output / "downloads.json"
    history = read_history(path, args.repository)
    total, release_count, asset_count = download_totals(args.repository)
    now = datetime.now(SHANGHAI)
    today = now.date().isoformat()
    samples = history["snapshots"]
    if samples and samples[-1]["date"] > today:
        raise ValueError("History contains a future observation")
    snapshot = {"date": today, "observed_at": now.isoformat(timespec="seconds"),
                "total": total, "release_count": release_count, "asset_count": asset_count}
    if samples and samples[-1]["date"] == today:
        samples[-1] = snapshot
    else:
        samples.append(snapshot)
    # Render everything before replacing any output, so failed collection or
    # malformed history cannot erase the last published chart.
    outputs = {"downloads.json": json.dumps(history, ensure_ascii=False, indent=2) + "\n",
               "downloads.svg": chart(history), "downloads-dark.svg": chart(history, dark=True)}
    args.output.mkdir(parents=True, exist_ok=True)
    for name, text in outputs.items():
        temporary = args.output / (name + ".tmp")
        temporary.write_text(text, encoding="utf-8")
        temporary.replace(args.output / name)
    print(f"Recorded {today}: {total:,} downloads across {release_count} releases / {asset_count} assets")


if __name__ == "__main__":
    main()
