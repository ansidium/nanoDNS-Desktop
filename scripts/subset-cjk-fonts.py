#!/usr/bin/env python3
"""Build/check catalog-only Noto CJK subsets; never install fonts system-wide.

Prepare the ignored local tool environment:
  python -m venv .font-build/venv
  .font-build/venv/Scripts/python -m pip install fonttools==4.68.0
Build: .font-build/venv/Scripts/python scripts/subset-cjk-fonts.py
Check: .font-build/venv/Scripts/python scripts/subset-cjk-fonts.py --check
Only the eight small generated subsets and notices belong in the repository.
"""

import argparse
import hashlib
import json
from pathlib import Path
from urllib.request import urlopen

import fontTools
from fontTools import subset
from fontTools.ttLib import TTFont


ROOT = Path(__file__).resolve().parents[1]
COMMIT = "f8d157532fbfaeda587e826d4cd5b21a49186f7c"
REPOSITORY = "https://github.com/notofonts/noto-cjk"
BASE = f"https://raw.githubusercontent.com/notofonts/noto-cjk/{COMMIT}/"
FONTTOOLS_VERSION = "4.68.0"
REGIONS = {
    "zh-Hans": ("SimplifiedChinese", "sc"),
    "zh-Hant": ("TraditionalChinese", "tc"),
    "ja": ("Japanese", "jp"),
    "ko": ("Korean", "kr"),
}
WEIGHTS = ("Regular", "Light")
PUNCTUATION = "…·—–→←↗×•−"
SOURCES = {
    "NotoSansCJKsc-Regular.otf": "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b",
    "NotoSansCJKsc-Light.otf": "e78bbbe1573bbb006156c1198f5c8cd6329470cb6e0b6702c7a3c83913db4abf",
    "NotoSansCJKtc-Regular.otf": "dce08bd4fd91aa8aa76ed8fea4b694c2dfb8550f67871e326843212ddbeb88b4",
    "NotoSansCJKtc-Light.otf": "a49db53f6aac529d91a036606e55d68e2ab1df1360f507504ac4ff2cbb0f9407",
    "NotoSansCJKjp-Regular.otf": "68a3fc98800b2a27b371f2fb79991daf3633bd89309d4ffaa6946fd587f375b5",
    "NotoSansCJKjp-Light.otf": "f29b82874c6e80cef845770b35e111cc3bdff7b8b25d8a41f2285083152ecdfe",
    "NotoSansCJKkr-Regular.otf": "6bcb2a0703aa137e874fc2dffa85f6c21ba9a67fa329e81b8c801663af7e992a",
    "NotoSansCJKkr-Light.otf": "2f45abf3908d88ee45d2831484c920ec4a65e214ec14a8e0341ff2213579d5fc",
    "LICENSE": "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2",
    "README-third_party.md": "aa229a2136c426e3deca957c305727e02415198116138d8c1721222b12e22131",
}


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def get_source(source_path):
    path = ROOT / ".font-build" / "sources" / Path(source_path).name
    expected = SOURCES[path.name]
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        data = urlopen(BASE + source_path, timeout=90).read()
        if sha256(data) != expected:
            raise ValueError(f"upstream checksum mismatch: {source_path}")
        path.write_bytes(data)
    if sha256(path.read_bytes()) != expected:
        raise ValueError(f"cached source checksum mismatch: {path}")
    return path


def repertoire(tag):
    path = ROOT / "locales" / f"{tag}.json"
    catalog = json.loads(path.read_text(encoding="utf-8"))
    points = {ord(char) for text in catalog.values() for char in text if ord(char) >= 32}
    points.update(range(32, 127))
    points.update(map(ord, PUNCTUATION))
    return points, sha256(path.read_bytes())


