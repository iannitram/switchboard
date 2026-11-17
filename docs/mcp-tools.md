# MCP tools

Generated from `switchboard mcp`'s `tools/list` registry by `tools/arch_lint.py`.
Regenerate with `python3 -I tools/arch_lint.py --write-mcp-tools`.

## Read only

| Tool | Description | Input schema |
| --- | --- | --- |
| `sky` | The sky: one constellation per repo, as the GUI shows it. | `{"type":"object","properties":{}}` |
| `constellation` | One repo's constellation: its stars and where they sit. | `{"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"}},"required":["repo"]}` |
| `star` | One star: an issue or PR with its labels, size, PR and review. | `{"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"},"issue":{"type":"integer","minimum":1}},"required":["repo","issue"]}` |
| `waiting` | The Waiting on you list: items that need a person, one merged list. | `{"type":"object","properties":{"limit":{"type":"integer","minimum":1}}}` |
| `reports` | Reports built from job cards and handoff threads. | `{"type":"object","properties":{"limit":{"type":"integer","minimum":1},"project":{"type":"string"},"outcome":{"type":"string"},"pool":{"type":"string"}}}` |
| `budget` | Fleet budget: the pool rows and what is left in each. | `{"type":"object","properties":{}}` |

## project-manager

| Tool | Description | Input schema |
| --- | --- | --- |
| `sky` | The sky: one constellation per repo, as the GUI shows it. | `{"type":"object","properties":{}}` |
| `constellation` | One repo's constellation: its stars and where they sit. | `{"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"}},"required":["repo"]}` |
| `star` | One star: an issue or PR with its labels, size, PR and review. | `{"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"},"issue":{"type":"integer","minimum":1}},"required":["repo","issue"]}` |
| `waiting` | The Waiting on you list: items that need a person, one merged list. | `{"type":"object","properties":{"limit":{"type":"integer","minimum":1}}}` |
| `reports` | Reports built from job cards and handoff threads. | `{"type":"object","properties":{"limit":{"type":"integer","minimum":1},"project":{"type":"string"},"outcome":{"type":"string"},"pool":{"type":"string"}}}` |
| `budget` | Fleet budget: the pool rows and what is left in each. | `{"type":"object","properties":{}}` |
| `autopilot_status` | The autopilot's state: what its passes have started and seen, whether the weekly run is due, the running jobs, and the satellite's loops and holds. | `{"type":"object","properties":{}}` |
| `merge` | Merge a PR once its latest PASS is on its head and CI is green on that head. | `{"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"},"pr":{"type":"integer","minimum":1}},"required":["repo","pr"]}` |
| `ask` | Route one message through the front door and answer it; dry_run returns the route and its reason and sends, stores and spends nothing. | `{"type":"object","properties":{"message":{"type":"string"},"thread":{"type":"string","description":"An open thread's id; none starts a thread"},"dry_run":{"type":"boolean"}},"required":["message"]}` |
| `ask_as_job` | Send a request this machine cannot serve as a fleet job another machine pulls. With thread, the job carries that chat's packet (summary, last 6 turns, links) and the move is stored. | `{"type":"object","properties":{"kind":{"type":"string","description":"The request kind"},"request":{"type":"string"},"unmet":{"type":"array","items":{"type":"string"},"minItems":1,"description":"The needs this machine does not meet; left out, they are read from this machine's loadout"},"thread":{"type":"string","description":"The chat's thread id, to send the chat with its packet"}},"required":["kind","request"]}` |
| `rate` | Rate one answer of a thread up or down; a second rating of a turn replaces the first. | `{"type":"object","properties":{"thread":{"type":"string"},"turn":{"type":"integer","minimum":0,"description":"The answer's index among the thread's turns"},"rating":{"type":"string","enum":["up","down"]}},"required":["thread","turn","rating"]}` |
| `request_archive` | Ask to archive a repo (read-only on GitHub). This only files a Waiting on you request; the owner confirms it in the app. | `{"type":"object","properties":{"repo":{"type":"string","description":"The repo's name or owner/name"}},"required":["repo"]}` |
| `request_restore` | Ask to restore a repo's backup over the live repo. This only files a Waiting on you request; the owner confirms it in the app. | `{"type":"object","properties":{"repo":{"type":"string","description":"The repo's name or owner/name"}},"required":["repo"]}` |
| `drop_request` | Turn down a waiting archive or restore request: it leaves Waiting on you and nothing else happens. | `{"type":"object","properties":{"kind":{"type":"string","description":"archive or restore"},"repo":{"type":"string","description":"The repo's name or owner/name"}},"required":["kind","repo"]}` |
| `autopilot_pass` | Run one autopilot pass now, as the app's poll does, and return its report. With dry_run it starts no job, merges nothing and writes no state, and reports what it would do. | `{"type":"object","properties":{"pass":{"type":"string","enum":["roadmap","review","merge","weekly"]},"dry_run":{"type":"boolean"}},"required":["pass"]}` |

