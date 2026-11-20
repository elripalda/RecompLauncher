#!/usr/bin/env python3
"""Import upstream compatibility rows and separately resolve title artwork."""
import argparse
import hashlib
import html
import json
from pathlib import Path
import re
import urllib.request


def titles(readme):
    result = []
    for line in readme.splitlines():
        cells = [part.strip().replace("**", "").strip("`") for part in line.split("|")[1:-1]]
        if len(cells) < 5 or not re.fullmatch(r"CUSA[0-9]{5}", cells[1]):
            continue
        if not any(word in cells[3].lower() for word in ("playable", "playble", "in-game")):
            continue
        result.append({"title_id": cells[1], "name": cells[0], "upstream_status": cells[3], "notes": cells[4]})
    return result


def artwork(page):
    # Some metadata mirrors serialize the HTML once before returning it.
    page = page.replace('\\"', '"')
    match = re.search(r'<script[^>]*type="application/ld\+json"[^>]*>(.*?)</script>', page, re.S)
    if not match:
        raise ValueError("The official store page contains no product metadata")
    product = json.loads(html.unescape(match.group(1)))
    cover = product["image"]
    if not isinstance(cover, str) or not cover.startswith("https://image.api.playstation.com/"):
        raise ValueError("Unexpected artwork publisher")
    background = re.search(r'"role":"BACKGROUND","type":"IMAGE","url":"([^"]+)"', page)
    return {"cover": cover, "hero": background.group(1) if background else cover}


def fetch(url):
    with urllib.request.urlopen(url, timeout=30) as response:
        return response.read()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--title-data", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    manifest = json.loads(Path(__file__).with_name("components.json").read_text())
    revision = manifest["upstream_revision"]
    metadata = {row["titleId"].split("_")[0]: row for row in json.loads(args.title_data.read_text())}
    result = []
    for title in titles((args.source / "README.md").read_text()):
        record = metadata[title["title_id"]]
        url = "https://store.playstation.com/en-us/concept/" + str(record["conceptId"])
        title.update(artwork(fetch(url).decode()))
        for key in ("cover", "hero"):
            image = fetch(title[key])
            if not image.startswith((b"\x89PNG", b"\xff\xd8\xff", b"RIFF")):
                raise ValueError("Artwork URL did not return an image: " + title[key])
        title["source"] = f"https://github.com/wie-project/PS4Recomp/blob/{revision}/README.md#compatibility-status"
        title["artwork_source"] = url
        title["title_metadata_source"] = manifest["title_metadata_source"]
        title["title_metadata_sha256"] = hashlib.sha256(args.title_data.read_bytes()).hexdigest()
        result.append(title)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    print(f"Imported {len(result)} upstream titles; no game-specific build rules.")


if __name__ == "__main__":
    main()
