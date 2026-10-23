# Security

## Reporting a vulnerability

Please do not open a public issue for a security problem. Report it privately through GitHub
instead: **[Report a vulnerability](https://github.com/openOMSI-org/openMapper/security/advisories/new)**
(the *Security* tab of the repository). Only the maintainers see it.

Say what is affected, how to reproduce it and what an attacker could do with it. A small example (a
file, a request, a log) helps a lot. The maintainers are volunteers: we aim to answer within a week.

## Supported versions

Only the latest release.

## What counts

* **Files the editor opens**: a map, model, mod, zip, GPX / KML / GeoJSON or picture that makes
  openMapper read or write files outside the folders it works in, or run code, just by being
  opened or dropped on the window.
* **Downloads**: imagery, terrain, place search and OpenStreetMap requests going to a host the user
  did not choose, or keys for a user's own imagery source being sent anywhere else.
* **Packages**: anything that could make a release contain something that is not built from this
  repository.

Not security problems: crashes on a broken map file with no further effect, and anything that needs
the attacker to already control the user's computer.
