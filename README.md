# SnipForge (Rust edition) v0.8

Native Windows screenshot / annotate / OCR / screen-recording tool (Rust + egui, one `.exe`).

> Written without access to a Rust compiler. Run `cargo check` first; if it prints errors,
> paste them back and they get fixed fast.

## Build & run
1. Install Rust from https://rustup.rs (accept the "Desktop development with C++" build tools prompt).
2. In this folder: `cargo run --release`  ->  `target\release\snipforge.exe`
3. That's all. On first start SnipForge downloads the recording engine (ffmpeg, ~90 MB) by itself, with a
   progress bar. If your network blocks the download, use **Choose ffmpeg.exe...** in the Components box
   (or on the recording setup screen) to point it at an ffmpeg.exe you downloaded yourself.
   Text recognition uses the OCR built into Windows 10/11, so there is nothing to install.

## The window
Four tabs: **Capture**, **Record**, **Tools**, **Settings**.

- **What to capture** is chosen the same way for screenshots, scrolling screenshots and recording: a small map shows
  your real monitors. Click one to choose just that screen, or pick *Select an area* / *Full screen* (*All screens* when
  you have several monitors) from the chips under the map. No selection step is needed for a whole screen.
- **Record** also has the sound options with *Test* buttons (see "Sound" below).
- **Tools**: History, video editor, picture editor, pin the clipboard picture, open the folders.
- **Settings**: screens, clipboard, save folders, the recording engine, and the list of keyboard shortcuts.

## Use
| Action | Hotkey |
|---|---|
| Region capture -> editor | Ctrl+Alt+A |
| Full-screen capture -> editor | Ctrl+Alt+F |
| Grab text (OCR) -> clipboard | Ctrl+Alt+T |
| Record a region (press again to stop) | Ctrl+Alt+R |
| Record the whole screen (press again to stop) | Ctrl+Alt+V |
| Pause / resume a recording | Ctrl+Alt+P |
| Scrolling capture (press again to stop early) | Ctrl+Alt+S |
| Pin a region on the screen | Ctrl+Alt+N |
| History (search all captures) | Ctrl+Alt+H |

Closing the main window only minimises it, so the hotkeys keep working. Use **Quit SnipForge** to exit.
Default folders: screenshots `Pictures\SnipForge`, recordings `Videos\SnipForge`, text `Documents\SnipForge Text`.
Settings are in `%APPDATA%\SnipForge\settings.txt`.

## How a capture works now
Every capture runs as its own small process: it hides the other SnipForge windows, grabs the screen,
then shows the selection overlay immediately - nothing needs to be clicked twice.

## Editor
Arrow, rectangle, ellipse, pen, marker, text, numbered steps, blur, **crop**, and **text extraction**:
use the *Extract text* button (whole image) or the *Read text* tool (drag over just the part you want).
The text appears in a window and is copied to the clipboard.
Large screenshots open in a maximised editor window; the capture overlay is forced to cover the whole screen.
Undo/redo (Ctrl+Z / Ctrl+Y), copy (Ctrl+C), save (Ctrl+S), save as. Crop: drag, then Enter to apply.

## Recording bar
While recording, the floating bar has **Pause**, **Screenshot** (saves a picture of the recorded area without
stopping; also Ctrl+Alt+K), **Stop**, and **Mic** / **Audio** switches with live level meters.

## Sound: permission and testing
- Windows does not let an app give itself microphone permission, so SnipForge checks the Windows privacy switch
  before recording. If the microphone is blocked it tells you, opens the right Windows settings page, and lets you
  try again, record without the microphone, or cancel.
- *Device audio* (what you hear) needs no permission.
- On the Record tab, **Test microphone** and **Test device audio** listen for 3 seconds and report what they heard, so
  you can see which one is silent. On the bar, a green meter moves when sound is arriving; a source that could not
  start shows *n/a* with the reason on hover (and **Fix mic** opens the Windows settings).

## Scrolling capture
Drag over a scrollable area (a web page, a document...). SnipForge scrolls it for you, grabs frames and stitches them
into one tall image, then opens it in the editor. It stops at the end of the page, or press **Stop and save** /
Ctrl+Alt+S. Keep the pointer over the page; move it away to reach the Stop button. Sticky headers are ignored.
The scroll distance is calibrated automatically, each step waits for the page to stop moving, and the page is
brought to the front first. If it stops early you get a note saying why.

