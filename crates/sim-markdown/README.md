# Shared Markdown presentation model

`parse()` converts CommonMark into renderer-independent blocks, styled spans,
and explicit link destinations using `pulldown-cmark` 0.13.4. The native viewer
renders headings, bold/italic emphasis, lists, block quotes, inline/fenced code,
rules and compact reference buttons. Tables have a text-row fallback. Images
are represented by their alternative text, without fetching remote assets.
Raw HTML is inert text; requesting generated HTML is unnecessary.

The original Markdown is retained in the annotation document and REST API.
Display formatting never rewrites a comment. Hosts choose how links behave;
`read_source()` supplies bounded UTF-8 excerpts for local source references,
resolves symlinks, rejects paths outside the project, and highlights the requested
line. Call it off the UI thread. `:line`, `:line:column`, and `#Lline` locations
are supported. Missing files and stale line numbers report an error.

The reusable Bevy renderer is `sim-spatial::markdown::render`. Callers supply a
font/color theme and a link-to-component callback; the renderer knows nothing
about builder state or REST. Builder file links use an asynchronous read-only
source preview in the inspector. HTTP(S) links open the system browser only when
activated. Part links continue to use shared discussion selection/hover actions.

`cargo test -p sim-markdown` covers styled spans, compact source citations,
fenced code, inert HTML, source location parsing, and project-bound source reads.
