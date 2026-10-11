#!/usr/bin/env python3
"""Derive an app payload with the sun still up, for the render harness.

The captured app payload (test/fixtures/app-leiden.json) was taken after sunset, when the sun
and moon card is *right* to leave its daylight bar out and to say "night" — which means the bar
itself is never rendered by the harness. This script only moves the day into the afternoon: the
sky block gains a progress through the daylight and an event still ahead, and nothing else about
the payload changes.

It also moves the moon a few days on, from the one per cent sliver of the captured night to a
third lit. A one-pixel crescent is the *right* thing to draw on that night and almost invisible
in a render; a third lit is the same shape at a size a harness can check.

    python test/fixtures/make-app-daylight.py

The result, app-daylight-synthetic.json, is test data for the render harness. It is not a
captured response and must not be presented as one.
"""

import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
SOURCE = HERE / "app-leiden.json"
TARGET = HERE / "app-daylight-synthetic.json"


def main() -> None:
    payload = json.loads(SOURCE.read_text())
    sky = payload.get("sky")
    if not sky:
        raise SystemExit(f"{SOURCE.name} has no sky block — capture one with a newer binary first")

    # Late afternoon: two thirds of the way through the light, with an hour and a half of it left.
    sky["daylightProgress"] = 0.67
    sky["isDaylight"] = True
    sky["sunEventLabel"] = "sets in 1h 30m"

    # A crescent that can be seen: still waxing, still a crescent, a third of the disc lit.
    sky["moonIllumination"] = 0.32
    sky["moonLabel"] = "32% lit"

    payload["synthetic"] = True

    TARGET.write_text(json.dumps(payload, separators=(",", ":")) + "\n")
    print(f"wrote {TARGET.name}: daylight {sky['daylightProgress']}, moon {sky['moonIllumination']} lit")


if __name__ == "__main__":
    main()
