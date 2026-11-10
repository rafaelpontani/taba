---
title: Known issues
summary: "Index of the known issues: the open ones in a table, where the open (36a) and the fixed (36f) are, and the pattern every section follows."
type: reference
status: partial
verified-on: 2026-11-10 @ d6e1d66
sources: [AGENTS.md, docker/hang-hunt.sh, crates/taba-node/tests/common/cluster.rs]
---

# Known issues

> Read first: [25-testing.md](25-testing.md). The rules for handling intermittent tests are in [AGENTS.md](../../AGENTS.md).

A known issue is an intermittent test or a problem found in the system that is not solved on the spot. Each one has a code `KI-NNN` (known issue), given in the order they were found: the code is never reused, a reopened issue keeps it, and it is the way to cite the issue in commits, pending items and decisions. The next free code is the one after the highest of [36a-known-issues.md](36a-known-issues.md) and [36f-known-issues.md](36f-known-issues.md).

## Where they are

| File | What it holds |
|---|---|
| This one | The index of the **open** issues (the table below), where they are and how a section is written. |
| [36a-known-issues.md](36a-known-issues.md) | The sections of the **open** issues (`Open` or `Reopened`), and the lessons learned from all of them. |
| [36f-known-issues.md](36f-known-issues.md) | The sections of the **fixed** issues (`Fixed`), with their own table: history, and the place to look when one comes back. |

A section is **never deleted**; it moves between the two files, and its row moves with it (the table below for the open ones, the table of 36f-known-issues.md for the fixed ones):

- A new issue goes to 36a-known-issues.md and gets a row here. One found and fixed in the same change goes straight to 36f-known-issues.md.
- When fixed: status `Fixed`, the fixed date and a history row (what changed and how it was verified); the section goes to 36f-known-issues.md and the row leaves this table for the one of 36f-known-issues.md.
- When it **comes back**: no new section. It goes back to 36a-known-issues.md with the status `Reopened`, the fixed date cleared and a history row for the new occurrence; when fixed again, another row, `Fixed` with the new date, and back to 36f-known-issues.md.
- A link to a section points to the file it is in at that moment (`36a-known-issues.md#ki-...` or `36f-known-issues.md#ki-...`): moving a section updates them.

## Open issues

| Code | Issue | Status | Found on |
|---|---|---|---|
| KI-034 | [A restarted node in the table together with the old one in an overloaded hunt](36a-known-issues.md#ki-034-a-restarted-node-in-the-table-together-with-the-old-one-in-an-overloaded-hunt) | Open | 2026-11-10 |
| KI-005 | [A FIFO restart test that waits 30 s in a combined run](36a-known-issues.md#ki-005-a-fifo-restart-test-that-waits-30-s-in-a-combined-run) | Open | 2026-10-22 |

## How a section is written

The title starts with the code; the line right below it has the status and the dates; then the fields, in this order, and the history. The fields of a fixed issue are added to the ones it had while open (the cause, once found, replaces "not identified"):

```markdown
## KI-NNN: <what is seen, in a few words: the test or the behaviour>

**Status:** Open · **Found on:** YYYY-MM-DD · **Fixed on:** —

- **Test:** `tests/<file>.rs::<test>` (UC-…), or **Where:** the job, the command or the module.
- **Symptom:** the message or the wrong behaviour, where it was seen (job, commit, run) and how often.
- **Cause:** what was found and how (log, stack, code); `not identified` while it is not known.
- **Next step:** (while open) what to run or read to find the cause.
- **Do not:** (optional) the tempting fix that would hide it, such as raising the 30 s of `eventually`.
- **Fix (YYYY-MM-DD, in the server | in the test):** what changed, the test that fails without it.
- **Verification:** the command, how many runs under load, how many failures.

**History**

| Date | Event |
|---|---|
| YYYY-MM-DD | Found: where, how many failures in how many runs. |
| YYYY-MM-DD | Cause found …; fixed …; N runs with no failure. |
```

- **Status:** `Open`, `Fixed` or `Reopened`; the dates are `YYYY-MM-DD` and `—` while there is none.
- The texts say what was seen and measured (runs, failures, commit, job), not what is supposed; a cause not found is written as such, with the next step.
- An extra field is fine when the case needs it (**How it was found**, **Risk**, **Test of the fix**, **Second fix**); the ones above always come in this order.

## Sources and evidence

- [AGENTS.md](../../AGENTS.md), section "Intermittent tests and known issues" (the rules and the method of the hunt).
- `docker/hang-hunt.sh` and `tests/common/cluster.rs` (`eventually`, `TABA_TEST_LOG`), the tools the sections cite.
