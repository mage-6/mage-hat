# MageHat {{VERSION}}

A tiny, deterministic compiler for plain HTML sites.

A site is a folder of HTML. MageHat fills in the few things HTML cannot
express on its own (components, loops, conditions, translations, collections)
at build time and writes ordinary HTML, CSS and JS to `dist/`. No framework,
no runtime, no plugin system, and a syntax small enough to fit on one page.

This document is that page, and it is the complete reference: if something
is not here, it does not exist. The compiler carries its own copy, so
`magehat -h` prints all of it even when this file is nowhere to be found.
Never edit `dist/`. `magehat check` tells you what is wrong and how to fix
it; run it after every change.

## Install

There is no installer and nothing to install alongside it. `magehat.exe` is
self-contained: put it anywhere and either call it by its full path or add
that folder to your `PATH`:

    setx PATH "%PATH%;C:\tools\magehat"

Open a new terminal afterwards, then check it answers:

    magehat --version

## Quick start

    magehat init mysite
    cd mysite
    magehat dev

`dev` serves the site on http://localhost:8080, rebuilds when a file changes
and reloads the browser. Edit `src/pages/index.html` and watch it update.
When the site is ready:

    magehat build

That writes the finished site to `dist/`: ordinary HTML, CSS, JS and images
that any static host will serve.

## A site from nothing

`init` gives you a sample site, but four files are a complete one.
`site.toml`:

    name = "Hat Co"
    url = "https://example.com"
    languages = ["en"]

`src/components/base.html`, the layout (a component whose template is a
whole document; pages land in the `<slot>`):

    <template>
    <!doctype html>
    <html lang="{{ lang }}">
    <head>
      <meta charset="utf-8">
      <meta name="viewport" content="width=device-width, initial-scale=1">
      <title>{{ page.title }} · {{ site.name }}</title>
      <meta name="description" content="{{ page.description }}">
      <link rel="stylesheet" href="/site.css">
    </head>
    <body>
      <nav><a href="/">{{ site.name }}</a></nav>
      <main><slot></slot></main>
    </body>
    </html>
    </template>

`src/pages/index.html`, the home page:

    <title>Home</title>
    <meta name="description" content="Hats, made by hand.">

    <x-base>
      <h1>Hats, made by hand.</h1>
      <p>Plain HTML goes here.</p>
    </x-base>

`src/assets/site.css`, the global styles (served at `/site.css`). Then
`magehat check`, fix anything it reports, `magehat build`, deploy `dist/`.

## Files

    site.toml            name, url, languages, collections
    src/pages/           one HTML file per page: about.html -> /about/
    src/components/      one HTML file per component: card.html -> <x-card>
    src/content/         collections only (blog posts, docs); optional
    src/i18n/en.json     UI strings, read with {{ t.key }}; optional
    src/data/            optional JSON or TOML files, read as data.<name>.<key>
    src/icons/           SVG files, a folder per set: icons/lucide/x.svg -> icon="lucide:x"
    src/assets/          copied to the site root as-is: assets/site.css -> /site.css
    src/assets/fonts/    font files and their stylesheet, saved from a Google Fonts link

`magehat new page <name>`, `new component <name>` and `new item <collection> <id>`
create correctly shaped files; prefer them over writing files from scratch.

## Pages

A page is an HTML file. Its content lives in it; use components inline.
Wrap it in the layout:

    <title>About us</title>
    <meta name="description" content="Who we are.">

    <x-base>
      <h1>About us</h1>
      <p>We make hats.</p>
    </x-base>

Leading `<title>` and `<meta name= content=>` elements are the page's
metadata, available as `page.title`, `page.description`, `page.<name>`.
Every page needs a title and a description. `404.html` becomes `/404.html`.
Folders nest: `src/pages/shop/hats.html` -> `/shop/hats/`.

## Components

`src/components/card.html`, always in this shape:

    <template>
      <article class="card">
        <h2>{{ title }}</h2>
        <slot></slot>
      </article>
    </template>
    <style>.card { padding: 1rem }</style>
    <script>/* optional browser JS, included only on pages that use it */</script>

Use it as `<x-card title="Hello">body</x-card>`. Attributes are props;
`title="{{ post.title }}"` passes a value. Children fill `<slot>`; an element
with `slot="name"` fills `<slot name="name">`. Components see only their props
and the globals, never the caller's variables. A prop the caller may omit is
tested with `if="prop"`.

The `<x-card>` element stays in the output, so a script can
`customElements.define('x-card', ...)`. Styles are scoped to the component's
own markup (native `@scope`) and do not reach into nested components; global
styles go in `src/assets/site.css`.

## Expressions

    {{ title }}                text, HTML-escaped ({{ post.body }} is trusted HTML)
    {{ t.nav.home }}           translation string from src/i18n/<lang>.json
    <li each="post in blog">   repeat the element per item
    <p if="post.featured">     keep the element only if true
    <template each="..">       repeat or drop children with no wrapper element
    <svg icon="lucide:menu">   an icon, inlined at build time (see Icons)

Expressions: dotted paths, 'strings', numbers, true, false, null, `not`,
`and`, `or`, `==`, `!=`. That is the entire syntax. There are no filters,
no arithmetic, no functions, no `else`, no `{% %}` blocks, no includes:
compute values in metadata or `src/data`, and write a second element with
`if="not cond"` instead of else. Printing an undefined variable is an error;
testing one with `if` is not. Globals: `site`, `t`, `lang`, `page`, `data`,
and one list per collection.

