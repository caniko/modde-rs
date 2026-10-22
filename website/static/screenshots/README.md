# Screenshots

These images are generated headlessly and reproducibly from modde itself —
no GPU, display, or manual capture needed — via the `screenshot` cargo feature:

```sh
modde dev screenshot --all --out website/static/screenshots
# or one screen at a time:
modde dev screenshot --screen mod-list --out website/static/screenshots/mod-list.png
```

(Build with `cargo run -p modde --features screenshot -- dev screenshot ...`.)

Served at `https://modde.tartanoglu.com/screenshots/` and referenced by the landing page and
by `dist/com.tartanoglu.modde.metainfo.xml` (Flathub/AppStream). Regenerate
whenever the UI changes:

- `library.png` — the owned-game Library (start screen)
- `mod-list.png` — active profile's mod list
- `downloads.png` — download queue (`downloads` screen)
- `fomod-wizard.png` — FOMOD installer wizard
