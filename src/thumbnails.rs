use crate::store::Store;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
};
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::PostMessageW};

pub struct Worker {
    sender: Sender<Vec<i64>>,
    pub results: Receiver<(i64, std::result::Result<(), String>)>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    pub fn new(path: PathBuf, hwnd: HWND) -> Self {
        let (sender, requests) = mpsc::channel::<Vec<i64>>();
        let (completed, results) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let window = hwnd as usize;
        let thread = thread::spawn(move || {
            let store = match Store::open(&path) {
                Ok(s) => s,
                Err(e) => {
                    let _ = completed.send((0, Err(e.to_string())));
                    unsafe {
                        PostMessageW(window as HWND, crate::ACTION, crate::THUMBNAILS_READY, 0);
                    }
                    return;
                }
            };
            let mut queue = VecDeque::new();
            while !stopping.load(Ordering::Relaxed) {
                if queue.is_empty() {
                    match requests.recv() {
                        Ok(ids) => queue = ids.into(),
                        Err(_) => break,
                    }
                }
                // New searches/pages replace obsolete work; only one full image is decoded at a time.
                for ids in requests.try_iter() {
                    queue = ids.into();
                }
                if stopping.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(id) = queue.pop_front() {
                    let result = store.ensure_thumbnail(id).map_err(|e| e.to_string());
                    let _ = completed.send((id, result));
                    unsafe {
                        PostMessageW(window as HWND, crate::ACTION, crate::THUMBNAILS_READY, 0);
                    }
                }
            }
        });
        Self {
            sender,
            results,
            stop,
            thread: Some(thread),
        }
    }
    pub fn request(&self, ids: Vec<i64>) {
        let _ = self.sender.send(ids);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.sender.send(Vec::new());
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_worker_creates_disk_cache_and_stops() -> crate::store::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!("clipboard-thumb-worker-{stamp}.sqlite"));
        let store = Store::open(&path)?;
        store
            .0
            .execute("INSERT INTO entries(id,created) VALUES(1,0)", [])?;
        store.0.execute(
            "INSERT INTO formats VALUES(1,0,8,'DIB image',0,?)",
            [crate::image::thumbnail_fixture()],
        )?;
        let worker = Worker::new(path.clone(), std::ptr::null_mut());
        worker.request(vec![1]);
        let (id, result) = worker
            .results
            .recv_timeout(std::time::Duration::from_secs(5))?;
        assert_eq!(id, 1);
        assert!(result.is_ok());
        assert!(store.thumbnail(1)?.is_some());
        drop(worker);
        drop(store);
        std::fs::remove_file(path)?;
        Ok(())
    }
}
