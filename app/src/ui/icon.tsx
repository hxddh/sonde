// sonde's one icon family: Lucide, drawn on a Canvas in a theme token's
// color (lucide.ts, made by scripts/lucide.py). A terminal draws no icons:
// labels carry the meaning there.

import { Canvas, type CanvasRenderingContext2D, Path2D, token } from "nsh";
import { type IconName, lucide, type LucideElement } from "./lucide";

export type { IconName };

/** Theme tokens an icon may be drawn in. */
export type Tint =
  | "foreground"
  | "muted-foreground"
  | "primary"
  | "primary-foreground"
  | "destructive"
  | "sidebar-foreground"
  | "sidebar-accent-foreground"
  | "status-ok"
  | "status-miss"
  | "status-client"
  | "status-server"
  | "status-refused"
  | "warn"
  | "info"
  | "pass"
  | "fail";

function points(list: string): number[] {
  return list
    .trim()
    .split(/[\s,]+/)
    .map(Number);
}

/** One element, as Lucide's SVG draws it: stroked, never filled. */
function draw(cx: CanvasRenderingContext2D, el: LucideElement) {
  switch (el.tag) {
    case "path":
      cx.stroke(new Path2D(el.d));
      return;
    case "circle":
      cx.beginPath();
      cx.arc(el.cx, el.cy, el.r, 0, Math.PI * 2);
      break;
    case "ellipse":
      cx.beginPath();
      cx.ellipse(el.cx, el.cy, el.rx, el.ry, 0, 0, Math.PI * 2);
      break;
    case "rect":
      cx.beginPath();
      cx.roundRect(el.x, el.y, el.width, el.height, el.rx ?? 0);
      break;
    case "line":
      cx.beginPath();
      cx.moveTo(el.x1, el.y1);
      cx.lineTo(el.x2, el.y2);
      break;
    case "polyline":
    case "polygon": {
      const p = points(el.points);
      cx.beginPath();
      cx.moveTo(p[0], p[1]);
      for (let i = 2; i < p.length; i += 2) cx.lineTo(p[i], p[i + 1]);
      if (el.tag === "polygon") cx.closePath();
      break;
    }
  }
  cx.stroke();
}

/** A Lucide icon, `size` points square (16), stroked in `tint` (muted foreground). */
export function Icon(props: { name: IconName; size?: number; tint?: Tint; stroke?: number }) {
  const size = () => props.size ?? 16;
  return (
    <Canvas
      style={{ pointerEvents: "none", width: size(), height: size(), flexShrink: 0 }}
      draw={(cx, { width }) => {
        cx.scale(width / 24, width / 24);
        cx.strokeStyle = token(props.tint ?? "muted-foreground") as string;
        // thinner strokes at small sizes keep the family's weight even
        cx.lineWidth = props.stroke ?? (size() <= 14 ? 2.25 : 2);
        cx.lineCap = "round";
        cx.lineJoin = "round";
        for (const el of lucide[props.name] as LucideElement[]) draw(cx, el);
      }}
    />
  );
}

/** A Lucide icon as an SVG `data:` URL, for what takes an image: a native
 * menu item's `icon`, the tray. */
export function iconSvg(name: IconName): string {
  const elements = (lucide[name] as LucideElement[]).map(({ tag, ...attributes }) => {
    const written = Object.entries(attributes).map(([key, value]) => `${key}="${value}"`);
    return `<${tag} ${written.join(" ")}/>`;
  });
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${elements.join("")}</svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}
