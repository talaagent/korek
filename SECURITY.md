# Security policy

Security fixes target the latest published release. Report vulnerabilities
privately through [GitHub private vulnerability reporting](https://github.com/talaagent/korek/security/advisories/new).
If that form is unavailable, email **github@igm.pub**. Avoid public disclosure
until a fix is available.

Include the affected version, the generator and options involved, and a
minimal synthetic schema that reproduces the problem. Relevant issues include
generated code that skips or weakens a declared validation rule, code
injection through schema strings (docs, defaults, patterns, CEL), and a
plugin response that gets the driver to write outside the output directory.

korek runs the plugins you configure (`korek-gen-*` on `PATH`, `--plugin`)
as ordinary programs with your permissions; only run plugins you trust.
Vulnerabilities in protovalidate, Zod, buf or other downstream tools should
be reported to those projects.
