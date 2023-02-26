# mpd-stream-proxy
A (insecure?) proxy for MPD to listen (youtube) streams with artwork. It depends on [yt-dlp](https://github.com/yt-dlp/yt-dlp).

This is in a very Beta stage, and i have no interest in continuing it.
Fell free to send Issues, PRs or fork the project, if you want to support it.

## Quickstart
1. Install rust and [yt-dlp](https://github.com/yt-dlp/yt-dlp).
2. Run the proxy. Example with `RUST_LOG=DEBUG cargo run`. Use `INFO` for a more quiet output.
3. Add a entry in the MPD queue.
```
# Assuming the MPD server is running in the same machine as the proxy
# Also, the trailing '/' is very important, don't forget it
mpc insert 'http://localhost:4000/https://www.youtube.com/watch?v=dQw4w9WgXcQ/'
```
4. Profit.

## How it works

When `MPD` tries to access the HTTP path `/<link>/`, 
the program will ask `yt-dlp` for the video information, mainly it's stream and thumbnail links.
It will store this information in a cache and open a connection to the stream, and pass the connection to MPD.
Then MPD can listen to it and gather any other information, like metadata, if possible.

When `MPD` tries to access the HTTP path `/<link>/cover.jpg` (or other extensions), then the proxy will
open a connection to the thumbnail link, and pass that connection to MPD.
So a MPD client can gather the image and display it.

The trailing `/` is very important, because without it, MPD will ask `/cover.jpg` instead of `/<link>/cover.jpg`.
It's still safe because the program can recognize this and ignore the request, issuing a warning.


## TODOs and Problems

* The image will only be gathered if the MPD client calls `albumart`. `readpicture` could possibly work
but it's harder to set up, as we need to edit the song binary stream.
For more information read [the MPD protocol](https://mpd.readthedocs.io/en/latest/protocol.html#the-music-database).

* We can possibly edit the song binary stream and append ID3 tags to support more extensible music metadata,
so the clients can show the music title and author instead of a long crypt URL.
