---
name: tunebox-i18n
description: Add or change UI strings and languages in Tunebox (English-keyed `i18n::t()` table, 6 languages). Use when adding any user-visible text to a view/menu/tray/notification, adding a language, or fixing a missing translation.
---

# UI strings and languages

Source: `crates/ytm-app/src/i18n.rs`. English text **is the key**: `crate::i18n::t("Play")` returns the translation for the
current language, or the English string itself when none exists. Only app chrome is translated; titles/names served by
YouTube are shown as-is.

## Add a string
1. In the view, wrap the literal: `crate::i18n::t("Add to queue")`. It takes `&'static str`, so build dynamic text
   with `format!("{} {}", t("Songs"), n)` rather than translating the formatted result.
2. Add a row to `TABLE` (kept `#[rustfmt::skip]`): `("English", ["Türkçe", "Deutsch", "Español", "Français", "简体中文"])`.
   Order is fixed: tr, de, es, fr, zh (`lang.index() - 1`). The English key must match the literal exactly.
3. `cargo test -p ytm-app i18n` — `table_has_no_duplicates_or_blanks` fails on a duplicate key or empty translation.
   A typo in the key is *not* caught (it silently falls back to English), so grep the literal after adding it.

## Add a language
Add a `Lang` variant, extend `Lang::ALL`, `code()`, `hl()` (YouTube `hl`, e.g. `zh-CN`), `native_name()`, grow every
`TABLE` row's array from `[&str; 5]` to N and the `MAP` type, and extend the `codes_round_trip` test.
`ui_language` (chrome) and `language` (content `hl`) are separate config fields in `ytm-core/config.rs`.
The comment there still lists only "en, tr, de, es or fr"; `zh` is supported.

## Gotchas
* Text drawn with icon families must not go through `t()`; icons come from `theme::icons()` (see `CLAUDE.md`).
* Check layout with a non-English language: German/French strings are longer (player-bar tiers, sidebar rail, buttons).
  Screenshot with `tunebox-ui-capture`. Set the language in the app's Settings, or `ui_language` in `config.toml`.
* Tray, notifications and the macOS menu are also chrome; translate their labels the same way.
* Translations written by the model are not native-reviewed — say so when reporting.
