//! Per-project window layout + bounds (`window-state.json` in userData).
//! Port of `apps/noteliner/src/main/window-state-service.js`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use laankit_core::debounce::Debouncer;
use serde_json::{json, Map, Value};

pub struct WindowState {
    path: PathBuf,
    data: Arc<Mutex<Map<String, Value>>>,
    saver: Debouncer,
}

impl WindowState {
    pub fn new(path: PathBuf) -> Self {
        let data = laankit_core::json::read_object(&path);
        Self {
            path,
            data: Arc::new(Mutex::new(data)),
            saver: Debouncer::new(),
        }
    }

    fn schedule_save(&self) {
        let (path, data) = (self.path.clone(), self.data.clone());
        self.saver
            .schedule(Duration::from_secs(1), move || async move {
                let snapshot = data.lock().unwrap().clone();
                let _ = laankit_core::json::write(&path, &snapshot);
            });
    }

    pub fn save_now(&self) {
        self.saver.cancel();
        let snapshot = self.data.lock().unwrap().clone();
        let _ = laankit_core::json::write(&self.path, &snapshot);
    }

    fn update(&self, folder: &str, f: impl FnOnce(&mut Map<String, Value>)) {
        let mut data = self.data.lock().unwrap();
        let entry = data.entry(folder.to_string()).or_insert_with(|| json!({}));
        if !entry.is_object() {
            *entry = json!({});
        }
        f(entry.as_object_mut().unwrap());
    }

    pub fn layout(&self, folder: &str) -> Value {
        self.data
            .lock()
            .unwrap()
            .get(folder)
            .and_then(|e| e.get("layout"))
            .cloned()
            .unwrap_or(Value::Null)
    }

    pub fn set_layout(&self, folder: &str, layout: Value) {
        self.update(folder, |e| {
            e.insert("layout".into(), layout);
        });
        self.schedule_save();
    }

    /// `{ bounds, isMaximized }` or null.
    pub fn bounds(&self, folder: &str) -> Option<(Value, bool)> {
        let data = self.data.lock().unwrap();
        let e = data.get(folder)?;
        Some((
            e.get("bounds").cloned().unwrap_or(Value::Null),
            e.get("isMaximized")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ))
    }

    pub fn set_bounds(&self, folder: &str, bounds: Value, maximized: bool, immediate: bool) {
        self.update(folder, |e| {
            e.insert("bounds".into(), bounds);
            e.insert("isMaximized".into(), json!(maximized));
        });
        if immediate {
            self.save_now()
        } else {
            self.schedule_save()
        }
    }
}
