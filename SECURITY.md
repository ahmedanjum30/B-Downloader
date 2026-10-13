# Security policy

## Supported versions

Security fixes are made for the latest release only.

## Reporting a vulnerability

Please **don't** open a public issue for security problems. Report them privately through GitHub's [private vulnerability reporting](https://github.com/ahmedanjum30/BDownloader/security/advisories/new) instead.

Include a description of the issue, steps to reproduce, and the impact you expect. You should get a reply within a week.

## Scope

Issues we do want to hear about include:

- a server response (headers, file names, redirects) that makes BDownloader write outside the chosen download folder
- TLS certificate checks being skipped
- crashes or memory exhaustion caused by malformed server responses
- the browser extension accepting download requests from web pages instead of only from the browser
