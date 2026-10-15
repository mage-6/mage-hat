---
name: mage-hat
description: MageHat, a tiny deterministic compiler for plain-HTML static sites, written in Rust and built to be driven by agents
kind: tool
version_file: Cargo.toml
links:
  repo: https://github.com/robo-kick/mage-hat
---

## Decisions
- Hard boundary: a static-site compiler, never an application framework. No server rendering, auth, databases, API routes, middleware, hydration, framework integrations, plugin system, or arbitrary code in templates. A project that needs those uses another framework
- Boring, deterministic, tiny, stable: a deliberately small fixed syntax; failing clearly beats producing bad output
- The build reads the clock for one thing: an item whose `date` is still to come is left out, which is how content is scheduled (a date, never a second field or a publish script); `--now` pins the clock for a reproducible build. The deploy every site runs is the reusable workflow in this repository, called from the `deploy.yml` (push, manual) and `drip.yml` (daily) that `init` and `new deploy` write; each deploy switches `drip.yml` on while items are scheduled and off when none are left, so an idle site runs nothing. A link to a scheduled item is written as its text until that item's date (scheduled_links.rs): items written ahead link to siblings dated later, and the alternative was a 404 on the live page or a wrapper script per site
- Agents never edit `dist/`; the loop is edit source, build, check, fix errors
- `each` and `if` attribute names, not `for`: `for` is a real attribute on `<label>`
- An unsuffixed page file is the default language only; `[item]` templates are shared by all languages
- Printing an undefined variable is an error, testing it with `if` is not, and `a or b` treats a missing `a` as false (that is how a default is written)
- A literal `href="/x/"` is localized per language; interpolated hrefs are not
- Component CSS is scoped with native `@scope`, the wrapper element is kept in the output with `display: contents`
- The `<picture>` the build wraps an `<img>` in is marked `data-mh` and given `display: contents` by a rule the site stylesheet always carries, so the `<img>` stays the flex or grid item; an authored `<picture>` is unmarked and keeps its box
- Layout styles do not reach into components; global CSS lives in `src/assets`, layered tokens, elements, variants
- Content files are flat (`id.md`, `id.pt-BR.md`); sub-folders are an error with a rename hint
- Components must wrap their markup in `<template>`; anything else at top level is an error
- Hashed asset copies are added and the originals kept, so unseen references (scripts, RSS) still work
- Every hashed file lives under `/_mh/` (site assets under `/_mh/a/`, referenced root-absolute) so one `_headers` rule caches them all for a year: Cloudflare's `_headers` matches paths by prefix only, not by extension, and allows 100 rules. A site's own `_headers` is kept with the rule appended
- Component CSS ships as one site-wide stylesheet, not one per component: a page linked 14 render-blocking files before 0.13.0. The few bytes of components a page does not use cost less than the requests
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
- The Google News sitemap (`news = true`) takes the items dated within two days of the newest one: the build has no clock, so the newest story stands in for today, which holds because a news site deploys when it publishes
- Generated robots.txt turns away training-only crawlers (CCBot, Bytespider, meta-externalagent) and every page defaults to `max-image-preview:large`; a site overrides robots.txt or llms.txt by shipping its own in `src/assets`
- `media prune` keeps objects younger than 14 days whatever the records say: a story on an open branch has uploaded its cover before its record is merged, and a deploy of the main branch must not delete it
- Analytics is `[analytics] endpoint`, opt-in and nothing else: the build ships the analytics skill's `beacon.js` verbatim (`src/beacon.js`, a copy, never a runtime fetch) in one hashed module every page loads, fires `page_view` on load and `conversion` on a click of `data-track="<kind>"`, sends nothing from localhost, and adds no byte when the table is absent
- Images load lazily except the first one MageHat handles on each page, which gets `loading="eager" fetchpriority="high"`: it is almost always the hero, and lazy loading it delayed the largest paint on every site. An author's own `loading` on that image wins, and a page whose first image is the author's own `<picture>` or that already has a `fetchpriority="high"` image promotes nothing (0.12.1 promoted the next image down on hand-built heroes)
- Item h2 and h3 headings get an id slugged by the archive-term rule (one slug rule) and the item exposes them as `toc`; Markdown's `{#id}` heading attributes stay off because the dialect is frozen, so an author's own id is a raw HTML heading. Ids are unique within an item; a page that renders several bodies or reuses a heading's id elsewhere is caught by `check`'s duplicate-id warning

## Open questions
- Should missing translation keys fall back to the default language instead of failing the build?
