# Background music

Optional music for the web pages (operator page, public pool page, miner page). Put audio files into this folder
(mp3, ogg, opus, m4a, wav, flac); the pages then show a player in the title bar (play, pause, stop, previous, next,
volume). The playlist loops. Without files here the pages show no player.

If the music was playing when a visitor left, it resumes at the first click or key press on their next visit, on the same track and position (browsers only allow sound after such an interaction); pause and stop switch this off.

Files are streamed by the browser, so they do not use memory on the server or in the page.

## Playlist and credits

By default the files are listed in alphabetical order with their file name as title. To set titles and the credit that a license
asks for, copy `playlist.example.json` to `playlist.json` and edit it. The credit is shown in a tooltip that opens when the
pointer rests on the title (or when the title is focused or tapped):

| Field | Meaning |
|---|---|
| `file` | file name in this folder (required, no sub-folders) |
| `title`, `artist` | shown in the player |
| `credit` | credit line the license asks for, for example `Music: 'Space Ranger' by Moire on #Uppbeat` |
| `url` | link to the artist page (https only) |
| `license` | license code of your download, if the site gives one |

The files must be readable by the pool (in Docker it runs as user 10001): `chmod 644 music/*` after copying them.

## Licensing

Check the license of every track before you publish a pool page with it. Free tiers usually require a credit, and many
(for example Uppbeat) license a track for your own use, not for passing the file on. For that reason `.gitignore` keeps new audio
files out of git by default: copy them to the server yourself (`scp`, or the Docker bind mount `./music`). If the repository is public, anything committed here is public too.
Only commit a track that is released under a license that allows redistribution, for example CC0, and remove its line from `.gitignore` for that file.
