---
name: buddy
description: Drive desk-buddy's body and keep reminders with the `buddy` command (say, emote, remind, status).
---

# buddy

`buddy` talks to the desk-buddy app over its local API. Every command you run already has the address and token it
needs; never print or copy them.

## Speak and move

- `buddy say "<text>"` shows a speech bubble. Keep it to one or two short sentences.
  Add `--emotion <e>` to set the bear's face (`buddy --help --json` lists the emotions).
- `buddy mascot emote <name>` plays an emote, such as a wave (`buddy --help --json` lists them).
- `buddy status` checks the connection.

## Reminders

Reminders are kept by the app and survive a restart. When one is due, the bear shows it to the person; you don't
need to do anything then.

- `buddy remind --in <duration> <text…>`: durations like `90s`, `20m`, `1h30m`, `2d`. At least 10 seconds, at most 7 days.
- `buddy remind --at HH:MM <text…>`: the next time the clock shows HH:MM (24-hour, local time).
- `buddy remind list` (`--json` for the ids and times), `buddy remind cancel <id>`.

Write the reminder text as the person should read it later ("Stretch your legs"), not as an instruction to you.
Tell the person what you set and when it fires. If the agent isn't running, `buddy remind` says so; don't retry in a loop.
