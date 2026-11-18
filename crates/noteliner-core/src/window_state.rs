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

    /// Keeps a line for the Log pane until the project in `folder` is next
    /// opened: for something that happened as it closed. Written at once.
    pub fn add_notice(&self, folder: &str, notice: String) {
        self.update(folder, |e| {
            let list = e.entry("notices").or_insert_with(|| json!([]));
            match list.as_array_mut() {
                Some(list) => list.push(json!(notice)),
                None => *list = json!([notice]),
            }
        });
        self.save_now();
    }

    /// The lines kept by [`WindowState::add_notice`], which are then
    /// forgotten.
    pub fn take_notices(&self, folder: &str) -> Vec<String> {
        let taken = self
            .data
            .lock()
            .unwrap()
            .get_mut(folder)
            .and_then(Value::as_object_mut)
            .and_then(|e| e.remove("notices"));
        let Some(Value::Array(list)) = taken else {
            return Vec::new();
        };
        self.save_now();
        list.iter()
            .filter_map(|n| n.as_str().map(str::to_string))
            .collect()
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
