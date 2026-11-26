# MingShot

Screen capture and markup in the Snagit mould: a New button (or ⌃⇧2), a capture overlay, a real annotation editor and a
searchable library of everything you've captured. Mac first, then Windows. Free and MIT-licensed.

A [Four Ounces](https://fourounces.live) tool, alongside [MingTab](https://github.com/kratner/mingtab) and
[MingDesk](https://github.com/kratner/mingdesk).

**Status:** M0 done; M1 in progress (Mac, macOS 26+). New / ⌃⇧2 → overlay → drag a region, or click a window
(with or without its shadow) or a display → PNG on the clipboard, pixel-exact. Results:
[docs/M0.md](docs/M0.md), [docs/M1.md](docs/M1.md). Plan: [PLAN.md](PLAN.md).

```sh
npm install
npm run app          # build MingShot.app (signed with the "MingDesk Dev" cert if present)
npm run selftest     # pixel comparison with screencapture
```

Placeholder icons are MingDesk's until MingShot has its own.
