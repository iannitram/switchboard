// A link to another site opens in the system browser, never inside the app window, where there is
// no way back. One click handler for the whole document; outside Tauri it does nothing and the
// link behaves as usual.
import { openUrl } from "@tauri-apps/plugin-opener";

/** True for an http(s) link whose origin is not the app's own. */
export function isExternalLink(href: string | null, base: string): boolean {
  if (!href) return false;
  let url: URL;
  try {
    url = new URL(href, base);
  } catch {
    return false;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return false;
  return url.origin !== new URL(base).origin;
}

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Mounts the handler; returns the function that removes it. */
export function openExternalLinksInBrowser(doc: Document = document): () => void {
  const onClick = (event: MouseEvent) => {
    if (event.defaultPrevented || event.button !== 0 || !inTauri()) return;
    const target = event.target instanceof Element ? event.target.closest("a[href]") : null;
    if (!target) return;
    const href = target.getAttribute("href");
    if (!isExternalLink(href, doc.location.href)) return;
    event.preventDefault();
    void openUrl(new URL(href!, doc.location.href).href).catch(() => undefined);
  };
  doc.addEventListener("click", onClick);
  return () => doc.removeEventListener("click", onClick);
}
