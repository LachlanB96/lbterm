# Letting go of a folder

Windows won't rename or remove a folder while any handle inside it is open, whatever that handle's share mode. Every watch LBTerm keeps (`lbterm_extras::watch`: ReadDirectoryChangesW, through the vendored notify) holds a handle on the folder it watches, and another on that folder's parent for its name (deps/notify/LBTERM.md). So while LBTerm ran, a folder with a watched folder in it couldn't be renamed. Lachlan's InMail button watches every project's `InMail` folder, which put a handle inside every project: on 2026-11-03 renaming one of his projects failed twice, and a scan of every process's open handles found the GUI holding the project's `InMail` folder and the project folder itself. Closing the window was the only way through.

Asked for that day, of the two ways out offered (Lachlan's 2a): LBTerm lets go of a folder when a program asks it to, rather than one recursive watch over all the projects, which would hold no handle inside any of them but would wake for everything written under them (8,693 changes in two minutes while a build ran in one, against none at rest now).

## Asking

A program about to rename or remove a folder writes a request into the folder of requests the config names:

```lua
rawset(config, 'lbterm_let_go', { folder = [[C:\Work\State\let-go]] })
```

One JSON file a request, any name ending `.json`, written through a rename (a temporary file renamed into place, so it's never read half written):

```json
{ "folder": "C:\\Work\\Projects\\Resume", "until": 1793612345678, "by": "rename" }
```

- `folder`: the folder to let go of. Every watch whose folder is that folder or in it lets go.
- `until`: unix milliseconds. The request holds while its file is there and until then, so one left behind by a program that died doesn't keep LBTerm blind for long.
- `by`: who asked, for LBTerm's log. Optional.

While a request holds, the program tries its rename again until it goes through (LBTerm writes nothing back), then removes the file. Several requests can hold at once; a folder named by two is let go of once.

## What LBTerm does

A thread of its own, `lbterm-let-go` (`wezterm-gui/src/let_go.rs`), watches the folder of requests and reads it (`lbterm_extras::let_go::asked`) when something in it changes, when the config is reloaded, and a millisecond after the soonest request's `until`. It hands the folders asked for to `lbterm_extras::watch::let_go`, which keeps them for the whole process and tells each watch a change in them can be about (one with a target in or on the way to such a folder), as a change at that folder, the way a folder made or removed is told. That watch's own thread then follows its targets again, as for any folder on the way to them that comes or goes:

- A folder to watch that is, or is in, a folder let go of gives way to the nearest folder above that one that's there, watched alone (not with its folders): it holds no handle inside the folder let go of, and still sees it renamed or removed, as a folder on the way to its targets. Its own name's watch is on the folder above that.
- Once the folder is taken back (its request removed, or its time come), the watch follows again: the folder itself if it's still there, else wherever its targets are now.
- What a watch's thread does on a change it would do anyway: the InMail thread looks in its folders again, the summaries' and the transcripts' threads read again. That read opens a file or folder inside for as long as the read takes, so a rename tried in that moment is refused once and goes through on the next try.

Every watch in the process lets go, not only the InMail button's: the thumbnails' images, the summaries, the transcripts and the tasks card's all go through `lbterm_extras::watch`. The config's own watch of its files (upstream's, `config/src/lib.rs`) doesn't: a folder holding the config can't be renamed while LBTerm runs.

## What it costs

At rest, nothing: the folder of requests is a quiet one of its own, so its watch doesn't wake, and the thread sleeps until a request comes, goes or runs out. A request wakes the thread once, and each watch it's about once; no other watch is told. How long a program waits: in `perf/let-go.ps1`'s test window on 2026-11-03, a project whose InMail was watched could be renamed 48 ms after the request landed. Most of that is two settles of 25 ms each, the requests' thread taking a request's changes together and then the InMail thread taking the change it's told together, as each does for any change. Put the folder of requests in a folder of its own: a folder other programs write in wakes its watch for each of their writes (lbterm/AT-REST.md, "Busy folders").

## Limits

- A request naming a folder the folder of requests is in (the folder above it, say) has the requests' own watch step out too, so it doesn't see the request removed: the request holds until its `until`.
- A folder named only in a later config (a project made after the config was read) isn't watched before then, so there's nothing to let go of; a link to a message for it is drawn as any link (lbterm/INBOX.md, "Links to a message").

## Checking it

- `cargo test -p lbterm-extras let_go watch`: a request read (`folder`, `until`, `by`; anything else refused), the requests that hold at a time and the soonest to run out; the folder a watch gives way to (nested, missing, in any case); a watched project refused a rename by Windows, renamed once let go of, watched again once taken back (a message in it seen); and a watch with nothing in the folder let go of not told at all.
- `pwsh -File lbterm\perf\let-go.ps1`: the same end to end in a test window with an InMail folder and a folder of requests of its own: refused while watched, renamed within a second of a request landing (how long it took is said), watched again once the request is removed or runs out.
