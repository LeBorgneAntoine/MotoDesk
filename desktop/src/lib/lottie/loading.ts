import type { AnimationItem } from "lottie-web";
import lottie from "lottie-web";
import source from "$lib/assets/loading.json";

type LottieNode = Record<string, unknown>;

function cloneSource(): LottieNode {
  return JSON.parse(JSON.stringify(source)) as LottieNode;
}

/** Replace every fill in the Lottie with a 0..1 RGB triple. */
function recolor(data: LottieNode, rgb: [number, number, number]): LottieNode {
  const walk = (node: unknown): void => {
    if (Array.isArray(node)) {
      node.forEach(walk);
      return;
    }
    if (!node || typeof node !== "object") return;
    const obj = node as LottieNode;
    if (obj.ty === "fl" && obj.c && typeof obj.c === "object") {
      const c = obj.c as { k?: number[] };
      if (Array.isArray(c.k) && c.k.length >= 3) {
        c.k = [rgb[0], rgb[1], rgb[2], c.k[3] ?? 1];
      }
    }
    for (const value of Object.values(obj)) walk(value);
  };
  walk(data);
  return data;
}

function cssToUnitRgb(host: HTMLElement, cssColor: string): [number, number, number] {
  const probe = document.createElement("span");
  probe.style.color = cssColor;
  host.appendChild(probe);
  const computed = getComputedStyle(probe).color;
  probe.remove();
  const nums = computed.match(/[\d.]+/g)?.map(Number) ?? [25, 118, 210];
  return [nums[0] / 255, nums[1] / 255, nums[2] / 255];
}

export interface PlayLoadingOptions {
  host: HTMLElement;
  /** `accent` uses `--color-accent`; `current` inherits the parent colour. */
  tone: "default" | "current";
  reducedMotion: boolean;
}

export function playLoadingAnimation(opts: PlayLoadingOptions): AnimationItem {
  const cssColor =
    opts.tone === "current"
      ? "currentColor"
      : getComputedStyle(opts.host).getPropertyValue("--color-accent").trim() || "currentColor";
  const data = recolor(cloneSource(), cssToUnitRgb(opts.host, cssColor));

  const anim = lottie.loadAnimation({
    container: opts.host,
    renderer: "svg",
    loop: !opts.reducedMotion,
    autoplay: !opts.reducedMotion,
    animationData: data,
  });

  if (opts.reducedMotion) {
    anim.goToAndStop(8, true);
  }

  return anim;
}
