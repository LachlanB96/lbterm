# Thumbnails

A link in a pane to `lbterm://image/<path>` names an image file, and the rows under it can hold the image itself, drawn small into the pane over blank cells. A click on it does what the config's `open-uri` handler says; in Lachlan's config that opens the image full size on another screen. Asked for on 2026-10-24, building on the image viewer agents had used to show him images until then: "These should not auto open, but should show as thumbnails in the conversation. I can then click these thumbnails to open them in the external media." His picks: the name and size on a line with the picture under it (1a), five rows tall, about 100 px at his font (2a), the link naming the file itself, with its box grey and its caption struck through once the file's gone (3b), several images side by side (4a), and a hairline round each (5b).

## What a pane prints

A caption, then one line of links per row of the picture, each line's words blank cells:

```text
[shot 1.png · 2400 × 1600](lbterm://image/C:/Users/me/shot%201.png)
[⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀](lbterm://image/C:/Users/me/shot%201.png?row=0&of=5)
[⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀](lbterm://image/C:/Users/me/shot%201.png?row=1&of=5)
...
[⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀](lbterm://image/C:/Users/me/shot%201.png?row=4&of=5)
```

That's Markdown, as Claude Code draws it (the link's words, its address behind them as OSC 8, with `FORCE_HYPERLINK=1`); any program can print the same with OSC 8 itself. The blank cells are U+2800, which Claude Code keeps as they are where it folds blank lines into one (measured in 2.1.310). Lachlan's agents get these lines from Terminal's `show-image`, which sizes the rows to the image and puts several side by side, each picture under its caption.

- The path is a local drive's, percent-encoded as a link needs (`%20` for a space), with `/` or `\`. Anything else, a network path among them, isn't an image link: the GUI never reaches out to a server to draw a link someone else wrote.
- `row` and `of` say where a line sits in its picture: row `row` of `of` (up to 40). Every line saying so is what lets a picture whose caption has scrolled out of view still show the part that's in it, since nothing tells a terminal where a fullscreen program is scrolled to.
- The picture takes the cells its rows' links cover: from where they start to where the widest ends, from the picture's first row to its last, but only the rows a link was found on, so a row left out takes nothing and covers nothing. Rows of one picture are those of the same file that put its top on the same row and start in the same column; the same image twice is two pictures.

## How it's drawn

**Finding the links** (`lbterm_extras::thumbnails`, tested without a window): as a pane's lines are drawn, each line that has a link has its cells looked at for runs linking to a picture row (`runs_in_row`), noted before the line's cached quads can return early, and after the lines the runs are made pictures (`blocks`). Only a line holding a link drawn by state (`link_looks::by_state`, which the line's shape hash finds once with it) has its cells looked at; every other line, links to files and pages included, costs a flag test (until 2026-10-31 every line with any link had its cells looked through on every frame).

**Decoding** (`wezterm-gui/src/thumbnails.rs`): a thread of its own decodes each picture the first time a pane asks for it, at the size it's drawn at, the picture as big as its box allows with its shape kept and never bigger than it is (`fit`), the first frame of an animation, and the pane draws it pixel for pixel. Nothing is read on the GUI thread. The limits are those of `decode`: a file over 256 MB, a side over 30,000 pixels or a decoding that would allocate over 512 MB is unreadable, so no link can take the memory Terminal's viewer once did (41 GB, 2026-10-21). The thread watches each file it holds through its folder (`lbterm_extras::watch`): a file that's gone is drawn gone, a changed one is decoded again, and one that comes back shows again, as soon as Windows says so (it looked at every file every two seconds until 2026-10-30, so a deleted file showed as gone up to two seconds late). A picture no pane has drawn for five minutes is let go and its folder no longer watched, and with none held the thread sleeps until a pane asks. When one is ready or changes, every window repaints.

**An SVG** (a file ending `.svg`, since 2026-11-01) is drawn rather than decoded (`draw_svg`): as big as its box allows with its shape kept, bigger than the size it says too (`fit_vector`), so a chart is as sharp at any font size, its text in the system's fonts (found the first time an SVG is drawn, on the thread, and kept). It draws only itself: no file or address its `<image>` names is read, an image inside it isn't drawn, and a compressed one (gzip) isn't opened; one over 8 MB is unreadable. Asked for so a dashboard's chart could live in a conversation (below, "A live tile").

**Its place in the atlas.** A picture keeps one place in the window's texture atlas for as long as it's drawn at that size: a new decoding of its file is written over the last one there (`place`), never given a place of its own. Until 2026-11-01 each decoding went through the glyph cache's `cached_image`, which keys a picture by its pixels, so each took a new place, and a file rewritten every few seconds filled the atlas every few minutes, which is made again then, every glyph drawn again in that frame. A place is let go when the atlas is made again, or when no pane has drawn its picture for five minutes.

**Drawing** (`wezterm-gui/src/termwindow/render/thumbnails.rs`), in the pane's own layers, over its text, so it moves and fades with the pane:

| State | What's drawn |
|---|---|
| decoding | nothing yet: the blank cells |
| shown | the image at the top left of its box, two pixels in from the rows above and below, and a hairline round it in the tab rail's grey at rest (`#5E5E6E`), white 50% while the pointer's on one of its links |
| gone, unreadable | its whole box filled white 4%, outlined white 14%; a gone file's caption grey (white 40%, the read InMail link's) and struck through |