## budget-manager

| Tool | Description | Input schema |
| --- | --- | --- |
| `sky` | The sky: one constellation per repo, as the GUI shows it. | `{"type":"object","properties":{}}` |
| `constellation` | One repo's constellation: its stars and where they sit. | `{"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"}},"required":["repo"]}` |
| `star` | One star: an issue or PR with its labels, size, PR and review. | `{"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"},"issue":{"type":"integer","minimum":1}},"required":["repo","issue"]}` |
| `waiting` | The Waiting on you list: items that need a person, one merged list. | `{"type":"object","properties":{"limit":{"type":"integer","minimum":1}}}` |
| `reports` | Reports built from job cards and handoff threads. | `{"type":"object","properties":{"limit":{"type":"integer","minimum":1},"project":{"type":"string"},"outcome":{"type":"string"},"pool":{"type":"string"}}}` |
| `budget` | Fleet budget: the pool rows and what is left in each. | `{"type":"object","properties":{}}` |
| `set_pool_cap` | Set one budget pool's dollar cap; the reply states the new cap. | `{"type":"object","properties":{"pool":{"type":"string","description":"The pool's name"},"cap":{"type":"number","minimum":0}},"required":["pool","cap"]}` |
| `pool_edit` | Edit one budget pool in place (plan, cap, reset, cost), as the Budget panel does. | `{"type":"object","properties":{"pool":{"type":"string","description":"The pool's name"},"plan":{"type":"string"},"cap":{"type":"number","minimum":0},"reset":{"type":"string","description":"e.g. Sun 6:00am"},"cost_usd":{"type":"number","minimum":0}},"required":["pool"]}` |
| `budget_refresh` | Re-read every pool that has a reader now, then return the budget rows. | `{"type":"object","properties":{}}` |

## satellite

| Tool | Description | Input schema |
| --- | --- | --- |
| `sky` | The sky: one constellation per repo, as the GUI shows it. | `{"type":"object","properties":{}}` |
| `constellation` | One repo's constellation: its stars and where they sit. | `{"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"}},"required":["repo"]}` |
| `star` | One star: an issue or PR with its labels, size, PR and review. | `{"type":"object","properties":{"repo":{"type":"string","description":"The repo's name, e.g. switchboard"},"issue":{"type":"integer","minimum":1}},"required":["repo","issue"]}` |
| `waiting` | The Waiting on you list: items that need a person, one merged list. | `{"type":"object","properties":{"limit":{"type":"integer","minimum":1}}}` |
| `reports` | Reports built from job cards and handoff threads. | `{"type":"object","properties":{"limit":{"type":"integer","minimum":1},"project":{"type":"string"},"outcome":{"type":"string"},"pool":{"type":"string"}}}` |
| `budget` | Fleet budget: the pool rows and what is left in each. | `{"type":"object","properties":{}}` |
| `errors` | The fleet errors the app recorded, newest first, and the open IT tickets. | `{"type":"object","properties":{}}` |
| `autopilot_status` | The autopilot's state: what its passes have started and seen, whether the weekly run is due, the running jobs, and the satellite's loops and holds. | `{"type":"object","properties":{}}` |
| `satellite_tick` | Run one tick of the satellite loop on this machine and return its report; the loops it starts run on while this server lives. With dry_run it starts nothing and reports the lanes it would start. | `{"type":"object","properties":{"dry_run":{"type":"boolean"}}}` |
