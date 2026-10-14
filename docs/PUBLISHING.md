# Publishing the nowcast widget to the official Omarchy plugin listings

DRAFT — nothing here has been executed. Read, then run.

Two destinations, in this order:

## 1. plugins.omarchy.org (the official marketplace by Brian Blakely)

It requires: public repo, `manifest.json` at repository root, README + license,
no symlinks. The submission is an issue with repository link, category, tags;
automated validation checks the current commit before a maintainer approves.

Prereq (already prepared in this repo, but must actually run once):
- [ ] `gh repo create Hizoul/omarchy-weather-nowcast --public` (the empty mirror repo)
- [ ] Push this repo to `Hizoul/rusty-bui` with the mirror workflow
- [ ] Add `MIRROR_TOKEN` secret (fine-grained PAT, Contents:write on the mirror only)
- [ ] Tag `v0.1.0` so the release workflow attaches prebuilt binaries
- [ ] Wait for the mirror workflow (green), then verify:
      `omarchy plugin add https://github.com/Hizoul/omarchy-weather-nowcast.git`
      against a validate-only dry check, plus `omarchy plugin validate` output

Issue form content (paste into the form at plugins.omarchy.org):

    Repository: https://github.com/Hizoul/omarchy-weather-nowcast
    Name:       Weather nowcast
    Category:   bar widget / weather
    Tags:       weather, nowcast, rain, radar, bar-widget

    Weather nowcast is a bar widget for Omarchy 4: rain for the next two hours,
    at radar resolution, with a detail popout (chart + short explanation +
    provider attribution) on click or SUPER+SHIFT+R.

    Omarchy already has weather widgets for conditions and forecasts; what it
    lacked was nowcasting — "will it rain on me in the next two hours", which
    needs weather radar rather than a forecast model. Radar nowcast widgets so
    far exist for Japan (hakatamike.weather), the Netherlands + Belgium
    (io.github.dzanaga.omarchy-rain-radar-belgium-netherlands-widget) and
    Denmark (io.github.diegogardini.omarchy-rain-radar-denmark-widget). This
    widget serves KNMI radar for the Netherlands, DWD radar for Germany, and
    falls back to Open-Meteo's minutely-15 model everywhere else — a near-global
    nowcast, with whichever provider answered named in the tooltip and popout.

    It requires the small `rustybui` binary (fetching, caching, rate limiting),
    installable via ./install.sh in the source repo or a prebuilt release
    tarball; instructions are in the plugin README.

## 2. Okomart catalog (plugins.txt PR)

One line per canonical .git URL. PR target:
`brianblakely/omarchy-plugins` → `plugins.txt` — simply append:

    https://github.com/Hizoul/omarchy-weather-nowcast.git

The catalog generator reads the root manifest's `name`, `author` and
`description`, so the manifest copy in the mirror is what is shown. The listing
will read roughly:

    | Weather nowcast | Matthias MB | Rain for the next two hours at radar
    resolution (KNMI NL, DWD DE, Open-Meteo minutely-15 fallback), bar pill with
    detail popout. |

## Note on the "only Japan / NL+BE nowcast" claim

The README/README-mirror copy deliberately says the pill's radar nowcast comes
from KNMI (NL) and DWD (DE) with worldwide model fallback, and only claims that
radar-based nowcast was previously available for Japan and the Netherlands.
No Belgium claims: KNMI's scans partially reach Belgium, but that is coverage
incidence, not a promised service. If you later test and document Belgian
quality, it is a one-line change.