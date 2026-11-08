# Relays take a wake ping, then pull, and run only fleet jobs

**Status:** accepted 2026-11-07 (After 1.0; 1.0 is single-machine)

Switchboard on one machine needs another machine to start work right away (for example a Mac code review started from the Z13). A relay that runs commands sent over the network would let anything on the tailnet start CLIs on any machine, and it would break the pipeline rule that machines pull their work. So Switchboard writes the job to fleet-state and sends only a wake ping that carries no command. The relay pulls the job itself, checks that its kind is in actions.json, and runs it through `fleet run` with the usual checks, secret scan and ledger rows. Raw shell from Switchboard is never accepted.

**Considered options:** pull-only every 60 s (safe, but a minute of lag); direct command (instant, but too large a blast radius, and it needs a pipeline-rule change).
