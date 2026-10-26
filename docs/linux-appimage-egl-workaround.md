# Linux AppImage: blank window and EGL_BAD_PARAMETER

Some Linux Wayland systems show a blank gray/white CC Switch AppImage window while the backend starts normally. Terminal output may include:

```text
Could not create default EGL display: EGL_BAD_PARAMETER. Aborting...
```

## Confirmed workaround

On systems where `/usr/lib/libwayland-client.so.0` is present, try:

```bash
LD_PRELOAD=/usr/lib/libwayland-client.so.0 ./CC-Switch-v4.0.20-Linux-x86_64.AppImage
```

Adjust the AppImage path and filename. This is a per-launch workaround, **not** a universal fix; system library locations and compatibility vary by distro.

### Verified environment

- CC Switch v4.0.20 Linux x86_64 AppImage
- CachyOS / KDE Plasma Wayland / AMD HawkPoint integrated GPU
- System webkit2gtk-4.1 2.54.2-1
- `WEBKIT_DISABLE_DMABUF_RENDERER=1 LIBGL_ALWAYS_SOFTWARE=1` did not resolve it.
- Preloading the host `libwayland-client.so.0` restored the UI.

See [#5609](https://github.com/farion1231/cc-switch/issues/5609) and [#7709](https://github.com/farion1231/cc-switch/issues/7709). Investigate the bundled vs host graphics libraries before applying any global packaging change.
