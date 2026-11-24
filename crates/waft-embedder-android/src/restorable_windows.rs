//! The windows an earlier run of the app left: Android keeps a task, its card in Recents, apart
//! from the app's process, so a task outlives the process that asked for its view, and Android
//! restores it, in a process started afresh, when the user picks its card (`view_requests.rs`).
//! An app that would only close such a window as it is restored may see it here first, by the
//! initial data its view was asked for with, and remove it: Android's
//! `ActivityManager.getAppTasks`, and `AppTask.finishAndRemoveTask`.
//!
//! Android's alone: no other system keeps a window of a process that ended. So it is reached by
//! downcasting the platform ([`AndroidPlatform::restorable_windows`](crate::AndroidPlatform)).
//!
//! The invariant a later editor will break: a window of this run is never among them, neither one
//! this process asked for, which its intent names, nor one Android already restored to it, whose
//! view the app was given and answers for as it arrives.

use jni::objects::{JIntArray, JObject, JString, JValue};
use jni::{jni_sig, jni_str};

use crate::application::{Application, package_class};
use crate::log::warn_to_log;
use crate::this_process::this_process;
use crate::views::Views;

const WAFT_TASK: &str = "rs.waft.WaftTask";

/// A window an earlier run of the app left, which Android restores when the user picks its card
/// in Recents.
pub struct RestorableWindow {
    application: Application,
    task_id: i32,
    initial_data: Option<String>,
}

impl RestorableWindow {
    /// What the window's view was asked for with (`ViewRequest::initial_data`); none where it was
    /// asked for with none.
    pub fn initial_data(&self) -> Option<&str> {
        self.initial_data.as_deref()
    }

    /// Removes the window, its card with it, so Android restores it no more.
    pub fn remove(&self) {
        let id = self.task_id;
        let removed = self.application.with(|env, context| {
            let class = package_class(env, context, WAFT_TASK)?;
            env.call_static_method(
                &class,
                jni_str!("remove"),
                jni_sig!("(Landroid/content/Context;I)Z"),
                &[JValue::Object(context), JValue::Int(id)],
            )?
            .z()
        });
        match removed {
            Ok(true) => {}
            Ok(false) => warn_to_log!("waft: removing a window an earlier run left: no task {id}"),
            Err(error) => {
                self.application.clear_exception();
                warn_to_log!("waft: removing a window an earlier run left failed: {error}");
            }
        }
    }
}

/// The windows an earlier run of the app left, none of them a view of `views`; none where
/// Android would not say.
pub(crate) fn restorable_windows(
    application: &Application,
    views: &Views,
) -> Vec<RestorableWindow> {
    let shown: Vec<i32> = (views.all().iter())
        .filter_map(|view| view.task().task_id())
        .collect();
    let read = application.with(|env, context| {
        let class = package_class(env, context, WAFT_TASK)?;
        let ids = env
            .call_static_method(
                &class,
                jni_str!("restorableTaskIds"),
                jni_sig!("(Landroid/content/Context;J)[I"),
                &[JValue::Object(context), JValue::Long(this_process() as i64)],
            )?
            .l()?;
        let ids = env.cast_local::<JIntArray>(ids)?;
        let mut task_ids = vec![0; ids.len(env)?];
        ids.get_region(env, 0, &mut task_ids)?;
        let mut windows = Vec::new();
        for task_id in task_ids.into_iter().filter(|id| !shown.contains(id)) {
            let data = env
                .call_static_method(
                    &class,
                    jni_str!("initialData"),
                    jni_sig!("(Landroid/content/Context;I)Ljava/lang/String;"),
                    &[JValue::Object(context), JValue::Int(task_id)],
                )?
                .l()?;
            windows.push((task_id, initial_data(env, data)?));
        }
        Ok(windows)
    });
    let windows = read.unwrap_or_else(|error| {
        application.clear_exception();
        warn_to_log!("waft: the windows an earlier run left are not known: {error}");
        Vec::new()
    });
    (windows.into_iter())
        .map(|(task_id, initial_data)| RestorableWindow {
            application: application.clone(),
            task_id,
            initial_data,
        })
        .collect()
}

/// The initial data `data` is, a Java string or null.
fn initial_data(env: &mut jni::Env<'_>, data: JObject<'_>) -> jni::errors::Result<Option<String>> {
    if data.is_null() {
        return Ok(None);
    }
    Ok(Some(env.cast_local::<JString>(data)?.try_to_string(env)?))
}
