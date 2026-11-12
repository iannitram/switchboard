import type { DriftItem } from "./model";

// SKY-5: every constellation's drift in one list, each item linking to the issue or ROADMAP.md
// that fixes it. The face holds no rules; it paints the drift the core sent, in the order given.
export function DriftList({ drift }: { drift: (DriftItem & { repo: string })[] }) {
  return (
    <section aria-label="Drift">
      <h3>Drift ({drift.length})</h3>
      {drift.length === 0 ? (
        <p>No drift</p>
      ) : (
        <ul>
          {drift.map((d, i) => (
            <li key={`${d.repo}#${i}`} data-kind={d.kind}>
              <a href={d.link} target="_blank" rel="noreferrer">{`${d.repo.slice(d.repo.lastIndexOf("/") + 1)}: ${d.text}`}</a>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
