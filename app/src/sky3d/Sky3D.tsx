import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { Constellation, SkyModel, Star } from "../sky/model";
import { DriftList } from "../sky/DriftList";
import { StarPanel } from "../sky/StarPanel";
import { SkyScene, type ZoneEvent } from "./SkyScene";
import { anchorPanel } from "./anchor";
import { outsideX, placeLabels, shortName, truncate, type LabelCandidate, type Rect } from "./labels";
import { RING_NAMES } from "./layout";
import { isCentre } from "./milkyway";
import { labelBudget, pinchFactor, wheelFactor } from "./camera";
import { LENSES, lensMatches, systemFacts, type Lens } from "./physics";
import { breadcrumbs, searchSky, type Crumb, type Hit } from "./wayfind";
import { devSkyStart, prefersReducedMotion } from "./support";
import { matchSky } from "./wink";
import "./sky3d.css";

// The 3D Sky (SKY-1 to SKY-9, #297, #299): one living galaxy. The scene draws; this overlay
// keeps the meaning in the DOM: a button per project and per star (role, aria-label, keyboard),
// labels for the current zoom, the panel beside the object, the breadcrumb, search, lenses,
// the controls and the drift drawer. The face holds no rules: it shows what the core sent.

const FEATURE_CHARS = 26;
/** Below this share of the overview distance the drift counts show beside each name. */
const NEAR_SHARE = 0.35;

type Props = {
  sky: SkyModel;
  open: string | null;
  onOpen: (repo: string) => void;
  onBack: () => void;
  picked: Star | null;
  onPick: (star: Star | null) => void;
  /** Refresh and back up controls, from the Sky view. */
  toolbar: ReactNode;
  /** WebGL failed or was lost: the Sky view falls back to the flat map. */
  onFail: () => void;
};

type Els = {
  hit: Map<string, HTMLElement>;
  label: Map<string, HTMLElement>;
  star: Map<number, HTMLElement>;
  feature: Map<string, HTMLElement>;
  ring: Map<string, HTMLElement>;
};

function keep<K>(map: Map<K, HTMLElement>, key: K) {
  return (el: HTMLElement | null) => {
    if (el) map.set(key, el);
    else map.delete(key);
  };
}

function rel(r: DOMRect, root: DOMRect): Rect {
  return { left: r.left - root.left, top: r.top - root.top, width: r.width, height: r.height };
}

function place(el: HTMLElement, left: number, top: number, show = true) {
  el.style.transform = `translate3d(${Math.round(left)}px, ${Math.round(top)}px, 0)`;
  el.style.visibility = show ? "visible" : "hidden";
}

const ageLine = (sky: SkyModel) =>
  sky.priorityAgeDays === null
    ? `snapshot ${sky.ageMinutes} min old · no priority copy`
    : `snapshot ${sky.ageMinutes} min old · priority copy ${sky.priorityAgeDays} days old`;

/** Controls that keep their own pointer and wheel: no orbit or zoom starts on them. */
const OWN_INPUT = ".sky3d-panel, .sky3d-drawer, .sky3d-drawer-toggle, .sky3d-toolbar, .sky3d-search, .sky3d-bottom, .sky3d-crumbs, .sky3d-head";

const isLens = (v: string | null): v is Lens => LENSES.some((l) => l.id === v);

