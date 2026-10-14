# base-camp-linux

A cleanroom specification for a Linux replacement of Mountain Base Camp, the Windows
configuration software for Mountain keyboards, keypads and mice.

This repository contains **specification and data only**. An implementer builds from
[docs/cleanroom/](docs/cleanroom/README.md) and must follow the rules at the top of that
README. The spec was written from an analysis of Base Camp 1.9.10; no analysis artifacts,
decompiled code or derived implementation are kept here.

| Path | Contents |
|---|---|
| [docs/cleanroom/](docs/cleanroom/README.md) | Functional spec, architecture, data model, per-device protocols, host actions, acceptance tests |
| [data/](data/README.md) | Key and layout tables needed for interoperability |

Nothing in the spec has been verified on real hardware yet. Each claim is tagged proven,
inferred or unknown.
