//! A `flow-next-charts` scene painted as Dioxus SVG elements, never as
//! markup: every colour in the scene is a theme role (`series.1`, `grid`,
//! `accent` …), painted here as the custom property `--chart-series-1`,
//! `--chart-grid` … so `app.css` colours a chart in light and dark like the
//! rest of the app. Only painting lives here; the chart's config and layout
//! are the screen's.

use dioxus::prelude::*;
use flow_next_draw::{Defs, Drawing, Paint, Prim, Style, pattern, svg};

/// A role as the CSS value it is painted with; a literal colour as written.
pub fn role(paint: &Paint) -> String {
    let lower = paint.0.trim().to_ascii_lowercase();
    if lower.starts_with('#') || lower.starts_with("rgb") {
        paint.0.clone()
    } else {
        format!("var(--chart-{})", paint.0.replace('.', "-"))
    }
}

/// A box's, path's or marker's inline style; `pattern` is the id of the
/// tile it fills with, when it has one.
fn style(style: &Style, pattern: Option<&str>) -> String {
    let fill = match (pattern, &style.fill) {
        (Some(id), _) => format!("url(#{id})"),
        (None, Some(paint)) => role(paint),
        (None, None) => "none".to_owned(),
    };
    let mut out = format!("fill:{fill};");
    if let Some(stroke) = &style.stroke {
        out.push_str(&format!(
            "stroke:{};stroke-width:{};",
            role(stroke),
            svg::num(style.stroke_width)
        ));
    }
    if let Some(dash) = svg::dash(&style.dash) {
        out.push_str(&format!("stroke-dasharray:{dash};"));
    }
    if style.opacity < 1.0 {
        out.push_str(&format!("opacity:{};", svg::num(style.opacity)));
    }
    out
}

fn primitive(prim: &Prim, id: &str, defs: &Defs) -> Element {
    let pattern = |own| defs.pattern(own).map(|at| svg::def_id(id, "pattern", at));
    match prim {
        Prim::Rect {
            x,
            y,
            w,
            h,
            radius,
            style: own,
        } => rsx! {
            rect {
                x: svg::num(*x),
                y: svg::num(*y),
                width: svg::num(*w),
                height: svg::num(*h),
                rx: svg::num(*radius),
                style: style(own, pattern(own).as_deref()),
            }
        },
        Prim::Path {
            cmds,
            style: own,
            arrow,
        } => {
            let marker = defs
                .arrow(&own.stroke)
                .filter(|_| *arrow)
                .map(|at| format!("url(#{})", svg::def_id(id, "arrow", at)));
            rsx! {
                path {
                    d: svg::path_data(cmds),
                    style: style(own, pattern(own).as_deref()),
                    marker_end: marker,
                }
            }
        }
        Prim::Marker {
            x,
            y,
            size,
            marker,
            angle,
            style: own,
        } => rsx! {
            path {
                d: svg::path_data(&marker.cmds(*x, *y, *size, *angle)),
                style: style(own, pattern(own).as_deref()),
            }
        },
        Prim::Text {
            x,
            y,
            text,
            size,
            weight,
            anchor,
            paint,
            angle,
        } => rsx! {
            text {
                x: svg::num(*x),
                y: svg::num(*y),
                font_size: svg::num(*size),
                font_weight: "{svg::weight(*weight)}",
                text_anchor: svg::anchor(*anchor),
                "transform": svg::rotation(*angle, *x, *y),
                style: "fill:{role(paint)}",
                "{text}"
            }
        },
        Prim::Group(group) => {
            let clip = defs
                .clip(group.clip)
                .map(|at| format!("url(#{})", svg::def_id(id, "clip", at)));
            let opacity = (group.opacity < 1.0).then(|| svg::num(group.opacity));
            rsx! {
                g {
                    id: group.id.as_ref().map(|own| format!("{id}-{own}")),
                    role: group.role.clone(),
                    "aria-label": group.label.clone(),
                    "clip-path": clip,
                    "opacity": opacity,
                    for prim in group.prims.iter() {
                        {primitive(prim, id, defs)}
                    }
                }
            }
        }
    }
}

/// The arrow heads, pattern tiles and clips the drawing uses, defined once.
fn definitions(defs: &Defs, id: &str) -> Element {
    let t = svg::num(pattern::TILE);
    rsx! {
        defs {
            for (at, stroke) in defs.arrows.iter().enumerate() {
                marker {
                    id: svg::def_id(id, "arrow", at),
                    view_box: "0 0 10 10",
                    ref_x: "9",
                    ref_y: "5",
                    marker_width: "7",
                    marker_height: "7",
                    orient: "auto-start-reverse",
                    path {
                        d: "M0 0 L10 5 L0 10 z",
                        style: stroke.as_ref().map_or("fill:currentColor".to_owned(), |p| format!("fill:{}", role(p))),
                    }
                }
            }
            for (at, (fill, background)) in defs.patterns.iter().enumerate() {
                {
                    let (lines, dots) = svg::tile(fill.pattern);
                    let ink = role(&fill.ink);
                    rsx! {
                        pattern {
                            id: svg::def_id(id, "pattern", at),
                            "patternUnits": "userSpaceOnUse",
                            width: "{t}",
                            height: "{t}",
                            rect { width: "{t}", height: "{t}", style: "fill:{role(background)}" }
                            if !lines.is_empty() {
                                path { d: "{lines}", style: "fill:none;stroke:{ink};stroke-width:{svg::num(pattern::LINE_WIDTH)}" }
                            }
                            for (cx, cy, r) in dots {
                                circle { cx: svg::num(cx), cy: svg::num(cy), r: svg::num(r), style: "fill:{ink}" }
                            }
                        }
                    }
                }
            }
            for (at, clip) in defs.clips.iter().enumerate() {
                clipPath { id: svg::def_id(id, "clip", at),
                    rect {
                        x: svg::num(clip.x),
                        y: svg::num(clip.y),
                        width: svg::num(clip.w),
                        height: svg::num(clip.h),
                    }
                }
            }
        }
    }
}

/// `drawing` (both layers) as an SVG image that scales to its container's
/// width, described by `label` for screen readers. `id` keeps its
/// definitions apart from other drawings on the page.
pub fn painting(drawing: &Drawing, id: &str, class: &str, label: &str) -> Element {
    let defs = Defs::of(drawing);
    let (w, h) = (svg::num(drawing.width), svg::num(drawing.height));
    rsx! {
        svg {
            class: "{class}",
            role: "img",
            "aria-label": "{label}",
            view_box: "0 0 {w} {h}",
            width: "100%",
            {definitions(&defs, id)}
            g { "data-layer": "base",
                for prim in drawing.prims.iter() {
                    {primitive(prim, id, &defs)}
                }
            }
            g { "data-layer": "overlay",
                for prim in drawing.overlay.iter() {
                    {primitive(prim, id, &defs)}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_paints_as_its_custom_property_and_a_literal_as_written() {
        assert_eq!(role(&Paint::role("series.1")), "var(--chart-series-1)");
        assert_eq!(role(&Paint::role("tooltip.text")), "var(--chart-tooltip-text)");
        assert_eq!(role(&Paint("#ff0000".into())), "#ff0000");
        let s = Style::stroked(Paint::role("accent"), 1.5).dashed(4.0, 3.0);
        assert_eq!(
            style(&s, None),
            "fill:none;stroke:var(--chart-accent);stroke-width:1.5;stroke-dasharray:4 3;"
        );
    }
}
