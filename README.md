# Clipboard Manager

[Русская версия](README.ru.md)

A lightweight Windows 11 application built with Rust and Win32. Runs in the system tray and stores clipboard history locally in SQLite. The compiled executable requires no Rust, .NET, or WebView installation.

## Usage

- Left-click the tray icon to open history.
- Start typing to search clipboard contents and file names, ignoring case.
- Use Up/Down to select an entry.
- Press Enter, double-click, or click **Copy** to restore an entry to the clipboard, hide history, and paste into the application from which history was opened.
- Press Escape or close the window to hide history and keep recording.
- Right-click the tray icon to access history, startup with Windows, and exit.
- Use **Newer** and **Older** to navigate pages of 100 entries.

The content list is on the left, without timestamps or entry sizes; the text or image preview is on the right. Search covers the entire database, not just the current page.

Entries copied from history move to the top and are marked with a yellow dot on the left. The order and marks are saved between launches. Copying does not reset an entry's retention age.

Automatic paste returns focus to the previous application and sends `Ctrl+V` after you release the selection key and shortcut modifiers. If the target window is closed or focus moves elsewhere, paste is cancelled; the entry remains on the clipboard. Windows may block automatic paste into applications running as administrator.

Images in the list have thumbnails up to 64 × 48 pixels. They are generated in the background and stored in the `thumbnails` table of the same SQLite database; original data remains unchanged. For older entries, thumbnails are generated when their page is opened. Only the current page is held in memory; thumbnails are released when the window is hidden. Text rows stay compact, while image rows are taller.

## Language and keyboard shortcut

Set a retention period in days in **Settings** to automatically delete older entries. `0` (the default) keeps history indefinitely. Cleanup runs at startup, when opening history, when saving settings, and every hour while the application is running. Entry age is measured from the time it was copied. Associated formats, thumbnails, and search data are also removed; freed space within SQLite is reused for new entries.

To delete an individual entry, select it and click **Delete** at the bottom of the window, or press the Delete key while the list has focus. Deleting an entry does not change the current clipboard.

The default interface language is English, and the default global shortcut is `Ctrl+Alt+V`. The shortcut opens a hidden history window and hides an open one when pressed again. Open **Settings** using the button in the history window or the tray menu. Choose **English** or **Русский**; the language changes immediately after saving, without a restart.

To change the shortcut, press the desired combination in the **Open clipboard history** field. You can also include the Windows key. **Clear** disables the shortcut. **Save** applies your choices; **Cancel** and Escape discard changes. Combinations used by other applications or reserved by Windows cannot be saved; a message asks you to choose another combination.

While editing settings, the current shortcut is temporarily unregistered so the field can capture that same combination. It is registered again after saving or cancelling. Settings are stored in `%LOCALAPPDATA%\ClipboardManager\settings.ini` and restored on the next launch. Copied text is never translated; only the interface and metadata labels are localized.

History: `%LOCALAPPDATA%\ClipboardManager\history.sqlite`. Errors: `errors.log` in the same folder. Exiting the application does not delete history. SQLite `-wal` and `-shm` files, when present, belong to the same database; do not delete them while the application is running.

## Data and memory

The application saves Unicode/ANSI text, HTML, RTF, DIB/DIBV5 images, file lists from File Explorer, metafiles, palettes, and portable registered/private binary formats. Restoring an entry restores its formats, not just plain text. File lists store paths, as the standard Windows clipboard does; the original files themselves are not copied into an archive.

The window loads only the current page and the selected entry's preview. Each SQLite connection has a 2 MB cache limit; the background thumbnail worker uses a separate connection and processes one original image at a time. Images and original data from the entire history are not loaded into RAM. Closing the window releases the preview and current-page thumbnails. Copying a large object temporarily requires memory for the object and its formats.

Search queries of three or more characters use an FTS5 trigram index; one- or two-character queries search the entries' text index directly. Search supports Cyrillic and literal `%`, `_`, and `\` characters.

Non-portable formats containing references to internal objects in another process (`DataObject`, `Ole Private Data`, owner-display) are marked as unsaved in the preview. Virtual attachments from email clients are outside the supported scope.

## Build and installation

```powershell
cargo build --release
```

Executable: `target\release\clipboard-manager.exe`.

`install.ps1` builds the application, copies the executable to the project folder, enables startup for the current user, and launches the application. Administrator privileges are not required. Startup can be disabled from the tray menu.

To use a separate test database:

```powershell
.\clipboard-manager.exe --data-dir=C:\path\to\test-history
```

Only one instance runs in the current user session. Launching it again opens the existing instance's history.

## Verification

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
cargo test windows_clipboard_tray_search_restore_and_restart -- --ignored --nocapture --test-threads=1
```

The last test uses the real Windows clipboard, saves its contents to a separate temporary database, and restores them afterward. It checks notifications, the tray, filter input, copying from history, exact restoration of text/HTML/RTF/images/files/binary formats, retries when the clipboard is busy, and disk persistence.

External verification of the release process with 10,000 entries and a 64 MB object:

```powershell
cargo test create_release_runtime_fixture -- --ignored --nocapture
.\verify-runtime.ps1
```

Report: `artifacts\runtime-report.json`. This check launches and stops two instances sequentially, verifies that a saved filter works after restarting, and measures background memory and CPU usage.

Measurements on the development computer: executable size of 1,304,576 bytes; with a test history of 10,000 entries and a 64 MB object, background working set of approximately 19 MB and private memory of approximately 2.9 MB. The process used no CPU during a two-second idle measurement. Indexed search over 10,000 entries took approximately 1.6 ms. These are measured results, not fixed limits for all clipboard contents.
