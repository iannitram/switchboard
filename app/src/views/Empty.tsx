// SHELL-1: an empty view says "no data yet", never a blank pane.
export function Empty({ view }: { view: string }) {
  return (
    <section aria-label={view}>
      <p>no data yet</p>
    </section>
  );
}
