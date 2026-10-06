# User guide

## Your library (no account needed)

Tunebox has no sign-in; it is meant to be used locally. Likes, playlists and saved items are kept on this device:

* **Liked songs** — the heart in the player bar / now-playing view, or right-click a song. Library → Songs.
* **Playlists** — create from the Playlists page or any song's menu (right-click → Add to playlist), and use the **Edit ▾** button's menu to rename or delete;
  press **Edit** on a playlist to drag songs into a new order (grip on the left, Esc cancels) and remove songs (✕); "Save as playlist" in the queue panel.
* **Saved albums, artists and YouTube playlists** — the Save button on their pages. Library → Albums / Artists.

Everything lives in human-readable JSON in the app's data directory (Linux: `~/.local/share/tunebox/`):
`config.toml` (settings), `library.json` (index, liked and saved items), `playlists/` (one file per playlist)
and `covers/`, written atomically. Settings → Application data shows this folder and can move all of it
(D25); Back up data / Restore are in the same place.

## Keyboard shortcuts

Press **?** anywhere for the cheat-sheet. (`Ctrl` is `⌘` on macOS; shortcuts are ignored while a text field has focus.)

| Keys | Action |
|---|---|
| `Space` | Play / pause |
| `Ctrl`+`→` / `Ctrl`+`←` | Next / previous track |
| `→` / `←` | Seek ±5 s |
| `Ctrl`+`↑` / `Ctrl`+`↓` | Volume ±5 % |
| `M` · `S` · `R` · `L` | Mute · shuffle · repeat · like the current song |
| `Ctrl`+`K` or `/` | Search |
| `Ctrl`+`1…4` · `Ctrl`+`,` | Home, Explore, Library, Playlists · Settings |
| `Alt`+`←` | Back |
| `Q` · `N` | Queue · now-playing view |
| `Esc` | Close the topmost panel (cheat-sheet, queue, now-playing) |

## Tray icon and notifications

* A tray icon (Show Tunebox, play/pause, next, previous, Quit) is on by default; turn it off in Settings → Desktop
  (applies at the next start). Linux uses the StatusNotifierItem protocol, so on GNOME it needs the AppIndicator
  extension (Ubuntu ships it enabled). If no tray host exists the app simply runs without the icon.
* The tray tooltip shows "Title — Artist" and the icon gets a small green pause badge while playing.
  **Show the song next to the tray icon** (off by default) adds the same text as a label beside the icon
  (`XAyatanaLabel`, truncated to 32 characters); GNOME's AppIndicator extension shows it, KDE and others ignore it.
* Windows and macOS: Play / pause, Next and Previous have small menu icons drawn in code; Windows shows the same
  play badge on the icon; macOS uses a monochrome menu-bar template (play or pause cut out of a disc) and shows the
  song as text next to it when the label option is on. Windows has no text next to tray icons. **Compiled for both;
  the macOS app has been run, but its menu-bar icon was not specifically checked, and nothing has run on Windows.**
* **Closing the window keeps Tunebox in the tray** (off by default) only works while the icon is showing. On Wayland the
  window is minimised instead of hidden, because compositors ignore "hide"; so with this option on, Linux runs through
  XWayland (applies at next start) to keep the tray's Show and Quit working.
* **Notify when the song changes** (on by default) sends a desktop notification, but only while the window is not focused.

## Settings

The gear button next to the search box opens Settings: dark/light theme, interface language (also the language
of YouTube's own text), how long before Home/Explore are fetched again (opening a page after
that long refetches it), the application data folder (change it to move everything), backup/restore of all your data (library plus theme, language and refresh interval), and version info.
Everything except the library file itself is stored in `config.toml` in the platform config directory.
Deleting a playlist asks for confirmation.

