# How downloading works

ENSave calls other tools to do the transfers. It runs them, watches them and keeps the queue in order.

- **aria2** fetches plain files over HTTP(S), FTP and BitTorrent.
- **yt-dlp** works out how to download from media sites.
- **FFmpeg** joins video and audio into one file.
- **gallery-dl** handles image galleries.

This page explains what these tools do on the network and on disk. It also explains how DRM works and why ENSave stops when it finds it.

```text
            ┌──────────── ENSave ────────────┐
 link ───▶  │ link parser ─▶ queue (SQLite) ─▶ scheduler │ ──▶ UI (progress 4×/s)
            └──────────────────────┬──────────────┘
                                   │ one Engine trait: start / pause / resume / cancel / events
               ┌───────────────────┼────────────────────┐
               ▼                   ▼                    ▼
        aria2c (JSON-RPC)   yt-dlp (+ FFmpeg)       gallery-dl
        plain files         media sites            image galleries
```

---

## 1. HTTP range requests and downloading over several connections

An HTTP server can send part of a file. The client asks for a byte range:

```http
GET /ubuntu.iso HTTP/1.1
Range: bytes=1048576-2097151
```

The server replies `206 Partial Content` with `Content-Range: bytes 1048576-2097151/6114656256`. A server that supports this announces it with `Accept-Ranges: bytes`. A server that doesn't support it ignores the header and replies `200` with the whole file.

Two features rely on ranges.

**Resume.** If a download stops at byte N, the next request asks for `bytes=N-`, so nothing is fetched twice.

**Several connections.** The file is cut into segments and each connection downloads one segment at the same time:

```text
 file (600 MB)  ├──────────┼──────────┼──────────┼──────────┤
 connection 1   ████████░░  bytes 0–150M
 connection 2              ███████░░░  bytes 150M–300M
 connection 3                         ██████████  done → takes over part of a slower segment
 connection 4                                    █████░░░░░  bytes 450M–600M
```

**Why this is faster.** Many servers and CDNs limit the speed of each connection, not of each client. TCP also needs time to ramp up on a long or slightly lossy link. Four connections at 5 MB/s each finish sooner than one at 5 MB/s.

The test suite measures this against a local server that caps each connection at 2 MB/s. One connection took 8.2 s; eight connections took 1.4 s (11.4 MB/s).

There are limits. A server whose total bandwidth is already used up gains nothing. Some servers reject or throttle clients that open many connections, so the default is 8 and the maximum is 16. Settings → Downloads → *Connections per download* changes it.

**How aria2 keeps track of this.** aria2 writes `file.iso.aria2` next to the partial file. This control file records which pieces are finished. ENSave asks aria2 to save it every second.

After a crash, a power cut or `kill -9`, ENSave puts unfinished tasks back in the queue. aria2 reads the control file and requests only the missing ranges. The control file is deleted when the file is complete.

```text
 crash ─▶ restart ─▶ tasks "active" in SQLite → back to "queued"
                  ─▶ scheduler starts them  ─▶ aria2 finds big1.iso.aria2 ─▶ Range: bytes=<missing>…
```

aria2 is started with `--stop-with-process=<our PID>`, so it shuts down cleanly as soon as the app's process is gone. A crash never leaves `aria2c` downloading in the background.

---

## 2. HLS and DASH: video delivered as segments

Streaming sites rarely serve one big MP4. They cut the video into short segments, usually 2 to 10 seconds each. A playlist (the *manifest*) lists the segments.

**HLS** (`.m3u8`, from Apple) uses a two-level text playlist:

```text
master.m3u8                         1080p.m3u8 (media playlist)
#EXT-X-STREAM-INF:BANDWIDTH=5000000, #EXTINF:6.0,
  RESOLUTION=1920x1080              seg_00001.ts      ← 6 s of video
1080p.m3u8                          #EXTINF:6.0,
#EXT-X-STREAM-INF:BANDWIDTH=1500000, seg_00002.ts
  RESOLUTION=854x480                …
480p.m3u8                           #EXT-X-ENDLIST
```

**DASH** (`.mpd`, an ISO standard) describes the same thing in XML. It has *Periods*, *AdaptationSets* (video, audio and each subtitle language) and *Representations* (one per quality). Segment addresses come from templates such as `video_$Number$.m4s` or from byte ranges inside one file.

Downloading either format works like this:

1. Fetch the manifest.
2. Pick one *variant* (for example 1080p video plus the best audio).
3. Download every segment of that variant, several at a time.
4. Join the segments in order. For HLS that is MPEG-TS or fMP4; for DASH it is fMP4.
5. Remux the joined segments into one MP4 or MKV (see §4).

```text
 manifest ─▶ choose variant ─▶ seg1 seg2 seg3 … segN  (parallel fetch)
                                  └──── concatenate ────┘ ─▶ FFmpeg remux ─▶ movie.mp4
```

Live streams have no `#EXT-X-ENDLIST`. The playlist keeps growing and is fetched again every few seconds.

**HLS with `METHOD=AES-128`.** The segments are encrypted with AES, and the playlist gives an ordinary HTTPS URL for the key. Any client that can see the playlist can fetch that key, so this is encryption in transit, not DRM. FFmpeg and yt-dlp handle it as normal HLS. DRM is different (§5): the key is never handed to the client.

---

## 3. How yt-dlp extracts formats

A link to a video page is not a link to a video file. yt-dlp has one *extractor* per site, and over 1,800 sites are covered. An extractor knows how that site's player gets its media:

