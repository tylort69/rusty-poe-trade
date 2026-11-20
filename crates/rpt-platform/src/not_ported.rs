//! Upstream main-process modules that are intentionally **not** ported.
//!
//! * `vision/*` (`HeistGemFinder.ts`, `link-main.ts`, `link-worker.ts`,
//!   `wasm-bindings.ts`, `utils.ts`): screenshot of the game window + OpenCV /
//!   Tesseract WASM OCR to read Heist gem names (`ocr-text` shortcut, Windows
//!   only upstream). The `ocr-text` action is still parsed; pressing it logs
//!   a warning. TODO: implement with a native OCR crate (e.g. `ocrs`) plus a
//!   window capture (`GetWindowDC`/`PrintWindow` on Windows, `XGetImage`
//!   on X11) and emit a new `PlatformEvent::OcrText { target, press_time,
//!   ocr_time, paragraphs }`.
//! * `server.ts`: HTTP + WebSocket server that served the Vue UI to the
//!   Electron window and to external browsers ("Open in Browser" tray item,
//!   `/config`, `/proxy/*`, `/uploads/*`, `/events`). The egui app talks to
//!   this crate directly through [`crate::Platform`] and the
//!   [`crate::PlatformEvent`] channel, so there is no IPC transport. Config
//!   persistence and uploads moved to [`crate::host_files::ConfigStore`];
//!   the trade-site proxy (`proxy.ts`) belongs to the trade crate.
//!   TODO (optional): a browser-client mode would need an equivalent server.
//! * `RemoteLogger.ts`: replaced by `tracing` + [`crate::PlatformEvent::LogEntry`].
//! * `electron-updater` download/install (`update-and-restart`): replaced by
//!   a release check only (see [`crate::updater`]).
