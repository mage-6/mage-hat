## Goal
MageHat: a tiny compiler for plain-HTML sites, owned by us so nothing ever breaks on upgrade, optimized for agents.
v0.6.0: ready-made components (`magehat add`), the SEO head and schema in the scaffold, fixes from the Mage6 blind test.

## Steps
- [x] Rust port; Python removed
- [x] Agent-first: canonical forms, errors with fix and excerpt, foreign-syntax detection, --json, `new`
- [x] Images, hashed assets, minified HTML/CSS, markup lints
- [x] JSON-LD scripts as templates; icons by Iconify name; Google Fonts served locally
- [x] Windows and Linux releases from GitHub Actions; repo public under mage-6
- [x] Blind test (Mage6 site by an Opus agent): one self-inflicted error, friction list collected
- [x] 0.6.0: font dedupe by URL, package.ps1 UTF-8, `or` as default, `order` key, noindex out of sitemap, anchor and og:image checks, `[assets]`/`[icons]` folders
- [x] 0.6.0: `magehat add` with library/faq.html; scaffold gets the full head, Organization+WebSite schema, layered site.css, favicon
- [ ] 0.6.0: tests, golden regen, tag and release, install the exe
- [ ] Rewrite the `website` skill against 0.6.0 (drop Astro, ui/ folder gone, CSS layering rule, Cloudflare recipe); design/identity skill lines that point at SEOHead
- [ ] Global CLAUDE.md `## Websites` paragraph: only after the user reviews it
- [ ] Mage6 site: move brand assets to `[assets]`/`[icons]`, drop the junctions, `.btn` -> `button`
- [ ] Pagination, tag/archive pages (not in v1); syntax highlighting (declined)

## Decisions
- `each`/`if` attribute names, not `for`: `for` is a real attribute on `<label>`.
- Unsuffixed page file = default language only; `[item]` templates shared by all languages.
- Printing an undefined variable is an error, testing it with `if` is not, and `a or b` treats a missing `a` as false (that is how a default is written).
- Literal `href="/x/"` is localized per language; interpolated hrefs are not.
- Component CSS scoped with native `@scope`, wrapper element kept in output, `display: contents`.
- Layout styles do not reach into components; global CSS lives in src/assets, layered tokens > elements > variants.
- Content files are flat (`id.md`, `id.pt-BR.md`); sub-folders are an error with a rename hint.
- Components must wrap markup in `<template>`; anything else at top level is an error.
- Hashed asset copies are added, originals kept, so unseen references (scripts, RSS) still work.
- Minification only shortens whitespace runs, never removes them; tags and raw elements untouched.
- Full rebuild every time; only image encoding is cached (.magehat/cache, keyed by content hash).
- JSON-LD: `{{ }}` inserts JSON-string-escaped text, the author writes the quotes; trailing commas forgiven, result re-parsed.
- Icons download into src/icons on first use only; a set mapped in `[icons]` is never downloaded into.
- Ready-made components are copied, not resolved from the binary: the site owns and restyles its copy; the scaffold includes them from library/ so they cannot drift; a test builds every entry.
- Output URLs stay root-absolute (no file:// preview); a `--portable` build is a possible later option for bundled help pages.
- Cargo runs from PowerShell on this machine (Git Bash shadows link.exe). Edit files with Bash tools, not PowerShell Set-Content.

## Open questions
- Should missing translation keys fall back to the default language instead of failing the build?
