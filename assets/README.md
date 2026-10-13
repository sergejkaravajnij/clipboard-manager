# Application icon

`icon.png` is the transparent source. `app.ico` contains 16, 20, 24, 32, 48, 64, 128 and 256 pixel frames. `app.rc` embeds resource 101 into the executable; Windows uses it for the executable, application windows and notification tray.

`tray.png` and `tray.ico` provide a separate, broad, nearly square tray version under resource 102. The paper sheets are wider and the paperclip protrudes less so the visible mark fills more of a small tray slot. Application windows and the executable retain resource 101.

To rebuild the ICO from the source:

```powershell
.\assets\build-icon.ps1
.\assets\build-icon.ps1 -Source tray.png -Output tray.ico
cargo build --release
```

The source was generated with the built-in image generation tool from the approved paperclip concept. Final prompt:

> Extract and finalize ONLY the large approved paperclip-and-two-sheets icon at the top of this reference. Preserve its exact identity: upright white rounded front paper sheet, a small offset rear white sheet, bold near-black outlines, large amber/yellow paperclip with an open clear loop at the top. One isolated icon centered on a square truly transparent canvas, approximately 85% canvas coverage with even padding. Simplify to flat solid fills and crisp vector-like edges, no gradients, texture or shadows. Keep thick shapes and large gaps for Windows tray 16px readability. Remove ALL comparison panels, backgrounds, labels, text, grids and smaller icon examples. No new symbols or design alternatives. The output is a production application icon source, not a presentation.

Tray edit prompt (built-in image generation): preserve the white sheets and amber paperclip identity; adapt the tall narrow silhouette to a nearly square broad mark for a 16px tray slot. Widen and shorten the front sheet, keep a small offset rear sheet, and place a shorter paperclip mostly inside the silhouette. Use thin dark outlines, large visible white surfaces, simple solid colors and minimal transparent margins; no text, background or clipped edges.
