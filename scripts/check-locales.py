#!/usr/bin/env python3
"""Validate the complete embedded GUI catalogs using only Python's standard library."""

import json
from pathlib import Path
import re
import sys
import unicodedata


LANGUAGES = (
    "en", "ru", "de", "fr", "es", "it", "pt-BR", "pt-PT", "nl", "pl",
    "uk", "cs", "sk", "tr", "el", "hu", "ro", "bg", "hr", "da", "sv",
    "nb", "fi", "id", "vi",
    "zh-Hans", "zh-Hant", "ja", "ko", "ms",
)
EXPECTED_KEYS = 184


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate key: {key!r}")
        result[key] = value
    return result


def read_catalog(path):
    catalog = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    if not isinstance(catalog, dict):
        raise ValueError("catalog must be a JSON object")
    for key, value in catalog.items():
        if not isinstance(key, str) or not isinstance(value, str) or not value.strip():
            raise ValueError(f"non-string or empty translation: {key!r}")
        for text in (key, value):
            if unicodedata.normalize("NFC", text) != text:
                raise ValueError(f"text must be Unicode NFC: {key!r}")
            if "\ufffd" in text or any(ord(char) < 32 and char != "\n" for char in text):
                raise ValueError(f"invalid display character: {key!r}")
        if key.count("{}") != value.count("{}"):
            raise ValueError(f"placeholder count changed: {key!r}")
        if re.sub(r"\{\}", "", value).count("{") or re.sub(r"\{\}", "", value).count("}"):
            raise ValueError(f"unsupported placeholder syntax: {key!r}")
        if key.count("\n") != value.count("\n"):
            raise ValueError(f"line break count changed: {key!r}")
    return catalog


def main():
    directory = Path(__file__).resolve().parents[1] / "locales"
    errors = []
    expected_files = {f"{tag}.json" for tag in LANGUAGES}
    actual_files = {path.name for path in directory.glob("*.json")}
    if expected_files != actual_files:
        errors.append(f"catalog files: missing={sorted(expected_files - actual_files)}, "
                      f"unexpected={sorted(actual_files - expected_files)}")
    try:
        english = read_catalog(directory / "en.json")
        if len(english) != EXPECTED_KEYS:
            errors.append(f"English catalog has {len(english)} keys; expected {EXPECTED_KEYS}")
        if any(key != value for key, value in english.items()):
            errors.append("English values must equal their stable English source keys")
    except (OSError, ValueError) as error:
        errors.append(f"en: {error}")
        english = {}
    for tag in LANGUAGES:
        try:
            catalog = read_catalog(directory / f"{tag}.json")
            missing = english.keys() - catalog.keys()
            unexpected = catalog.keys() - english.keys()
            if missing or unexpected:
                errors.append(f"{tag}: missing={sorted(missing)!r}, unexpected={sorted(unexpected)!r}")
        except (OSError, ValueError) as error:
            errors.append(f"{tag}: {error}")
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print(f"PASS: {len(LANGUAGES)} catalogs x {len(english)} keys; exact coverage, "
          "unique keys, UTF-8/NFC, placeholders and line breaks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
