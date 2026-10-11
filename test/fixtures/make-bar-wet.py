#!/usr/bin/env python3
"""Derive the synthetic "rain later" bar payload from the captured dry one.

The dry case (test/fixtures/bar-dry.json) is a real payload captured from KNMI; the wet
one cannot be captured on demand — it needs rain over Leiden — so this script only
rewrites the per-frame millimetre rates and the summary fields derived from them, using
the same intensity bands as rustybui-core (`nowcast.rs`): <0.3 drizzle, <1 light,
<3 moderate, <10 heavy, <25 very heavy.

    python test/fixtures/make-bar-wet.py

The result, bar-wet-synthetic.json, is test data for the render harness and the node
model checks. It is not a captured response and must not be presented as one.
"""

import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
SOURCE = HERE / "bar-dry.json"
TARGET = HERE / "bar-wet-synthetic.json"

# Frames from "now" on: dry for the first five (about 20 minutes), then a shower that
# builds, peaks, and eases off by roughly the hour. Keyed by frame index rather than by
# offset, because offsets land on the radar cycle (5-minute slots offset from the moment
# the payload was captured), not on round numbers.
PROFILE = [0.0, 0.0, 0.0, 0.0, 0.0, 0.2, 0.6, 1.4, 2.6, 4.0, 3.2, 1.1, 0.5, 0.2]

RAIN_THRESHOLD = 0.1


def intensity_class(mm_per_hour: float) -> str:
    if mm_per_hour <= 0.0:
        return "dry"
    if mm_per_hour < 0.3:
        return "drizzle"
    if mm_per_hour < 1.0:
        return "light"
    if mm_per_hour < 3.0:
        return "moderate"
    if mm_per_hour < 10.0:
        return "heavy"
    if mm_per_hour < 25.0:
        return "very heavy"
    return "extreme"


def minutes_label(minutes: int) -> str:
    hours, rest = divmod(minutes, 60)
    if hours == 0:
        return f"{rest} min"
    return f"{hours} h {rest:02d}"


def main() -> None:
    payload = json.loads(SOURCE.read_text())

    for index, point in enumerate(payload["points"]):
        mm_per_hour = PROFILE[index] if index < len(PROFILE) else 0.0
        point["mmPerHour"] = mm_per_hour
        point["class"] = intensity_class(mm_per_hour)
        point["isNow"] = point["offsetMinutes"] <= 0

    wet = [p for p in payload["points"] if p["mmPerHour"] >= RAIN_THRESHOLD]
    total_mm = sum(p["mmPerHour"] * p["intervalMinutes"] / 60.0 for p in payload["points"])
    peak = max(payload["points"], key=lambda p: p["mmPerHour"])
    starts, ends = wet[0], wet[-1]

    payload["summary"] = {
        "isRainingNow": False,
        "pill": f"in {minutes_label(starts['offsetMinutes'])}",
        "currentMmPerHour": 0.0,
        "currentClass": "dry",
        "currentLabel": "dry",
        "startsInMinutes": starts["offsetMinutes"],
        "startsInLabel": minutes_label(starts["offsetMinutes"]),
        "endsInMinutes": ends["offsetMinutes"] + ends["intervalMinutes"],
        "endsInLabel": minutes_label(ends["offsetMinutes"] + ends["intervalMinutes"]),
        "totalMm": round(total_mm, 2),
        "peakMmPerHour": peak["mmPerHour"],
        "peakInMinutes": peak["offsetMinutes"],
        "peakInLabel": minutes_label(peak["offsetMinutes"]),
    }
    payload["synthetic"] = True

    TARGET.write_text(json.dumps(payload, separators=(",", ":")) + "\n")
    print(f"wrote {TARGET.name}: {len(payload['points'])} frames, "
          f"{payload['summary']['pill']}, {payload['summary']['totalMm']} mm total")


if __name__ == "__main__":
    main()