## Collections

A folder of items with the same shape, ordered by `date` (newest first):

    src/content/blog/hats-in-2026.md          default language
    src/content/blog/hats-in-2026.pt-BR.md    the same item in Portuguese

Markdown items start with a `---` metadata block (`title`, `description`,
`date`, `slug`, `tags: [a, b]`, `draft: true`, anything else). HTML items
start with `<title>` and `<meta>` instead. Every item exposes `id`, `url`,
`title`, `body` (rendered HTML), `translations`, and its metadata.
Components can be used inside content: `<x-figure src="..."></x-figure>`.

List items with `each="post in blog"`. Item pages come from
`src/pages/blog/[post].html`, which receives the item as `post` and takes
`page.title` and `page.description` from it:

    <x-base>
      <article>
        <h1>{{ post.title }}</h1>
        {{ post.body }}
      </article>
    </x-base>

Set `feed = true` under `[collections.blog]` in site.toml to get `/blog/feed.xml`.

## Images and assets

    <img src="/photos/hat.jpg" alt="A hat" width="600">

Write a plain `<img>` pointing at a JPEG, PNG or WebP under `src/assets`.
MageHat fills in width and height, adds lazy loading and a WebP version,
and with a `width` generates resized 1x and 2x variants. An `<img>` you put
inside your own `<picture>` is left alone. Every asset also gets a
content-hashed copy and references are rewritten to it; keep writing the
plain path. Output HTML and CSS are minified. Encoded images are cached in
`.magehat/cache` by content hash, the only cache there is.

## Icons

    <svg icon="lucide:shield" aria-hidden="true"></svg>

Any icon from the Iconify sets (https://icon-sets.iconify.design), named
`set:name`. The element is replaced by the icon's own SVG, inline, with your
attributes (class, aria-hidden, width) on it; it is 1em tall and takes the
text colour. Icons are files in `src/icons/<set>/<name>.svg`. One that is not
there yet is downloaded once, into that folder, the first time a build meets
it; commit the folder and no build needs the network again. Your own SVGs go
in the same place under a set name of your choice (`icon="brand:logo"`).

## Fonts

    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter:wght@400;700&display=swap">

Put the link Google Fonts hands out in the layout, as it is. The first build
fetches that stylesheet, saves the font files it names under
`src/assets/fonts/<family>/`, writes `src/assets/fonts/<slug>.css` next to
them and points the link there; preconnect hints for Google's hosts are
dropped. Commit the folder: no later build asks Google, and no visitor
connects to Google either. Only the families and weights in the link are
saved, in every script Google offers; a browser fetches the ones a page
needs. Fonts you already have go in the same folder with a stylesheet of
your own. `@import` of Google Fonts in CSS is not converted; `check` says so.

## Structured data

    <script type="application/ld+json">
    {
      "@context": "https://schema.org",
      "@type": "Article",
      "headline": "{{ post.title }}",
      "url": "{{ site.url }}{{ post.url }}",
      "keywords": [<template each="tag in post.tags">"{{ tag }}",</template>]
    }
    </script>

A JSON-LD script is a template like the rest of the page: `{{ }}` inserts
text escaped for a JSON string (you write the quotes, as in an attribute),
`<template each="..">` repeats a piece, `<template if="..">` drops one, and
a comma a loop leaves before `]` or `}` is forgiven. The result must be
valid JSON or the build fails and says where; it is written compact. Every
other `<script>` and `<style>` is left exactly as written.

## Languages

    languages = ["en", "pt-BR"]     in site.toml; the first is the default

The default language lives at `/`, others under `/pt-br/`. A page exists in
a language when its file does: `about.html` (default), `about.pt-BR.html`.
Item pages (`[post].html`) are shared by all languages. Write links as they
are in the default language (`href="/about/"`); MageHat localizes them on
every other language's pages. UI strings live in `src/i18n/<lang>.json`
(nested objects, read as `t.nav.home`); a key missing in a language fails
the build. Canonical, hreflang, sitemap.xml and robots.txt are generated.
`magehat check` reports missing translations.

## Commands

    magehat                   this list of commands
    magehat -h                the manual: the complete reference
    magehat check [--json]    errors and warnings, each with its fix: undefined
                              variables, foreign syntax, unclosed tags, missing
                              translations, broken links, missing title,
                              description or alt text, duplicate ids
    magehat build [--json]    write dist/
    magehat new ...           page <name> | component <name> | item <coll> <id>
                              (--lang xx for a translation of a page or item)
    magehat dev [--port N]    local server with live reload
    magehat inspect [--json]  pages, components with props and a usage example,
                              collections, languages, i18n keys
    magehat init [dir]        sample site with a page, layout, post and image
    magehat clean             remove dist/ and the image cache
    magehat --version         print the version

Workflow: `magehat inspect --json`, create files with `magehat new`, edit
under `src/`, `magehat check`, fix what it reports, repeat. Deploy `dist/`
to any static host. `check`, `build` and `inspect` report findings as
`{file, line, message, fix, excerpt}` under `--json`, which is what makes
MageHat pleasant to drive from a script or a coding agent.

## Principles

- Source files are readable HTML. If MageHat disappeared, the site would
  still make sense.
- One way to do each thing. Syntax borrowed from other template languages is
  recognised and answered with the MageHat equivalent.
- The surface is append-only. Nothing that works today will stop working.
- Same source, same bytes. No clocks, no randomness, sorted everything.
- One build, no cache to invalidate. A full rebuild takes milliseconds.

## License

MIT.
