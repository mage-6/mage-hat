---
name: magehat
description: Build and edit a MageHat site. MageHat compiles a folder of plain HTML (plus a few blanks filled in at build time) into a static site. Use when a project has a site.toml and src/pages, or when asked to create a static website with MageHat.
---

# MageHat

A site is a folder of HTML. MageHat fills in the few things HTML cannot
express, at build time, and writes plain HTML/CSS/JS to `dist/`.
Never edit `dist/`. This file is the complete reference: if something is
not here, it does not exist. `magehat check` tells you what is wrong and
how to fix it; run it after every change.

## A site from nothing

Four files make a complete site. `site.toml`:

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
`magehat init` writes a larger sample site if you want one to start from.

## Files

    site.toml            name, url, languages, collections
    src/pages/           one HTML file per page: about.html -> /about/
    src/components/      one HTML file per component: card.html -> <x-card>
    src/content/         collections only (blog posts, docs); optional
    src/i18n/en.json     UI strings, read with {{ t.key }}; optional
    src/data/            optional JSON or TOML files, read as data.<name>.<key>
    src/assets/          copied to the site root as-is: assets/site.css -> /site.css

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
plain path. Output HTML and CSS are minified.

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

    magehat check [--json]    errors and warnings, each with its fix: undefined
                              variables, foreign syntax, unclosed tags, missing
                              translations, broken links, missing title,
                              description or alt text, duplicate ids
    magehat build [--json]    write dist/
    magehat new ...           page <name> | component <name> | item <coll> <id>
    magehat dev [--port N]    local server with live reload
    magehat inspect --json    pages, components with props and a usage example,
                              collections, languages, i18n keys
    magehat init [dir]        sample site with a page, layout, post and image
    magehat skill [--write]   print this reference, or write it into the site

Workflow: `magehat inspect --json`, create files with `magehat new`, edit
under `src/`, `magehat check`, fix what it reports, repeat. Deploy `dist/`
to any static host.
