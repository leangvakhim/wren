//! Keeps the computer from sleeping by itself while the phone link is on, so
//! the phone can still reach Wren. (Closing a laptop's lid still puts it to
//! sleep on most computers.) Dropping the guard lets the computer sleep again.

pub struct Guard {
    #[cfg(not(windows))]
    child: Option<std::process::Child>,
    #[cfg(windows)]
    stop: std::sync::mpsc::Sender<()>,
}

impl Guard {
    pub fn start() -> Option<Self> {
        #[cfg(not(windows))]
        {
            use std::process::{Command, Stdio};
            let pid = std::process::id().to_string();
            #[cfg(target_os = "macos")]
            let cmd = Command::new("/usr/bin/caffeinate").args(["-i", "-w", &pid]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
            #[cfg(not(target_os = "macos"))]
            let cmd = {
                let _ = &pid;
                Command::new("systemd-inhibit")
                    .args(["--what=idle:sleep", "--who=Wren", "--why=Phone link is on", "--mode=block", "sleep", "infinity"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            };
            cmd.ok().map(|c| Self { child: Some(c) })
        }
        #[cfg(windows)]
        {
            // SetThreadExecutionState lasts as long as the thread that called it.
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn SetThreadExecutionState(flags: u32) -> u32;
            }
            const ES_CONTINUOUS: u32 = 0x8000_0000;
            const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
                let _ = rx.recv();
                unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
            });
            Some(Self { stop: tx })
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        #[cfg(not(windows))]
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        #[cfg(windows)]
        {
            let _ = self.stop.send(());
        }
    }
}
