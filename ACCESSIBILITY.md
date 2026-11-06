# Accessibility

Taba is a messaging server operated from a command line, an HTTP API and a terminal interface (`taba-cli`), with
documentation published as Markdown.

## Our commitment

- The documentation is plain Markdown, so it works with screen readers.
- Command-line output and the HTTP API do not depend on colour alone to convey meaning.

## Known limitations

- The terminal interface of `taba-cli` draws charts with characters and colours, which a screen reader may not
  read well. Every value shown there is also available through the API and the plain CLI commands.
- The documentation diagrams are SVG images; their meaning is described in the surrounding text.

## Reporting a barrier

Open an [issue](https://github.com/rafaelpontani/taba/issues/new/choose) describing the barrier, the tool you use
and what you expected, and we will fix what we can.
