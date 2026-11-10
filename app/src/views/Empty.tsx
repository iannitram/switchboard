// SHELL-1: an empty view says "no data yet", never a blank pane.
export function Empty({ view }: { view: string }) {
  return (
    <section aria-label={view} className="empty">
      <svg aria-hidden="true" width="44" height="44" viewBox="0 0 48 48" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round">
        <circle cx="24" cy="24" r="17" opacity="0.5" />
        <circle cx="24" cy="24" r="8" />
        <circle cx="24" cy="24" r="1.6" fill="currentColor" />
        <path d="M24 3v6M24 39v6M3 24h6M39 24h6" opacity="0.7" />
      </svg>
      <p>no data yet</p>
    </section>
  );
}
