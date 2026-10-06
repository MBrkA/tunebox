---
name: ytm-api-fixtures
description: Record fresh InnerTube responses as test fixtures and repair ytm-api parsers when YouTube changes its JSON. Use when search/browse/lyrics results are empty or wrong, a parser test fails after refreshing fixtures, or adding a new endpoint parser.
---

# Refreshing fixtures and fixing parsers

1. **Record** (WEB_REMIX needs no key; any visitor works):
   ```sh
   cd crates/ytm-api/tests/fixtures
   q(){ curl -s "https://music.youtube.com/youtubei/v1/$1?prettyPrint=false" \
     -H 'User-Agent: Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0' \
     -H 'Content-Type: application/json' -H 'Origin: https://music.youtube.com' \
     -d "{\"context\":{\"client\":{\"clientName\":\"WEB_REMIX\",\"clientVersion\":\"1.20260101.01.00\",\"hl\":\"en\",\"gl\":\"US\"}},$2}" -o $3; }
   q search '"query":"daft punk"' search_all.json           # + "params" from SearchFilter for filtered
   q browse '"browseId":"UCRr1xG_2WIDs18a6cIiCxeA"' artist.json
   q next '"videoId":"khnokW3Mw24","playlistId":"RDAMVMkhnokW3Mw24","isAudioOnly":true' next.json
   ```
   Files in use: search_all/songs, suggestions, home, explore, moods (FEmusic_moods_and_genres), mood_category
   (FEmusic_moods_and_genres_category + `params`; trimmed to 3 shelves — the real response is ~2.8 MB), artist, album, playlist, next, lyrics.
   Content is region/time dependent (home/explore): tests must assert shape, not exact titles.
2. **Inspect** with python: print `list(d.keys())`, then walk to the renderer that holds items
   (`musicResponsiveListItemRenderer`, `musicTwoRowItemRenderer`, `musicCardShelfRenderer`, `gridRenderer`…).
3. **Fix in the right layer:** row/card classification is `parse.rs` (`target_of`, `parse_subtitle`, `assemble`);
   page structure is `pages.rs`. Prefer finding rows by key anywhere in the tree over hard-coding paths.
4. `cargo test -p ytm-api` → then `cargo test -p ytm-api --features live-tests` (checks the real service and
   the client/query plumbing, e.g. continuation tokens).
5. Facts learned: anonymous Home has no continuation content; lyrics need a second request
   (`next` → tab `MPLY…` → `browse`); album rows lack artists/thumbnails (inherited from the header).
