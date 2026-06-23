use std::sync::{Condvar, Mutex};

pub struct FlowControl {
    paused: Mutex<bool>,
    cond: Condvar,
}

impl FlowControl {
    pub fn new() -> Self {
        Self {
            paused: Mutex::new(false),
            cond: Condvar::new(),
        }
    }
    pub fn pause(&self) {
        *self.paused.lock().unwrap() = true;
    }
    pub fn resume(&self) {
        let mut p = self.paused.lock().unwrap();
        *p = false;
        self.cond.notify_all();
    }
    pub fn wait_if_paused(&self) {
        let mut p = self.paused.lock().unwrap();
        while *p {
            p = self.cond.wait(p).unwrap();
        }
    }
}
