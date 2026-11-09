import type { Move } from "../brocrew/ReportEntry";

// REP-2 (#47): the work tree of a report, one line per move, oldest first. No moves says so.
export function WorkTree({ moves }: { moves: Move[] }) {
  if (moves.length === 0) return <p data-part="work-tree">no moves</p>;
  return (
    <ol data-part="work-tree" aria-label="Work tree">
      {moves.map((m, i) => (
        <li key={i}>{`${m.from_service}/${m.from_model} → ${m.to_service}/${m.to_model}: ${m.why}`}</li>
      ))}
    </ol>
  );
}
