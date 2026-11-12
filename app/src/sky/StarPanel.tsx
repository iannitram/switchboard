import type { Star } from "./model";

// SKY-8: one star's panel: its labels, size, PR with state and review verdict, fleet job and a
// link to the issue. The face holds no rules; it paints the Star as the core sent it.
export function StarPanel({ star, onClose }: { star: Star; onClose: () => void }) {
  const { pr } = star;
  return (
    <aside aria-label={`Star #${star.number}`}>
      <h3>{`#${star.number} ${star.title}`}</h3>
      <ul aria-label="Labels">
        {star.labels.map((label) => (
          <li key={label}>{label}</li>
        ))}
      </ul>
      <p>{star.size === null ? "Size: not sized" : `Size: ${star.size}`}</p>
      {pr ? (
        <p>
          <a href={pr.url} target="_blank" rel="noreferrer">{`PR #${pr.number} (${pr.state})`}</a>
        </p>
      ) : (
        <p>{star.state === "closed" ? "Closed without a PR" : "No PR yet"}</p>
      )}
      <p>{`Review: ${pr?.verdict ?? "not reviewed"}`}</p>
      <p>{`Job: ${star.job ?? "none"}`}</p>
      <a href={star.url} target="_blank" rel="noreferrer">Open on GitHub</a>
      <button className="btn" onClick={() => onClose()}>
        Close
      </button>
    </aside>
  );
}