def rename_subset(font, region, weight, license_text):
    family = f"nanoDNS UI {region.upper()}"
    full_name = f"{family} {weight}"
    postscript_name = f"nanoDNSUI{region.upper()}-{weight}"
    names = font["name"]
    # Keep all upstream copyright/trademark/manufacturer records. Give the
    # modified font its own primary names and embed the complete OFL text.
    replaced_ids = {1, 2, 3, 4, 6, 10, 13, 16, 17}
    names.names = [record for record in names.names if record.nameID not in replaced_ids]
    replacements = {
        1: family, 2: weight, 3: f"{postscript_name};Noto2.004;{COMMIT[:12]}",
        4: full_name, 6: postscript_name,
        10: f"Catalog-only subset of Noto Sans CJK; source commit {COMMIT}.",
        13: license_text, 16: family, 17: weight,
    }
    for name_id, value in replacements.items():
        names.setName(value, name_id, 3, 1, 0x0409)
        names.setName(value, name_id, 1, 0, 0)
    cff = font["CFF "].cff
    cff.fontNames[0] = postscript_name
    cff.topDictIndex[0].FullName = full_name
    cff.topDictIndex[0].FamilyName = family


def build():
    if fontTools.__version__ != FONTTOOLS_VERSION:
        raise ValueError(f"build requires FontTools {FONTTOOLS_VERSION}, got {fontTools.__version__}")
    license_source = get_source("Sans/LICENSE")
    notice_source = get_source("Sans/README-third_party.md")
    license_text = license_source.read_text(encoding="utf-8")
    license_dir = ROOT / "third_party" / "licenses" / "noto-cjk"
    license_dir.mkdir(parents=True, exist_ok=True)
    (license_dir / "OFL.txt").write_bytes(license_source.read_bytes())
    (license_dir / "UPSTREAM-NOTICE.md").write_bytes(notice_source.read_bytes())
    (license_dir / "NOTICE.md").write_text(
        "# nanoDNS UI CJK subsets\n\n"
        "Copyright 2014-2021 Adobe (http://www.adobe.com/).\n"
        "Noto is a trademark of Google Inc.\n\n"
        "These eight modified fonts derive from Noto Sans CJK version 2.004,\n"
        f"{REPOSITORY}, commit `{COMMIT}`.\n"
        "They remain licensed under SIL Open Font License 1.1; see OFL.txt.\n"
        "The original copyright, trademark, and license records are retained in\n"
        "each font, including the complete OFL text. Modified family names are\n"
        "nanoDNS UI SC, TC, JP, and KR. The full upstream fonts are not shipped.\n\n"
        "Regional Regular and Light fonts are restricted to each GUI catalog,\n"
        "printable ASCII, and the punctuation listed by subset-cjk-fonts.py.\n"
        "UPSTREAM-NOTICE.md is preserved verbatim from the pinned repository.\n",
        encoding="utf-8",
    )
    output = ROOT / "assets" / "fonts"
    output.mkdir(parents=True, exist_ok=True)
    records = []
    for tag, (directory, region) in REGIONS.items():
        points, catalog_hash = repertoire(tag)
        for weight in WEIGHTS:
            source_path = f"Sans/OTF/{directory}/NotoSansCJK{region}-{weight}.otf"
            source = get_source(source_path)
            destination = output / f"nanodns-ui-{region}-{weight.lower()}.otf"
            with TTFont(source, recalcTimestamp=False) as font:
                missing = points - font.getBestCmap().keys()
                if missing:
                    raise ValueError(f"source {source.name} lacks {sorted(missing)}")
                options = subset.Options()
                options.layout_features = []
                options.layout_scripts = []
                options.name_IDs = ["*"]
                options.name_languages = ["*"]
                options.name_legacy = True
                options.notdef_outline = True
                options.recommended_glyphs = True
                options.recalc_timestamp = False
                # egui uses the regional font's default cmap, without shaping.
                # Layout closure would retain unrelated language alternates.
                subsetter = subset.Subsetter(options=options)
                subsetter.populate(unicodes=points)
                subsetter.subset(font)
                rename_subset(font, region, weight, license_text)
                font.save(destination, reorderTables=True)
            with TTFont(destination) as generated:
                cmap = generated.getBestCmap()
                if points != cmap.keys():
                    raise ValueError(f"generated cmap differs from repertoire: {destination}")
                glyph_count = len(generated.getGlyphOrder())
            record = {
                "language": tag, "region": region, "weight": weight,
                "source_url": BASE + source_path,
                "source_sha256": SOURCES[source.name], "source_bytes": source.stat().st_size,
                "subset_path": destination.relative_to(ROOT).as_posix(),
                "subset_sha256": sha256(destination.read_bytes()), "subset_bytes": destination.stat().st_size,
                "catalog_sha256": catalog_hash, "codepoints": [f"U+{point:04X}" for point in sorted(points)],
                "cmap_codepoint_count": len(points), "glyph_count": glyph_count,
                "missing_codepoints": [],
            }
            records.append(record)
            print(f"{tag} {weight}: {destination.stat().st_size:,} bytes, {len(points)} codepoints, no gaps")
    provenance = {
        "schema": 1, "repository": REPOSITORY, "commit": COMMIT,
        "font_version": "2.004", "license": "OFL-1.1",
        "tool": {"name": "FontTools", "version": FONTTOOLS_VERSION},
        "repertoire": "Selected locale catalog values + printable ASCII U+0020-U+007E + explicit punctuation",
        "punctuation": PUNCTUATION, "layout_features": [], "hinting": True,
        "licenses": [
            {"path": (license_dir / "OFL.txt").relative_to(ROOT).as_posix(), "sha256": SOURCES["LICENSE"], "source_url": BASE + "Sans/LICENSE"},
            {"path": (license_dir / "UPSTREAM-NOTICE.md").relative_to(ROOT).as_posix(), "sha256": SOURCES["README-third_party.md"], "source_url": BASE + "Sans/README-third_party.md"},
        ],
        "fonts": records,
    }
    path = ROOT / "docs" / "font-provenance.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(provenance, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def check():
    provenance = json.loads((ROOT / "docs" / "font-provenance.json").read_text(encoding="utf-8"))
    if provenance["commit"] != COMMIT or len(provenance["fonts"]) != 8:
        raise ValueError("unexpected source pin or subset count")
    seen = set()
    for record in provenance["fonts"]:
        tag, weight = record["language"], record["weight"]
        if tag not in REGIONS or weight not in WEIGHTS or (tag, weight) in seen:
            raise ValueError("unexpected or duplicate subset variant")
        seen.add((tag, weight))
        points, catalog_hash = repertoire(tag)
        if record["catalog_sha256"] != catalog_hash:
            raise ValueError(f"catalog changed; rebuild fonts for {tag}")
        path = ROOT / record["subset_path"]
        if sha256(path.read_bytes()) != record["subset_sha256"]:
            raise ValueError(f"subset checksum mismatch: {path}")
        if record["source_sha256"] != SOURCES[Path(record["source_url"]).name]:
            raise ValueError(f"source checksum differs from pin: {tag} {weight}")
        with TTFont(path) as font:
            if points != font.getBestCmap().keys():
                raise ValueError(f"subset has missing or unnecessary codepoints: {path}")
            licenses = [name.toUnicode() for name in font["name"].names if name.nameID == 13]
            if not licenses or not all("SIL OPEN FONT LICENSE Version 1.1" in text for text in licenses):
                raise ValueError(f"embedded full license missing: {path}")
            if "Copyright 2014-2021 Adobe" not in font["CFF "].cff.topDictIndex[0].Notice:
                raise ValueError(f"embedded copyright notice missing: {path}")
    for record in provenance["licenses"]:
        if sha256((ROOT / record["path"]).read_bytes()) != record["sha256"]:
            raise ValueError(f"license checksum mismatch: {record['path']}")
    total = sum(record["subset_bytes"] for record in provenance["fonts"])
    print(f"PASS: 8 region/weight subsets, {total:,} bytes; SHA256, exact catalog cmaps and OFL notices verified")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="check committed outputs without downloads")
    args = parser.parse_args()
    if not args.check:
        build()
    check()
