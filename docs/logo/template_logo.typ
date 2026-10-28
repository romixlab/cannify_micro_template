// Logo of the WireWeaver firmware template repos: WireWeaver's twisted pair running into a chip,
// the board name and a subtitle below. One function, the same for every board; `logo.typ` sets
// this board's values. Units: 1 pt = 1 px of the 1x logo.
//
// The pair is fixed artwork (pair.svg, pair-dark.svg: WireWeaver's logo geometry, 6 crossings). It
// stays an SVG image because the clear rims where one conductor crosses the other are an SVG mask,
// which Typst cannot draw; Typst embeds the SVG as it is, so the rims survive in SVG and PNG.

#let themes = (
  light: (
    pair: "pair.svg",
    text: rgb("#1a1a1a"),
    sub: rgb("#6b6b6b"),
    chip: rgb("#1a1a1a"),
    chip-text: rgb("#ffffff"),
    pin: rgb("#a8a8a8"),
    dot: rgb("#c8141c"),
  ),
  dark: (
    pair: "pair-dark.svg",
    text: rgb("#ececec"),
    sub: rgb("#a0a0a0"),
    chip: rgb("#3a3a3c"),
    chip-text: rgb("#ececec"),
    pin: rgb("#a8a8a8"),
    dot: rgb("#d81e1e"),
  ),
)

#let width = 568pt
#let cy = 70pt // centre line of the pair and the chip
#let chip-x = 420pt // left edge of the chip body
#let chip-size = 104pt

// `s` at `size`, shrunk to fit `max` if it is wider.
#let fitted(s, size, max, fill) = context {
  let w = measure(text(size, s)).width
  text(if w > max { size * (max / w) } else { size }, fill: fill, s)
}

#let chip-art(label, th) = {
  let top = cy - chip-size / 2
  let pin(start, end) = place(line(start: start, end: end, stroke: 6pt + th.pin))
  for i in range(5) {
    let y = cy + (i - 2) * 15pt
    pin((chip-x - 14pt, y), (chip-x, y))
    pin((chip-x + chip-size, y), (chip-x + chip-size + 14pt, y))
    let x = chip-x + chip-size / 2 + (i - 2) * 15pt
    pin((x, top - 14pt), (x, top))
    pin((x, top + chip-size), (x, top + chip-size + 14pt))
  }
  place(dx: chip-x, dy: top, rect(width: chip-size, height: chip-size, radius: 6pt, fill: th.chip))
  place(dx: chip-x + 10pt, dy: top + 10pt, circle(radius: 5pt, fill: th.dot))
  place(dx: chip-x, dy: top, box(width: chip-size, height: chip-size, align(
    center + horizon,
    fitted(label, 26pt, chip-size - 18pt, th.chip-text),
  )))
}

// The logo as content. `board`: the name under the picture, `chip`: the label on the chip,
// `subtitle`: the line under the name, `theme`: "light" or "dark".
#let logo-art(board: "", chip: "", subtitle: "WireWeaver firmware template", theme: "light") = {
  let th = themes.at(theme)
  set text(font: "JetBrains Mono", weight: "bold", top-edge: "cap-height", bottom-edge: "baseline")
  stack(
    dir: ttb,
    box(width: width, height: 140pt, {
      place(dy: cy - 30pt, image(th.pair, width: 416pt))
      chip-art(chip, th)
    }),
    v(8pt),
    align(center, fitted(board, 40pt, width - 28pt, th.text)),
    v(12.5pt),
    align(center, fitted(subtitle, 16.8pt, width - 28pt, th.sub)),
    v(17pt),
  )
}

// The logo as a whole document, the page fitted to it with a transparent background:
// `#show: template-logo.with(board: .., chip: .., theme: ..)`.
#let template-logo(..args, body) = {
  set page(width: auto, height: auto, margin: 0pt, fill: none)
  logo-art(..args)
  body
}

// GitHub's social preview, 1280 x 640 px rendered at 72 ppi: the light logo on white.
#let social-preview(..args, body) = {
  set page(width: 1280pt, height: 640pt, margin: 0pt, fill: white)
  place(center + horizon, scale(176%, reflow: true, logo-art(..args, theme: "light")))
  body
}
