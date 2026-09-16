# Technical specifications and validation

This directory holds implementation notes, design research, protocol details,
and test reports. Reader-facing documentation lives in [www/docs](../www/docs).

- [Provider composition](composition.md)
- [Session protocol](session.md)
- [Platform implementations](backends.md)
- [Validation](validation.md)
- [Windows interactive results](windows-interactive-results.md)
- [Dependency preparation](dependencies.md)
- [SDK and CLI reference review](sdk-reference-audit.md)

Keep protocol details here and user workflows in `www/docs`. The CLI parser owns
command definitions; run `actuate spec` to export them instead of maintaining a
second checked-in command specification. Temporary probes, captures, and reports
belong under the ignored `target/` directory.

These documents include both implemented behavior and proposed work. Check each
document's status and validation limits before treating a capability as available.
