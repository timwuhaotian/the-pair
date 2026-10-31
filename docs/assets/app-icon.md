# App icon

Source of truth (edit these, not the PNGs):

- `app-icon-dark.svg` — shipped app icon: near-black tile `#2D2D2D` → `#1A1A1A`, white double-check
- `app-icon-light.svg` — light counterpart: `#FFFFFF` → `#F0EEEA` tile, `#141312` double-check

Style follows [oMLX](https://omlx.ai/) (`icon-rounded-dark.svg`): one rounded tile, one monochrome glyph, no colour, no detail that dies below 32px.

The glyph is a double-check (✓✓), not `</>`: two agents each check the work once — Mentor plans/reviews, Executor writes/reviews back — which is the product promise ("reviewed, validated work"). An earlier `</>` cut read as "generic code editor" and said nothing about the pair; two overlapping-ring and handoff-loop cuts were also tried and read as infinity/refresh at 32px.

`app-icon-preview.png` shows both at 256/128/64/32/16 on light and dark.

## Regenerating

```bash
rsvg-convert -w 1024 -h 1024 -o /tmp/app-icon-master.png docs/assets/app-icon-dark.svg
npx tauri icon /tmp/app-icon-master.png   # rebuilds src-tauri/icons/* (icns/ico/png/android/ios)
cp /tmp/app-icon-master.png resources/logo-the-pair.png
cp /tmp/app-icon-master.png docs/assets/logo-the-pair.png
```

The dark tile is the shipped icon: it reads on both light and dark docks/desktops, same as oMLX shipping only the dark tile. `docs/assets/social-preview.png` still uses the old mark — needs a re-layout, not a swap.
