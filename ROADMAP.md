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

## Done in v0.7
- Scrolling capture, pin to screen, searchable history (with text inside screenshots), video editor (cut, speed, captions)

## Waiting for approval
- Automatic caption generator (speech to text). Optional download; size notice sent in chat first.

## Done in v0.6
- Mic and Audio buttons always on the recording bar; either can be switched on mid-recording
- Countdown before recording starts (configurable)
- One-click whole-screen recording (Ctrl+Alt+V)

## Done in v0.5
- Recording fixed: Stop now always finishes and saves (it used to hang when the microphone was on)
- Pause / resume (button + Ctrl+Alt+P), recorded time excludes pauses
- Separate microphone and device-audio options, with live mute toggles on the recording bar

## Done in v0.4.1
- Visible countdown when a capture delay is set (cancellable)

## Done in v0.4
- Choose where screenshots, recordings, extracted text and the recording engine are stored
- Optional .txt file for every text extraction
- One screenshot of all screens together; selection overlay across every monitor (A = all, 1-5 = one screen)
- Record any region, one screen, or several screens together

## Done in v0.3
- Capture overlay covers the entire screen; big screenshots open maximised
- Extract text from the image inside the editor (whole image or dragged region)
- Recording engine installer rewritten (built-in downloader, progress bar, clear errors, manual fallback)
- Light theme with a consistent palette

## Done in v0.2
- Region capture starts instantly: the app hides itself and the selection overlay appears right away
- Ellipse tool draws a real ellipse (preview and result)
- Crop tool in the editor (apply with Enter, undoable)
- New look: dark theme, painted icons, colour swatches, action cards
- Settings are remembered between runs
- Closing the window minimises it so the hotkeys keep working
- Recording engine (ffmpeg) is downloaded automatically - no winget needed
- Text recognition uses Windows' built-in OCR - nothing to install
