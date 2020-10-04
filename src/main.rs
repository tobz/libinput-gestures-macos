use futures::future::poll_fn;
use input::{
    event::pointer::{Axis, AxisSource, PointerEvent, PointerEventTrait},
    event::Event,
    Libinput, LibinputInterface,
};
use mio::unix::EventedFd;
use mio::{event::Evented, Poll, PollOpt, Ready, Token};
use nix::{
    fcntl::{open, OFlag},
    sys::stat::Mode,
    unistd::close,
};
use std::os::unix::io::AsRawFd;
use std::os::unix::io::RawFd;
use std::path::Path;
use std::process::Stdio;
use std::task::Poll as FuturesPoll;
use tokio::io::PollEvented;
use tokio::process::Command;

const TOUCH_DEVICE: &str = "/dev/input/by-path/pci-0000:00:15.1-platform-i2c_designware.1-event-mouse";
const SWIPE_VDELTA_THRESHOLD: f64 = 0.00175;
const LEFT_SWIPE_ACTION: &[&str] = &["key", "super+shift+Left"];
const RIGHT_SWIPE_ACTION: &[&str] = &["key", "super+shift+Right"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let mut context = LibinputContext::open(TOUCH_DEVICE).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "failed to create libinput context",
            )
        })?;
        context.resume().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "failed to resume libinput context",
            )
        })?;

        let mut left_swipe = SwipeTracking::new();
        let mut right_swipe = SwipeTracking::new();

        while let Ok(e) = context.next().await {
            match e {
                // We only care about horizontal scroll pointer events, which are generated when
                // libinput detects two-finger swipes.
                Event::Pointer(PointerEvent::Axis(pae)) => {
                    if pae.has_axis(Axis::Horizontal) && pae.axis_source() == AxisSource::Finger {
                        // Track which direction the swipe is going.
                        let av = pae.axis_value(Axis::Horizontal);
                        if av < 0.0 {
                            left_swipe.measure_event(pae.time_usec(), av);
                        } else if av > 0.0 {
                            right_swipe.measure_event(pae.time_usec(), av);
                        } else {
                            // No magnitude for the swipe action, which is a special signal that the
                            // swipe has stopped.  Calculate based on our running total if we should
                            // actually treat this as a swipe, based on our velocity threshold.
                            let tend = pae.time_usec();
                            let lvdelta = left_swipe.flush(tend);
                            let rvdelta = right_swipe.flush(tend);

                            // We reverse the direction to emulate natural scrolling: if you drag your
                            // fingers right to left (left swipe), you're swiping the page to the left,
                            // or pulling the next page to you, and vise versa.  Just like a book.
                            let result = if lvdelta.is_some() && rvdelta.is_none() {
                                Some((lvdelta.unwrap(), LEFT_SWIPE_ACTION))
                            } else if rvdelta.is_some() && lvdelta.is_none() {
                                Some((rvdelta.unwrap(), RIGHT_SWIPE_ACTION))
                            } else {
                                None
                            };

                            // This cancels out weird events where the user scrolled/swiped both left
                            // and right or their touchpad picked up something weird.
                            if let Some((vdelta, cmd)) = result {
                                if vdelta.abs() >= SWIPE_VDELTA_THRESHOLD {
                                    launch_xdotool(cmd);
                                }
                            }
                        }
                    }
                }
                // We only handle pointer events.
                _ => {}
            }
        }

        Ok(())
    })
}

fn launch_xdotool(cmd_opts: &[&str]) {
    let mut cmd = Command::new("xdotool");
    cmd.args(cmd_opts)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = cmd.spawn();
}

// Tracks the velocity of a swipe.
struct SwipeTracking {
    tstart: u64,
    vtotal: f64,
}

impl SwipeTracking {
    pub fn new() -> SwipeTracking {
        SwipeTracking {
            tstart: 0,
            vtotal: 0.0,
        }
    }

    pub fn measure_event(&mut self, t: u64, v: f64) {
        if self.tstart == 0 {
            self.tstart = t;
        }
        self.vtotal += v;
    }

    pub fn flush(&mut self, t: u64) -> Option<f64> {
        if self.tstart == 0 {
            return None;
        }

        let tdelta = t - self.tstart;
        let vdelta = self.vtotal / tdelta as f64;
        self.tstart = 0;
        self.vtotal = 0.0;
        Some(vdelta)
    }
}

// Basic libinput interface for opening/closing FDs.
struct BasicLibinputInterface;

impl LibinputInterface for BasicLibinputInterface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<RawFd, i32> {
        open(path, OFlag::from_bits_truncate(flags), Mode::empty())
            // TODO: we should derive errno from err here but there's no conversion
            // from Errnp to i32 for w/e god damn reason so...
            .map_err(|_err| 1)
    }

    fn close_restricted(&mut self, fd: RawFd) {
        let _ = close(fd);
    }
}

// Wrapper for libinput context that handles the asynchronous aspect.
struct LibinputContext(Libinput, PollEvented<LibinputEvented>);

impl LibinputContext {
    pub fn open<P>(p: P) -> Result<LibinputContext, ()>
    where
        P: AsRef<str>,
    {
        let mut context = Libinput::new_from_path(BasicLibinputInterface);
        if let None = context.path_add_device(p.as_ref()) {
            return Err(());
        }

        let ev = PollEvented::new(LibinputEvented(context.as_raw_fd())).map_err(|_err| ())?;

        Ok(LibinputContext(context, ev))
    }

    pub fn resume(&mut self) -> Result<(), ()> {
        self.0.resume()
    }

    pub async fn next(&mut self) -> Result<Event, ()> {
        loop {
            let _ = self.0.dispatch().map_err(|_| ())?;
            match self.0.next() {
                Some(e) => return Ok(e),
                None => {
                    let _ = poll_fn(|cx| self.1.poll_read_ready(cx, Ready::readable()))
                        .await
                        .map_err(|_| ())?;
                    let _ = poll_fn(|cx| {
                        FuturesPoll::Ready(self.1.clear_read_ready(cx, Ready::readable()))
                    })
                    .await
                    .map_err(|_| ())?;
                }
            }
        }
    }
}

// Eventd wrapper for the underlying libinput FD.
struct LibinputEvented(RawFd);

impl Evented for LibinputEvented {
    fn register(
        &self,
        poll: &Poll,
        token: Token,
        interest: Ready,
        opts: PollOpt,
    ) -> std::io::Result<()> {
        EventedFd(&self.0).register(poll, token, interest, opts)
    }

    fn reregister(
        &self,
        poll: &Poll,
        token: Token,
        interest: Ready,
        opts: PollOpt,
    ) -> std::io::Result<()> {
        EventedFd(&self.0).reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &Poll) -> std::io::Result<()> {
        EventedFd(&self.0).deregister(poll)
    }
}
