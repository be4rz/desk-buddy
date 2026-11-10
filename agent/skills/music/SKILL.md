---
name: music
description: Play music from YouTube in desk-buddy's music widget, and pause, skip or check what plays (also Music or Spotify) with `buddy music`.
---

# music

The music widget is a small player window on the person's desktop. It keeps playing when the person hides it.

- `buddy music play <search words | YouTube link | video id>` finds and plays a video. Add `--json` for the state
  (`{ok, state: {playing, title, videoId, time, …}}`).
- `buddy music pause`, `buddy music next` (the next search result) and `buddy music status --json`.
  With nothing in the widget, pause, next and status go to Music or Spotify if one is already running.
- `buddy widget list --json` shows the open widgets and what they hold.

Some videos refuse to play outside YouTube (error 150). Then the video opens in the browser widget instead, and
the reply says so: tell the person it is playing in the browser widget.

Play what the person asked for; don't start music on your own. Say what you started in one short line.
