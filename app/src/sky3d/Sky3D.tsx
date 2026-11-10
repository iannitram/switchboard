import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { Constellation, SkyModel, Star } from "../sky/model";
import { DriftList } from "../sky/DriftList";
import { StarPanel } from "../sky/StarPanel";
import { SkyScene } from "./SkyScene";
import { anchorPanel } from "./anchor";
import { overviewLabelRepos, placeLabels, shortName, truncate, type LabelCandidate, type Rect } from "./labels";
import { RING_NAMES } from "./layout";
import { prefersReducedMotion } from "./support";
import "./sky3d.css";

// The 3D Sky (SKY-1 to SKY-9, #297, #299). The scene draws; this overlay keeps the meaning in
// the DOM: a button per galaxy and per star (role, aria-label, keyboard), the labels for the
// current zoom level, the info panel beside the object, the controls and the drift drawer.
// The face holds no rules: everything shown comes from the SkyModel the core sent.

/** Project names shown at the overview without hovering: the top of the priority order. */
const TOP_LABELS = 14;
const FEATURE_CHARS = 26;

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

export default function Sky3D({ sky, open, onOpen, onBack, picked, onPick, toolbar, onFail }: Props) {
  const rootRef = useRef<HTMLElement>(null);
  const anchorRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const sceneRef = useRef<SkyScene | null>(null);
  const els = useRef<Els>({ hit: new Map(), label: new Map(), star: new Map(), feature: new Map(), ring: new Map() });
  const panelRef = useRef<HTMLDivElement>(null);
  const starPanelRef = useRef<HTMLDivElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const overviewLayer = useRef<HTMLDivElement>(null);
  const galaxyLayer = useRef<HTMLDivElement>(null);
  const blockersRef = useRef<HTMLElement[]>([]);
  const topRef = useRef(-1);
  const frameRef = useRef<() => void>(() => {});
  const failRef = useRef(onFail);
  failRef.current = onFail;

  const [hovered, setHovered] = useState<string | null>(null);
  const [hoverStar, setHoverStar] = useState<number | null>(null);
  const [hoverFeature, setHoverFeature] = useState<string | null>(null);
  const [drawer, setDrawer] = useState(false);
  const [reduced, setReduced] = useState(prefersReducedMotion);

  const zoomed: Constellation | undefined = open === null ? undefined : sky.constellations.find((c) => c.repo === open);
  const labelled = useMemo(() => overviewLabelRepos(sky.constellations, TOP_LABELS, [hovered]), [sky, hovered]);
  const stars = useMemo(() => new Map((zoomed?.features ?? []).flatMap((f) => f.stars.map((s) => [s.number, s] as const))), [zoomed]);

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

  useEffect(() => sceneRef.current?.setSky(sky), [sky]);
  useEffect(() => {
    sceneRef.current?.setOpen(open);
    setHoverStar(null);
    setHoverFeature(null);
    setHovered(null);
    setDrawer(false);
  }, [open]);
  useEffect(() => sceneRef.current?.setHovered(hovered), [hovered]);
  useEffect(() => sceneRef.current?.setFocusStar(hoverStar), [hoverStar]);
  useEffect(() => sceneRef.current?.setPicked(picked?.number ?? null), [picked]);
  useEffect(() => sceneRef.current?.invalidate());

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

  // Escape: reverse a fly-in, finish a fly-out, close the panel, close the drawer, fly out.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const scene = sceneRef.current;
      if (scene?.isFlying()) {
        if (open) onBack();
        else scene.completeFlight();
      } else if (picked) onPick(null);
      else if (drawer) setDrawer(false);
      else if (open) onBack();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, picked, drawer, onBack, onPick]);

  // Drag to orbit a little, wheel to zoom a little; a click during a flight completes it.
  const drag = useRef<{ x: number; y: number; moved: number } | null>(null);
  const onPointerDown = (e: React.PointerEvent) => {
    drag.current = { x: e.clientX, y: e.clientY, moved: 0 };
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dx = e.clientX - d.x;
    const dy = e.clientY - d.y;
    d.x = e.clientX;
    d.y = e.clientY;
    d.moved += Math.abs(dx) + Math.abs(dy);
    sceneRef.current?.orbitBy(dx, dy);
  };
  const onPointerUp = () => {
    const d = drag.current;
    drag.current = null;
    if (!d || d.moved > 4) return;
    const scene = sceneRef.current;
    if (scene?.isFlying()) scene.completeFlight();
    else if (picked) onPick(null);
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
    const blockers = blockersRef.current
      .filter((el) => el.isConnected && el.offsetWidth > 0)
      .map((el) => rel(el.getBoundingClientRect(), rootBox));
    const e = scene.openBlend();
    const E = els.current;
    const cands: LabelCandidate[] = [];
    const size = (el: HTMLElement) => ({ width: el.offsetWidth, height: el.offsetHeight });
    const panel = panelRef.current;
    const panelSize = panel ? size(panel) : null;
    const starPanel = starPanelRef.current;
    const starPanelSize = starPanel ? size(starPanel) : null;
    const tip = tipRef.current;
    const tipSize = tip ? size(tip) : null;

    if (overviewLayer.current) overviewLayer.current.style.opacity = String(Math.max(0, 1 - e * 2));
    if (galaxyLayer.current) galaxyLayer.current.style.opacity = String(Math.max(0, (e - 0.5) * 2));

    if (open === null) {
      for (const c of sky.constellations) {
        const s = scene.galaxyScreen(c.repo);
        const hit = E.hit.get(c.repo);
        if (!s || !hit) continue;
        const d = Math.max(24, s.r * 1.7);
        hit.style.width = hit.style.height = `${Math.round(d)}px`;
        place(hit, s.x - d / 2, s.y - d / 2, s.visible);
        const label = E.label.get(c.repo);
        if (label && labelled.has(c.repo) && s.visible) cands.push({ id: `g:${c.repo}`, x: s.x, y: s.y + s.r * 0.62 + 6, ...size(label), priority: c.rank, forced: c.repo === hovered });
      }
    } else if (zoomed && scene.interiorRepo() === zoomed.repo) {
      for (const [n, el] of E.star) {
        const s = scene.starScreen(n);
        if (s) place(el, s.x - 9, s.y - 9, s.visible);
      }
      zoomed.features.forEach((f, i) => {
        const el = E.feature.get(f.id);
        const s = scene.featureScreen(f.id);
        if (!el || !s) return;
        const order = f.release === "1.0" ? 0 : f.release === "after-1.0" ? 100 : 200;
        cands.push({ id: `f:${f.id}`, x: s.x, y: s.y + s.r + 5, ...size(el), priority: order - f.stars.length + i * 0.01, forced: f.id === hoverFeature });
      });
      for (const [release, el] of E.ring) {
        const s = scene.ringScreen(release, Math.PI * 0.82);
        if (s) cands.push({ id: `r:${release}`, x: s.x, y: s.y - 8, ...size(el), priority: -1 });
      }
      const tipStar = hoverStar ?? picked?.number ?? null;
      if (tip && tipSize && tipStar !== null && !picked) {
        const s = scene.starScreen(tipStar);
        if (s) cands.push({ id: "tip", x: s.x, y: s.y + 12, ...tipSize, priority: -10, forced: true });
      }
    }

    const panelRects: Rect[] = [];
    // The info panel beside the hovered galaxy, or beside the picked star (#299).
    if (panel && panelSize && open === null && hovered) {
      const s = scene.galaxyScreen(hovered);
      if (s) {
        const a = anchorPanel({ x: s.x, y: s.y, r: s.r * 0.9 }, panelSize, view);
        place(panel, a.left, a.top, e < 0.2);
        panelRects.push({ left: a.left, top: a.top, ...panelSize });
      }
    }
    if (starPanel && starPanelSize && picked && open !== null) {
      const s = scene.starScreen(picked.number);
      if (s) {
        const a = anchorPanel({ x: s.x, y: s.y, r: 14 }, starPanelSize, view);
        place(starPanel, a.left, a.top, e > 0.6);
        panelRects.push({ left: a.left, top: a.top, ...starPanelSize });
      }
    }

    const placed = placeLabels(cands, view, [...blockers, ...panelRects]);
    const show = (id: string, el: HTMLElement | undefined | null) => {
      if (!el) return;
      const p = placed.get(id);
      if (p) place(el, p.left, p.top, true);
      else el.style.visibility = "hidden";
    };
    if (open === null) for (const [repo, el] of E.label) show(`g:${repo}`, el);
    for (const [id, el] of E.feature) show(`f:${id}`, el);
    for (const [release, el] of E.ring) show(`r:${release}`, el);
    show("tip", tip);
  };

  const drift = useMemo(
    () => (zoomed ? [zoomed] : sky.constellations).flatMap((c) => c.drift.map((d) => ({ ...d, repo: c.repo }))),
    [sky, zoomed],
  );
  const hov = hovered ? sky.constellations.find((c) => c.repo === hovered) : undefined;
  const tipStar = hoverStar !== null ? stars.get(hoverStar) : undefined;
  const blocker = (el: HTMLElement | null) => {
    blockersRef.current = blockersRef.current.filter((b) => b.isConnected);
    if (el && !blockersRef.current.includes(el)) blockersRef.current.push(el);
  };

  return (
    <>
      <div ref={anchorRef} className="sky3d-anchor" />
      <section
        ref={rootRef}
        className={`sky3d${reduced ? " sky3d-still" : ""}`}
        aria-label={zoomed ? `Constellation ${zoomed.repo}` : "Sky"}
      >
        <div
          ref={stageRef}
          className="sky3d-stage"
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={() => (drag.current = null)}
          onWheel={(e) => sceneRef.current?.zoomBy(e.deltaY)}
        />

        {open === null ? (
          <div ref={overviewLayer} className="sky3d-layer">
            {sky.constellations.map((c) => (
              <button
                key={c.repo}
                ref={keep(els.current.hit, c.repo)}
                className={`sky3d-hit${c.repo === hovered ? " is-hovered" : ""}`}
                data-testid="constellation"
                data-repo={c.repo}
                aria-label={c.repo}
                onClick={() => onOpen(c.repo)}
                onPointerEnter={() => setHovered(c.repo)}
                onPointerLeave={() => setHovered((h) => (h === c.repo ? null : h))}
                onFocus={() => setHovered(c.repo)}
                onBlur={() => setHovered((h) => (h === c.repo ? null : h))}
              />
            ))}
            {sky.constellations.map((c) => (
              <div key={c.repo} ref={keep(els.current.label, c.repo)} className={`sky3d-label${c.rank < 3 ? " is-top" : ""}`} aria-hidden="true">
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
        ) : (
          zoomed && (
            <div ref={galaxyLayer} className="sky3d-layer">
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
          )
        )}

        {zoomed && (
          <header ref={blocker} className="sky3d-head">
            <h2>{zoomed.repo}</h2>
            <GalaxyFacts c={zoomed} />
            {zoomed.chain.length > 0 && <p data-testid="chain-count">Longest chain: {zoomed.chain.length} stars</p>}
          </header>
        )}
        {zoomed && zoomed.features.length === 0 && (
          <div className="sky3d-panel sky3d-uncharted">
            <h3>Uncharted</h3>
            <p>No roadmap features yet, so there are no constellations to draw.</p>
            <dl>
              <dt>Drift</dt>
              <dd className={zoomed.drift.length ? "is-warn" : undefined}>{zoomed.drift.length}</dd>
              <dt>Blocked-by lines</dt>
              <dd>{zoomed.edges.length}</dd>
              <dt>Progress to 1.0</dt>
              <dd>{Math.round(zoomed.brightness * 100)}%</dd>
            </dl>
            {zoomed.drift.length > 0 && <button onClick={() => setDrawer(true)}>Show drift</button>}
          </div>
        )}

        <div ref={blocker} className="sky3d-toolbar">
          {zoomed && (
            <button className="sky3d-back" onClick={onBack}>
              ← Back to the sky
            </button>
          )}
          {toolbar}
        </div>

        <button
          ref={blocker}
          className={`sky3d-drawer-toggle${drift.length ? " has-drift" : ""}`}
          aria-expanded={drawer}
          aria-controls="sky3d-drawer"
          onClick={() => setDrawer((d) => !d)}
        >
          Drift <span>{drift.length}</span>
        </button>
        <aside id="sky3d-drawer" className={`sky3d-drawer${drawer ? " is-open" : ""}`} aria-hidden={!drawer}>
          {drawer && (
            <>
              <button className="sky3d-drawer-close" aria-label="Close drift" onClick={() => setDrawer(false)}>
                ×
              </button>
              <DriftList drift={drift} />
            </>
          )}
        </aside>

        <p ref={blocker} className="sky3d-ages" data-testid="ages">
          {ageLine(sky)}
        </p>
        <p ref={blocker} className="sky3d-hint" aria-hidden="true">
          {zoomed ? (
            <>
              <i className="k closed" /> closed <i className="k open" /> open <i className="k flight" /> in flight <i className="k dark" /> dark{" "}
              <i className="k chain" /> chain · Esc to go back
            </>
          ) : (
            "Drag to orbit · scroll to zoom · click a galaxy to fly in"
          )}
        </p>
      </section>
    </>
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
      <p className="sky3d-rank">Rank {c.rank + 1}</p>
      <div className="sky3d-progress" role="img" aria-label={`Progress to 1.0: ${pct}%`}>
        <span style={{ width: `${pct}%` }} />
      </div>
      <p>Progress to 1.0 · {pct}%</p>
      <p className={c.drift.length ? "is-warn" : undefined}>Drift · {c.drift.length}</p>
      <p className="sky3d-dim">{c.features.length ? `${c.features.length} features · ${stars} stars` : "Uncharted: no features yet"}</p>
    </>
  );
}
