<img src=".github/splash.png" alt="TAO - Window Creation Library" />

[![](https://img.shields.io/crates/v/tao?style=flat-square)](https://crates.io/crates/tao)
[![](https://img.shields.io/docsrs/tao?style=flat-square)](https://docs.rs/tao/)
[![License](https://img.shields.io/badge/License-Apache%202-green.svg)](https://opencollective.com/tauri)
[![Chat Server](https://img.shields.io/badge/chat-discord-7289da.svg)](https://discord.gg/SpmNs4S)
[![website](https://img.shields.io/badge/website-tauri.app-purple.svg)](https://tauri.app)
[![https://good-labs.github.io/greater-good-affirmation/assets/images/badge.svg](https://good-labs.github.io/greater-good-affirmation/assets/images/badge.svg)](https://good-labs.github.io/greater-good-affirmation)
[![support](https://img.shields.io/badge/sponsor-Open%20Collective-blue.svg)](https://opencollective.com/tauri)

Cross-platform application window creation library in Rust that supports all major platforms like
Windows, macOS, Linux, iOS and Android. Built for you, maintained for Tauri.

### Cargo Features

TAO provides the following features, which can be enabled in your `Cargo.toml` file:

- `serde`: Enables serialization/deserialization of certain types with [Serde](https://crates.io/crates/serde).

## Platform-specific notes

### Android

This library makes use of the [ndk-rs](https://github.com/rust-windowing/android-ndk-rs) crates, refer to that repo for more documentation.

Running on an Android device needs a dynamic system library, add this to Cargo.toml:

```toml
[[example]]
name = "request_redraw_threaded"
crate-type = ["cdylib"]
```

And add this to the example file to add the native activity glue:

```rust
#[cfg_attr(target_os = "android", ndk_glue::main(backtrace = "on"))]
fn main() {
    ...
}
```

And run the application with `cargo apk run --example request_redraw_threaded`

### Linux

Gtk and its related libraries are used to build the support of Linux. Be sure to install following packages before building:

#### Arch Linux / Manjaro:

```bash
sudo pacman -S gtk3
```

#### Debian / Ubuntu:

```bash
sudo apt install libgtk-3-dev
```

### Acknowledgement

This is a fork of [winit](https://crates.io/crates/winit) which replaces Linux's port to Gtk.
In the future, we want to make these features more modular as separate crates. So we can switch back to winit and also benefit the whole community.

## Partners

<table>
  <tbody>
    <tr>
      <td align="center" valign="middle">
        <a href="https://crabnebula.dev" target="_blank">
          <img src=".github/sponsors/crabnebula.svg" alt="CrabNebula" width="283">
        </a>
      </td>
    </tr>
  </tbody>
</table>

For the complete list of sponsors please visit our [website](https://tauri.app#sponsors) and [Open Collective](https://opencollective.com/tauri).
