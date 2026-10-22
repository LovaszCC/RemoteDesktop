//! System search launchers (see [`rd_platform::launchers`]) and the "open this entry" requests they
//! send. The launchers follow the database open right now; while it is locked they are left alone.

use std::path::PathBuf;
use std::sync::PoisonError;

use rd_platform::launchers::{self, LauncherFormat, LauncherTarget};

use super::AppCore;
use crate::dto::Phase;

/// What the launcher folder should become.
enum LauncherPlan {
    /// The setting is off: the folder goes.
    Remove,
    /// The library is readable: one launcher per entry.
    Reconcile(Vec<LauncherTarget>),
    /// Locked or no database: leave the files as they are.
    Keep,
}

impl AppCore {
    /// Uses `dir` as the launcher folder; `None` turns the launchers off.
    #[must_use]
    pub fn with_launcher_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.launcher_dir = dir;
        self
    }

    fn launcher_plan(&self) -> LauncherPlan {
        let inner = self.inner();
        if !inner.settings.show_in_system_search {
            return LauncherPlan::Remove;
        }
        let Ok((db, _)) = Self::unlocked(&inner) else {
            return LauncherPlan::Keep;
        };
        match db.library() {
            Ok(library) => LauncherPlan::Reconcile(
                library
                    .entries
                    .into_iter()
                    .map(|entry| LauncherTarget {
                        entry_id: entry.id,
                        name: entry.name,
                        created_at: entry.created_at,
                    })
                    .collect(),
            ),
            Err(_) => LauncherPlan::Keep,
        }
    }

    /// Brings the launcher folder in line with the setting and the open library. Runs after every library
    /// or status change, so failures are only logged: they never fail the user's action.
    pub fn sync_launchers(&self) {
        let Some(dir) = &self.launcher_dir else { return };
        let _serial = self.launcher_sync.lock().unwrap_or_else(PoisonError::into_inner);
        let result = match self.launcher_plan() {
            LauncherPlan::Remove => launchers::remove_all(dir),
            LauncherPlan::Reconcile(targets) => launchers::launcher_exe()
                .and_then(|exe| launchers::reconcile(dir, LauncherFormat::current(), &targets, &exe)),
            LauncherPlan::Keep => Ok(()),
        };
        if let Err(error) = result {
            eprintln!("system search launchers in {}: {error}", dir.display());
        }
    }

    /// Remembers an entry a launcher asked to open; a newer request replaces an older one.
    pub fn request_entry(&self, id: String) {
        *self.entry_request.lock().unwrap_or_else(PoisonError::into_inner) = Some(id);
    }

    /// The waiting request, handed out once the main window shows the library (unlocked); until then it
    /// stays.
    #[must_use]
    pub fn take_entry_request(&self) -> Option<String> {
        if !matches!(self.inner().phase, Phase::Unlocked { .. }) {
            return None;
        }
        self.entry_request
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}
