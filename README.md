# SnipForge (Rust edition) v0.5

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

## Use
| Action | Hotkey |
|---|---|
| Region capture -> editor | Ctrl+Alt+A |
| Full-screen capture -> editor | Ctrl+Alt+F |
| Grab text (OCR) -> clipboard | Ctrl+Alt+T |
| Record region (press again to stop) | Ctrl+Alt+R |

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

## Recording
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
