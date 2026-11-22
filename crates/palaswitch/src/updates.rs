//! Installation and updates, through the Velopack installer and the
//! releases of the GitHub repository.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateInfo, UpdateManager, VelopackApp};

use crate::autostart::RUN_KEY;
use crate::text::Texts;

const REPOSITORY: &str = "https://github.com/Palawizard/palaswitch";
/// After a start with Windows, the network may not be up yet.
const FIRST_CHECK: Duration = Duration::from_secs(60);
const AFTER_CHECK: Duration = Duration::from_secs(24 * 60 * 60);
const AFTER_FAILED_CHECK: Duration = Duration::from_secs(60 * 60);

/// Answers the installer when it starts the app for one of its hooks, and
/// exits then; returns at once on a normal start, or when the app runs
/// outside an installation.
///
/// Must run before the single-instance lock: the installer starts the app
/// while another copy may be running.
pub fn run_installer_hooks() {
    VelopackApp::build()
        // A downloaded update is applied when the user asks for it, never
        // by a second launch that only meant to open the settings.
        .set_auto_apply_on_startup(false)
        .on_before_uninstall_fast_callback(|_| {
            let _ = RUN_KEY.forget();
        })
        .run();
}

/// What the update thread tells the interface thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateEvent {
    /// A newer version can be installed.
    Available(String),
    /// The version announced is no longer offered: it was taken down.
    Withdrawn,
    /// The download or the start of the installer failed; the version can
    /// be asked for again.
    InstallFailed(String),
    /// The installer waits for the app to quit, then applies the update and
    /// starts the new version.
    QuitToInstall,
}

/// What the menu and the settings window offer about updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    Nothing,
    Available(String),
    Installing(String),
}

impl Offer {
    /// The menu entry about the update, and whether it can be chosen.
    pub fn menu_entry(&self, texts: &Texts) -> Option<(String, bool)> {
        match self {
            Self::Nothing => None,
            Self::Available(version) => Some((with_version(texts.update_menu, version), true)),
            Self::Installing(version) => {
                Some((with_version(texts.update_installing, version), false))
            }
        }
    }

    /// The line of the settings window about the update, and whether its
    /// button can be pressed.
    pub fn settings_line(&self, texts: &Texts) -> Option<(String, bool)> {
        match self {
            Self::Nothing => None,
            Self::Available(version) => Some((with_version(texts.update_available, version), true)),
            Self::Installing(version) => {
                Some((with_version(texts.update_installing, version), false))
            }
        }
    }
}

fn with_version(template: &str, version: &str) -> String {
    template.replace("{version}", version)
}

/// Where versions come from, and how one is installed.
trait Feed {
    type Release;
    type Error: std::fmt::Display;

    /// The newest version above the running one, if any.
    fn newer(&self) -> Result<Option<(String, Self::Release)>, Self::Error>;

    /// Downloads `release` and starts the installer, which waits for the
    /// app to quit.
    fn install(&self, release: &Self::Release) -> Result<(), Self::Error>;
}

struct Github(UpdateManager);

impl Feed for Github {
    type Release = UpdateInfo;
    type Error = velopack::Error;

    fn newer(&self) -> Result<Option<(String, UpdateInfo)>, velopack::Error> {
        Ok(match self.0.check_for_updates()? {
            UpdateCheck::UpdateAvailable(info) => {
                Some((info.TargetFullRelease.Version.clone(), *info))
            }
            UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty => None,
        })
    }

    fn install(&self, release: &UpdateInfo) -> Result<(), velopack::Error> {
        self.0.download_updates(release, None)?;
        self.0.wait_exit_then_apply_updates(
            &release.TargetFullRelease,
            false,
            true,
            Vec::<String>::new(),
        )
    }
}

/// Asks the update thread to install the version it found.
pub struct Updater {
    install: Sender<()>,
}

impl Updater {
    /// Starts the thread that checks for updates; `None` when the app runs
    /// outside an installation, which has nothing to update.
    pub fn spawn(notify: impl Fn(UpdateEvent) + Send + 'static) -> Option<Self> {
        // A release candidate is offered the next candidates as well; a
        // stable version only stable ones.
        let manager = |prerelease| {
            UpdateManager::new(GithubSource::new(REPOSITORY, None, prerelease), None, None)
        };
        let prerelease = match manager(false) {
            Ok(manager) => !manager.get_current_version().pre.is_empty(),
            Err(error) => {
                tracing::info!(%error, "not installed: no update checks");
                return None;
            }
        };
        let feed = match manager(prerelease) {
            Ok(manager) => Github(manager),
            Err(error) => {
                tracing::warn!(%error, "no update checks");
                return None;
            }
        };
        let (install, requests) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("updates".to_owned())
            .spawn(move || watch(&feed, &requests, &notify, FIRST_CHECK));
        match spawned {
            Ok(_) => Some(Self { install }),
            Err(error) => {
                tracing::error!(%error, "could not start the update thread");
                None
            }
        }
    }