A caption's look is chosen where a line is shaped, from what the thread last found, so each time a file goes or comes back every window shapes its lines again once (`lbterm_thumbnails_refresh`, as `lbterm_inbox_links_refresh` does for InMail links).

**A click** on the caption or anywhere on the picture is a click on a link, which LBTerm opens even while the program has the mouse (`lbterm/FEEDBACK.md`, "Click a link"), and the pointer's hand and the link's flash answer it. What it does is the `open-uri` handler's: a picture row's link carries its `?row=&of=` too.

The state file (`lbterm/STATE.md`) lists what each window drew: `thumbnails: [{pane, path, state, rect, hovered, version, places}]`, `version` the decoding drawn and `places` the places in the atlas it has taken.

## A live tile

A file a program rewrites is a picture that changes: the thread decodes it again each time, and each window draws the new one in the old one's place. So a program that keeps writing a chart (an SVG, moved into place whole so no window reads half of one) gets a live chart in a conversation, from lines it printed once. The lines and the box stay as printed; only the picture in it changes. It's as fresh as its writer keeps it, and when the writer stops, the last picture stays.

## Lachlan's config

Terminal's `lbterm.lua` opens a link to an image that's there, a local drive's image file, in Terminal's image viewer (`shared/image-viewer`'s `ImageView.exe`, a window on the right-most screen that opens without taking focus, so typing stays where it was; a click into it gives it the keyboard for its own keys), started from the handler with `wezterm.background_child_process`. A click on one that's gone does nothing, and any other `lbterm://image` link does nothing either.

His live tiles come from his dashboards' server: a dashboard that offers a tile (a temperature graph, the first) has it written as an SVG that the server keeps rewriting whenever the dashboard's page would redraw, and an agent asks the server for one and prints its lines. A click on one opens its dashboard.

## What it can't do

- Show a picture in a terminal other than LBTerm: there it's its caption's words and blank rows, and a click is that terminal's (WezTerm and Windows Terminal know no `lbterm://`).
- Draw into rows that aren't blank: the rows are the program's, and LBTerm only draws over the cells its links cover.
- Save a reply's tokens: every row names the file again, about 400 tokens an image with a long path. That's what lets a picture half scrolled out still show.

## Checking it

- `cargo test -p lbterm-extras thumbnails` (in `lbterm.ps1 check`): the links, local drives only, rows in range, runs and pictures, a picture half out of view, the fit, and decoding with its limits; an SVG drawn at its box's size, its half-covered pixels' colour, its text in the system's fonts, the files it names left unread, and what's refused.
- `pwsh -File lbterm\perf\thumbnails.ps1`: a program with the mouse on, as Claude Code has it, printing a magenta picture and a missing one side by side under two rows of a teal one scrolled half out, and an SVG under them. Both pictures in their colours where the state file says, the teal one's two rows only, the missing one's caption struck, the hairline (`shown.png`); the pointer over it brightening the hairline (`hovered.png`); a click opening its link past the program's mouse; the file deleted, gone and struck (`deleted.png`), written again, back (`back.png`); the SVG at its box's size (`svg.png`), rewritten 30 times in two colours, each drawn, in one place in the atlas (`target\perf\thumbnails`).
- Terminal's check: `show-image`'s lines (`shared/image-viewer/show-image.Tests.ps1`) and the handler (`lbterm/lbterm.Tests.ps1`).
