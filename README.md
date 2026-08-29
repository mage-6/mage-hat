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

## Use

    magehat init mysite
    cd mysite
    magehat check          # errors and warnings, each with its fix
    magehat new page team  # files with the right shape: page, component, item
    magehat dev            # http://localhost:8080 with live reload
    magehat build          # writes dist/

The whole language is [SKILL.md](SKILL.md). `magehat init` writes it into
the new site as `AGENTS.md` and `.claude/skills/magehat/SKILL.md`, and
`magehat skill --write` does the same in any folder, so an agent opening
the project has the complete reference and nothing else to learn. The doc
is enough to build a site from an empty folder; a test builds the four
files it shows and checks them. `magehat inspect --json` describes a specific site; `check --json`
and `build --json` report findings as `{file, line, message, fix, excerpt}`.

## What the build does for you

- Components with scoped CSS and per-page JS, included only where used.
- Translations, localized links, canonical and hreflang, sitemap, RSS.
- Images: a plain `<img>` gets its size, lazy loading, a WebP source and
  resized 1x/2x variants, cached in `.magehat/cache` by content hash.
- Content-hashed asset names with references rewritten; minified output.
- `check` finds undefined variables, syntax from other template languages,
  unclosed tags, broken links, missing titles, descriptions and alt text,
  duplicate ids, and missing translations, and says how to fix each one.

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

`tests/golden/scaffold` is a byte-for-byte snapshot of the sample site;
regenerate it with `UPDATE_GOLDEN=1 cargo test` after an intended change.
On this machine run cargo from PowerShell: under Git Bash, `link.exe`
resolves to coreutils `link` instead of the MSVC linker.