## Pin to screen
Pin a region (Ctrl+Alt+N), the editor image (*Pin to screen* button), a history item, or the clipboard image.
The pin floats above all windows: drag to move, mouse wheel to resize, double-click or Esc to close, right-click
for Copy / Save / Reset size. Pins stay put while you capture other things.

## History
A searchable gallery of your screenshots, recordings and extracted text files (from your Save locations).
Search by name or date. Press **Index text in screenshots** once and you can also search for words *inside* the
screenshots (this reads each image with Windows OCR in the background and remembers the result).
Double-click to open: images in the editor, videos in the video editor.

## Video editor
Open from the recording-finished screen, from History, or *Edit a video*.
- Drag on the timeline to select a range, then **Delete selection** (cut the middle out) or **Keep only selection**.
- **Speed** 0.5x to 2x. **Captions**: add by hand, import / export `.srt`, burned into the picture on export.
- Export MP4, WebM, GIF or MP3 (audio only); sizes 1080p/720p/480p; three quality levels.
- The preview is silent (a low-resolution look at the picture); the exported video keeps its sound.
- Captions you type are saved next to the video as `name.srt` and reload next time.

## Recording
- **Whole-screen recording:** the *Record full screen* card (or **Ctrl+Alt+V**) records everything without asking you
  to select anything. It follows the *Capture area* setting: all screens together, or the primary screen only.
- **Countdown:** after you choose what to record, a 3-2-1 card counts down before recording starts
  (Settings -> *Countdown before recording*, 0 turns it off). Esc / Cancel aborts.
- **The recording bar** always shows **Mic** and **Audio** (device audio) buttons. They can be switched on or off at
  any moment, even if the option was off when you started. Time before you switch a source on is silent.
  A button shows *n/a* if that source could not be started (no device / access denied).
- **Start / stop:** Ctrl+Alt+R (or the Record card). Stop with the **Stop** button on the floating bar, or Ctrl+Alt+R again.
- **Pause / resume:** the **Pause** button, or **Ctrl+Alt+P**. Paused time is cut out of the video and the sound.
- **Microphone** and **device audio** (everything you hear through speakers / headphones) are separate options in
  Settings. During a recording the bar has **Mic** and **Audio** toggles to mute / unmute either one on the fly.
  Both can be on at the same time; they are mixed into one track.
- Audio is captured by SnipForge itself (not ffmpeg), and the finished file is assembled when you press Stop.
  If sound can't be added, you still get the video plus a warning message.
- Uses the default microphone and the default playback device.

## Delay timer
Set *Delay before capture* in Settings. A small countdown window (3 - 2 - 1, with a ring) appears at the top of
the screen while you set things up. It closes before the screenshot is taken, so it is never in the picture.
Press **Esc** or click **Cancel** in it to abort.

## Several monitors
- **Capture area** (Settings): *All screens together* (default) puts every monitor into ONE screenshot and one
  selection overlay; *Primary screen only* restores the old behaviour.
- In the selection overlay: drag any rectangle (it may cross monitors), press **A** to take all screens, or
  **1-5** to take just that screen (numbered badges are shown when you have more than one monitor).
- Recording works the same way: record a small part of a screen, one whole screen, or all screens together.
  Regions on the primary screen use fast DXGI capture; regions on other screens or spanning screens use GDI,
  which is heavier, so use a lower FPS for very large multi-screen areas.

## Very large multi-screen areas
- Screenshots wider than 8192 px are shown scaled-down on screen (a GPU limit) but saved at full resolution.
- Recordings larger than 3840 x 2160 (for example two 4K monitors) are scaled down to fit that size.

## Save locations
Settings -> *Save locations* lets you change the folder for screenshots, screen recordings, extracted text
(optionally saved as .txt files) and the recording engine (ffmpeg). *Use default* resets a folder.
Changing the engine folder installs ffmpeg there if it is missing.

## Limits
- The mouse cursor is not drawn into screenshots or recordings.
- Mixed-DPI multi-monitor setups are supported but are the most likely place for small offsets; report any.
- Recording is video + microphone (first audio device); no system audio yet.
- See ROADMAP.md for what is planned next.

## Colour palette (light theme)
Defined in one place: `src/ui.rs`.

| Role | Colour |
|---|---|
| Window background | #F5F6FA |
| Panels / cards | #FFFFFF with #DCE1EA border |
| Canvas behind screenshot | #E4E7EE |
| Accent (buttons, selection) | #2563EB, soft tint #E1EBFF |
| Text / dim text | #1C2029 / #687080 |
| Success / danger | #12804A / #DC3545 |
