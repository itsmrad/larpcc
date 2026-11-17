use super::{Error, ProfileName, ProfileState, Provider, Registry, model::DesktopBinding, store};
use crate::{
    desktop::{self, CheckedDirectory, CheckedSnapshot},
    launch::admission::{AuthGuard, acquire_auth_guard},
    platform::{self, ObjectKind},
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn directory(path: PathBuf) -> Result<CheckedDirectory, Error> {
    let ancestry = platform::anchor(&path)?;
    let object = platform::inspect(&path, ObjectKind::Directory)?.ok_or(Error::AdmissionChanged)?;
    Ok(CheckedDirectory {
        path,
        object,
        ancestry,
    })
}
impl Registry {
    pub(crate) fn desktop_target(
        &self,
        name: &ProfileName,
    ) -> Result<(CheckedSnapshot, desktop::CheckedApp, AuthGuard), Error> {
        let (id, revision) = {
            let (data, _lock) = store::snapshot(self.root(), self.expected)?;
            store::require_v3(&data)?;
            let p = data
                .profiles
                .iter()
                .find(|p| &p.name == name)
                .ok_or_else(|| Error::NotFound { name: name.clone() })?;
            if p.provider != Provider::Codex {
                return Err(Error::QualificationPending);
            }
            (p.id, data.revision)
        };
        // Lifecycle guard precedes registry lock and survives all native effects.
        let guard = acquire_auth_guard(self.root(), id)?;
        let (data, _lock) = store::snapshot(self.root(), self.expected)?;
        if data.revision != revision {
            return Err(Error::AdmissionChanged);
        }
        let p = data
            .profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or(Error::AdmissionChanged)?;
        if p.state != Some(ProfileState::Active) || !p.surfaces.is_some_and(|s| s.desktop) {
            return Err(Error::RecoveryRequired);
        }
        let b = data
            .desktop_bindings
            .iter()
            .find(|b| b.provider == p.provider)
            .ok_or(Error::Unbound {
                provider: p.provider,
            })?;
        let app = desktop::validate_binding(p.provider, &b.path)?;
        let container = self.root().join("profiles").join(id.to_string());
        let snapshot = CheckedSnapshot {
            profile_id: id,
            provider: p.provider,
            revision,
            recorded_version: b.recorded_version.clone(),
            root: directory(self.root().to_owned())?,
            cli: directory(container.join("cli"))?,
            desktop: directory(container.join("desktop"))?,
            container: directory(container)?,
        };
        self.observed.set(Some(revision));
        Ok((snapshot, app, guard))
    }
    pub(crate) fn recheck_desktop(
        &self,
        snapshot: &CheckedSnapshot,
        binding_path: &Path,
    ) -> Result<(), Error> {
        let (data, _lock) = store::snapshot(self.root(), self.expected)?;
        if snapshot.root.path != self.root() || data.revision != snapshot.revision {
            return Err(Error::AdmissionChanged);
        }
        store::require_v3(&data)?;
        if !data.profiles.iter().any(|p| {
            p.id == snapshot.profile_id
                && p.provider == snapshot.provider
                && p.state == Some(ProfileState::Active)
                && p.surfaces.is_some_and(|s| s.desktop)
        }) || !data.desktop_bindings.iter().any(|b| {
            b.provider == snapshot.provider
                && b.path == binding_path
                && b.recorded_version == snapshot.recorded_version
        }) {
            return Err(Error::AdmissionChanged);
        }
        Ok(())
    }
    pub(crate) fn bind_desktop(
        &self,
        provider: Provider,
        path: PathBuf,
        recorded_version: Option<String>,
        replace: bool,
    ) -> Result<bool, Error> {
        let _lock = store::lock(self.root(), Duration::from_secs(10))?;
        let mut data = store::load(self.root())?;
        store::precondition(&data, self.expected)?;
        store::require_v3(&data)?;
        let checked = desktop::validate_binding(provider, &path)?;
        let binding = DesktopBinding {
            provider,
            path,
            recorded_version,
        };
        let old = data
            .desktop_bindings
            .iter()
            .position(|b| b.provider == provider);
        if let Some(i) = old {
            if data.desktop_bindings[i] == binding {
                self.observed.set(Some(data.revision));
                return Ok(false);
            }
            if !replace {
                return Err(Error::Conflict);
            }
            data.desktop_bindings[i] = binding;
        } else {
            data.desktop_bindings.push(binding);
        }
        data.revision = data.revision.checked_add(1).ok_or(Error::Exhausted)?;
        let bytes = store::serialize(&data)?;
        checked.recheck()?;
        store::persist(self.root(), &bytes, &mut false, &mut false)?;
        self.observed.set(Some(data.revision));
        Ok(true)
    }
    pub(crate) fn open_desktop(
        &self,
        name: &ProfileName,
        json: bool,
    ) -> Result<desktop::DesktopReceipt, Error> {
        let (snapshot, app, _guard) = self.desktop_target(name)?;
        let target = desktop::ValidatedDesktopTarget::from_checked(snapshot, app)?;
        if let desktop::Activity::Unknown(_) = desktop::activity_for(&target) {
            return Err(Error::AdmissionChanged);
        }
        desktop::open_target(
            &target,
            || self.recheck_desktop(target.snapshot(), target.binding_path()),
            |message| {
                if !json {
                    eprintln!("warning: {message}");
                }
            },
        )
    }
}