    pub fn install(&self) {
        if self.install.send(()).is_err() {
            tracing::error!("the update thread is gone");
        }
    }
}

/// Checks `feed` after `first_wait`, then on the schedule of [`Watch`];
/// installs the version found when asked. Returns once the installer waits
/// for the app, or when the app drops its `Updater`.
fn watch<F: Feed>(
    feed: &F,
    requests: &Receiver<()>,
    notify: &dyn Fn(UpdateEvent),
    first_wait: Duration,
) {
    let mut state = Watch::new(first_wait);
    loop {
        let event = match requests.recv_timeout(state.wait) {
            Err(RecvTimeoutError::Timeout) => state.check(feed),
            Ok(()) => state.install(feed),
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if let Some(event) = event {
            let done = event == UpdateEvent::QuitToInstall;
            notify(event);
            if done {
                return;
            }
        }
    }
}

/// What the update thread knows between two wake-ups.
struct Watch<R> {
    found: Option<(String, R)>,
    wait: Duration,
}

impl<R> Watch<R> {
    fn new(first_wait: Duration) -> Self {
        Self {
            found: None,
            wait: first_wait,
        }
    }

    /// Checks again in a day, or in an hour after a failure; a version is
    /// announced the first time it is found, and withdrawn when the feed no
    /// longer has it.
    fn check<F: Feed<Release = R>>(&mut self, feed: &F) -> Option<UpdateEvent> {
        match feed.newer() {
            Ok(newer) => {
                self.wait = AFTER_CHECK;
                let Some((version, release)) = newer else {
                    return self.found.take().map(|_| UpdateEvent::Withdrawn);
                };
                let known = self
                    .found
                    .as_ref()
                    .is_some_and(|(seen, _)| *seen == version);
                self.found = Some((version.clone(), release));
                (!known).then(|| {
                    tracing::info!(version, "update available");
                    UpdateEvent::Available(version)
                })
            }
            Err(error) => {
                tracing::warn!(%error, "could not check for updates");
                self.wait = AFTER_FAILED_CHECK;
                None
            }
        }
    }

    fn install<F: Feed<Release = R>>(&mut self, feed: &F) -> Option<UpdateEvent> {
        let (version, release) = self.found.as_ref()?;
        tracing::info!(version, "installing an update");
        match feed.install(release) {
            Ok(()) => Some(UpdateEvent::QuitToInstall),
            Err(error) => {
                tracing::error!(%error, version, "could not install the update");
                Some(UpdateEvent::InstallFailed(version.clone()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    type Check = Result<Option<&'static str>, &'static str>;

    /// A feed that answers from a script, in order.
    struct Scripted {
        checks: RefCell<Vec<Check>>,
        installs: RefCell<Vec<Result<(), &'static str>>>,
        installed: RefCell<Vec<String>>,
    }

    impl Scripted {
        fn new(checks: Vec<Check>, installs: Vec<Result<(), &'static str>>) -> Self {
            Self {
                checks: RefCell::new(checks),
                installs: RefCell::new(installs),
                installed: RefCell::new(Vec::new()),
            }
        }
    }

    impl Feed for Scripted {
        type Release = String;
        type Error = &'static str;

        fn newer(&self) -> Result<Option<(String, String)>, &'static str> {
            let found = self.checks.borrow_mut().remove(0)?;
            Ok(found.map(|version| (version.to_owned(), version.to_owned())))
        }

        fn install(&self, release: &String) -> Result<(), &'static str> {
            self.installed.borrow_mut().push(release.clone());
            self.installs.borrow_mut().remove(0)
        }
    }

    #[test]
    fn the_offer_can_be_taken_until_the_install_starts() {
        let texts = crate::text::texts(crate::text::Language::English);
        let available = Offer::Available("1.1.0".to_owned());
        let installing = Offer::Installing("1.1.0".to_owned());

        assert_eq!(Offer::Nothing.menu_entry(texts), None);
        assert_eq!(Offer::Nothing.settings_line(texts), None);
        assert_eq!(
            available.menu_entry(texts),
            Some(("Install version 1.1.0".to_owned(), true))
        );
        assert_eq!(
            available.settings_line(texts),
            Some(("Version 1.1.0 is available.".to_owned(), true))
        );
        assert_eq!(
            installing.menu_entry(texts),
            Some(("Installing version 1.1.0…".to_owned(), false))
        );
        assert_eq!(
            installing.settings_line(texts),
            installing.menu_entry(texts)
        );
    }

    fn available(version: &str) -> Option<UpdateEvent> {
        Some(UpdateEvent::Available(version.to_owned()))
    }

    #[test]
    fn a_version_is_announced_once_then_a_newer_one_again() {
        let feed = Scripted::new(
            vec![Ok(Some("1.1.0")), Ok(Some("1.1.0")), Ok(Some("1.2.0"))],
            vec![],
        );
        let mut state = Watch::new(FIRST_CHECK);

        let events = [state.check(&feed), state.check(&feed), state.check(&feed)];

        assert_eq!(events, [available("1.1.0"), None, available("1.2.0")]);
        assert_eq!(state.wait, AFTER_CHECK);
    }

    #[test]
    fn a_failed_check_tries_again_sooner_and_keeps_the_version_found() {
        let feed = Scripted::new(vec![Ok(Some("1.1.0")), Err("offline")], vec![Ok(())]);
        let mut state = Watch::new(FIRST_CHECK);

        let found = state.check(&feed);
        let failed = state.check(&feed);
        let installed = state.install(&feed);

        assert_eq!(found, available("1.1.0"));
        assert_eq!((failed, state.wait), (None, AFTER_FAILED_CHECK));
        assert_eq!(installed, Some(UpdateEvent::QuitToInstall));
        assert_eq!(*feed.installed.borrow(), ["1.1.0"]);
    }

    #[test]
    fn a_version_taken_down_is_withdrawn_once_and_not_installed() {
        let feed = Scripted::new(vec![Ok(Some("1.1.0")), Ok(None), Ok(None)], vec![]);
        let mut state = Watch::new(FIRST_CHECK);
        state.check(&feed);

        let events = [state.check(&feed), state.check(&feed)];

        assert_eq!(events, [Some(UpdateEvent::Withdrawn), None]);
        assert_eq!(state.wait, AFTER_CHECK);
        assert_eq!(state.install(&feed), None);
    }

    #[test]
    fn nothing_is_installed_before_a_version_is_found() {
        let feed = Scripted::new(vec![], vec![]);
        let mut state = Watch::new(FIRST_CHECK);

        assert_eq!(state.install(&feed), None);
        assert!(feed.installed.borrow().is_empty());
    }

    #[test]
    fn a_failed_install_can_be_asked_for_again() {
        let feed = Scripted::new(vec![Ok(Some("1.1.0"))], vec![Err("no network"), Ok(())]);
        let mut state = Watch::new(FIRST_CHECK);
        state.check(&feed);

        let first = state.install(&feed);
        let second = state.install(&feed);

        assert_eq!(first, Some(UpdateEvent::InstallFailed("1.1.0".to_owned())));
        assert_eq!(second, Some(UpdateEvent::QuitToInstall));
    }

    #[test]
    fn the_thread_ends_when_the_app_lets_go_of_it() {
        let feed = Scripted::new(vec![], vec![]);
        let (install, requests) = mpsc::channel();
        drop(install);

        watch(
            &feed,
            &requests,
            &|_| panic!("nothing to report"),
            Duration::from_secs(3600),
        );
    }

    #[test]
    fn the_thread_ends_once_the_installer_waits_for_the_app() {
        let feed = Scripted::new(vec![Ok(Some("1.1.0"))], vec![Ok(())]);
        let (install, requests) = mpsc::channel();
        let events = RefCell::new(Vec::new());
        let notify = |event: UpdateEvent| {
            if matches!(event, UpdateEvent::Available(_)) {
                install.send(()).unwrap();
            }
            events.borrow_mut().push(event);
        };

        watch(&feed, &requests, &notify, Duration::ZERO);

        assert_eq!(
            *events.borrow(),
            [
                UpdateEvent::Available("1.1.0".to_owned()),
                UpdateEvent::QuitToInstall
            ]
        );
    }
}
