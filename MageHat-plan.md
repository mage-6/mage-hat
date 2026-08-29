# MageHat

A tiny, deterministic static-site compiler for plain HTML/CSS/JS.

## Core Principles

- Build-time only
- Output standard static HTML/CSS/JS/images
- Python CLI
- No Node/npm
- ImageMagick for image processing
- Minimal dependencies
- Stable, deliberately small syntax
- No plugin ecosystem
- No SSR
- No API routes
- No hydration
- No React/Vue/Svelte
- No arbitrary code execution inside templates

## Core Primitives

- Pages
- Layouts
- Components
- Props
- Slots
- Collections/content
- Translations
- Generators/pagination
- Assets
- Build/check

## Components

Self-contained component files containing:

- template
- style
- optional script

Components support:

- props/data
- loops
- conditions
- slots
- nesting
- build-time rendering
- component-local CSS
- optional vanilla JS for browser interaction
- JS/CSS included only when the component is actually used
- simple scoped styling, preferably `:host`-style semantics

## Templates

Keep syntax small and fixed:

- `{{ variable }}`
- `{{ t.key }}`
- loops
- conditions
- components/includes
- props
- slots

No arbitrary Python inside templates.

## i18n

Built in from v1.

Structure example:

```text
src/i18n/
├── en.json
├── pt-BR.json
└── es.json
```

Translated content grouped by identity:

```text
src/content/blog/article-id/
├── en.md
├── pt-BR.md
└── es.md
```

Automatically handle:

- translated URLs/slugs
- `hreflang`
- canonical URLs
- translated metadata
- sitemap entries
- missing translation validation

UI translation strings and full content translations should remain separate concepts.

## Content / Blog

Markdown + frontmatter.

Support:

- collections
- tags/categories
- archives
- RSS
- pagination
- declarative generated pages
- simple content schema validation

## SEO

Generate at build time:

- title
- description
- canonical
- Open Graph
- structured metadata when configured
- sitemap.xml
- robots.txt
- `hreflang`

## Images

Use ImageMagick for:

- resize
- WebP
- AVIF
- responsive variants
- compression
- metadata stripping
- thumbnails
- cached transformations

Do not reprocess unchanged images.

## CLI

```bash
magehat init
magehat dev
magehat build
magehat check
magehat inspect --json
magehat clean
```

## Incremental Builds

Maintain a small local build cache.

Track:

- changed source files
- dependencies
- generated outputs
- component usage
- image transformations

Examples:

- article changed -> rebuild article + affected indexes/RSS/sitemap
- translation changed -> rebuild affected language pages
- shared layout/header changed -> rebuild dependent pages
- image unchanged -> do not rerun ImageMagick

Keep dependency rules explicit and deterministic. Avoid arbitrary extensibility that makes dependency tracking impossible.

## Validation

`magehat check` should catch:

- missing translations
- undefined variables
- invalid content metadata
- broken internal links
- missing assets
- duplicate URLs
- duplicate IDs where practical
- missing required SEO fields
- invalid component usage
- invalid template syntax

Prefer failing clearly over silently producing bad output.

## Project Structure

Example:

```text
website/
├── site.toml
├── src/
│   ├── pages/
│   ├── content/
│   ├── layouts/
│   ├── components/
│   ├── i18n/
│   ├── data/
│   └── assets/
├── .magehat/
│   └── cache
└── dist/
```

`dist/` is generated and should never be edited manually.

## Distribution

MageHat should live in its own GitHub repository.

Preferred usage:

```bash
uv tool install magehat
```

Then:

```bash
magehat init mysite
magehat dev
magehat build
magehat check
```

Website projects remain separate repositories.

Do not clone/copy the MageHat framework into every website.

Optional portable distribution can also be supported as a single Python file or standalone executable.

## AI Usage

Include a small `SKILL.md` with MageHat.

Recommended agent workflow:

```text
magehat inspect --json
-> edit source files only
-> magehat build
-> magehat check
-> fix errors
```

Agents should never edit `dist/`.

Keep the skill short. The framework itself should expose enough structure through `inspect --json` that agents do not need a huge instruction document.

## Hard Boundary

MageHat is a static-site compiler, not an application framework.

Do not add:

- server-side rendering
- authentication
- databases
- API routes
- middleware
- reactive application state
- framework hydration
- React/Vue/Svelte integrations
- general-purpose plugin systems
- arbitrary build code inside templates

If a project needs those capabilities, use another framework.

The goal is to keep MageHat boring, deterministic, tiny, stable, and future-proof.
