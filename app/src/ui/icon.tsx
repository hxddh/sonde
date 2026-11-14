// sonde's one icon family: Lucide, as images in a theme token's color
// (lucide.ts, made by scripts/lucide.py). A terminal draws no icons: labels
// carry the meaning there.

import { Image, platform, token, View } from "nsh";
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
  | "status-injected"
  | "warn"
  | "info"
  | "pass"
  | "fail";

/** A Lucide icon, `size` points square (16), in `tint` (muted foreground):
 * Native Shell's `Image` on the icon's SVG, recolored by the token (a
 * template image), so a theme or the dark appearance reaches it natively. */
export function Icon(props: { name: IconName; size?: number; tint?: Tint; stroke?: number }) {
  const size = () => props.size ?? 16;
  // a terminal draws no icons (labels carry the meaning there): the icon's room, empty, as its canvas was
  if (platform.idiom === "terminal") return <View style={{ pointerEvents: "none", width: size(), height: size(), flexShrink: 0 }} />;
  return (
    <Image
      // thinner strokes at small sizes keep the family's weight even
      src={iconSvg(props.name, props.stroke ?? (size() <= 14 ? 2.25 : 2))}
      fadeDuration={0}
      style={{ pointerEvents: "none", width: size(), height: size(), flexShrink: 0, tintColor: token(props.tint ?? "muted-foreground") as string }}
    />
  );
}

/** A Lucide icon as an SVG `data:` URL, for what takes an image: a native
 * menu item's `icon`, the tray. */
export function iconSvg(name: IconName, stroke = 2): string {
  const elements = (lucide[name] as LucideElement[]).map(({ tag, ...attributes }) => {
    const written = Object.entries(attributes).map(([key, value]) => `${key}="${value}"`);
    return `<${tag} ${written.join(" ")}/>`;
  });
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${stroke}" stroke-linecap="round" stroke-linejoin="round">${elements.join("")}</svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}
