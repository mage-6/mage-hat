---
name: mage-hat
description: MageHat, a tiny deterministic compiler for plain-HTML static sites, written in Rust and built to be driven by agents
kind: tool
version_file: Cargo.toml
links:
  repo: https://github.com/mage-6/mage-hat
---

## Next
- A Google News sitemap needs "the last two days", which the clockless build cannot know; open until a site needs Google News (Discover does not)

## Decisions
- Hard boundary: a static-site compiler, never an application framework. No server rendering, auth, databases, API routes, middleware, hydration, framework integrations, plugin system, or arbitrary code in templates. A project that needs those uses another framework
- Boring, deterministic, tiny, stable: a deliberately small fixed syntax; failing clearly beats producing bad output
- Agents never edit `dist/`; the loop is edit source, build, check, fix errors
- `each` and `if` attribute names, not `for`: `for` is a real attribute on `<label>`
- An unsuffixed page file is the default language only; `[item]` templates are shared by all languages
- Printing an undefined variable is an error, testing it with `if` is not, and `a or b` treats a missing `a` as false (that is how a default is written)
- A literal `href="/x/"` is localized per language; interpolated hrefs are not
- Component CSS is scoped with native `@scope`, the wrapper element is kept in the output with `display: contents`
- Layout styles do not reach into components; global CSS lives in `src/assets`, layered tokens, elements, variants
- Content files are flat (`id.md`, `id.pt-BR.md`); sub-folders are an error with a rename hint
- Components must wrap their markup in `<template>`; anything else at top level is an error
- Hashed asset copies are added and the originals kept, so unseen references (scripts, RSS) still work
- Minification only shortens whitespace runs, never removes them; tags and raw elements are untouched
- Full rebuild every time; only image encoding is cached (`.magehat/cache`, keyed by content hash)
- JSON-LD: `{{ }}` inserts JSON-string-escaped text and the author writes the quotes; trailing commas are forgiven and the result is re-parsed
- Icons download into `src/icons` on first use only; a set mapped in `[icons]` is never downloaded into
- Ready-made components are copied, not resolved from the binary: the site owns and restyles its copy; the scaffold includes them from `library/` so they cannot drift, and a test builds every entry
- Output URLs stay root-absolute (no file:// preview); a `--portable` build is a possible later option for bundled help pages
- Syntax highlighting is declined
- Lists and archives are declared in page metadata (`list`, `per-page`, `by`), not in site.toml or the file name, so a page says what it is where its title is; pages live at `/page/N/` under the page's URL, archive terms at `/<page>/<slug>/` with Latin accents folded; the `pager` and term variables are build-time values, never a filter or a function
- `limit` on `each` is the one list operator: a news home page needs the first N, and a filter syntax was the alternative
- Media (`[media]`, `magehat media add`, `media:<name>`) is the one piece that talks to a bucket, and only from a command, never from the build: the build reads committed records (`src/media/<name>.json`) and stays offline. Same shape as fonts and icons: network once, commit the result. Opt-in per site, because a landing page with a dozen fixed images gains nothing from a bucket; a site whose images grow with its content (a cover per story) turns it on
- The media bucket is spoken to through the Cloudflare API's object endpoints with the account's API token (what wrangler uses), not S3: no second credential to create, store or rotate
- Image variants for media are a fixed ladder (400, 800, 1200, 1600, 2000, 2400, then the source width) encoded at upload, since the build cannot encode a file it does not have; a record written today still matches a build made in two years
- `media prune` keeps objects younger than 14 days whatever the records say: a story on an open branch has uploaded its cover before its record is merged, and a deploy of the main branch must not delete it

## Open questions
- Should missing translation keys fall back to the default language instead of failing the build?
