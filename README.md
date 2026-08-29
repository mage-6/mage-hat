# MageHat

A tiny, deterministic compiler for plain HTML sites, built for coding agents.

A site is a folder of HTML. MageHat fills in the few things HTML cannot
express on its own (components, loops, conditions, translations, collections)
at build time and writes ordinary HTML, CSS and JS to `dist/`. No framework,
no runtime, no plugin system, and a syntax small enough to fit on one page.

## Build

    cargo build --release

The result is a single executable at `target/release/magehat.exe` with no
runtime dependencies. Put it wherever your tools live.

## Package a release

    pwsh packaging\package.ps1                                     this machine
    pwsh packaging\package.ps1 -Target x86_64-unknown-linux-musl   another target

Builds the release binary and assembles `release/magehat-<version>-<os>-<arch>/`
plus an archive of it (zip on Windows, tar.gz elsewhere): the executable plus
`README.md` and `SKILL.md`, both generated from `MANUAL.md` (the skill is the
same text under the frontmatter in `packaging/skill-header.md`). Bump
`version` in Cargo.toml first; it is the only place the version lives, and
the script refuses to package a binary that reports a different one or that
does not print the manual for it.

Pushing a tag `v<version>` runs `.github/workflows/release.yml`, which
packages Windows and static Linux (musl) builds and attaches them to a
GitHub release. The Linux one is what a hosting build step (Cloudflare,
GitHub Pages) downloads to run `magehat build`.

## Use

    magehat -h             the manual, printed from inside the executable
    magehat init mysite
    cd mysite
    magehat check          # errors and warnings, each with its fix
    magehat new page team  # files with the right shape: page, component, item
    magehat dev            # http://localhost:8080 with live reload
    magehat build          # writes dist/

The whole language is [MANUAL.md](MANUAL.md), and the binary embeds it, so
an executable on its own still carries its complete reference with nothing
to look up. It is enough to build a site from an empty folder; a test builds
the four files it shows and checks them. `magehat inspect --json` describes a
specific site; `check --json` and `build --json` report findings as
`{file, line, message, fix, excerpt}`.

## What the build does for you

- Components with scoped CSS and per-page JS, included only where used.
- Ready-made components (`magehat add faq`): copied into the site, built by
  a test on every release, themed through custom properties.
- Translations, localized links, canonical and hreflang, sitemap (minus
  `noindex` pages), RSS.
- Folders outside `src/` served or used as icon sets through `[assets]` and
  `[icons]` in site.toml, for a brand kit shared between projects.
- Icons: `<svg icon="lucide:shield">` inlines any Iconify icon; the SVG is
  downloaded once into `src/icons/` and is source from then on.
- Fonts: a Google Fonts `<link>` is fetched once into `src/assets/fonts/`
  and served from the site; visitors never connect to Google.
- Structured data: a JSON-LD script is a template, checked as JSON.
- Images: a plain `<img>` gets its size, lazy loading, a WebP source and
  resized 1x/2x variants, cached in `.magehat/cache` by content hash.
- Content-hashed asset names with references rewritten; minified output.
- `check` finds undefined variables, syntax from other template languages,
  unclosed tags, broken links and anchors, missing titles, descriptions and
  alt text, duplicate ids, relative social images, and missing translations,
  and says how to fix each one.

## Principles

- Source files are readable HTML. If MageHat disappeared, the site would
  still make sense.
- One way to do each thing. Syntax borrowed from other template languages is
  recognised and answered with the MageHat equivalent.
- The surface is append-only. Nothing that works today will stop working.
- Same source, same bytes. No clocks, no randomness, sorted everything.
- One build, no cache to invalidate. A full rebuild takes milliseconds;
  only image encoding is cached, by content hash.

## Develop

    cargo test

Documentation lives in `MANUAL.md`; the package's README and SKILL are built
from it, so edit it there and never in `release/`.
`tests/golden/scaffold` is a byte-for-byte snapshot of the sample site;
regenerate it with `UPDATE_GOLDEN=1 cargo test` after an intended change.
On this machine run cargo from PowerShell: under Git Bash, `link.exe`
resolves to coreutils `link` instead of the MSVC linker.
