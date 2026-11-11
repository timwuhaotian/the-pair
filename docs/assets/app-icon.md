# App icon

Source of truth (edit this, not the PNGs):

- `app-icon.svg` — the shipped app icon: navy field `#1E3049`, two tiles with a check each, coral stick, gold spark

The icon belongs to the family killi, kil and kiled share. It is killi's icon (`../killi/docs/icon.svg`) with the same field, stick, spark and colors, and two tiles where killi has one. The tiles are the brand mark's (`src/renderer/src/components/BrandMark.tsx`), which is also what the boot splash animates, so the icon in the Dock and the first thing the window shows are the same drawing.

The glyph is still the double-check (✓✓), now one check per tile: two agents each check the work once — Mentor plans/reviews, Executor writes/reviews back — which is the product promise ("reviewed, validated work").

One icon serves light and dark: the navy field reads on both, as killi's does.

`app-icon-preview.png` shows it at 256/128/64/32/16 on light and dark.

## Regenerating

```bash
# The SVG's field is 480 of 512. On the 1024 master it is 896 wide, the footprint the app has always shipped.
rsvg-convert -w 956 -h 956 -o /tmp/app-icon-956.png docs/assets/app-icon.svg
magick /tmp/app-icon-956.png -background none -gravity center -extent 1024x1024 /tmp/app-icon-master.png
npx tauri icon /tmp/app-icon-master.png   # rebuilds src-tauri/icons/* (icns/ico/png/android/ios)
cp /tmp/app-icon-master.png resources/logo-the-pair.png
cp /tmp/app-icon-master.png docs/assets/logo-the-pair.png
```

`social-preview.svg` carries a copy of this drawing beside the product line; it is the repository card (1280×640). After changing the icon, copy the drawing into it again and re-render:

```bash
rsvg-convert -w 1280 -h 640 -o docs/assets/social-preview.png docs/assets/social-preview.svg
magick docs/assets/social-preview.png -quality 92 .github/social-preview.jpeg
```

GitHub has no API for the card: upload the PNG under the repository's Settings → Social preview.
