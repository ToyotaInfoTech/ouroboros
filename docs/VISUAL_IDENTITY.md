# Visual Identity

The [Ouroboros logo](image/Ouroboros.svg) is an original geometric illustration
created for this project and licensed under Apache-2.0. A slender, faceted serpent
closes around an off-center void, suggesting recursion rather than illustrating
the active learning workflow. Fine branching seams connect the amber body facets,
hinting at a computer network without using literal nodes or device icons.
A narrow amber glint within a dark eye lends the impassive profile quiet authority;
the tapered tail visibly passes between compact jaws. Orange and
burnt-orange tones preserve the identity. Soft shading, translucent highlights,
and layered silver mist give the faceted silhouette depth and a mysterious
atmosphere while keeping the face and tail grip legible.
The artwork uses no external images, fonts, or icon libraries.

### Logo variants

| Variant | SVG | Transparent PNG | Recommended use |
| --- | --- | --- | --- |
| Basic | [SVG](image/Ouroboros-basic.svg) | [PNG](image/Ouroboros-basic.png) | Default for slides, interfaces, and general reuse |
| Monochrome black | [SVG](image/Ouroboros-mono-black.svg) | [PNG](image/Ouroboros-mono-black.png) | Light backgrounds and single-ink printing |
| Monochrome white | [SVG](image/Ouroboros-mono-white.svg) | [PNG](image/Ouroboros-mono-white.png) | Dark backgrounds and reversed-out marks |
| Atmospheric | [SVG](image/Ouroboros.svg) | [PNG](image/Ouroboros.png) | Large cover images and presentation artwork |

The basic version uses opaque flat colors without mist, gradients, or shadows.
The monochrome versions use one solid ink with transparent cutouts, not a second
background color. Their SVGs contain plain vector paths without masks or filters.
PNG edges are antialiased; the basic and monochrome artwork has no intentionally
translucent surfaces. Check fine details at the intended print or icon size.
The existing documentation logo references still use the atmospheric version.

All PNGs are 1024 by 1024 pixels and share the same framing. Regenerate them from
their corresponding SVGs with librsvg:

```bash
for name in Ouroboros Ouroboros-basic Ouroboros-mono-black Ouroboros-mono-white; do
    rsvg-convert --width 1024 --height 1024 \
        --output "docs/image/${name}.png" "docs/image/${name}.svg"
done
```