export default function Sky3D({ sky, open, onOpen, onBack, picked, onPick, toolbar, onFail }: Props) {
  const dev = useRef(devSkyStart()).current;
  const rootRef = useRef<HTMLElement>(null);
  const anchorRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const sceneRef = useRef<SkyScene | null>(null);
  const els = useRef<Els>({ hit: new Map(), label: new Map(), star: new Map(), feature: new Map(), ring: new Map() });
  const panelRef = useRef<HTMLDivElement>(null);
  const starPanelRef = useRef<HTMLDivElement>(null);
  const unchartedRef = useRef<HTMLDivElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const overviewLayer = useRef<HTMLDivElement>(null);
  const galaxyLayer = useRef<HTMLDivElement>(null);
  const blockersRef = useRef<HTMLElement[]>([]);
  const topRef = useRef(-1);
  const frameRef = useRef<() => void>(() => {});
  const zoneRef = useRef<(z: ZoneEvent) => void>(() => {});
  const failRef = useRef(onFail);
  failRef.current = onFail;
  const pending = useRef<{ repo: string; feature?: string; star?: number } | null>(null);
  const nearRef = useRef<string | null>(null);

  const [hovered, setHovered] = useState<string | null>(null);
  const [hoverStar, setHoverStar] = useState<number | null>(null);
  const [hoverFeature, setHoverFeature] = useState<string | null>(null);
  const [nearFeature, setNearFeature] = useState<string | null>(null);
  const [drawer, setDrawer] = useState(false);
  const [legend, setLegend] = useState(false);
  const [reduced, setReduced] = useState(prefersReducedMotion);
  const [lens, setLens] = useState<Lens>(isLens(dev.lens) ? dev.lens : "natural");
  const [search, setSearch] = useState<string | null>(dev.search);
  const [hitIndex, setHitIndex] = useState(0);

  const zoomed: Constellation | undefined = open === null ? undefined : sky.constellations.find((c) => c.repo === open);
  const stars = useMemo(() => new Map((zoomed?.features ?? []).flatMap((f) => f.stars.map((s) => [s.number, s] as const))), [zoomed]);
  const hits = useMemo(() => (search ? searchSky(sky, search) : []), [sky, search]);
  // Live, as it is typed: what does not match winks out of the map (see wink.ts).
  const filter = useMemo(() => matchSky(sky.constellations, search ?? "", zoomed ?? null), [sky, search, zoomed]);
  useEffect(() => {
    sceneRef.current?.setFilter(filter);
  }, [filter]);
  // The search closes on a click anywhere outside it (its own toggle button included: that
  // toggles it itself).
  useEffect(() => {
    if (search === null) return;
    const onDown = (e: PointerEvent) => {
      const t = e.target as Element | null;
      if (t?.closest?.(".sky3d-search, .sky3d-search-open")) return;
      setSearch(null);
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [search === null]);
  const crumbs = useMemo(() => breadcrumbs(sky, open, nearFeature, picked), [sky, open, nearFeature, picked]);

  // The renderer: made once per mount, on a canvas of its own, disposed on unmount.
  useLayoutEffect(() => {
    const stage = stageRef.current!;
    const canvas = document.createElement("canvas");
    canvas.className = "sky3d-canvas";
    canvas.setAttribute("aria-hidden", "true");
    stage.appendChild(canvas);
    let scene: SkyScene;
    try {
      scene = new SkyScene(canvas, prefersReducedMotion());
    } catch {
      canvas.remove();
      failRef.current();
      return;
    }
    const lost = (e: Event) => {
      e.preventDefault();
      failRef.current();
    };
    canvas.addEventListener("webglcontextlost", lost);
    sceneRef.current = scene;
    scene.onFrame = () => frameRef.current();
    scene.onZone = (z) => zoneRef.current(z);
    const fit = () => {
      const root = rootRef.current;
      if (root) scene.resize(root.clientWidth, root.clientHeight);
    };
    const ro = new ResizeObserver(fit);
    ro.observe(rootRef.current!);
    window.addEventListener("resize", fit);
    fit();
    scene.start();
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", fit);
      canvas.removeEventListener("webglcontextlost", lost);
      scene.dispose();
      canvas.remove();
      sceneRef.current = null;
    };
  }, []);

  useEffect(() => {
    const scene = sceneRef.current;
    if (!scene) return;
    if (dev.stages) scene.devSpreadProgress();
    scene.setSky(sky);
    if (dev.near && !scene.hasMoved()) scene.devPlace(dev.near, dev.dist ?? 4, dev.az, dev.elev);
  }, [sky, dev]);
  // DEV only: the frame time, as the rAF delta averaged over 5 s with every frame drawn.
  const [devFps, setDevFps] = useState<string | null>(null);
  useEffect(() => {
    if (!dev.fps) return;
    let raf = 0;
    let first = 0;
    let last = 0;
    let n = 0;
    let worst = 0;
    const tick = (now: number) => {
      sceneRef.current?.poke();
      if (first === 0) first = last = now;
      else {
        worst = Math.max(worst, now - last);
        n++;
        last = now;
      }
      if (now - first >= 5000) {
        const avg = (last - first) / Math.max(1, n);
        setDevFps(`frame ${avg.toFixed(1)} ms avg over 5 s (${n} frames, worst ${worst.toFixed(1)} ms)`);
        return;
      }
      raf = requestAnimationFrame(tick);
    };
    const wait = window.setTimeout(() => (raf = requestAnimationFrame(tick)), 4000);
    return () => {
      window.clearTimeout(wait);
      cancelAnimationFrame(raf);
    };
  }, [dev]);
  useEffect(() => {
    const scene = sceneRef.current;
    scene?.setOpen(open);
    setHoverStar(null);
    setHoverFeature(null);
    setHovered(null);
    setDrawer(false);
    const p = pending.current;
    if (scene && p && p.repo === open) {
      pending.current = null;
      if (p.star !== undefined) scene.flyToStar(p.star);
      else if (p.feature !== undefined) scene.flyToFeature(p.feature);
    }
  }, [open]);
  useEffect(() => sceneRef.current?.setHovered(hovered), [hovered]);
  useEffect(() => sceneRef.current?.setFocusStar(hoverStar), [hoverStar]);
  // Picking a star (a click, search, or the DEV hook) flies in to it, as a project click does.
  useEffect(() => {
    const scene = sceneRef.current;
    if (!scene) return;
    scene.setPicked(picked?.number ?? null);
    if (picked && scene.interiorRepo() === open) scene.flyToStar(picked.number);
  }, [picked]);
  useEffect(() => sceneRef.current?.setLens(lens), [lens]);
  useEffect(() => sceneRef.current?.invalidate());

  // Zooming in far enough enters a project, the same as a click; backing out leaves it.
  zoneRef.current = (z) => {
    if ("enter" in z) {
      if (z.enter !== open) onOpen(z.enter);
    } else if (open !== null) onBack();
  };

  // Reduced motion: no idle drift, no pulse, instant flights; follows the system setting live.
  useEffect(() => {
    let mq: MediaQueryList;
    try {
      mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    } catch {
      return;
    }
    const on = () => {
      setReduced(mq.matches);
      sceneRef.current?.setReducedMotion(mq.matches);
    };
    mq.addEventListener?.("change", on);
    return () => mq.removeEventListener?.("change", on);
  }, []);

  /** Go where a search hit or a breadcrumb points. */
  const go = (h: Hit) => {
    const scene = sceneRef.current;
    setSearch(null);
    if (h.kind === "project") {
      if (h.repo === open) scene?.flyToSystem(h.repo);
      else onOpen(h.repo);
      return;
    }
    const want = h.kind === "issue" ? { repo: h.repo, star: h.star.number } : { repo: h.repo, feature: h.feature };
    if (h.repo === open) {
      if (want.star !== undefined) scene?.flyToStar(want.star);
      else if (want.feature !== undefined) scene?.flyToFeature(want.feature);
    } else {
      pending.current = want;
      onOpen(h.repo);
    }
    if (h.kind === "issue") queueMicrotask(() => onPick(h.star));
    else onPick(null);
  };

  const crumbTo = (c: Crumb) => {
    const scene = sceneRef.current;
    if (!scene) return;
    if (c.level === "galaxy") {
      if (open) onBack();
      else scene.flyOverview();
    } else if (c.level === "project" && open) {
      onPick(null);
      scene.flyToSystem(open);
    } else if (c.level === "feature") {
      const f = zoomed?.features.find((x) => x.title === c.label);
      onPick(null);
      if (f) scene.flyToFeature(f.id);
    } else if (c.level === "issue" && picked) scene.flyToStar(picked.number);
  };

  // Keys: Escape steps back one level; / searches; n p d f b pick a lens.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      const scene = sceneRef.current;
      if (e.key === "Escape") {
        if (search !== null) setSearch(null);
        else if (picked) onPick(null);
        else if (drawer) setDrawer(false);
        else if (open && scene?.deeperThanSystem()) scene.flyToSystem(open);
        else if (open) onBack();
        else scene?.completeFlight();
        return;
      }
      if (typing || e.ctrlKey || e.metaKey || e.altKey) return;
      if (e.key === "/") {
        e.preventDefault();
        setSearch("");
        setHitIndex(0);
        return;
      }
      const l = LENSES.find((x) => x.key === e.key.toLowerCase());
      if (l) setLens((cur) => (cur === l.id && l.id !== "natural" ? "natural" : l.id));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, picked, drawer, search, onBack, onPick]);

  // Wheel and trackpad pinch (a wheel event with ctrlKey) zoom toward the cursor. Native and not
  // passive, so the webview does not zoom the page instead.
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey) e.preventDefault();
      if ((e.target as Element | null)?.closest?.(OWN_INPUT)) return;
      e.preventDefault();
      const box = root.getBoundingClientRect();
      sceneRef.current?.zoomAt(wheelFactor(e.deltaY, e.deltaMode, e.ctrlKey), e.clientX - box.left, e.clientY - box.top);
    };
    root.addEventListener("wheel", onWheel, { passive: false });
    return () => root.removeEventListener("wheel", onWheel);
  }, []);

  // One pointer drags to orbit; two (a touchscreen) pinch to zoom about their midpoint.
  const pointers = useRef(new Map<number, { x: number; y: number }>());
  const gesture = useRef<{ moved: number; span: number; captured: boolean } | null>(null);
  const suppressClick = useRef(false);
  const local = (e: { clientX: number; clientY: number }) => {
    const box = rootRef.current!.getBoundingClientRect();
    return { x: e.clientX - box.left, y: e.clientY - box.top };
  };
  const span = () => {
    const [a, b] = [...pointers.current.values()];
    return a && b ? { d: Math.hypot(a.x - b.x, a.y - b.y), x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 } : null;
  };
  const onPointerDown = (e: React.PointerEvent) => {
    if ((e.target as Element).closest(OWN_INPUT)) return;
    if (e.pointerType === "mouse" && e.button !== 0) return;
    pointers.current.set(e.pointerId, local(e));
    const s = span();
    gesture.current = { moved: gesture.current?.moved ?? 0, span: s?.d ?? 0, captured: gesture.current?.captured ?? false };
    sceneRef.current?.poke();
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const g = gesture.current;
    const prev = pointers.current.get(e.pointerId);
    if (!g || !prev) return;
    const p = local(e);
    pointers.current.set(e.pointerId, p);
    const scene = sceneRef.current;
    if (pointers.current.size >= 2) {
      const s = span();
      if (s && g.span > 0) scene?.zoomAt(pinchFactor(g.span, s.d), s.x, s.y);
      if (s) g.span = s.d;
      g.moved += 10;
    } else {
      const dx = p.x - prev.x;
      const dy = p.y - prev.y;
      g.moved += Math.abs(dx) + Math.abs(dy);
      if (g.moved > 4) scene?.orbitBy(dx, dy);
    }
    if (g.moved > 4 && !g.captured) {
      g.captured = true;
      rootRef.current?.setPointerCapture?.(e.pointerId);
    }
  };
  const onPointerUp = (e: React.PointerEvent) => {
    if (!pointers.current.delete(e.pointerId)) return;
    const g = gesture.current;
    if (pointers.current.size > 0) {
      if (g) g.span = span()?.d ?? 0;
      return;
    }
    gesture.current = null;
    if (!g) return;
    if (g.moved > 4) {
      suppressClick.current = true;
      setTimeout(() => (suppressClick.current = false), 0);
      return;
    }
    // A plain click on the sky: finish a flight, or close the star panel.
    if ((e.target as Element).closest("button")) return;
    const scene = sceneRef.current;
    if (scene?.isFlying()) scene.completeFlight();
    else if (picked) onPick(null);
  };
  const onPointerCancel = (e: React.PointerEvent) => {
    pointers.current.delete(e.pointerId);
    if (pointers.current.size === 0) gesture.current = null;
  };

  // Each drawn frame: read sizes, place labels without overlap, then write positions.
  frameRef.current = () => {
    const scene = sceneRef.current;
    const root = rootRef.current;
    if (!scene || !root) return;
    const anchorTop = Math.max(0, Math.round(anchorRef.current?.getBoundingClientRect().top ?? 0));
    if (anchorTop !== topRef.current) {
      topRef.current = anchorTop;
      root.style.top = `${anchorTop}px`;
    }
    const W = root.clientWidth;
    const H = root.clientHeight;
    const view = { width: W, height: H };
    const rootBox = root.getBoundingClientRect();
    const blockers = blockersRef.current.filter((el) => el.isConnected && el.offsetWidth > 0).map((el) => rel(el.getBoundingClientRect(), rootBox));
    // Nothing is placed below the top of the bottom bar, forced labels included.
    const bar = rootRef.current?.querySelector<HTMLElement>(".sky3d-bottom");
    const floor = bar && bar.offsetHeight > 0 ? rel(bar.getBoundingClientRect(), rootBox).top : H - 64;
    const E = els.current;
    const cands: LabelCandidate[] = [];
    const size = (el: HTMLElement) => ({ width: el.offsetWidth, height: el.offsetHeight });
    const share = scene.zoomShare();
    const inside = zoomed && scene.interiorRepo() === zoomed.repo ? scene.detail() : 0;

    const nf = scene.nearFeature();
    if (nf !== nearRef.current) {
      nearRef.current = nf;
      setNearFeature(nf);
    }

    // Projects: a hit target over each, and its name to its right (fewer when far out).
    const layer = overviewLayer.current;
    if (layer) {
      layer.classList.toggle("is-near", share < NEAR_SHARE);
      layer.style.opacity = String(1 - 0.55 * inside);
    }
    const onScreen: { c: Constellation; s: { x: number; y: number; r: number } }[] = [];
    for (const c of sky.constellations) {
      const s = scene.galaxyScreen(c.repo);
      const hit = E.hit.get(c.repo);
      if (!s || !hit) continue;
      const d = Math.min(Math.max(26, s.r * 1.5), 260);
      const shown = s.visible && c.repo !== open && !scene.isOut(`g:${c.repo}`) && s.x > -d && s.x < W + d && s.y > -d && s.y < H + d;
      hit.style.width = hit.style.height = `${Math.round(d)}px`;
      place(hit, s.x - d / 2, s.y - d / 2, shown);
      if (shown) onScreen.push({ c, s });
    }
    const budget = labelBudget(share, 1, onScreen.length, 12);
    const labelled = new Set(
      [...onScreen]
        .sort((a, b) => a.c.rank - b.c.rank)
        .slice(0, budget)
        .map((o) => o.c.repo),
    );
    // The bright core keeps clear of names: each sits on the side of its project away from the
    // core, and the black hole's own name sits just below the core's glow. (Close in, the core
    // fills the view and stops being an obstacle.)
    const core = scene.coreScreen();
    const coreOn = core.visible && core.r > 6 && core.r < Math.min(W, H) * 0.3;
    const coreRect = { left: core.x - core.r, top: core.y - core.r, width: core.r * 2, height: core.r * 2 };
    const labelBlockers: Rect[] = coreOn ? [coreRect] : [];
    for (const { c, s } of onScreen) {
      const label = E.label.get(c.repo);
      if (!label || !(labelled.has(c.repo) || c.repo === hovered)) continue;
      const sz = size(label);
      const gap = Math.min(s.r * 0.55, 90) + 8;
      const centre = isCentre(c.repo);
      const beside = s.x + gap + sz.width / 2;
      // Close to the black hole its name hangs just under the shadow and photon ring, off the disc.
      const x = centre ? (coreOn ? core.x : s.x) : coreOn ? outsideX({ x: s.x, r: gap }, core, sz.width, 0) : beside;
      const shadow = centre && !coreOn ? scene.shadowScreen() : null;
      const y = centre ? (shadow ? shadow.y + shadow.r * 1.08 + 6 : core.y + core.r + 4) : s.y - sz.height / 2;
      cands.push({ id: `g:${c.repo}`, x, y, ...sz, priority: centre ? -2 : c.rank, forced: c.repo === hovered });
    }

    // Inside a project: stars, feature names, the three ring names, the star tip.
    if (galaxyLayer.current) galaxyLayer.current.style.opacity = String(inside);
    if (zoomed && inside > 0.05) {
      for (const [n, el] of E.star) {
        const s = scene.starScreen(n);
        if (!s) continue;
        const d = Math.min(Math.max(20, s.r * 2.4), 120);
        el.style.width = el.style.height = `${Math.round(d)}px`;
        place(el, s.x - d / 2, s.y - d / 2, s.visible && !scene.isOut(`s:${n}`));
      }
      // Feature names sit on the outer side of their figure, away from the bright core.
      const hub = scene.galaxyScreen(zoomed.repo) ?? { x: W / 2, y: H / 2, r: 0 };
      zoomed.features.forEach((f, i) => {
        const el = E.feature.get(f.id);
        const s = scene.featureScreen(f.id);
        if (!el || !s || !s.visible || (filter.features && !filter.features.has(f.id))) return;
        const sz = size(el);
        const order = f.release === "1.0" ? 0 : f.release === "after-1.0" ? 100 : 200;
        cands.push({ id: `f:${f.id}`, x: outsideX(f.stars.length ? s : { x: s.x, r: s.r * 1.3 }, hub, sz.width, 6), y: s.y - sz.height / 2, ...sz, priority: order - f.stars.length + i * 0.01, forced: f.id === hoverFeature });
      });
      for (const [release, el] of E.ring) {
        const s = scene.ringScreen(release);
        if (!s || !s.visible) continue;
        const sz = size(el);
        // Always shown (the brief): pulled into the view when the ring is larger than the screen.
        const x = Math.min(Math.max(s.x, sz.width / 2 + 8), W - sz.width / 2 - 8);
        const y = Math.min(Math.max(s.y + 6, 64), floor - sz.height - 8);
        cands.push({ id: `r:${release}`, x, y, ...sz, priority: -5, forced: true });
      }
      const tip = tipRef.current;
      if (tip && hoverStar !== null && !picked) {
        const s = scene.starScreen(hoverStar);
        if (s) {
          const sz = size(tip);
          cands.push({ id: "tip", x: s.x + s.r + 10 + sz.width / 2, y: s.y - sz.height / 2, ...sz, priority: -10, forced: true });
        }
      }
    }

    const panelRects: Rect[] = [];
    const anchorAt = (el: HTMLElement | null, s: { x: number; y: number; r: number } | null, show: boolean, prefer: "right" | "left" = "right") => {
      if (!el || !s) return;
      const sz = size(el);
      const a = anchorPanel(s, sz, view, 14, 12, prefer);
      place(el, a.left, a.top, show);
      if (show) panelRects.push({ left: a.left, top: a.top, ...sz });
    };
    // The info panel beside the hovered project, or beside the picked star (#299).
    if (hovered && hovered !== open) {
      const s = scene.galaxyScreen(hovered);
      anchorAt(panelRef.current, s && { x: s.x, y: s.y, r: Math.min(s.r * 0.6, 120) }, true);
    }
    if (picked && open !== null) {
      const s = scene.starScreen(picked.number);
      // On the open side of the star, away from the rest of its figure.
      const fid = zoomed?.features.find((f) => f.stars.some((x) => x.number === picked.number))?.id;
      const fs = fid ? scene.featureScreen(fid) : null;
      const prefer = s && fs && s.x < fs.x ? "left" : "right";
      anchorAt(starPanelRef.current, s && { x: s.x, y: s.y, r: Math.max(14, s.r) }, inside > 0.6, prefer);
    }
    if (zoomed && zoomed.features.length === 0) {
      const s = scene.galaxyScreen(zoomed.repo);
      anchorAt(unchartedRef.current, s && { x: s.x, y: s.y, r: Math.min(s.r * 0.7, 200) }, true);
    }

    const placed = placeLabels(cands, view, [...blockers, ...panelRects, ...labelBlockers]);
    const show = (id: string, el: HTMLElement | undefined | null) => {
      if (!el) return;
      const p = placed.get(id);
      if (p) place(el, p.left, p.top, true);
      else el.style.visibility = "hidden";
    };
    for (const [repo, el] of E.label) show(`g:${repo}`, el);
    for (const [id, el] of E.feature) show(`f:${id}`, el);
    for (const [release, el] of E.ring) show(`r:${release}`, el);
    show("tip", tipRef.current);
  };

  const drift = useMemo(() => (zoomed ? [zoomed] : sky.constellations).flatMap((c) => c.drift.map((d) => ({ ...d, repo: c.repo }))), [sky, zoomed]);
  const hov = hovered && hovered !== open ? sky.constellations.find((c) => c.repo === hovered) : undefined;
  const tipStar = hoverStar !== null ? stars.get(hoverStar) : undefined;
  const blocker = (el: HTMLElement | null) => {
    blockersRef.current = blockersRef.current.filter((b) => b.isConnected);
    if (el && !blockersRef.current.includes(el)) blockersRef.current.push(el);
  };
  const lensInfo = LENSES.find((l) => l.id === lens)!;
  const lensCount = useMemo(() => lensMatches(lens, sky.constellations.map(systemFacts)), [lens, sky]);

  return (
    <>
      <div ref={anchorRef} className="sky3d-anchor" />
      <section
        ref={rootRef}
        className={`sky3d${reduced ? " sky3d-still" : ""}`}
        aria-label={zoomed ? `Constellation ${zoomed.repo}` : "Sky"}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerCancel}
        onClickCapture={(e) => {
          if (suppressClick.current) {
            e.stopPropagation();
            e.preventDefault();
          }
        }}
      >
        <div ref={stageRef} className="sky3d-stage" />

        <div ref={overviewLayer} className="sky3d-layer">
          {sky.constellations.map((c) => (
            <button
              key={c.repo}
              ref={keep(els.current.hit, c.repo)}
              className={`sky3d-hit${c.repo === hovered ? " is-hovered" : ""}`}
              data-testid="constellation"
              data-repo={c.repo}
              aria-label={isCentre(c.repo) ? `${c.repo}, the black hole at the centre` : c.repo}
              hidden={c.repo === open}
              onClick={() => onOpen(c.repo)}
              onPointerEnter={() => setHovered(c.repo)}
              onPointerLeave={() => setHovered((h) => (h === c.repo ? null : h))}
              onFocus={() => setHovered(c.repo)}
              onBlur={() => setHovered((h) => (h === c.repo ? null : h))}
            />
          ))}
          {sky.constellations.map((c) => (
            <div
              key={c.repo}
              ref={keep(els.current.label, c.repo)}
              className={`sky3d-label${c.rank < 3 || isCentre(c.repo) ? " is-top" : ""}${c.repo === hovered ? " is-hovered" : ""}`}
              aria-hidden="true"
            >
              {shortName(c.repo)}
              {c.drift.length > 0 && (
                <span className="sky3d-drift" data-testid="drift-count">
                  {c.drift.length}
                </span>
              )}
            </div>
          ))}
          <div ref={panelRef} className="sky3d-panel sky3d-project" role="tooltip" hidden={!hov}>
            {hov && <ProjectCard c={hov} />}
          </div>
        </div>

        {zoomed && (
          <div ref={galaxyLayer} className="sky3d-layer" style={{ opacity: 0 }}>
            {zoomed.features.map((f) =>
              f.stars.map((s) => (
                <button
                  key={s.number}
                  ref={keep(els.current.star, s.number)}
                  className={`sky3d-star${picked?.number === s.number ? " is-picked" : ""}`}
                  data-testid="star"
                  data-number={s.number}
                  data-state={s.state}
                  data-pulse={s.inFlight ? "true" : undefined}
                  aria-label={`#${s.number} ${s.title}`}
                  onClick={() => onPick(s)}
                  onPointerEnter={() => setHoverStar(s.number)}
                  onPointerLeave={() => setHoverStar((h) => (h === s.number ? null : h))}
                  onFocus={() => setHoverStar(s.number)}
                  onBlur={() => setHoverStar((h) => (h === s.number ? null : h))}
                />
              )),
            )}
            {zoomed.features.map((f) => (
              <div
                key={f.id}
                ref={keep(els.current.feature, f.id)}
                className={`sky3d-label sky3d-feature${f.stars.length === 0 ? " is-dark" : ""}`}
                data-testid="feature"
                data-feature={f.id}
                data-release={f.release}
                title={f.title}
                onPointerEnter={() => setHoverFeature(f.id)}
                onPointerLeave={() => setHoverFeature((h) => (h === f.id ? null : h))}
              >
                {f.id === hoverFeature ? f.title : truncate(f.title, FEATURE_CHARS)}
                {f.stars.length > 0 && <span className="sky3d-count">{f.stars.length}</span>}
              </div>
            ))}
            {zoomed.features.length > 0 &&
              (["1.0", "after-1.0", "after-2.0"] as const).map((r) => (
                <div key={r} ref={keep(els.current.ring, r)} className="sky3d-label sky3d-ring" data-testid="ring" data-milestone={RING_NAMES[r]}>
                  {RING_NAMES[r]}
                </div>
              ))}
            <div ref={tipRef} className="sky3d-label sky3d-tip" aria-hidden="true">
              {tipStar && `#${tipStar.number} ${truncate(tipStar.title, 64)}`}
            </div>
            {picked && (
              <div ref={starPanelRef} className="sky3d-panel sky3d-starpanel">
                <StarPanel star={picked} onClose={() => onPick(null)} />
              </div>
            )}
          </div>
        )}

        <nav ref={blocker} className="sky3d-crumbs" aria-label="Where you are">
          <ol>
            {crumbs.map((c, i) => (
              <li key={c.level}>
                {i < crumbs.length - 1 ? (
                  <button onClick={() => crumbTo(c)} title={c.title}>
                    {truncate(c.label, 28)}
                  </button>
                ) : (
                  <span aria-current="location" title={c.title}>
                    {truncate(c.label, 36)}
                  </span>
                )}
              </li>
            ))}
          </ol>
          <button
            className="btn sky3d-search-open"
            aria-label="Search the sky"
            aria-keyshortcuts="/"
            aria-expanded={search !== null}
            aria-controls="sky3d-search"
            onClick={() => {
              setSearch((q) => (q === null ? "" : null));
              setHitIndex(0);
            }}
          >
            ⌕
          </button>
        </nav>

        {search !== null && (
          <div id="sky3d-search" className="sky3d-search" role="dialog" aria-label="Search the sky">
            <input
              autoFocus
              value={search}
              placeholder="Project, feature, or #issue"
              aria-label="Search"
              aria-controls="sky3d-hits"
              onChange={(e) => {
                setSearch(e.target.value);
                setHitIndex(0);
              }}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setHitIndex((i) => Math.min(i + 1, hits.length - 1));
                } else if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setHitIndex((i) => Math.max(i - 1, 0));
                } else if (e.key === "Enter" && hits[hitIndex]) go(hits[hitIndex]);
              }}
            />
            <ul id="sky3d-hits" role="listbox" aria-label="Matches">
              {hits.map((h, i) => (
                <li key={`${h.kind}:${h.label}:${h.sub}`} role="option" aria-selected={i === hitIndex}>
                  <button onClick={() => go(h)} onPointerEnter={() => setHitIndex(i)}>
                    <i className={`sky3d-kind ${h.kind}`}>{h.kind}</i>
                    <b>{truncate(h.label, 54)}</b>
                    <small>{h.sub}</small>
                  </button>
                </li>
              ))}
              {search.trim() && filter.count === 0 && (
                <li className="sky3d-dim" role="status">
                  No match
                </li>
              )}
            </ul>
          </div>
        )}

        {zoomed && (
          <header ref={blocker} className="sky3d-head">
            <h2>{zoomed.repo}</h2>
            <GalaxyFacts c={zoomed} />
            {zoomed.chain.length > 0 && <p data-testid="chain-count">Longest chain: {zoomed.chain.length} stars</p>}
          </header>
        )}
        {zoomed && zoomed.features.length === 0 && (
          <div ref={unchartedRef} className="sky3d-panel sky3d-uncharted">
            <h3>Uncharted</h3>
            <p>A nebula: no roadmap features yet, so nothing has condensed into stars.</p>
            <dl>
              <dt>Rank</dt>
              <dd>{zoomed.rank + 1}</dd>
              <dt>Drift</dt>
              <dd className={zoomed.drift.length ? "is-warn" : undefined}>{zoomed.drift.length}</dd>
              <dt>Blocked-by lines</dt>
              <dd>{zoomed.edges.length}</dd>
              <dt>Progress to 1.0</dt>
              <dd>{Math.round(zoomed.brightness * 100)}%</dd>
            </dl>
            {zoomed.drift.length > 0 && (
              <button className="btn" onClick={() => setDrawer(true)}>
                Show drift
              </button>
            )}
          </div>
        )}

        <div ref={blocker} className="sky3d-toolbar">
          {zoomed && (
            <button className="btn primary sky3d-back" onClick={onBack}>
              ← Back to the sky
            </button>
          )}
          {zoomed && <ProjectActions url={zoomed.url} repo={zoomed.repo} />}
          {toolbar}
        </div>

        <button
          ref={blocker}
          className={`btn sky3d-drawer-toggle${drift.length ? " has-drift" : ""}`}
          aria-expanded={drawer}
          aria-controls="sky3d-drawer"
          onClick={() => setDrawer((d) => !d)}
        >
          Drift <b>{drift.length}</b>
        </button>
        <aside id="sky3d-drawer" className={`sky3d-drawer${drawer ? " is-open" : ""}`} aria-hidden={!drawer}>
          {drawer && (
            <>
              <button className="btn sky3d-drawer-close" aria-label="Close drift" onClick={() => setDrawer(false)}>
                ×
              </button>
              <DriftList drift={drift} />
            </>
          )}
        </aside>

        {devFps && <p className="sky3d-devfps">{devFps}</p>}
        {/* The bottom bar: the snapshot ages, the lens switch and the legend share one row and
            wrap rather than overlap; narrow, the legend folds into a toggle. */}
        <footer ref={blocker} className="sky3d-bottom">
          <p className="sky3d-ages" data-testid="ages">
            {ageLine(sky)}
          </p>
          <div className="seg-group sky3d-lenses" role="group" aria-label="Lens">
            {LENSES.map((l) => (
              <button key={l.id} className="seg" aria-pressed={lens === l.id} aria-keyshortcuts={l.key} title={`${l.asks} (${l.key})`} onClick={() => setLens(l.id)}>
                {l.name}
              </button>
            ))}
          </div>
          <div className="sky3d-legend">
            <button className="btn sky3d-legend-toggle" aria-expanded={legend} aria-controls="sky3d-legend" onClick={() => setLegend((v) => !v)}>
              Legend
            </button>
            <div id="sky3d-legend" className={`sky3d-hint${legend ? " is-open" : ""}`}>
              {lens !== "natural" ? (
                <p>
                  <b>{lensInfo.name}</b> · {lensInfo.asks} · {lensCount === 0 ? "none right now" : `${lensCount} of ${sky.constellations.length} projects`} · n for natural
                </p>
              ) : zoomed && zoomed.features.length > 0 ? (
                <p>
                  <i className="k closed" /> closed <i className="k open" /> open <i className="k flight" /> in flight <i className="k pass" /> PR pass{" "}
                  <i className="k fail" /> PR fail <i className="k dark" /> no issues <i className="k chain" /> longest chain · bigger = harder
                </p>
              ) : (
                <p>
                  <i className="k hole" /> switchboard <i className="k nebula" /> nebula: uncharted <i className="k cluster" /> cluster: near 1.0{" "}
                  <i className="k debris" /> drift <i className="k bridge" /> reuse
                </p>
              )}
              <p className="sky3d-dim">Scroll or pinch to zoom · drag to orbit · / to search · Esc to step back</p>
            </div>
          </div>
        </footer>
      </section>
    </>
  );
}

