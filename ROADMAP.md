# SnipForge roadmap (saved)

## Deferred (not forgotten)
- Scrolling capture
- Pin-to-screen (floating always-on-top screenshots)
- History browser (thumbnails, search, delete)
- System tray icon
- Auto-captions (speech-to-text from the microphone track)
- Video editor (trim / cut / speed / captions / export)
- Further UI polish

(The Python edition in `snipforge.zip` already has working versions of all of these, so they
are a port job, not new design.)

## Done in this version (v0.2)
- Region capture starts instantly: the app hides itself and the selection overlay appears right away
- Ellipse tool draws a real ellipse (preview and result)
- Crop tool in the editor (apply with Enter, undoable)
- New look: dark theme, painted icons, colour swatches, action cards
- Settings are remembered between runs
- Closing the window minimises it so the hotkeys keep working
- Recording engine (ffmpeg) is downloaded automatically - no winget needed
- Text recognition uses Windows' built-in OCR - nothing to install
