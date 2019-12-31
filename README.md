# libinput-gestures-macos

Emulates macOS-style swipes for forward and backward.

On Linux, libinput will normally interpret this as horizontal scrolling, which isn't wrong, but if you're like me, the muscle memory to switch from two fingers to three fingers can be hard.  This program will track horizontal scrolling activity and calculate the velocity of the scrolling event, triggering actions when it quantifies the scrolling as a full-fledged swipe.

## warning

This program is very rough, totally hard-coded, and is designed purely (right now) to emulate macOS two-finger swipes, nothing more.

- Hard-coded to send `alt+Right` or `alt+Left` for left and right swipes, respectively.
- Hard-coded to a specific nput device. (My touchpad on my particular laptop.)
- Hard-coded velocity threshold. (This is likely fine for most people, though.)

Requires [`xdotool`](https://github.com/jordansissel/xdotool) to actually send the commands to the OS.

## tech specs

Based on the [`input`](https://github.com/Smithay/input.rs) crate to parse `libinput` events, and [`mio`](https://github.com/tokio-rs/mio) and [`tokio`](https://github.com/tokio-rs/tokio) to asynchronously listen to the libinput data.

## usage

1.) Figure out which input device is your touchpad.  This will be a `/dev/input/eventXX` device.  `libinput-list-devices` can help you figure this out if you don't already know.

2.) Checkout the project.

    git clone https://github.com/tobz/libinput-gestures-macos
    cd libinput-gestures-macos

3.) Edit `TOUCH_DEVICE` in `src/main.rs` to match watever your input device path is.  Also, edit the swipe actions if you want them to trigger another shortcut, but the default values should match the ideal macOS behavior out of the box.

4.) Build and run the project.  (You need [Rust](https://www.rust-lang.org/tools/install) for this.)

    cargo build --release
    target/release/libinput-gestrures-macos

5.) Switch to another window -- a browser is ideal obviously -- and you should be able to two-finger swipe to go forwards and backwards.

6.) Making this run on system boot, etc, is an exercise left to the reader.

## open source

PRs welcome for any improvements.  MIT license, so do whatever you want.