/** The open project on GitHub: file an issue, see the repo, see its pull requests (new tabs). */
function ProjectActions({ url, repo }: { url: string; repo: string }) {
  const base = (url || `https://github.com/${repo}`).replace(/\/+$/, "");
  return (
    <nav className="sky3d-actions" aria-label={`${repo} on GitHub`}>
      <a className="btn" href={`${base}/issues/new`} target="_blank" rel="noreferrer">
        New issue
      </a>
      <a className="btn" href={base} target="_blank" rel="noreferrer">
        Open on GitHub
      </a>
      <a className="btn" href={`${base}/pulls`} target="_blank" rel="noreferrer">
        Pull requests
      </a>
    </nav>
  );
}

function GalaxyFacts({ c }: { c: Constellation }) {
  const all = c.features.flatMap((f) => f.stars);
  const closed = all.filter((s) => s.state === "closed").length;
  const flying = all.filter((s) => s.inFlight).length;
  const parts = [`Progress to 1.0 ${Math.round(c.brightness * 100)}%`, `${c.features.length} features`, `${all.length} stars`, `${closed} closed`];
  if (flying) parts.push(`${flying} in flight`);
  return <p className="sky3d-facts">{parts.join(" · ")}</p>;
}

function ProjectCard({ c }: { c: Constellation }) {
  const stars = c.features.reduce((n, f) => n + f.stars.length, 0);
  const pct = Math.round(c.brightness * 100);
  return (
    <>
      <h3>{c.repo}</h3>
      <p className="sky3d-rank">
        Rank {c.rank + 1}
        {isCentre(c.repo) && " · the black hole at the centre"}
      </p>
      <div className="sky3d-progress" role="img" aria-label={`Progress to 1.0: ${pct}%`}>
        <span style={{ width: `${pct}%` }} />
      </div>
      <p>Progress to 1.0 · {pct}%</p>
      <p>
        <span className="badge">
          Drift <b>{c.drift.length}</b>
        </span>
      </p>
      <p className="sky3d-dim">{c.features.length ? `${c.features.length} features · ${stars} stars` : "Uncharted: a nebula, no features yet"}</p>
    </>
  );
}
