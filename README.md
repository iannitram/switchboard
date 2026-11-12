# Switchboard

The front door that sends each ask to the cheapest model and service that can do it within budget, and one page that shows every handoff, everything waiting on me, and each result.

**Stage:** see `PIPELINE.md`.

## Start here

- `IDEA.md`: the idea, in my words.
- `SPEC.md`: what it must do, what it won't do, and what 1.0 means.
- `ROADMAP.md`: the feature list, the build stages and the platform plan; `docs/code-map.md`: the modules.
- `GLOSSARY.md`: the project's words.
- `docs/adr/`: decisions made so far.
- `AGENTS.md`: rules for anyone, person or agent, changing the code.
- `CHANGELOG.md`: what changed and when.

## Credits

Switchboard borrows ideas, and where the license allows, code, from:

- [BroCrew](https://github.com/brocrew/brocrew) (Apache-2.0): the work tree, the "Waiting on you" view and the fixed report.
- [rengwu/wayfinder-maps](https://github.com/rengwu/wayfinder-maps) (MIT): the map of a project's roadmap as a navigable sky.
- [ocuclaw/obsidian-wayfinder](https://github.com/ocuclaw/obsidian-wayfinder) (MIT): the dependency layers and in-column order behind the Roadmap lens (`app/src/sky/layers.ts`).

Any code lifted from them keeps its license notice and credit.