```text
 page URL ──▶ extractor (matched by URL pattern)
               ├─ fetch the page / the site's API (with cookies if given)
               ├─ read the player data: title, duration, thumbnails, subtitles…
               └─ list formats: every stream the player could pick
                     id   ext   resolution  vcodec       acodec    note
                     137  mp4   1920x1080   avc1         video only
                     248  webm  1920x1080   vp9          video only
                     140  m4a   audio only               mp4a 128k
                     251  webm  audio only               opus 160k
                     18   mp4   640x360     avc1         mp4a      (combined)
```

ENSave runs `yt-dlp --dump-single-json <url>` as a child process. The arguments are passed as an array, never through a shell. ENSave reads that JSON to show the title, the qualities and the subtitles before you start.

When you pick "1080p", ENSave builds a *format selector* such as `bestvideo[height<=1080]+bestaudio/best[height<=1080]`. That means the best video stream up to 1080p plus the best audio stream. If the site has no separate streams, it takes the best combined stream up to 1080p.

Large sites usually serve the highest qualities as separate video and audio streams, often over DASH or HLS (§2). That is why the next step is needed.

Sites change their players often, and an outdated yt-dlp is the most common cause of "unsupported URL" or "no formats" errors. ENSave therefore downloads yt-dlp on first run, checks its version, and offers to update it from Settings → Tools. A failure caused by an outdated version is reported as *ToolOutdated*, not as a generic error.

---

## 4. Muxing with FFmpeg

A video file is a *container* (MP4, MKV, WebM) holding *streams*: video encoded with a *codec* (H.264, VP9, AV1), audio (AAC, Opus) and subtitles.

**Muxing** puts streams into a container. **Demuxing** takes them out. Neither changes the encoded data. **Transcoding** decodes and re-encodes the data, which is slow and loses quality.

```text
  video.f137.mp4 (H.264, no audio) ─┐
                                    ├─▶ ffmpeg -i v -i a -c copy -map 0:v -map 1:a ─▶ Movie.mp4
  audio.f140.m4a (AAC)             ─┘        (stream copy: seconds, no quality loss)
```

ENSave always uses `-c copy` when the codecs fit the target container:

- H.264 and AAC go into MP4.
- VP9 and Opus go into WebM or MKV.
- MKV accepts nearly anything, so it is the fallback when MP4 can't hold the streams.

Re-encoding happens only when you ask for it, for example "audio only, MP3". It shows its own progress, because it takes far longer than copying.

FFmpeg also embeds subtitles, thumbnails and metadata (title, artist, chapters), all by stream copy.

---

## 5. DRM, explained conceptually, and what ENSave does about it

DRM (*Digital Rights Management*) protects paid streaming services. Netflix, Disney+, Prime Video and most music services use it. In browsers it works through these parts:

- **EME** (Encrypted Media Extensions) is the browser API that connects a web page's player to a *Content Decryption Module* (CDM).
- **The CDM** is a closed component inside the browser or the operating system:
  - **Widevine** (Google) is used in Chrome, Firefox and Android.
  - **PlayReady** (Microsoft) is used in Edge and Windows.
  - **FairPlay** (Apple) is used in Safari and on iOS.
- **The license server** belongs to the service. It checks the user's account and the device. It then sends the content keys *to the CDM*, encrypted so that only that CDM can read them.

```text
 page player ──EME──▶ CDM ──license request (signed by the CDM)──▶ license server
      ▲                │ ◀────── content keys, encrypted for this CDM ───────┘
      │                ▼
   decoded frames ◀── decrypt (+ often decode) inside the CDM / secure hardware
```

Neither the page nor the user ever sees the keys. On many devices the decrypted video doesn't reach normal memory either: it goes straight from secure hardware to the screen. The media segments are ordinary encrypted DASH or HLS. Downloading them yields only ciphertext that no player can use without a license.

**ENSave's rule: DRM-protected content is reported as unsupported.** The app contains no code to get keys, imitate a CDM or capture decrypted output, and it never will.

### How ENSave recognises DRM

The app checks the manifest and the metadata before downloading anything:

| Where | What indicates DRM |
|---|---|
| yt-dlp metadata | `"has_drm": true` on formats, or a "DRM protected" extractor error |
| DASH manifest | `<ContentProtection schemeIdUri="urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed">` (Widevine), `…9a04f079-9840-4286-ab92-e65be0885f95` (PlayReady), `…94ce86fb-07ff-4f43-adb8-93d2fa968ca2` (FairPlay) |
| HLS playlist | `#EXT-X-KEY` / `#EXT-X-SESSION-KEY` with `METHOD=SAMPLE-AES` or `SAMPLE-AES-CTR` together with a `KEYFORMAT` such as `com.widevine`, `com.microsoft.playready` or `com.apple.streamingkeydelivery` |
| MP4 / fMP4 init segment | a `pssh` box, or `encv`/`enca` sample entries with a `tenc` box |

When any of these appear, the task fails with the error kind **`drm`** and this message: *"This content is DRM-protected and cannot be downloaded."* No segments are downloaded, the task isn't retried, and a log line records which signal was found. Plain `METHOD=AES-128` HLS (see §2) is not DRM and downloads normally.

---

## 6. Where to read more

- RFC 9110 §14: HTTP range requests
- RFC 8216: HTTP Live Streaming
- ISO/IEC 23009-1: MPEG-DASH
- W3C Encrypted Media Extensions: <https://www.w3.org/TR/encrypted-media/>
- aria2 manual: <https://aria2.github.io/manual/en/html/>
- yt-dlp README (format selection): <https://github.com/yt-dlp/yt-dlp#format-selection>
- FFmpeg documentation (stream copy, `-map`): <https://ffmpeg.org/ffmpeg.html>
