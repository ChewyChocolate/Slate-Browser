# Task 4: H.264 / AAC Codec Licensing, Arch Linux Integration, and Software Fallbacks

## 1. Problem Statement & Legal Context
- **H.264 (AVC)** and **AAC** are covered by patent pools (MPEG LA and Via Licensing Alliance). Distributing proprietary codecs compiled directly into binaries can incur royalty obligations.
- **Royalty-Free Open Standards**: Modern open web media standards—**VP8, VP9, AV1, Opus, and Vorbis**—are 100% royalty-free, open source, and natively enabled in standard Chromium/CEF builds without any proprietary licensing entanglements.
- Major platforms (including YouTube) default to **VP9** and **AV1** for modern video delivery, paired with **Opus** audio.

---

## 2. Linux Distribution Strategy (Arch Linux First)
On Linux desktop systems (especially Arch Linux), multimedia codecs are typically handled through the host environment rather than proprietary closed browser bundles:

1. **Host FFmpeg / VA-API Acceleration**:
   - Arch Linux provides FFmpeg (`/usr/lib/libavcodec.so`) with full VA-API hardware acceleration for Intel (`intel-media-driver`), AMD (`mesa-va-drivers`), and NVIDIA (`nvidia-vaapi-driver`).
   - CEF's internal media pipeline can dynamically link or wrap system multimedia libraries when configured during compilation (`use_system_ffmpeg=true`), delegating codec patent handling to the user's distribution repository packages.

2. **Cisco OpenH264 Fallback**:
   - For H.264 video decoding without patent infringement, Cisco provides a binary OpenH264 module covered under Cisco's MPEG LA patent license.
   - Firefox uses this mechanism on Linux. Slate can similarly download Cisco's pre-built `libopenh264.so` at runtime if H.264 decoding is needed on a system lacking system FFmpeg.

3. **Widevine DRM Content (Netflix, Spotify, Hulu)**:
   - DRM streams typically require H.264 (video) and AAC (audio) inside an encrypted container (CENC).
   - In Slate, Widevine is **opt-in** and auto-detected from the user's existing Chrome/Brave/Firefox install (never bundled).
   - The Widevine CDM (`libwidevinecdm.so`) collaborates with CEF's media pipeline; decrypted samples are decoded either by the hardware decoder or through CEF's software fallbacks.

---

## 3. Fallback Matrix & Verification

| Codec | Type | Royalty Status | Supported in Slate CEF | Fallback Path |
| :--- | :--- | :--- | :--- | :--- |
| **VP9** | Video | Royalty-Free (Google / WebM) | **Native Built-in** | Hardware VA-API $\to$ libvpx software decode |
| **AV1** | Video | Royalty-Free (AOMedia) | **Native Built-in** | Hardware VA-API $\to$ libdav1d software decode |
| **VP8** | Video | Royalty-Free (WebM) | **Native Built-in** | Hardware VA-API $\to$ libvpx software decode |
| **Opus** | Audio | Royalty-Free (IETF) | **Native Built-in** | Built-in libopus software decode |
| **Vorbis** | Audio | Royalty-Free (Xiph.Org) | **Native Built-in** | Built-in libvorbis software decode |
| **H.264** | Video | Patent-encumbered (MPEG LA) | System / OpenH264 | VA-API hardware decode $\to$ OpenH264 $\to$ System FFmpeg |
| **AAC** | Audio | Patent-encumbered (Via LA) | System / Built-in | System FFmpeg / FAAD2 fallback |

### Verification on Real-World Workloads
During Task 1 / Task 1b benchmarking of YouTube 4K/60fps playback (Big Buck Bunny):
- Both Slate and Brave negotiated **VP9 (`codecs="vp09.00.51.08"`)** and **Opus (`codecs="opus"`)**.
- Both played smoothly with zero frame drops at 60 fps without triggering proprietary H.264 patent issues.
