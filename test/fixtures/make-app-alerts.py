#!/usr/bin/env python3
"""Derive an app payload that carries warnings, for the render harness.

The captured app payload (test/fixtures/app-leiden.json) was taken on a quiet day, so the
warnings card cannot be rendered from it. This script only adds an `alerts` array, with the
field names and the shape Bright Sky really returns (see `parse_bright_sky_alerts`) — one
severe warning and one moderate one, so the card's ordering and its two colour bands can
both be looked at.

    python test/fixtures/make-app-alerts.py

The result, app-alerts-synthetic.json, is test data for the render harness. It is not a
captured response and must not be presented as one.
"""

import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
SOURCE = HERE / "app-leiden.json"
TARGET = HERE / "app-alerts-synthetic.json"

ALERTS = [
    {
        "event": "Wind gusts",
        "headline": "Official WARNING of GUSTS",
        "severityLabel": "Severe",
        "severityRank": 2,
        "windowLabel": "08:00–19:00",
        "detail": (
            "There is a risk of wind gusts (level 2 of 4). Max. gusts: ~ 70 km/h; "
            "Wind direction: south-west; Peak gusts from 11:00 to 15:00."
        ),
        "instruction": "Take care when outdoors, and secure loose objects.",
    },
    {
        "event": "Frost",
        "headline": "Official WARNING of FROST",
        "severityLabel": "Moderate",
        "severityRank": 1,
        "windowLabel": "until 09:00",
        "detail": "There is a risk of frost (level 1 of 4).",
        "instruction": "Be aware of icy surfaces on roads and pavements.",
    },
]


def main() -> None:
    payload = json.loads(SOURCE.read_text())
    payload["alerts"] = ALERTS
    payload["synthetic"] = True
    TARGET.write_text(json.dumps(payload, separators=(",", ":")) + "\n")
    print(f"wrote {TARGET.name}: {len(ALERTS)} warnings")


if __name__ == "__main__":
    main()
