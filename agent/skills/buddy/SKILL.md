---
name: buddy
description: Drive desk-buddy's body, keep reminders and remember the person with the `buddy` command (say, mascot, remind, memory, status).
---

# buddy

`buddy` talks to the desk-buddy app over its local API. Every command you run already has the address and token it
needs; never print or copy them.

## Speak and move

- `buddy say "<text>"` shows a speech bubble. Keep it to one or two short sentences.
  Add `--emotion <e>` to set the bear's face (`buddy --help --json` lists the emotions).
- `buddy mascot do <activity> [--for 5m]` does something physical (read a book, take a nap) after your reply;
  `buddy mascot stop` ends it. `buddy mascot emote <name>` plays a short gesture, such as a wave.
  `buddy mascot list` shows the activities and emotes this bear has. A reply that starts with "Not now" (status
  "refused" in `--json`) means it did not happen: say so instead of pretending.
- `buddy status` checks the connection.

Your answer already streams into the bear's speech bubble as plain text (no Markdown), so don't repeat it with
`buddy say`. Use `buddy say` for a short line while you work ("Looking that up…") or for a note after the turn.
Keep bubble lines short: after six lines the bubble scrolls.

## Reminders

Reminders are kept by the app and survive a restart. When one is due, the bear shows it to the person; you don't
need to do anything then.

- `buddy remind --in <duration> <text…>`: durations like `90s`, `20m`, `1h30m`, `2d`. At least 10 seconds, at most 7 days.
- `buddy remind --at HH:MM <text…>`: the next time the clock shows HH:MM (24-hour, local time).
- `buddy remind list` (`--json` for the ids and times), `buddy remind cancel <id>`.

Write the reminder text as the person should read it later ("Stretch your legs"), not as an instruction to you.
Tell the person what you set and when it fires. If the agent isn't running, `buddy remind` says so; don't retry in a loop.

## Memory

The app remembers the person across chats on its own: after a chat it notes what matters, and the "What I know
about you" section of your prompt shows the core of it. You don't need to save every fact yourself.

- `buddy memory remember "<text>"` when the person asks you to remember something, or states a clear, lasting fact
  the notes may miss. Write one short third-person sentence ("Has a cat, Mochi"). For a known kind of fact add
  `--key <predicate>[:<qualifier>] --value <value>` (`--key home_city --value Hanoi`, `--key pets:cat --value Mochi`);
  `--kind preference|promise` for how they like help or something you promised without a time. The keys:
  `home_city`, `timezone`, `job`, `employer`, `reply_style`, `checkin_hours` (one value each: a new one replaces the
  old), `language`, `likes`, `dislikes`, `projects`, `people:<relation>`, `pets:<species>` (several). Any other key
  is kept as a plain note.
- "Not sure yet" lists a guess that differs from what the person told you. Ask once, when it fits. When they answer,
  `buddy memory remember` the right value with its `--key` (or `buddy memory forget <id>` the wrong one).
- A promise with a time is a reminder: use `buddy remind`, not memory.
- `buddy memory list` and `buddy memory search "<words>"` (`--json` for ids) before asking the person something
  you may already know.
- `buddy memory forget <id>` (or `--key <predicate>[:<qualifier>]`) when the person says a fact is wrong or asks you
  to forget it. Only the person can erase completely, in the window.
- `buddy memory open` opens the "What I remember" window when they want to see, fix or erase what you know.

Never recite what you remember unprompted, never store passwords, keys or codes (they are refused anyway), and
believe the person over your notes when they differ. If a command says memory is off, don't retry: the person
turned it off.
